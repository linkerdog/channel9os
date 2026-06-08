use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const APP_DIR: &str = "channel9";
pub const CONFIG_FILE: &str = "config.json";
pub const RECORDINGS_DIR: &str = "recordings";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub operation: bool,
    pub size_bytes: u64,
}

pub fn app_dir(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join(APP_DIR)
}

pub fn config_path(root: impl AsRef<Path>) -> PathBuf {
    app_dir(root).join(CONFIG_FILE)
}

pub fn recordings_dir(root: impl AsRef<Path>) -> PathBuf {
    app_dir(root).join(RECORDINGS_DIR)
}

pub fn ensure_dir(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))
}

pub fn ensure_parent(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .with_context(|| format!("path does not have a parent directory {}", path.display()))?;
    ensure_dir(parent)
}

pub fn create_file(path: impl AsRef<Path>) -> Result<fs::File> {
    let path = path.as_ref();
    fs::File::create(path).with_context(|| format!("failed to create file {}", path.display()))
}

pub fn open_file(path: impl AsRef<Path>) -> Result<fs::File> {
    let path = path.as_ref();
    fs::File::open(path).with_context(|| format!("failed to open file {}", path.display()))
}

pub fn list_directory(path: impl AsRef<Path>, limit: usize) -> Result<Vec<FileEntry>> {
    let path = path.as_ref();
    let mut entries = Vec::new();

    for entry in fs::read_dir(path)
        .with_context(|| format!("failed to read directory {}", path.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", path.display()))?;
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to stat {}", entry.path().display()))?;
        entries.push(FileEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: metadata.is_dir(),
            operation: false,
            size_bytes: metadata.len(),
        });

        if entries.len() >= limit {
            break;
        }
    }

    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.cmp(&right.name))
    });
    entries.push(FileEntry {
        name: "> Back".to_owned(),
        is_dir: false,
        operation: true,
        size_bytes: 0,
    });

    Ok(entries)
}

pub fn next_recording_path(root: impl AsRef<Path>) -> Result<PathBuf> {
    let dir = recordings_dir(root);
    for index in 1..=999 {
        let path = dir.join(format!("rec_{index:03}.wav"));
        if !path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!("recording directory is full");
}
