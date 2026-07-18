use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use mapkeeper_core::projects::{projects_file_path, trash_dir_path, ProjectEntry, ProjectsFile};
use mapkeeper_core::spatial::{SpatialState, SPATIAL_STATE_RELATIVE};
use mapkeeper_core::world;
use serde::{Deserialize, Serialize};

use crate::atomic_io;

/// Process-local lock for app `projects.json` read-modify-write (N-025).
///
/// Canonical lock order — never invert:
/// 1. `PROJECTS_REGISTRY_LOCK` — shared registry mutations
/// 2. `ServerState.app` — active world pointer
/// 3. per-world lock — world content (spatial / stroke)
///
/// Never acquire (1) while holding (2) or (3).
static PROJECTS_REGISTRY_LOCK: Mutex<()> = Mutex::new(());

/// Marker written at Create start; absent after successful Create (N-025).
pub const CREATE_INCOMPLETE_MARKER: &str = ".mapkeeper-create-incomplete";

/// Disk facts for Create-marker reconciliation (pure classify input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDiskFacts {
    pub has_marker: bool,
    pub valid_manifest_id: Option<String>,
    pub valid_spatial: bool,
    pub has_foreign_entries: bool,
    /// Registry entry id for this path, if any.
    pub registry_id: Option<String>,
}

/// Create-marker reconciliation classes (N-025).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateMarkerClass {
    NoMarker,
    /// A — incomplete Create owned by marker; safe directory cleanup.
    SafeIncomplete,
    /// B — durable world + matching registry; clear marker, keep world.
    CompleteRegistered { world_id: String },
    /// C — durable world, no registry; keep world; repair or recover.
    CompleteUnregistered { world_id: String },
    /// D — partial/contradictory/foreign; never auto-delete.
    Ambiguous { reason: &'static str },
}

/// Pure classifier — fully unit-testable without filesystem.
pub fn classify_create_marker(facts: &CreateDiskFacts) -> CreateMarkerClass {
    if !facts.has_marker {
        return CreateMarkerClass::NoMarker;
    }
    let complete = facts.valid_manifest_id.is_some() && facts.valid_spatial;
    if complete {
        let world_id = facts
            .valid_manifest_id
            .clone()
            .expect("complete implies manifest id");
        return match &facts.registry_id {
            Some(reg_id) if reg_id == &world_id => {
                CreateMarkerClass::CompleteRegistered { world_id }
            }
            Some(_) => CreateMarkerClass::Ambiguous {
                reason: "registry_id_mismatch",
            },
            None => CreateMarkerClass::CompleteUnregistered { world_id },
        };
    }
    if facts.has_foreign_entries {
        return CreateMarkerClass::Ambiguous {
            reason: "foreign_entries",
        };
    }
    if facts.registry_id.is_some() {
        return CreateMarkerClass::Ambiguous {
            reason: "registry_without_complete_world",
        };
    }
    CreateMarkerClass::SafeIncomplete
}

pub fn inspect_create_disk(world_path: &Path, registry: &ProjectsFile) -> CreateDiskFacts {
    let marker = incomplete_marker_path(world_path);
    let has_marker = marker.is_file();
    let valid_manifest_id = read_manifest_id(world_path).ok();
    let spatial_path = world_path.join(SPATIAL_STATE_RELATIVE);
    let valid_spatial = match fs::read_to_string(&spatial_path) {
        Ok(raw) => {
            SpatialState::assert_no_screen_keys(&raw).is_ok() && SpatialState::from_json(&raw).is_ok()
        }
        Err(_) => false,
    };
    let has_foreign_entries = has_foreign_create_entries(world_path);
    let registry_id = find_registered(registry, world_path).map(|e| e.id.clone());
    CreateDiskFacts {
        has_marker,
        valid_manifest_id,
        valid_spatial,
        has_foreign_entries,
        registry_id,
    }
}

