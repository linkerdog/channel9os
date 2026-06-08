use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use channel9_core::AppConfig;
pub use channel9_fs::{list_directory, FileEntry};

pub trait ConfigStore {
    fn load_or_create(&self) -> Result<AppConfig>;
    fn save(&self, config: &AppConfig) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct JsonConfigStore {
    root: PathBuf,
}

impl JsonConfigStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn path(&self) -> PathBuf {
        channel9_fs::config_path(&self.root)
    }
}

impl ConfigStore for JsonConfigStore {
    fn load_or_create(&self) -> Result<AppConfig> {
        let path = self.path();

        if !path.exists() {
            return Ok(AppConfig::default());
        }

        let data = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        serde_json::from_str(&data)
            .with_context(|| format!("failed to parse config {}", path.display()))
    }

    fn save(&self, config: &AppConfig) -> Result<()> {
        let path = self.path();
        channel9_fs::ensure_parent(&path)?;
        let data = serde_json::to_string_pretty(config)?;
        let mut file = channel9_fs::create_file(&path)
            .with_context(|| format!("failed to create config {}", path.display()))?;
        file.write_all(data.as_bytes())
            .with_context(|| format!("failed to write config {}", path.display()))?;
        file.flush()
            .with_context(|| format!("failed to flush config {}", path.display()))?;
        Ok(())
    }
}

pub fn littlefs2_probe() -> &'static str {
    let _ = littlefs2::DISK_VERSION;
    "littlefs2"
}
