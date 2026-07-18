//! Atomic file replace for world mutation contour (N-025).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Same-dir temp → fsync → replace target. One-generation `*.bak` when target existed.
///
/// On failed final rename after primary→bak: restore previous primary from bak
/// before returning (copy, so bak remains). Crash mid-replace may leave missing
/// primary + bak/temp; open must treat that as recovery, never silent defaults.
pub fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create_dir_all {}", parent.display()))?;

    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("atomic_replace: missing file name"))?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        file_name.to_string_lossy(),
        std::process::id()
    ));

    {
        let mut file = File::create(&tmp)
            .with_context(|| format!("create temp {}", tmp.display()))?;

        #[cfg(test)]
        if take_failpoint(AtomicFailAt::AfterTempCreate) {
            // Empty/partial temp; primary untouched.
            return Err(anyhow::anyhow!(
                "atomic_replace: failpoint AfterTempCreate"
            ));
        }

        file.write_all(bytes)
            .with_context(|| format!("write temp {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("fsync temp {}", tmp.display()))?;
    }

    #[cfg(test)]
    if take_failpoint(AtomicFailAt::AfterTempFsync) {
        // Complete temp; primary still intact.
        return Err(anyhow::anyhow!(
            "atomic_replace: failpoint AfterTempFsync"
        ));
    }

    let bak = bak_path(path);
    if path.is_file() {
        let _ = fs::remove_file(&bak);
        // Prefer rename so bak is the previous inode; fall back to copy.
        if fs::rename(path, &bak).is_err() {
            fs::copy(path, &bak)
                .with_context(|| format!("backup copy {}", path.display()))?;
            fs::remove_file(path)
                .with_context(|| format!("remove before replace {}", path.display()))?;
        }
    }

    #[cfg(test)]
    if take_failpoint(AtomicFailAt::AfterPrimaryToBak) {
        // Crash after bak rotate: primary missing, bak+temp present (no restore).
        return Err(anyhow::anyhow!(
            "atomic_replace: failpoint AfterPrimaryToBak"
        ));
    }

    #[cfg(test)]
    let force_rename_fail = take_failpoint(AtomicFailAt::FinalRename);

    let rename_result = {
        #[cfg(test)]
        if force_rename_fail {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "atomic_replace: failpoint FinalRename",
            ))
        } else {
            fs::rename(&tmp, path)
        }
        #[cfg(not(test))]
        {
            fs::rename(&tmp, path)
        }
    };

    if let Err(error) = rename_result {
        // Windows: target may still exist after failed rename-to-bak.
        if path.is_file() {
            fs::remove_file(path)
                .with_context(|| format!("remove target {}", path.display()))?;
            fs::rename(&tmp, path)
                .with_context(|| format!("rename temp→target {}", path.display()))?;
            return Ok(());
        }

        // Primary is gone (rotated to bak). Restore last-good before error return.
        if bak.is_file() {
            match fs::copy(&bak, path) {
                Ok(_) => {
                    let _ = fs::remove_file(&tmp);
                    return Err(error).with_context(|| {
                        format!(
                            "rename temp→target {}; previous primary restored from bak",
                            path.display()
                        )
                    });
                }
                Err(restore_err) => {
                    // Leave bak + temp as recoverable interrupted-write state.
                    return Err(error).with_context(|| {
                        format!(
                            "rename temp→target {}; restore from bak failed ({restore_err}); \
                             interrupted_write left on disk",
                            path.display()
                        )
                    });
                }
            }
        }

        // First write (no bak): keep temp as interrupted evidence; do not invent primary.
        return Err(error).with_context(|| {
            format!(
                "rename temp→target {}; interrupted_write (no bak)",
                path.display()
            )
        });
    }
    Ok(())
}

pub fn bak_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".bak");
    PathBuf::from(os)
}

/// Leftover same-dir temps from a crashed/failed replace: `.{name}.tmp-*`.
pub fn leftover_temp_paths(path: &Path) -> Vec<PathBuf> {
    let Some(file_name) = path.file_name() else {
        return Vec::new();
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let prefix = format!(".{}.tmp-", file_name.to_string_lossy());
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(&prefix) && entry.path().is_file() {
            out.push(entry.path());
        }
    }
    out
}

pub fn has_leftover_temp(path: &Path) -> bool {
    !leftover_temp_paths(path).is_empty()
}

/// Open/init classification for a durable contour file (N-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableOpenKind {
    /// Primary present — caller parses.
    PrimaryPresent,
    /// No primary, no bak, no leftover temp → true first init.
    AbsentClean,
    /// Missing primary with bak and/or leftover temp → recovery, never defaults.
    InterruptedWrite,
}

