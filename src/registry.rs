use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub path: PathBuf,
    pub destroy_at: DateTime<Utc>,
    pub engine: Engine,
    pub attempts: u32,
    pub last_attempt_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Tofu,
    Terragrunt,
}

impl std::fmt::Display for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Engine::Tofu => write!(f, "tofu"),
            Engine::Terragrunt => write!(f, "terragrunt"),
        }
    }
}

pub type Registry = BTreeMap<String, RegistryEntry>;

pub fn killalltofu_dir() -> PathBuf {
    dirs_home().join(".killalltofu")
}

fn dirs_home() -> PathBuf {
    home::home_dir().expect("could not determine home directory")
}

pub fn registry_path() -> PathBuf {
    killalltofu_dir().join("registry.json")
}

pub fn logs_dir() -> PathBuf {
    killalltofu_dir().join("logs")
}

pub fn load_registry() -> Result<Registry> {
    let path = registry_path();
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let content = fs::read_to_string(&path).with_context(|| "failed to read registry")?;
    serde_json::from_str(&content).with_context(|| "failed to parse registry")
}

pub fn save_registry(registry: &Registry) -> Result<()> {
    let dir = killalltofu_dir();
    fs::create_dir_all(&dir).with_context(|| "failed to create killalltofu directory")?;
    let content =
        serde_json::to_string_pretty(registry).with_context(|| "failed to serialize registry")?;
    fs::write(registry_path(), content).with_context(|| "failed to write registry")?;
    Ok(())
}

pub fn sanitize_path_to_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches('/')
        .replace(['/', '\\'], "-")
        .to_lowercase()
}

pub fn detect_engine(project_path: &Path) -> Engine {
    if project_path.join("terragrunt.hcl").exists() {
        Engine::Terragrunt
    } else {
        Engine::Tofu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_unix_path() {
        let key = sanitize_path_to_key(Path::new("/Users/ryan/Code/my-infra"));
        assert_eq!(key, "users-ryan-code-my-infra");
    }

    #[test]
    fn test_sanitize_strips_leading_slash() {
        let key = sanitize_path_to_key(Path::new("/home/user/project"));
        assert_eq!(key, "home-user-project");
    }

    #[test]
    fn test_detect_engine_tofu_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(detect_engine(tmp.path()), Engine::Tofu);
    }

    #[test]
    fn test_detect_engine_terragrunt() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("terragrunt.hcl"), "").unwrap();
        assert_eq!(detect_engine(tmp.path()), Engine::Terragrunt);
    }

    #[test]
    fn test_engine_display() {
        assert_eq!(Engine::Tofu.to_string(), "tofu");
        assert_eq!(Engine::Terragrunt.to_string(), "terragrunt");
    }

    #[test]
    fn test_registry_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let reg_path = tmp.path().join("registry.json");

        let mut registry = Registry::new();
        registry.insert(
            "test-key".to_string(),
            RegistryEntry {
                path: PathBuf::from("/tmp/test"),
                destroy_at: Utc::now(),
                engine: Engine::Tofu,
                attempts: 0,
                last_attempt_at: None,
            },
        );

        let content = serde_json::to_string_pretty(&registry).unwrap();
        fs::write(&reg_path, &content).unwrap();

        let loaded: Registry =
            serde_json::from_str(&fs::read_to_string(&reg_path).unwrap()).unwrap();
        assert!(loaded.contains_key("test-key"));
        assert_eq!(loaded["test-key"].engine, Engine::Tofu);
    }
}
