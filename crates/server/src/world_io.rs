use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mapkeeper_core::projects::{projects_file_path, ProjectsFile};
use mapkeeper_core::world;

pub fn read_manifest_id(world_path: &Path) -> Result<String> {
    let path = world_path.join("mapkeeper.toml");
    let raw =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let manifest =
        world::parse_manifest(&raw).with_context(|| format!("invalid {}", path.display()))?;
    if !world::is_valid_world_id(&manifest.world.id) {
        anyhow::bail!("invalid world id in {}", path.display());
    }
    Ok(manifest.world.id)
}

pub fn normalize_world_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

pub fn path_cmp_key(path: &Path) -> String {
    let normalized = normalize_world_path(path);
    let value = normalized.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

pub fn projects_path() -> PathBuf {
    PathBuf::from(projects_file_path(
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("HOME")
            .ok()
            .or_else(|| std::env::var("USERPROFILE").ok())
            .as_deref(),
    ))
}

pub fn load_projects() -> ProjectsFile {
    fs::read_to_string(projects_path())
        .map(|raw| ProjectsFile::parse(&raw))
        .unwrap_or_default()
}

pub fn save_projects(file: &ProjectsFile) -> Result<()> {
    let path = projects_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, file.to_json_pretty())
        .with_context(|| format!("cannot write {}", path.display()))
}

pub fn default_worlds_root_path() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join("Documents")
        .join("MAPKEEPER Worlds")
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_key_normalizes_separators() {
        assert!(!path_cmp_key(Path::new("world")).contains('\\'));
    }
}
