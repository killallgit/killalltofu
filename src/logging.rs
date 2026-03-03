use std::fs;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::registry::{logs_dir, Engine};

pub fn write_attempt_log(
    key: &str,
    attempt: u32,
    engine: Engine,
    path: &std::path::Path,
    exit_code: Option<i32>,
    output: &str,
) -> Result<()> {
    let dir = logs_dir();
    fs::create_dir_all(&dir).with_context(|| "failed to create logs directory")?;

    let filename = format!("{key}-attempt-{attempt}.log");
    let log_path = dir.join(filename);

    let content = format!(
        "Engine: {engine}\n\
         Path: {path}\n\
         Timestamp: {timestamp}\n\
         Exit code: {exit_code}\n\
         --- OUTPUT ---\n\
         {output}",
        path = path.display(),
        timestamp = Utc::now().to_rfc3339(),
        exit_code = exit_code.map_or("unknown".to_string(), |c| c.to_string()),
    );

    fs::write(&log_path, content).with_context(|| format!("failed to write log: {}", log_path.display()))?;
    Ok(())
}

pub fn write_killallerror(
    project_path: &std::path::Path,
    key: &str,
    engine: Engine,
    attempts: u32,
    output: &str,
) -> Result<()> {
    let content = format!(
        "Engine: {engine}\n\
         Attempts: {attempts}\n\
         Last attempt: {timestamp}\n\
         See logs: ~/.killalltofu/logs/{key}-attempt-*.log\n\
         --- LAST OUTPUT ---\n\
         {output}",
        timestamp = Utc::now().to_rfc3339(),
    );

    let error_path = project_path.join(".killallerror");
    fs::write(&error_path, content)
        .with_context(|| format!("failed to write .killallerror: {}", error_path.display()))?;
    Ok(())
}
