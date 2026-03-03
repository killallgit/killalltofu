mod cron;
mod destroy;
mod logging;
mod registry;
mod time;

use std::env;
use std::fs;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};

use crate::destroy::{run_destroy, run_destroy_streaming};
use crate::logging::{write_attempt_log, write_killallerror};
use crate::registry::{
    detect_engine, load_registry, save_registry, sanitize_path_to_key, RegistryEntry,
};
use crate::time::{compute_destroy_at, is_retry_eligible};

const MAX_ATTEMPTS: u32 = 3;

#[derive(Parser)]
#[command(name = "killalltofu", about = "Schedule automatic destruction of Terraform/OpenTofu projects")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Duration until destruction (e.g., 30s, 5m, 1h, 1d)
    #[arg(conflicts_with = "command")]
    duration: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show all registered projects
    List,
    /// Cancel scheduled destruction for current directory
    Cancel,
    /// Alias for cancel
    Delete,
    /// Destroy immediately
    Kill {
        /// Target a project by registry name instead of current directory
        #[arg(long)]
        name: Option<String>,
    },
    /// Internal: process registry for due projects (called by cron)
    Check,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::List) => cmd_list(),
        Some(Commands::Cancel) | Some(Commands::Delete) => cmd_cancel(),
        Some(Commands::Kill { name }) => cmd_kill(name),
        Some(Commands::Check) => cmd_check(),
        None => match cli.duration {
            Some(duration) => cmd_register(&duration),
            None => {
                Cli::parse_from(["killalltofu", "--help"]);
                Ok(())
            }
        },
    }
}

fn cmd_register(duration: &str) -> Result<()> {
    let cwd = env::current_dir().with_context(|| "failed to get current directory")?;
    let destroy_at = compute_destroy_at(duration)?;
    let engine = detect_engine(&cwd);
    let key = sanitize_path_to_key(&cwd);

    let mut registry = load_registry()?;
    registry.insert(
        key.clone(),
        RegistryEntry {
            path: cwd.clone(),
            destroy_at,
            engine,
            attempts: 0,
            last_attempt_at: None,
        },
    );
    save_registry(&registry)?;

    let marker_path = cwd.join(".killalltofu");
    fs::write(&marker_path, destroy_at.to_rfc3339())
        .with_context(|| "failed to write .killalltofu marker")?;

    cron::ensure_cron_installed()?;

    eprintln!(
        "registered: {key}\n  engine: {engine}\n  destroy at: {destroy_at}",
    );
    Ok(())
}

fn cmd_list() -> Result<()> {
    let registry = load_registry()?;

    if registry.is_empty() {
        println!("no projects registered");
        return Ok(());
    }

    println!(
        "{:<40} {:<26} {:<12} ATTEMPTS",
        "NAME", "DESTROY AT", "ENGINE"
    );
    for (key, entry) in &registry {
        println!(
            "{:<40} {:<26} {:<12} {}/{}",
            key,
            entry.destroy_at.to_rfc3339(),
            entry.engine,
            entry.attempts,
            MAX_ATTEMPTS,
        );
    }
    Ok(())
}

fn cmd_cancel() -> Result<()> {
    let cwd = env::current_dir().with_context(|| "failed to get current directory")?;
    let key = sanitize_path_to_key(&cwd);

    let mut registry = load_registry()?;
    if registry.remove(&key).is_none() {
        anyhow::bail!("no project registered for: {key}");
    }
    save_registry(&registry)?;

    let marker = cwd.join(".killalltofu");
    if marker.exists() {
        fs::remove_file(&marker).with_context(|| "failed to remove .killalltofu")?;
    }

    eprintln!("cancelled: {key}");
    Ok(())
}

fn cmd_kill(name: Option<String>) -> Result<()> {
    let (key, entry) = resolve_target(name)?;

    eprintln!(
        "destroying: {} (engine: {}, path: {})",
        key,
        entry.engine,
        entry.path.display()
    );

    let result = run_destroy_streaming(entry.engine, &entry.path)?;

    if result.success {
        cleanup_success(&key, &entry.path)?;
        eprintln!("destroyed: {key}");
    } else {
        let code = result.exit_code.map_or("unknown".to_string(), |c| c.to_string());
        anyhow::bail!("destroy failed (exit code: {code})");
    }
    Ok(())
}

fn cmd_check() -> Result<()> {
    let mut registry = load_registry()?;
    let now = Utc::now();

    let due_keys: Vec<String> = registry
        .iter()
        .filter(|(_, entry)| now >= entry.destroy_at)
        .filter(|(_, entry)| is_retry_eligible(entry.attempts, entry.last_attempt_at))
        .map(|(key, _)| key.clone())
        .collect();

    for key in due_keys {
        let entry = registry.get(&key).unwrap().clone();
        process_destroy(&key, &entry, &mut registry)?;
    }

    save_registry(&registry)?;
    Ok(())
}

fn process_destroy(
    key: &str,
    entry: &RegistryEntry,
    registry: &mut registry::Registry,
) -> Result<()> {
    let attempt = entry.attempts + 1;
    eprintln!("attempt {attempt}/{MAX_ATTEMPTS} for {key}");

    let result = run_destroy(entry.engine, &entry.path)?;

    write_attempt_log(
        key,
        attempt,
        entry.engine,
        &entry.path,
        result.exit_code,
        &result.output,
    )?;

    if result.success {
        cleanup_success(key, &entry.path)?;
        registry.remove(key);
        eprintln!("destroyed: {key}");
    } else if attempt >= MAX_ATTEMPTS {
        write_killallerror(&entry.path, key, entry.engine, attempt, &result.output)?;
        registry.remove(key);
        eprintln!("max retries reached for {key}, wrote .killallerror");
    } else {
        let reg_entry = registry.get_mut(key).unwrap();
        reg_entry.attempts = attempt;
        reg_entry.last_attempt_at = Some(Utc::now());
        eprintln!("attempt {attempt} failed for {key}, will retry");
    }

    Ok(())
}

fn resolve_target(name: Option<String>) -> Result<(String, RegistryEntry)> {
    let registry = load_registry()?;

    let key = match name {
        Some(n) => n,
        None => {
            let cwd = env::current_dir().with_context(|| "failed to get current directory")?;
            sanitize_path_to_key(&cwd)
        }
    };

    let entry = registry
        .get(&key)
        .with_context(|| format!("no project registered for: {key}"))?
        .clone();

    Ok((key, entry))
}

fn cleanup_success(key: &str, project_path: &std::path::Path) -> Result<()> {
    let marker = project_path.join(".killalltofu");
    if marker.exists() {
        fs::remove_file(&marker).ok();
    }

    let mut registry = load_registry()?;
    registry.remove(key);
    save_registry(&registry)?;

    Ok(())
}
