use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result};


pub struct DestroyResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}

pub fn run_destroy(binary_path: &str, project_path: &Path) -> Result<DestroyResult> {
    let args = destroy_args();

    let output: Output = Command::new(binary_path)
        .args(args)
        .current_dir(project_path)
        .output()
        .with_context(|| format!("failed to execute {binary_path} destroy in {}", project_path.display()))?;

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    Ok(DestroyResult {
        success: output.status.success(),
        exit_code: output.status.code(),
        output: combined,
    })
}

pub fn run_destroy_streaming(binary_path: &str, project_path: &Path) -> Result<DestroyResult> {
    let args = destroy_args();

    let status = Command::new(binary_path)
        .args(args)
        .current_dir(project_path)
        .status()
        .with_context(|| format!("failed to execute {binary_path} destroy in {}", project_path.display()))?;

    Ok(DestroyResult {
        success: status.success(),
        exit_code: status.code(),
        output: String::new(),
    })
}

fn destroy_args() -> Vec<&'static str> {
    vec!["destroy", "-auto-approve"]
}
