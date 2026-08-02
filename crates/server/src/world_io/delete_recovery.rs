//! Delete recovery: inflight records, restart reconcile, trash (N-025 / N-031).

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use mapkeeper_core::projects::ProjectEntry;
use serde::{Deserialize, Serialize};

use super::{
    app_paths, find_registered, mutate_projects, normalize_world_path, path_cmp_key, trash_root,
    upsert_registered,
};
use crate::atomic_io;

/// Durable in-flight Delete record (app-managed; N-025 recoverable Delete).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteInflight {
    pub key: String,
    pub id: String,
    pub path: String,
}

pub fn delete_inflight_root() -> PathBuf {
    let (appdata, home) = app_paths();
    if let Some(appdata) = appdata.filter(|v| !v.is_empty()) {
        return PathBuf::from(appdata.trim_end_matches(['/', '\\']))
            .join("mapkeeper/delete-inflight");
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

#[cfg_attr(not(test), allow(dead_code))]
pub fn load_delete_inflight(key: &str) -> Option<DeleteInflight> {
    let path = delete_inflight_file(key);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Valid inflight records plus per-file diagnostics (N-025 — no silent skip).
#[derive(Debug, Default)]
pub struct DeleteInflightList {
    pub records: Vec<DeleteInflight>,
    pub diagnostics: Vec<String>,
}

pub fn list_delete_inflights() -> Result<DeleteInflightList, String> {
    let root = delete_inflight_root();
    if !root.exists() {
        return Ok(DeleteInflightList::default());
    }
    let entries = fs::read_dir(&root).map_err(|e| {
        format!(
            "delete_recovery: cannot read inflight dir {}: {e}",
            root.display()
        )
    })?;
    let mut out = DeleteInflightList::default();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                out.diagnostics.push(format!(
                    "delete_recovery: inflight dir entry unreadable: {e}"
                ));
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let label = path.display().to_string();
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) => {
                out.diagnostics.push(format!(
                    "delete_recovery: inflight unreadable ({label}): {e}"
                ));
                continue;
            }
        };
        match serde_json::from_str::<DeleteInflight>(&raw) {
            Ok(item) => {
                if item.key.is_empty() || item.id.is_empty() || item.path.is_empty() {
                    out.diagnostics.push(format!(
                        "delete_recovery: inflight invalid fields ({label})"
                    ));
                } else {
                    out.records.push(item);
                }
            }
            Err(e) => {
                out.diagnostics.push(format!(
                    "delete_recovery: inflight malformed JSON ({label}): {e}"
                ));
            }
        }
    }
    Ok(out)
}

/// Restart reconciliation for interrupted Delete (N-025).
///
/// - World gone → ensure registry has no entry; clear inflight; report cleared.
/// - World still present → restore registry entry if missing; clear inflight
///   so author can retry Delete from a consistent state.
/// - Corrupt/unreadable inflight files → **error** (never silent skip / auto-delete).
pub fn reconcile_delete_inflights() -> Result<Vec<String>, String> {
    let list = list_delete_inflights()?;
    if !list.diagnostics.is_empty() {
        return Err(list.diagnostics.join("; "));
    }
    let mut notes = Vec::new();
    for inflight in list.records {
        let world_path = normalize_world_path(Path::new(&inflight.path));
        let key = path_cmp_key(&world_path);
        if world_path.exists() {
            mutate_projects(|file| {
                if find_registered(file, &world_path).is_none() {
                    upsert_registered(
                        file,
                        ProjectEntry {
                            id: inflight.id.clone(),
                            path: inflight.path.clone(),
                        },
                    );
                    notes.push(format!(
                        "delete_recovery: restored registry for {}",
                        inflight.path
                    ));
                }
                Ok(())
            })?;
            clear_delete_inflight(&inflight.key)?;
            if inflight.key != key {
                clear_delete_inflight(&key)?;
            }
        } else {
            mutate_projects(|file| {
                file.projects
                    .retain(|item| path_cmp_key(Path::new(&item.path)) != key);
                Ok(())
            })?;
            clear_delete_inflight(&inflight.key)?;
            if inflight.key != key {
                clear_delete_inflight(&key)?;
            }
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
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
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