pub fn classify_create_marker_at(world_path: &Path, registry: &ProjectsFile) -> CreateMarkerClass {
    classify_create_marker(&inspect_create_disk(world_path, registry))
}

fn is_create_allowlisted_name(name: &str) -> bool {
    if name == CREATE_INCOMPLETE_MARKER || name == "mapkeeper.toml" || name == "mapkeeper.toml.bak" {
        return true;
    }
    if name == "spatial" {
        return true;
    }
    if name.starts_with(".mapkeeper.toml.tmp-") {
        return true;
    }
    false
}

/// Top-level entries outside the Create artifact allowlist count as foreign.
fn has_foreign_create_entries(world_path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(world_path) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return true;
        };
        if !is_create_allowlisted_name(name) {
            return true;
        }
    }
    false
}

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

/// Absolute + lexical `.` / `..` collapse. Used for Create/open display paths.
/// Does not require the target to exist (Create fallback).
pub fn normalize_world_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    lexical_clean(&absolute)
}

/// Collapse lexical `.` and `..` without touching the filesystem.
pub fn lexical_clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// Resolve path for lock/registry identity (N-025).
///
/// Policy:
/// 1. Lexical clean absolute path.
/// 2. If target exists → `canonicalize` (follows symlink/junction when OS allows).
/// 3. Else if parent exists → canonicalize parent + join final name (Create fallback).
/// 4. Else → lexical path only.
/// 5. Format: `/` separators; Windows ASCII-lowercase; strip `\\?\` verbatim prefix.
pub fn resolve_world_identity_path(path: &Path) -> PathBuf {
    let normalized = normalize_world_path(path);
    if normalized.exists() {
        return normalized
            .canonicalize()
            .unwrap_or_else(|_| normalized.clone());
    }
    if let (Some(parent), Some(name)) = (normalized.parent(), normalized.file_name()) {
        if !parent.as_os_str().is_empty() && parent.exists() {
            if let Ok(parent_canon) = parent.canonicalize() {
                return parent_canon.join(name);
            }
        }
    }
    normalized
}

fn format_identity_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = value.strip_prefix("//?/") {
        value = if let Some(unc) = rest.strip_prefix("UNC/") {
            format!("//{unc}")
        } else {
            rest.to_string()
        };
    }
    if cfg!(windows) {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

/// One physical world directory → one lock/registry comparison key.
pub fn path_cmp_key(path: &Path) -> String {
    format_identity_key(&resolve_world_identity_path(path))
}

fn app_paths() -> (Option<String>, Option<String>) {
    let appdata = std::env::var("APPDATA").ok();
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok());
    (appdata, home)
}

/// Serialize tests that mutate process APPDATA (shared across server test mods).
#[cfg(test)]
pub(crate) static APPDATA_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
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

fn registry_bak_available(path: &Path) -> bool {
    atomic_io::bak_passes(path, |bytes| {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|raw| ProjectsFile::parse(raw).ok())
            .is_some()
    })
}

/// Read-only snapshot. Does not hold the mutation lock across the caller;
/// `atomic_replace` prevents torn file content (no artificial mid-write view).
pub fn load_projects() -> Result<ProjectsFile, String> {
    load_projects_from(&projects_path())
}

pub fn load_projects_from(path: &Path) -> Result<ProjectsFile, String> {
    match atomic_io::classify_durable_open(path) {
        atomic_io::DurableOpenKind::AbsentClean => Ok(ProjectsFile::default()),
        atomic_io::DurableOpenKind::InterruptedWrite => Err(format!(
            "corrupt_registry: interrupted_write (bak_available={})",
            registry_bak_available(path)
        )),
        atomic_io::DurableOpenKind::PrimaryPresent => match fs::read_to_string(path) {
            Ok(raw) => match ProjectsFile::parse(&raw) {
                Ok(file) => Ok(file),
                Err(error) => Err(format!(
                    "{error} (bak_available={})",
                    registry_bak_available(path)
                )),
            },
            Err(error) => Err(format!(
                "corrupt_registry: cannot read {}: {error}",
                path.display()
            )),
        },
    }
}

