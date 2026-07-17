use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use mapkeeper_core::projects::{projects_file_path, trash_dir_path, ProjectsFile};
use mapkeeper_core::world;

use crate::atomic_io;

/// Marker written at Create start; absent after successful Create (N-025).
pub const CREATE_INCOMPLETE_MARKER: &str = ".mapkeeper-create-incomplete";

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

fn app_paths() -> (Option<String>, Option<String>) {
    let appdata = std::env::var("APPDATA").ok();
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok());
    (appdata, home)
}

/// Serialize tests that mutate process APPDATA (shared across server test mods).
pub(crate) static APPDATA_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn lock_appdata_env() -> std::sync::MutexGuard<'static, ()> {
    APPDATA_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn projects_path() -> PathBuf {
    let (appdata, home) = app_paths();
    PathBuf::from(projects_file_path(
        appdata.as_deref(),
        home.as_deref(),
    ))
}

pub fn trash_root() -> PathBuf {
    let (appdata, home) = app_paths();
    PathBuf::from(trash_dir_path(appdata.as_deref(), home.as_deref()))
}

/// Missing file → empty registry. Present but malformed → error (never silent empty).
pub fn load_projects() -> Result<ProjectsFile, String> {
    load_projects_from(&projects_path())
}

pub fn load_projects_from(path: &Path) -> Result<ProjectsFile, String> {
    match fs::read_to_string(path) {
        Ok(raw) => ProjectsFile::parse(&raw),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(ProjectsFile::default()),
        Err(error) => Err(format!(
            "corrupt_registry: cannot read {}: {error}",
            path.display()
        )),
    }
}

pub fn save_projects(file: &ProjectsFile) -> Result<()> {
    save_projects_to(&projects_path(), file)
}

pub fn save_projects_to(path: &Path, file: &ProjectsFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = file
        .to_json_pretty()
        .map_err(anyhow::Error::msg)?;
    atomic_io::atomic_replace(path, raw.as_bytes())
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

pub fn incomplete_marker_path(world_path: &Path) -> PathBuf {
    world_path.join(CREATE_INCOMPLETE_MARKER)
}

pub fn is_incomplete_create(world_path: &Path) -> bool {
    incomplete_marker_path(world_path).is_file()
}

/// Cleanup only when our Create marker is present — never arbitrary user dirs.
pub fn cleanup_incomplete_create(world_path: &Path) -> Result<(), String> {
    if !is_incomplete_create(world_path) {
        return Err("create_incomplete: refuse cleanup without marker".into());
    }
    if world_path.exists() {
        fs::remove_dir_all(world_path).map_err(|e| format!("create_incomplete: cleanup failed: {e}"))?;
    }
    Ok(())
}

pub fn write_incomplete_marker(world_path: &Path) -> Result<(), String> {
    fs::write(incomplete_marker_path(world_path), b"create-in-progress\n")
        .map_err(|e| format!("create_incomplete: cannot write marker: {e}"))
}

pub fn clear_incomplete_marker(world_path: &Path) -> Result<(), String> {
    let marker = incomplete_marker_path(world_path);
    if marker.is_file() {
        fs::remove_file(&marker).map_err(|e| format!("create_incomplete: clear marker: {e}"))?;
    }
    Ok(())
}

pub fn find_registered<'a>(
    file: &'a ProjectsFile,
    world_path: &Path,
) -> Option<&'a mapkeeper_core::projects::ProjectEntry> {
    let key = path_cmp_key(world_path);
    file.projects
        .iter()
        .find(|entry| path_cmp_key(Path::new(&entry.path)) == key)
}

/// Collision-safe trash destination under app-managed trash root.
pub fn allocate_trash_dir(world_id: &str) -> Result<PathBuf, String> {
    let root = trash_root();
    fs::create_dir_all(&root).map_err(|e| format!("delete_rejected: trash root: {e}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let safe_id: String = world_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    for n in 0..10_000u32 {
        let name = if n == 0 {
            format!("{stamp}-{safe_id}")
        } else {
            format!("{stamp}-{safe_id}-{n}")
        };
        let dest = root.join(name);
        if !dest.exists() {
            return Ok(dest);
        }
    }
    Err("delete_rejected: trash name collision exhausted".into())
}

/// Move world directory into trash; write origin note. Not a permanent purge.
pub fn move_world_to_trash(world_path: &Path, world_id: &str) -> Result<PathBuf, String> {
    if !world_path.is_dir() {
        return Err("delete_rejected: world path is not a directory".into());
    }
    let dest = allocate_trash_dir(world_id)?;
    fs::rename(world_path, &dest).map_err(|e| format!("delete_rejected: move to trash: {e}"))?;
    let note = format!(
        "original_path={}\nworld_id={}\n",
        world_path.display(),
        world_id
    );
    let _ = fs::write(dest.join("mapkeeper-trash-origin.txt"), note);
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AppDataGuard {
        prev: Option<String>,
    }

    impl AppDataGuard {
        fn set(path: &Path) -> Self {
            let prev = std::env::var("APPDATA").ok();
            std::env::set_var("APPDATA", path);
            Self { prev }
        }
    }

    impl Drop for AppDataGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("APPDATA", v),
                None => std::env::remove_var("APPDATA"),
            }
        }
    }

    #[test]
    fn path_key_normalizes_separators() {
        assert!(!path_cmp_key(Path::new("world")).contains('\\'));
    }

    #[test]
    fn missing_registry_is_empty_not_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        let file = load_projects_from(&path).unwrap();
        assert!(file.projects.is_empty());
    }

    #[test]
    fn malformed_registry_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        fs::write(&path, "{broken").unwrap();
        let err = load_projects_from(&path).unwrap_err();
        assert!(err.starts_with("corrupt_registry:"));
    }

    #[test]
    fn cleanup_refuses_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("user-notes");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("notes.txt"), "keep").unwrap();
        assert!(cleanup_incomplete_create(&world).is_err());
        assert!(world.join("notes.txt").is_file());
    }

    #[test]
    fn cleanup_removes_marked_incomplete() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("partial");
        fs::create_dir_all(&world).unwrap();
        write_incomplete_marker(&world).unwrap();
        fs::write(world.join("mapkeeper.toml"), "x").unwrap();
        cleanup_incomplete_create(&world).unwrap();
        assert!(!world.exists());
    }

    #[test]
    fn trash_collision_safe_names() {
        let _lock = lock_appdata_env();
        let dir = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(dir.path());
        let a = allocate_trash_dir("wid").unwrap();
        fs::create_dir_all(&a).unwrap();
        let b = allocate_trash_dir("wid").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn move_to_trash_leaves_origin_note() {
        let _lock = lock_appdata_env();
        let dir = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(dir.path());
        let world = dir.path().join("worlds").join("w1");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("mapkeeper.toml"), "ok").unwrap();
        let trash = move_world_to_trash(&world, "w1").unwrap();
        assert!(!world.exists());
        assert!(trash.join("mapkeeper.toml").is_file());
        assert!(trash.join("mapkeeper-trash-origin.txt").is_file());
    }
}
