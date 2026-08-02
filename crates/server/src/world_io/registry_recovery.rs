//! Registry recovery: explicit restore of `projects.json` from its last good
//! `.bak` (N-025 corruption policy · N-031 file contour).

use std::fs;
use std::path::Path;

use mapkeeper_core::projects::ProjectsFile;

use super::{projects_path, save_projects_to, PROJECTS_REGISTRY_LOCK};
use crate::atomic_io;

pub fn restore_projects_from_bak() -> Result<ProjectsFile, String> {
    let _guard = PROJECTS_REGISTRY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    restore_projects_from_bak_at(&projects_path())
}

/// Quarantine the unreadable primary, then reinstate the validated backup.
/// Never destroys the corrupt file — it is renamed for diagnosis.
pub fn restore_projects_from_bak_at(path: &Path) -> Result<ProjectsFile, String> {
    let bak = atomic_io::bak_path(path);
    if !bak.is_file() {
        return Err("corrupt_registry: no bak available".to_string());
    }
    let raw = fs::read_to_string(&bak)
        .map_err(|error| format!("corrupt_registry: cannot read bak: {error}"))?;
    let restored = ProjectsFile::parse(&raw)
        .map_err(|error| format!("corrupt_registry: invalid bak: {error}"))?;

    if path.is_file() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let quarantine = path.with_file_name(format!("projects.json.corrupt-{stamp}"));
        fs::rename(path, &quarantine)
            .or_else(|_| {
                fs::copy(path, &quarantine)?;
                fs::remove_file(path)?;
                Ok::<(), std::io::Error>(())
            })
            .map_err(|error| format!("corrupt_registry: cannot quarantine primary: {error}"))?;
    }

    save_projects_to(path, &restored).map_err(|error| error.to_string())?;
    Ok(restored)
}