/// Canonical registry RMW: holds [`PROJECTS_REGISTRY_LOCK`] from load through
/// successful atomic replace. Callback errors release the lock without writing.
pub fn mutate_projects<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&mut ProjectsFile) -> Result<T, String>,
{
    let _guard = PROJECTS_REGISTRY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = projects_path();
    let mut file = load_projects_from(&path)?;
    let out = f(&mut file)?;
    save_projects_to(&path, &file).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Full replace under the registry lock (prefer [`mutate_projects`] for RMW).
pub fn save_projects(file: &ProjectsFile) -> Result<()> {
    let _guard = PROJECTS_REGISTRY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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

/// Remove directory only for [`CreateMarkerClass::SafeIncomplete`].
/// Marker alone never authorizes `remove_dir_all`.
pub fn cleanup_incomplete_create(world_path: &Path) -> Result<(), String> {
    let registry = match load_projects() {
        Ok(file) => file,
        Err(_) => ProjectsFile::default(),
    };
    match classify_create_marker_at(world_path, &registry) {
        CreateMarkerClass::SafeIncomplete => {
            if world_path.exists() {
                fs::remove_dir_all(world_path)
                    .map_err(|e| format!("create_incomplete: cleanup failed: {e}"))?;
            }
            Ok(())
        }
        CreateMarkerClass::NoMarker => {
            Err("create_incomplete: refuse cleanup without marker".into())
        }
        other => Err(format!(
            "create_incomplete: refuse cleanup (state={other:?})"
        )),
    }
}

/// Best-effort fail cleanup: wipe only SafeIncomplete; never delete B/C/D.
pub fn cleanup_after_failed_create(world_path: &Path) -> Result<(), String> {
    let registry = match load_projects() {
        Ok(file) => file,
        Err(_) => ProjectsFile::default(),
    };
    match classify_create_marker_at(world_path, &registry) {
        CreateMarkerClass::SafeIncomplete => cleanup_incomplete_create(world_path),
        CreateMarkerClass::NoMarker => Ok(()),
        CreateMarkerClass::CompleteRegistered { .. }
        | CreateMarkerClass::CompleteUnregistered { .. }
        | CreateMarkerClass::Ambiguous { .. } => Ok(()),
    }
}

pub fn write_incomplete_marker(world_path: &Path) -> Result<(), String> {
    fs::write(incomplete_marker_path(world_path), b"create-in-progress\n")
        .map_err(|e| format!("create_incomplete: cannot write marker: {e}"))
}

pub fn clear_incomplete_marker(world_path: &Path) -> Result<(), String> {
    #[cfg(test)]
    if take_clear_marker_failpoint() {
        return Err("create_incomplete: clear marker: failpoint".into());
    }
    let marker = incomplete_marker_path(world_path);
    if marker.is_file() {
        fs::remove_file(&marker).map_err(|e| format!("create_incomplete: clear marker: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    static CLEAR_MARKER_FAIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn take_clear_marker_failpoint() -> bool {
    CLEAR_MARKER_FAIL.with(|cell| {
        if cell.get() {
            cell.set(false);
            true
        } else {
            false
        }
    })
}

/// One-shot failpoint: next `clear_incomplete_marker` errors (tests only).
#[cfg(test)]
pub fn set_clear_marker_failpoint() {
    CLEAR_MARKER_FAIL.with(|cell| cell.set(true));
}

#[cfg(test)]
pub fn clear_clear_marker_failpoint() {
    CLEAR_MARKER_FAIL.with(|cell| cell.set(false));
}

pub fn find_registered<'a>(
    file: &'a ProjectsFile,
    world_path: &Path,
) -> Option<&'a ProjectEntry> {
    let key = path_cmp_key(world_path);
    file.projects
        .iter()
        .find(|entry| path_cmp_key(Path::new(&entry.path)) == key)
}

/// Durable in-flight Delete record (app-managed; N-025 recoverable Delete).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteInflight {
    pub key: String,
    pub id: String,
    pub path: String,
}

fn delete_inflight_root() -> PathBuf {
    let (appdata, home) = app_paths();
    if let Some(appdata) = appdata.filter(|v| !v.is_empty()) {
        return PathBuf::from(appdata.trim_end_matches(['/', '\\'])).join("mapkeeper/delete-inflight");
    }
    let home = home.filter(|v| !v.is_empty()).unwrap_or_else(|| ".".into());
    PathBuf::from(home.trim_end_matches(['/', '\\'])).join(".config/mapkeeper/delete-inflight")
}

fn delete_inflight_file(key: &str) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    delete_inflight_root().join(format!("{:016x}.json", hasher.finish()))
}

pub fn write_delete_inflight(entry: &DeleteInflight) -> Result<(), String> {
    let path = delete_inflight_file(&entry.key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("delete_recovery: inflight dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(entry)
        .map_err(|e| format!("delete_recovery: serialize inflight: {e}"))?;
    atomic_io::atomic_replace(&path, raw.as_bytes())
        .map_err(|e| format!("delete_recovery: write inflight: {e}"))
}

pub fn clear_delete_inflight(key: &str) -> Result<(), String> {
    let path = delete_inflight_file(key);
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| format!("delete_recovery: clear inflight: {e}"))?;
    }
    Ok(())
}

pub fn load_delete_inflight(key: &str) -> Option<DeleteInflight> {
    let path = delete_inflight_file(key);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn list_delete_inflights() -> Vec<DeleteInflight> {
    let root = delete_inflight_root();
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(item) = serde_json::from_str::<DeleteInflight>(&raw) {
                out.push(item);
            }
        }
    }
    out
}

