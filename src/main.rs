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
    detect_engine, load_registry, load_registry_shared, lock_registry, lock_registry_shared,
    save_registry, sanitize_path_to_key, RegistryEntry,
};
use crate::time::{compute_destroy_at, is_retry_eligible};

const MAX_ATTEMPTS: u32 = 3;

#[derive(Parser)]
#[command(name = "killalltofu", about = "Schedule automatic destruction of Terraform/OpenTofu projects")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Duration until destruction (e.g., 30s, 5m, 1h, 1d)
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

    // Preflight: validate everything before making any changes
    let destroy_at = compute_destroy_at(duration)?;
    let (engine, binary_path) = detect_engine(&cwd)?;
    cron::check_crontab_available()?;

    let key = sanitize_path_to_key(&cwd);

    let lock = lock_registry()?;
    let mut registry = load_registry(&lock)?;
    registry.insert(
        key.clone(),
        RegistryEntry {
            path: cwd.clone(),
            destroy_at,
            engine,
            binary_path: binary_path.clone(),
            attempts: 0,
            last_attempt_at: None,
        },
    );
    save_registry(&lock, &registry)?;

    let marker_path = cwd.join(".killalltofu");
    fs::write(&marker_path, destroy_at.to_rfc3339())
        .with_context(|| "failed to write .killalltofu marker")?;

    cron::ensure_cron_installed()?;

    eprintln!(
        "registered: {key}\n  engine: {engine} ({binary_path})\n  destroy at: {destroy_at}",
    );
    Ok(())
}

fn cmd_list() -> Result<()> {
    let lock = lock_registry_shared()?;
    let registry = load_registry_shared(&lock)?;

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

    let lock = lock_registry()?;
    let mut registry = load_registry(&lock)?;
    if registry.remove(&key).is_none() {
        anyhow::bail!("no project registered for: {key}");
    }
    let is_empty = registry.is_empty();
    save_registry(&lock, &registry)?;

    let marker = cwd.join(".killalltofu");
    if marker.exists() {
        fs::remove_file(&marker).with_context(|| "failed to remove .killalltofu")?;
    }

    eprintln!("cancelled: {key}");

    if is_empty {
        cron::uninstall_cron_entry()?;
    }

    Ok(())
}

fn cmd_kill(name: Option<String>) -> Result<()> {
    let (key, entry) = {
        let lock = lock_registry_shared()?;
        let registry = load_registry_shared(&lock)?;

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

        (key, entry)
    };

    eprintln!(
        "destroying: {} (engine: {}, path: {})",
        key,
        entry.engine,
        entry.path.display()
    );

    let result = run_destroy_streaming(&entry.binary_path, &entry.path)?;

    if result.success {
        let lock = lock_registry()?;
        let mut registry = load_registry(&lock)?;
        registry.remove(&key);
        let is_empty = registry.is_empty();
        save_registry(&lock, &registry)?;
        remove_marker_file(&entry.path);
        eprintln!("destroyed: {key}");
        if is_empty {
            cron::uninstall_cron_entry()?;
        }
    } else {
        let code = result.exit_code.map_or("unknown".to_string(), |c| c.to_string());
        anyhow::bail!("destroy failed (exit code: {code})");
    }
    Ok(())
}

fn cmd_check() -> Result<()> {
    let due_entries: Vec<(String, RegistryEntry)> = {
        let lock = lock_registry_shared()?;
        let registry = load_registry_shared(&lock)?;
        let now = Utc::now();

        registry
            .iter()
            .filter(|(_, entry)| now >= entry.destroy_at)
            .filter(|(_, entry)| is_retry_eligible(entry.attempts, entry.last_attempt_at))
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect()
    };

    let results: Vec<(String, DestroyOutcome)> = due_entries
        .iter()
        .map(|(key, entry)| {
            let outcome = process_destroy(key, entry);
            (key.clone(), outcome)
        })
        .collect();

    let lock = lock_registry()?;
    let mut registry = load_registry(&lock)?;
    for (key, outcome) in results {
        match outcome {
            DestroyOutcome::Success => {
                registry.remove(&key);
            }
            DestroyOutcome::MaxRetries => {
                registry.remove(&key);
            }
            DestroyOutcome::Retry { attempts } => {
                if let Some(entry) = registry.get_mut(&key) {
                    entry.attempts = attempts;
                    entry.last_attempt_at = Some(Utc::now());
                }
            }
            DestroyOutcome::Error => {}
        }
    }
    save_registry(&lock, &registry)?;

    if registry.is_empty() {
        cron::uninstall_cron_entry()?;
    }

    Ok(())
}

enum DestroyOutcome {
    Success,
    MaxRetries,
    Retry { attempts: u32 },
    Error,
}

fn process_destroy(key: &str, entry: &RegistryEntry) -> DestroyOutcome {
    let attempt = entry.attempts + 1;
    eprintln!("attempt {attempt}/{MAX_ATTEMPTS} for {key}");

    let result = match run_destroy(&entry.binary_path, &entry.path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error processing {key}: {e:#}");
            return DestroyOutcome::Error;
        }
    };

    if let Err(e) = write_attempt_log(
        key,
        attempt,
        entry.engine,
        &entry.path,
        result.exit_code,
        &result.output,
    ) {
        eprintln!("failed to write log for {key}: {e:#}");
    }

    if result.success {
        remove_marker_file(&entry.path);
        eprintln!("destroyed: {key}");
        DestroyOutcome::Success
    } else if attempt >= MAX_ATTEMPTS {
        if let Err(e) =
            write_killallerror(&entry.path, key, entry.engine, attempt, &result.output)
        {
            eprintln!("failed to write .killallerror for {key}: {e:#}");
        }
        eprintln!("max retries reached for {key}, wrote .killallerror");
        DestroyOutcome::MaxRetries
    } else {
        eprintln!("attempt {attempt} failed for {key}, will retry");
        DestroyOutcome::Retry { attempts: attempt }
    }
}


fn remove_marker_file(project_path: &std::path::Path) {
    let marker = project_path.join(".killalltofu");
    if marker.exists() {
        fs::remove_file(&marker).ok();
    }
}
