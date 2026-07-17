//! Atomic file replace for world mutation contour (N-025).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Same-dir temp → fsync → replace target. One-generation `*.bak` when target existed.
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
        file.write_all(bytes)
            .with_context(|| format!("write temp {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("fsync temp {}", tmp.display()))?;
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

    if let Err(error) = fs::rename(&tmp, path) {
        // Windows: target may still exist after failed rename-to-bak.
        if path.is_file() {
            fs::remove_file(path)
                .with_context(|| format!("remove target {}", path.display()))?;
            fs::rename(&tmp, path)
                .with_context(|| format!("rename temp→target {}", path.display()))?;
        } else {
            let _ = fs::remove_file(&tmp);
            return Err(error).with_context(|| format!("rename temp→target {}", path.display()));
        }
    }
    Ok(())
}

pub fn bak_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".bak");
    PathBuf::from(os)
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
}
