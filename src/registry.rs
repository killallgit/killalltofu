use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub path: PathBuf,
    pub destroy_at: DateTime<Utc>,
    pub engine: Engine,
    pub binary_path: String,
    pub attempts: u32,
    pub last_attempt_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Tofu,
    Terraform,
    Terragrunt,
}

impl std::fmt::Display for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Engine::Tofu => write!(f, "tofu"),
            Engine::Terraform => write!(f, "terraform"),
            Engine::Terragrunt => write!(f, "terragrunt"),
        }
    }
}

pub type Registry = BTreeMap<String, RegistryEntry>;

fn home_dir() -> Result<PathBuf> {
    home::home_dir().with_context(|| "could not determine home directory (is HOME set?)")
}

pub fn killalltofu_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".killalltofu"))
}

pub fn registry_path() -> Result<PathBuf> {
    Ok(killalltofu_dir()?.join("registry.json"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(killalltofu_dir()?.join("logs"))
}

fn lock_path() -> Result<PathBuf> {
    Ok(killalltofu_dir()?.join("registry.lock"))
}

pub struct RegistryLock {
    _lock_file: File,
}

pub struct RegistryReadLock {
    _lock_file: File,
}

fn open_lock_file() -> Result<File> {
    let dir = killalltofu_dir()?;
    fs::create_dir_all(&dir).with_context(|| "failed to create killalltofu directory")?;
    File::create(lock_path()?).with_context(|| "failed to create registry lock file")
}

pub fn lock_registry() -> Result<RegistryLock> {
    let lock_file = open_lock_file()?;
    lock_file
        .lock_exclusive()
        .with_context(|| "failed to acquire registry lock")?;
    Ok(RegistryLock {
        _lock_file: lock_file,
    })
}

pub fn lock_registry_shared() -> Result<RegistryReadLock> {
    let lock_file = open_lock_file()?;
    lock_file
        .lock_shared()
        .with_context(|| "failed to acquire registry read lock")?;
    Ok(RegistryReadLock {
        _lock_file: lock_file,
    })
}

fn read_registry() -> Result<Registry> {
    let path = registry_path()?;
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let content = fs::read_to_string(&path).with_context(|| "failed to read registry")?;
    serde_json::from_str(&content).with_context(|| "failed to parse registry")
}

pub fn load_registry(_lock: &RegistryLock) -> Result<Registry> {
    read_registry()
}

pub fn load_registry_shared(_lock: &RegistryReadLock) -> Result<Registry> {
    read_registry()
}

pub fn save_registry(_lock: &RegistryLock, registry: &Registry) -> Result<()> {
    let content =
        serde_json::to_string_pretty(registry).with_context(|| "failed to serialize registry")?;
    let path = registry_path()?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, content).with_context(|| "failed to write temporary registry file")?;
    fs::rename(&tmp_path, &path).with_context(|| "failed to atomically replace registry")?;
    Ok(())
}

pub fn sanitize_path_to_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches('/')
        .replace(['/', '\\'], "-")
        .to_lowercase()
}

fn find_binary(name: &str) -> Option<String> {
    which::which(name)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

pub fn detect_engine(project_path: &Path) -> Result<(Engine, String)> {
    if project_path.join("terragrunt.hcl").exists() {
        let path = find_binary("terragrunt")
            .with_context(|| "terragrunt.hcl found but terragrunt is not in PATH")?;
        return Ok((Engine::Terragrunt, path));
    }

    if let Some(path) = find_binary("tofu") {
        return Ok((Engine::Tofu, path));
    }

    if let Some(path) = find_binary("terraform") {
        return Ok((Engine::Terraform, path));
    }

    anyhow::bail!("neither tofu nor terraform found in PATH")
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
    fn test_detect_engine_tofu_or_terraform() {
        let tmp = tempfile::tempdir().unwrap();
        let (engine, binary_path) = detect_engine(tmp.path()).unwrap();
        assert!(engine == Engine::Tofu || engine == Engine::Terraform);
        assert!(!binary_path.is_empty());
    }

    #[test]
    fn test_detect_engine_terragrunt() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("terragrunt.hcl"), "").unwrap();
        match detect_engine(tmp.path()) {
            Ok((engine, path)) => {
                assert_eq!(engine, Engine::Terragrunt);
                assert!(!path.is_empty());
            }
            Err(_) => {
                // terragrunt not installed — that's fine for this test
            }
        }
    }

    #[test]
    fn test_engine_display() {
        assert_eq!(Engine::Tofu.to_string(), "tofu");
        assert_eq!(Engine::Terraform.to_string(), "terraform");
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
                binary_path: "/opt/homebrew/bin/tofu".to_string(),
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
