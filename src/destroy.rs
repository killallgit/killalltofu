use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result};

use crate::registry::Engine;

pub struct DestroyResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}

pub fn run_destroy(engine: Engine, project_path: &Path) -> Result<DestroyResult> {
    let (cmd, args) = destroy_command(engine);

    let output: Output = Command::new(cmd)
        .args(args)
        .current_dir(project_path)
        .output()
        .with_context(|| format!("failed to execute {engine} destroy in {}", project_path.display()))?;

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

pub fn run_destroy_streaming(engine: Engine, project_path: &Path) -> Result<DestroyResult> {
    let (cmd, args) = destroy_command(engine);

    let status = Command::new(cmd)
        .args(args)
        .current_dir(project_path)
        .status()
        .with_context(|| format!("failed to execute {engine} destroy in {}", project_path.display()))?;

    Ok(DestroyResult {
        success: status.success(),
        exit_code: status.code(),
        output: String::new(),
    })
}

fn destroy_command(engine: Engine) -> (&'static str, Vec<&'static str>) {
    match engine {
        Engine::Tofu => ("tofu", vec!["destroy", "-auto-approve"]),
        Engine::Terragrunt => ("terragrunt", vec!["destroy", "-auto-approve"]),
    }
}