/// Restart/open reconciliation for interrupted Delete (N-025).
///
/// - World gone → ensure registry has no entry; clear inflight; report cleared.
/// - World still present → restore registry entry if missing; clear inflight
///   so author can retry Delete from a consistent state.
pub fn reconcile_delete_inflights() -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for inflight in list_delete_inflights() {
        let world_path = normalize_world_path(Path::new(&inflight.path));
        let key = path_cmp_key(&world_path);
        if world_path.exists() {
            mutate_projects(|file| {
                if find_registered(file, &world_path).is_none() {
                    file.upsert(ProjectEntry {
                        id: inflight.id.clone(),
                        path: inflight.path.clone(),
                    });
                    notes.push(format!(
                        "delete_recovery: restored registry for {}",
                        inflight.path
                    ));
                }
                Ok(())
            })?;
            clear_delete_inflight(&inflight.key)?;
            // Also clear by recomputed key if hash path differed.
            let _ = clear_delete_inflight(&key);
        } else {
            mutate_projects(|file| {
                file.projects
                    .retain(|item| path_cmp_key(Path::new(&item.path)) != key);
                Ok(())
            })?;
            clear_delete_inflight(&inflight.key)?;
            let _ = clear_delete_inflight(&key);
            notes.push(format!(
                "delete_recovery: cleared stale inflight for {}",
                inflight.path
            ));
        }
    }
    Ok(notes)
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
    #[cfg(test)]
    if take_move_trash_failpoint() {
        return Err("delete_rejected: move to trash: failpoint".into());
    }
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
thread_local! {
    static MOVE_TRASH_FAIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DELETE_RESTORE_FAIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn take_move_trash_failpoint() -> bool {
    MOVE_TRASH_FAIL.with(|cell| {
        if cell.get() {
            cell.set(false);
            true
        } else {
            false
        }
    })
}

#[cfg(test)]
pub fn set_move_trash_failpoint() {
    MOVE_TRASH_FAIL.with(|cell| cell.set(true));
}

#[cfg(test)]
pub fn take_delete_restore_failpoint() -> bool {
    DELETE_RESTORE_FAIL.with(|cell| {
        if cell.get() {
            cell.set(false);
            true
        } else {
            false
        }
    })
}

#[cfg(test)]
pub fn set_delete_restore_failpoint() {
    DELETE_RESTORE_FAIL.with(|cell| cell.set(true));
}

#[cfg(test)]
pub fn clear_delete_failpoints() {
    MOVE_TRASH_FAIL.with(|cell| cell.set(false));
    DELETE_RESTORE_FAIL.with(|cell| cell.set(false));
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
    fn lexical_aliases_share_identity_key() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("world");
        fs::create_dir_all(&world).unwrap();
        let a = path_cmp_key(&world);
        let b = path_cmp_key(&dir.path().join("./world"));
        let c = path_cmp_key(&dir.path().join("x").join("..").join("world"));
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn create_fallback_key_uses_parent_canonical() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("nest");
        fs::create_dir_all(&parent).unwrap();
        let missing = parent.join("new-world");
        assert!(!missing.exists());
        let a = path_cmp_key(&missing);
        let b = path_cmp_key(&parent.join(".").join("new-world"));
        assert_eq!(a, b);
        assert!(a.contains("new-world") || a.ends_with("new-world"));
    }

    #[test]
    fn symlink_or_junction_alias_shares_key_when_supported() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-world");
        fs::create_dir_all(&real).unwrap();
        let alias = dir.path().join("alias-world");
        let linked = create_dir_alias(&real, &alias);
        if !linked {
            return;
        }
        assert_eq!(path_cmp_key(&real), path_cmp_key(&alias));
    }

    fn create_dir_alias(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        {
            // Junction does not require elevated privileges.
            let status = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    &link.display().to_string(),
                    &target.display().to_string(),
                ])
                .status();
            return status.map(|s| s.success()).unwrap_or(false);
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (target, link);
            false
        }
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
    fn missing_registry_with_valid_bak_is_recovery_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        let bak = crate::atomic_io::bak_path(&path);
        let good = ProjectsFile {
            projects: vec![mapkeeper_core::projects::ProjectEntry {
                id: "w".into(),
                path: "/world".into(),
            }],
        };
        fs::write(&bak, good.to_json_pretty().unwrap()).unwrap();
        let err = load_projects_from(&path).unwrap_err();
        assert!(err.contains("interrupted_write"));
        assert!(err.contains("bak_available=true"));
        assert!(!path.is_file());
        assert!(bak.is_file());
    }

    #[test]
    fn missing_registry_with_invalid_bak_never_empty_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        let bak = crate::atomic_io::bak_path(&path);
        fs::write(&bak, "{broken").unwrap();
        let err = load_projects_from(&path).unwrap_err();
        assert!(err.contains("interrupted_write"));
        assert!(err.contains("bak_available=false"));
        assert!(!path.is_file());
    }

    #[test]
    fn registry_failpoint_after_bak_survives_restart() {
        crate::atomic_io::clear_failpoint();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        let old = ProjectsFile {
            projects: vec![mapkeeper_core::projects::ProjectEntry {
                id: "old".into(),
                path: "/old".into(),
            }],
        };
        save_projects_to(&path, &old).unwrap();
        let next = ProjectsFile {
            projects: vec![mapkeeper_core::projects::ProjectEntry {
                id: "new".into(),
                path: "/new".into(),
            }],
        };
        crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::AfterPrimaryToBak);
        assert!(save_projects_to(&path, &next).is_err());
        assert!(!path.is_file());
        assert!(crate::atomic_io::bak_path(&path).is_file());
        let err = load_projects_from(&path).unwrap_err();
        assert!(err.contains("interrupted_write"));
        assert!(err.contains("bak_available=true"));
    }

    #[test]
    fn registry_failpoint_final_rename_restores_primary() {
        crate::atomic_io::clear_failpoint();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        let old = ProjectsFile {
            projects: vec![mapkeeper_core::projects::ProjectEntry {
                id: "old".into(),
                path: "/old".into(),
            }],
        };
        save_projects_to(&path, &old).unwrap();
        let next = ProjectsFile {
            projects: vec![mapkeeper_core::projects::ProjectEntry {
                id: "new".into(),
                path: "/new".into(),
            }],
        };
        crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::FinalRename);
        assert!(save_projects_to(&path, &next).is_err());
        let loaded = load_projects_from(&path).unwrap();
        assert_eq!(loaded.projects[0].id, "old");
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
    fn classify_pure_states() {
        assert_eq!(
            classify_create_marker(&CreateDiskFacts {
                has_marker: false,
                valid_manifest_id: None,
                valid_spatial: false,
                has_foreign_entries: false,
                registry_id: None,
            }),
            CreateMarkerClass::NoMarker
        );
        assert_eq!(
            classify_create_marker(&CreateDiskFacts {
                has_marker: true,
                valid_manifest_id: None,
                valid_spatial: false,
                has_foreign_entries: false,
                registry_id: None,
            }),
            CreateMarkerClass::SafeIncomplete
        );
        assert_eq!(
            classify_create_marker(&CreateDiskFacts {
                has_marker: true,
                valid_manifest_id: Some("w".into()),
                valid_spatial: true,
                has_foreign_entries: false,
                registry_id: Some("w".into()),
            }),
            CreateMarkerClass::CompleteRegistered {
                world_id: "w".into()
            }
        );
        assert_eq!(
            classify_create_marker(&CreateDiskFacts {
                has_marker: true,
                valid_manifest_id: Some("w".into()),
                valid_spatial: true,
                has_foreign_entries: false,
                registry_id: None,
            }),
            CreateMarkerClass::CompleteUnregistered {
                world_id: "w".into()
            }
        );
        assert_eq!(
            classify_create_marker(&CreateDiskFacts {
                has_marker: true,
                valid_manifest_id: Some("w".into()),
                valid_spatial: false,
                has_foreign_entries: true,
                registry_id: None,
            }),
            CreateMarkerClass::Ambiguous {
                reason: "foreign_entries"
            }
        );
    }

    #[test]
    fn planted_marker_with_author_files_refuses_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("user-folder");
        fs::create_dir_all(&world).unwrap();
        write_incomplete_marker(&world).unwrap();
        fs::write(world.join("notes.txt"), "author lore").unwrap();
        let err = cleanup_incomplete_create(&world).unwrap_err();
        assert!(err.contains("refuse cleanup"));
        assert!(world.join("notes.txt").is_file());
        assert!(is_incomplete_create(&world));
    }

    #[test]
    fn complete_registered_with_marker_refuses_cleanup() {
        let _lock = lock_appdata_env();
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("APPDATA").ok();
        std::env::set_var("APPDATA", dir.path());
        let world = dir.path().join("worlds").join("done");
        fs::create_dir_all(world.join("spatial")).unwrap();
        fs::write(
            world.join("mapkeeper.toml"),
            world::manifest_toml("done"),
        )
        .unwrap();
        // Minimal valid spatial via ensure would need more; plant via core default.
        let state = mapkeeper_core::spatial::default_spatial_state();
        let mut state = state;
        state.revision = 1;
        fs::write(
            world.join("spatial/state.json"),
            state.to_json_pretty().unwrap(),
        )
        .unwrap();
        write_incomplete_marker(&world).unwrap();
        let mut file = ProjectsFile::default();
        file.upsert(mapkeeper_core::projects::ProjectEntry {
            id: "done".into(),
            path: world.display().to_string(),
        });
        save_projects(&file).unwrap();
        assert!(matches!(
            classify_create_marker_at(&world, &file),
            CreateMarkerClass::CompleteRegistered { .. }
        ));
        assert!(cleanup_incomplete_create(&world).is_err());
        assert!(world.join("mapkeeper.toml").is_file());
        match prev {
            Some(v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
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