pub fn classify_durable_open(path: &Path) -> DurableOpenKind {
    if path.is_file() {
        return DurableOpenKind::PrimaryPresent;
    }
    if bak_path(path).is_file() || has_leftover_temp(path) {
        return DurableOpenKind::InterruptedWrite;
    }
    DurableOpenKind::AbsentClean
}

/// Whether `*.bak` exists and `validate` accepts its bytes.
pub fn bak_passes<F>(path: &Path, mut validate: F) -> bool
where
    F: FnMut(&[u8]) -> bool,
{
    let bak = bak_path(path);
    match fs::read(&bak) {
        Ok(bytes) => validate(&bytes),
        Err(_) => false,
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicFailAt {
    /// Temp created, not fully written; primary untouched.
    AfterTempCreate,
    /// Temp fsync done; primary untouched.
    AfterTempFsync,
    /// Primary moved to bak; final rename not attempted (crash).
    AfterPrimaryToBak,
    /// Final rename fails; restore-from-bak policy runs.
    FinalRename,
}

#[cfg(test)]
thread_local! {
    static FAILPOINT: std::cell::Cell<Option<AtomicFailAt>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn take_failpoint(at: AtomicFailAt) -> bool {
    FAILPOINT.with(|cell| {
        if cell.get() == Some(at) {
            cell.set(None);
            true
        } else {
            false
        }
    })
}

/// Install a one-shot failpoint for the current thread (tests only).
#[cfg(test)]
pub fn set_failpoint(at: AtomicFailAt) {
    FAILPOINT.with(|cell| cell.set(Some(at)));
}

/// Clears any pending failpoint (tests only).
#[cfg(test)]
pub fn clear_failpoint() {
    FAILPOINT.with(|cell| cell.set(None));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn replace_keeps_bak_and_final_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, b"old").unwrap();
        atomic_replace(&path, b"new-content").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new-content");
        assert_eq!(fs::read(bak_path(&path)).unwrap(), b"old");
    }

    #[test]
    fn create_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("state.json");
        atomic_replace(&path, b"first").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert!(!bak_path(&path).is_file());
    }

    #[test]
    fn failpoint_after_temp_create_leaves_primary() {
        clear_failpoint();
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, b"old").unwrap();
        set_failpoint(AtomicFailAt::AfterTempCreate);
        assert!(atomic_replace(&path, b"new").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert!(!bak_path(&path).is_file());
        let temps = leftover_temp_paths(&path);
        assert_eq!(temps.len(), 1);
        assert!(fs::metadata(&temps[0]).unwrap().len() == 0);
        assert_eq!(
            classify_durable_open(&path),
            DurableOpenKind::PrimaryPresent
        );
    }

    #[test]
    fn failpoint_after_temp_fsync_leaves_primary() {
        clear_failpoint();
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, b"old").unwrap();
        set_failpoint(AtomicFailAt::AfterTempFsync);
        assert!(atomic_replace(&path, b"new-bytes").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert!(!bak_path(&path).is_file());
        let temps = leftover_temp_paths(&path);
        assert_eq!(temps.len(), 1);
        assert_eq!(fs::read(&temps[0]).unwrap(), b"new-bytes");
    }

    #[test]
    fn failpoint_after_primary_to_bak_leaves_interrupted() {
        clear_failpoint();
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, b"old").unwrap();
        set_failpoint(AtomicFailAt::AfterPrimaryToBak);
        assert!(atomic_replace(&path, b"new").is_err());
        assert!(!path.is_file());
        assert_eq!(fs::read(bak_path(&path)).unwrap(), b"old");
        assert_eq!(leftover_temp_paths(&path).len(), 1);
        assert_eq!(
            classify_durable_open(&path),
            DurableOpenKind::InterruptedWrite
        );
    }

    #[test]
    fn failpoint_final_rename_restores_primary() {
        clear_failpoint();
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, b"old").unwrap();
        set_failpoint(AtomicFailAt::FinalRename);
        let err = atomic_replace(&path, b"new").unwrap_err().to_string();
        assert!(err.contains("previous primary restored") || err.contains("FinalRename"));
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(fs::read(bak_path(&path)).unwrap(), b"old");
        assert!(leftover_temp_paths(&path).is_empty());
        assert_eq!(
            classify_durable_open(&path),
            DurableOpenKind::PrimaryPresent
        );
    }

    #[test]
    fn classify_absent_clean_vs_interrupted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        assert_eq!(classify_durable_open(&path), DurableOpenKind::AbsentClean);
        fs::write(bak_path(&path), b"bak").unwrap();
        assert_eq!(
            classify_durable_open(&path),
            DurableOpenKind::InterruptedWrite
        );
    }
}
