use std::process::Command;

use anyhow::{Context, Result};

const CRON_MARKER: &str = "killalltofu check";

pub fn check_crontab_available() -> Result<()> {
    let output = match Command::new("crontab").arg("-l").output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("crontab not found. killalltofu requires cron to schedule destruction")
        }
        Err(e) => {
            anyhow::bail!("crontab not accessible: {e}")
        }
    };

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("no crontab for") {
        return Ok(());
    }

    anyhow::bail!("crontab not usable: {}", stderr.trim())
}

pub fn ensure_cron_installed() -> Result<()> {
    if is_cron_installed()? {
        return Ok(());
    }
    install_cron_entry()
}

fn is_cron_installed() -> Result<bool> {
    let output = Command::new("crontab")
        .arg("-l")
        .output()
        .with_context(|| "failed to read crontab")?;

    if !output.status.success() {
        return Ok(false);
    }

    let existing = String::from_utf8_lossy(&output.stdout);
    Ok(existing.contains(CRON_MARKER))
}

fn killalltofu_binary_path() -> Result<String> {
    let path = std::env::current_exe()
        .with_context(|| "failed to determine killalltofu binary path")?
        .to_string_lossy()
        .to_string();

    if path.contains("/target/debug/") || path.contains("/target/release/") {
        eprintln!(
            "warning: binary path looks like a build artifact: {path}\n  \
             cron will break if the build directory is cleaned.\n  \
             consider installing with: cargo install --path . or brew install killalltofu"
        );
    }

    Ok(path)
}

pub fn uninstall_cron_entry() -> Result<()> {
    let output = Command::new("crontab")
        .arg("-l")
        .output()
        .with_context(|| "failed to read crontab")?;

    if !output.status.success() {
        return Ok(());
    }

    let existing = String::from_utf8_lossy(&output.stdout);
    let filtered: Vec<&str> = existing
        .lines()
        .filter(|line| !line.contains(CRON_MARKER))
        .collect();

    let new_crontab = if filtered.is_empty() {
        String::new()
    } else {
        format!("{}\n", filtered.join("\n"))
    };

    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "failed to spawn crontab")?;

    use std::io::Write;
    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(new_crontab.as_bytes())
        .with_context(|| "failed to write to crontab")?;

    let status = child.wait().with_context(|| "failed to wait for crontab")?;
    if !status.success() {
        anyhow::bail!("crontab command failed");
    }

    eprintln!("removed cron entry (no projects remaining)");
    Ok(())
}

fn install_cron_entry() -> Result<()> {
    let binary = killalltofu_binary_path()?;
    let cron_line = format!("* * * * * {binary} check");

    let output = Command::new("crontab")
        .arg("-l")
        .output()
        .with_context(|| "failed to read existing crontab")?;

    let existing = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        String::new()
    };

    let new_crontab = if existing.is_empty() {
        format!("{cron_line}\n")
    } else {
        format!("{existing}{cron_line}\n")
    };

    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "failed to spawn crontab")?;

    use std::io::Write;
    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(new_crontab.as_bytes())
        .with_context(|| "failed to write to crontab")?;

    let status = child.wait().with_context(|| "failed to wait for crontab")?;
    if !status.success() {
        anyhow::bail!("crontab command failed");
    }

    eprintln!("installed cron entry: {cron_line}");
    Ok(())
}
