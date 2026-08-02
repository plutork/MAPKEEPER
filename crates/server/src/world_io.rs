use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use mapkeeper_core::projects::{projects_file_path, trash_dir_path, ProjectEntry, ProjectsFile};
use mapkeeper_core::spatial::{SpatialState, SPATIAL_STATE_RELATIVE};
use mapkeeper_core::world;

use crate::atomic_io;

mod delete_recovery;
mod registry_recovery;

pub use delete_recovery::*;
pub use registry_recovery::*;

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
    CompleteRegistered {
        world_id: String,
    },
    /// C — durable world, no registry; keep world; repair or recover.
    CompleteUnregistered {
        world_id: String,
    },
    /// D — partial/contradictory/foreign; never auto-delete.
    Ambiguous {
        reason: &'static str,
    },
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
    // N-035: spatial lives under maps/<id>/, not world root.
    let spatial_path = crate::world_layout::read_world_manifest(world_path)
        .ok()
        .and_then(|m| m.maps.first().cloned())
        .map(|map_ref| world_path.join(map_ref.path).join(SPATIAL_STATE_RELATIVE))
        .unwrap_or_else(|| world_path.join(SPATIAL_STATE_RELATIVE));
    let valid_spatial = match fs::read_to_string(&spatial_path) {
        Ok(raw) => {
            SpatialState::assert_no_screen_keys(&raw).is_ok()
                && SpatialState::from_json(&raw).is_ok()
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
    if name == CREATE_INCOMPLETE_MARKER || name == "mapkeeper.toml" || name == "mapkeeper.toml.bak"
    {
        return true;
    }
    // `maps/` = N-035 layout; `spatial/` kept for incomplete/legacy cleanup classify.
    if name == "maps" || name == "spatial" {
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
    PathBuf::from(projects_file_path(appdata.as_deref(), home.as_deref()))
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
#[cfg_attr(not(test), allow(dead_code))]
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
    let raw = file.to_json_pretty().map_err(anyhow::Error::msg)?;
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

#[cfg_attr(not(test), allow(dead_code))]
pub fn is_incomplete_create(world_path: &Path) -> bool {
    incomplete_marker_path(world_path).is_file()
}

/// Remove directory only for [`CreateMarkerClass::SafeIncomplete`].
/// Marker alone never authorizes `remove_dir_all`.
///
/// Caller must supply a **validated** registry snapshot — never invent an empty
/// registry on corrupt/interrupted read (N-025).
pub fn cleanup_incomplete_create(world_path: &Path, registry: &ProjectsFile) -> Result<(), String> {
    match classify_create_marker_at(world_path, registry) {
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

/// Fail cleanup: wipe only SafeIncomplete; never delete B/C/D.
/// Requires a validated registry — corrupt registry must refuse cleanup.
pub fn cleanup_after_failed_create(
    world_path: &Path,
    registry: &ProjectsFile,
) -> Result<(), String> {
    match classify_create_marker_at(world_path, registry) {
        CreateMarkerClass::SafeIncomplete => cleanup_incomplete_create(world_path, registry),
        CreateMarkerClass::NoMarker => Ok(()),
        CreateMarkerClass::CompleteRegistered { .. }
        | CreateMarkerClass::CompleteUnregistered { .. }
        | CreateMarkerClass::Ambiguous { .. } => Ok(()),
    }
}

/// Load registry then cleanup; on corrupt/interrupted registry refuse delete.
pub fn cleanup_after_failed_create_checked(world_path: &Path) -> Result<(), String> {
    let registry = load_projects()
        .map_err(|e| format!("create_incomplete: refuse cleanup on unreadable registry ({e})"))?;
    cleanup_after_failed_create(world_path, &registry)
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

pub fn find_registered<'a>(file: &'a ProjectsFile, world_path: &Path) -> Option<&'a ProjectEntry> {
    let key = path_cmp_key(world_path);
    file.projects
        .iter()
        .find(|entry| path_cmp_key(Path::new(&entry.path)) == key)
}

/// Registry insert/replace keyed by `path_cmp_key` — same key lookups use.
/// Raw string compare would let `C:\w` and `C:/w` register the same world twice.
pub fn upsert_registered(file: &mut ProjectsFile, entry: ProjectEntry) {
    let key = path_cmp_key(Path::new(&entry.path));
    match file
        .projects
        .iter_mut()
        .find(|item| path_cmp_key(Path::new(&item.path)) == key)
    {
        Some(existing) => *existing = entry,
        None => file.projects.push(entry),
    }
}

#[cfg(test)]
mod tests;
