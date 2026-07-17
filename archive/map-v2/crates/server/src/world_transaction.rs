//! Multi-file world mutations: stage → validate → commit with error rollback
//! and crash recovery via txn manifest (agent-reliability transactional-io).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::world_io::invalidate_hydrology_snapshot;

pub(crate) const STAGING_ROOT: &str = ".mapkeeper-staging";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TxnStatus {
    Staging,
    Committing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TxnAction {
    Write,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TxnTarget {
    rel_path: String,
    backup_name: Option<String>,
    staged_name: Option<String>,
    action: TxnAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TxnManifest {
    txn_id: String,
    status: TxnStatus,
    targets: Vec<TxnTarget>,
    committed_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PostCommitOp {
    InvalidateHydrologySnapshot,
}

impl PostCommitOp {
    fn label(&self) -> &'static str {
        match self {
            Self::InvalidateHydrologySnapshot => "invalidate_hydrology_snapshot",
        }
    }
}

/// Planned multi-file mutation for one world folder.
pub(crate) struct WorldMutationPlan {
    world_path: PathBuf,
    txn_dir: PathBuf,
    txn_id: String,
    targets: Vec<TxnTargetEntry>,
    post_commit: Vec<PostCommitOp>,
}

struct TxnTargetEntry {
    active: PathBuf,
    rel_path: String,
    backup_name: Option<String>,
    staged_name: Option<String>,
    action: TxnAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EffectiveFile {
    Absent,
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommitReport {
    pub txn_id: String,
    pub files_written: usize,
    pub files_deleted: usize,
    pub post_commit: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryReport {
    pub txn_id: String,
    pub action: RecoveryAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitError {
    Revision(crate::world_revision::RevisionError),
    Op(String),
}

impl From<String> for CommitError {
    fn from(s: String) -> Self {
        Self::Op(s)
    }
}

impl CommitError {
    pub(crate) fn into_revision_response(self) -> axum::response::Response {
        use axum::response::IntoResponse;
        match self {
            Self::Revision(err) => crate::world_revision::revision_error_response(err),
            Self::Op(msg) => {
                crate::op_log::note_op_error(&msg);
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    msg,
                )
                    .into_response()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecoveryAction {
    RemovedStagingOnly,
    RolledBackFromCommitting,
}

impl WorldMutationPlan {
    /// Begin a new txn: backups + staged bytes only; active paths untouched.
    pub(crate) fn begin(world_path: &Path) -> Result<Self, String> {
        recover_orphan_transactions(world_path)?;
        let txn_id = format!(
            "{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            std::process::id()
        );
        let txn_dir = world_path.join(STAGING_ROOT).join(&txn_id);
        std::fs::create_dir_all(txn_dir.join("staged")).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(txn_dir.join("backups")).map_err(|e| e.to_string())?;
        Ok(Self {
            world_path: world_path.to_path_buf(),
            txn_dir,
            txn_id,
            targets: Vec::new(),
            post_commit: Vec::new(),
        })
    }

    pub(crate) fn stage_write(&mut self, active: &Path, bytes: Vec<u8>) -> Result<(), String> {
        let rel_path = rel_path(&self.world_path, active)?;
        let idx = self.targets.len();
        let backup_name = format!("b{idx}");
        let staged_name = format!("s{idx}");
        let backup_path = self.txn_dir.join("backups").join(&backup_name);
        if active.exists() {
            let prior = std::fs::read(active).map_err(|e| e.to_string())?;
            std::fs::write(&backup_path, prior).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            self.txn_dir.join("staged").join(&staged_name),
            &bytes,
        )
        .map_err(|e| e.to_string())?;
        self.targets.push(TxnTargetEntry {
            active: active.to_path_buf(),
            rel_path,
            backup_name: Some(backup_name),
            staged_name: Some(staged_name),
            action: TxnAction::Write,
        });
        Ok(())
    }

    pub(crate) fn stage_delete(&mut self, active: &Path) -> Result<(), String> {
        let rel_path = rel_path(&self.world_path, active)?;
        let idx = self.targets.len();
        let backup_name = if active.exists() {
            let name = format!("b{idx}");
            std::fs::copy(
                active,
                self.txn_dir.join("backups").join(&name),
            )
            .map_err(|e| e.to_string())?;
            Some(name)
        } else {
            None
        };
        self.targets.push(TxnTargetEntry {
            active: active.to_path_buf(),
            rel_path,
            backup_name,
            staged_name: None,
            action: TxnAction::Delete,
        });
        Ok(())
    }

    pub(crate) fn post_commit_invalidate_hydrology(&mut self) {
        self.post_commit.push(PostCommitOp::InvalidateHydrologySnapshot);
    }

    pub(crate) fn will_invalidate_hydrology(&self) -> bool {
        self.post_commit
            .iter()
            .any(|op| matches!(op, PostCommitOp::InvalidateHydrologySnapshot))
    }

    pub(crate) fn staged_target_count(&self) -> usize {
        self.targets.len()
    }

    /// Effective on-disk bytes for a world file after applying staged writes/deletes.
    pub(crate) fn effective_file(&self, active: &Path) -> Result<EffectiveFile, String> {
        let rel = rel_path(&self.world_path, active)?;
        if let Some(target) = self.targets.iter().find(|t| t.rel_path == rel) {
            return match target.action {
                TxnAction::Delete => Ok(EffectiveFile::Absent),
                TxnAction::Write => {
                    let staged_name = target
                        .staged_name
                        .as_ref()
                        .ok_or("write target missing staged payload")?;
                    let bytes = std::fs::read(
                        self.txn_dir.join("staged").join(staged_name),
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(EffectiveFile::Bytes(bytes))
                }
            };
        }
        if active.exists() {
            Ok(EffectiveFile::Bytes(
                std::fs::read(active).map_err(|e| e.to_string())?,
            ))
        } else {
            Ok(EffectiveFile::Absent)
        }
    }

    pub(crate) fn staged_active_paths(&self) -> impl Iterator<Item = &Path> {
        self.targets.iter().map(|t| t.active.as_path())
    }

    /// Optional validation of staged payloads before touching active files.
    pub(crate) fn validate_staged<F>(&self, check: F) -> Result<(), String>
    where
        F: FnOnce(&Self) -> Result<(), String>,
    {
        check(self)
    }

    pub(crate) fn commit(mut self, base_revision: Option<u64>) -> Result<(CommitReport, u64), CommitError> {
        crate::world_revision::require_base_revision(&self.world_path, base_revision)
            .map_err(CommitError::Revision)?;
        self.write_manifest(TxnStatus::Staging, 0)?;
        if let Err(err) = crate::integrity::pre_commit_check(&self.world_path, Some(&self)) {
            let _ = self.rollback_active_from_backups();
            let _ = std::fs::remove_dir_all(&self.txn_dir);
            crate::op_log::note_op_error(&err);
            return Err(CommitError::Op(err));
        }
        let result = self.commit_inner();
        if let Err(err) = &result {
            let _ = self.rollback_active_from_backups();
            let _ = std::fs::remove_dir_all(&self.txn_dir);
            crate::op_log::note_op_error(err);
            return Err(CommitError::Op(err.clone()));
        }
        let report = result.unwrap();
        let revision = crate::world_revision::bump_world_revision(&self.world_path)
            .map_err(|e| {
                crate::op_log::note_op_error(&e);
                CommitError::Op(e)
            })?;
        crate::op_log::note_commit_success(&report, revision);
        Ok((report, revision))
    }

    /// Legacy test/internal path without revision gate (uses bootstrap revision 0 only).
    #[cfg(test)]
    pub(crate) fn commit_unchecked(mut self) -> Result<CommitReport, String> {
        self.write_manifest(TxnStatus::Staging, 0)?;
        crate::integrity::pre_commit_check(&self.world_path, Some(&self))?;
        let result = self.commit_inner();
        if result.is_err() {
            let _ = self.rollback_active_from_backups();
            let _ = std::fs::remove_dir_all(&self.txn_dir);
        }
        result
    }

    fn commit_inner(&mut self) -> Result<CommitReport, String> {
        self.write_manifest(TxnStatus::Committing, 0)?;
        let mut files_written = 0usize;
        let mut files_deleted = 0usize;
        for (index, target) in self.targets.iter().enumerate() {
            match target.action {
                TxnAction::Write => {
                    let staged_name = target
                        .staged_name
                        .as_ref()
                        .ok_or("write target missing staged payload")?;
                    let staged = self
                        .txn_dir
                        .join("staged")
                        .join(staged_name);
                    let bytes = std::fs::read(&staged).map_err(|e| e.to_string())?;
                    if is_layer_json_path(&target.active) {
                        crate::world_io::maybe_simulate_layer_write_failure()?;
                    }
                    if let Some(parent) = target.active.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&target.active, &bytes).map_err(|e| e.to_string())?;
                    files_written += 1;
                }
                TxnAction::Delete => {
                    if target.active.exists() {
                        std::fs::remove_file(&target.active).map_err(|e| e.to_string())?;
                        files_deleted += 1;
                    }
                }
            }
            self.write_manifest(TxnStatus::Committing, index + 1)?;
        }
        let mut post_commit = Vec::new();
        for op in &self.post_commit {
            apply_post_commit(&self.world_path, op)?;
            post_commit.push(op.label().to_string());
        }
        std::fs::remove_dir_all(&self.txn_dir).map_err(|e| e.to_string())?;
        remove_empty_staging_root(&self.world_path)?;
        Ok(CommitReport {
            txn_id: self.txn_id.clone(),
            files_written,
            files_deleted,
            post_commit,
        })
    }

    fn write_manifest(&self, status: TxnStatus, committed_count: usize) -> Result<(), String> {
        let manifest = TxnManifest {
            txn_id: self.txn_id.clone(),
            status,
            targets: self
                .targets
                .iter()
                .map(|t| TxnTarget {
                    rel_path: t.rel_path.clone(),
                    backup_name: t.backup_name.clone(),
                    staged_name: t.staged_name.clone(),
                    action: t.action.clone(),
                })
                .collect(),
            committed_count,
        };
        let body = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        std::fs::write(self.txn_dir.join("txn.json"), body).map_err(|e| e.to_string())
    }

    fn rollback_active_from_backups(&self) -> Result<(), String> {
        for target in self.targets.iter().rev() {
            restore_target(&self.world_path, &self.txn_dir, target)?;
        }
        Ok(())
    }
}

impl Drop for WorldMutationPlan {
    fn drop(&mut self) {
        if self.txn_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.txn_dir);
        }
    }
}

fn apply_post_commit(world_path: &Path, op: &PostCommitOp) -> Result<(), String> {
    match op {
        PostCommitOp::InvalidateHydrologySnapshot => invalidate_hydrology_snapshot(world_path),
    }
}

fn restore_target(world_path: &Path, txn_dir: &Path, target: &TxnTargetEntry) -> Result<(), String> {
    let active = world_path.join(&target.rel_path);
    match target.backup_name.as_ref() {
        Some(name) => {
            let bytes = std::fs::read(txn_dir.join("backups").join(name))
                .map_err(|e| e.to_string())?;
            if let Some(parent) = active.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&active, bytes).map_err(|e| e.to_string())?;
        }
        None => {
            if active.exists() {
                std::fs::remove_file(&active).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

fn rel_path(world_path: &Path, active: &Path) -> Result<String, String> {
    active
        .strip_prefix(world_path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|_| format!("path outside world: {}", active.display()))
}

fn remove_empty_staging_root(world_path: &Path) -> Result<(), String> {
    let root = world_path.join(STAGING_ROOT);
    if !root.exists() {
        return Ok(());
    }
    if std::fs::read_dir(&root).map_err(|e| e.to_string())?.next().is_none() {
        std::fs::remove_dir(&root).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn is_layer_json_path(path: &Path) -> bool {
    path.to_string_lossy().contains("/layers/")
        || path.to_string_lossy().contains("\\layers\\")
}

/// Recover or discard orphan txns under one world folder.
pub(crate) fn recover_orphan_transactions(world_path: &Path) -> Result<Vec<RecoveryReport>, String> {
    let staging_root = world_path.join(STAGING_ROOT);
    if !staging_root.exists() {
        return Ok(Vec::new());
    }
    let mut reports = Vec::new();
    let entries: Vec<PathBuf> = std::fs::read_dir(&staging_root)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    for txn_dir in entries {
        let manifest_path = txn_dir.join("txn.json");
        if !manifest_path.exists() {
            std::fs::remove_dir_all(&txn_dir).map_err(|e| e.to_string())?;
            reports.push(RecoveryReport {
                txn_id: txn_dir
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                action: RecoveryAction::RemovedStagingOnly,
            });
            continue;
        }
        let raw = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
        let manifest: TxnManifest = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        match manifest.status {
            TxnStatus::Staging => {
                std::fs::remove_dir_all(&txn_dir).map_err(|e| e.to_string())?;
                reports.push(RecoveryReport {
                    txn_id: manifest.txn_id,
                    action: RecoveryAction::RemovedStagingOnly,
                });
            }
            TxnStatus::Committing => {
                for target in manifest.targets.iter().rev() {
                    let entry = TxnTargetEntry {
                        active: world_path.join(&target.rel_path),
                        rel_path: target.rel_path.clone(),
                        backup_name: target.backup_name.clone(),
                        staged_name: target.staged_name.clone(),
                        action: target.action.clone(),
                    };
                    restore_target(world_path, &txn_dir, &entry)?;
                }
                std::fs::remove_dir_all(&txn_dir).map_err(|e| e.to_string())?;
                reports.push(RecoveryReport {
                    txn_id: manifest.txn_id,
                    action: RecoveryAction::RolledBackFromCommitting,
                });
            }
        }
    }
    Ok(reports)
}

/// Startup hygiene: recover orphan txns for every registered world.
pub(crate) fn recover_all_registered_worlds() -> Result<(), String> {
    let file = crate::world_io::load_projects();
    for entry in file.projects {
        recover_orphan_transactions(Path::new(&entry.path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_io::layer_file_path;
    use mapkeeper_core::hex::MapBounds;
    use mapkeeper_core::hydro::ELEVATION_LAYER_ID;
    use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue, MapManifest};
    use tempfile::tempdir;

    fn valid_elevation_bytes(bounds: &MapBounds, fill: i32) -> Vec<u8> {
        let mut layer = DenseLayer::new_integer(ELEVATION_LAYER_ID, bounds.len());
        for i in 0..layer.len() {
            layer.set(i, DenseState::Value(LayerValue::Int(fill)));
        }
        layer.to_json_pretty().unwrap().into_bytes()
    }

    fn seed_world(path: &Path, world_id: &str) -> MapBounds {
        let bounds = MapBounds::new(14, 8);
        std::fs::create_dir_all(path.join("map/layers")).unwrap();
        std::fs::write(
            path.join("mapkeeper.toml"),
            mapkeeper_core::build_state::manifest_toml_with_build(world_id, false),
        )
        .unwrap();
        let manifest = MapManifest::default_v0(14, 8);
        std::fs::write(
            path.join("map/manifest.json"),
            manifest.to_json_pretty().unwrap(),
        )
        .unwrap();
        bounds
    }

    #[test]
    fn commit_writes_all_staged_files() {
        let _lock = crate::world_io::failpoint_lock();
        crate::world_io::SIMULATE_LAYER_WRITE_FAILURE
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let dir = tempdir().unwrap();
        let world = dir.path();
        seed_world(world, "txn-ok");
        let bounds = MapBounds::new(14, 8);
        let target = world.join("map/layers/elevation.json");
        let mut plan = WorldMutationPlan::begin(world).unwrap();
        plan.stage_write(&target, valid_elevation_bytes(&bounds, 10))
            .unwrap();
        let report = plan.commit_unchecked().unwrap();
        assert_eq!(report.files_written, 1);
        assert!(std::fs::read_to_string(&target).unwrap().contains("elevation"));
        assert!(!world.join(STAGING_ROOT).exists());
    }

    #[test]
    fn error_rollback_restores_prior_bytes() {
        let _lock = crate::world_io::failpoint_lock();
        let dir = tempdir().unwrap();
        let world = dir.path();
        seed_world(world, "txn-rb");
        let bounds = MapBounds::new(14, 8);
        let layer = layer_file_path(world, "elevation");
        std::fs::write(&layer, valid_elevation_bytes(&bounds, 1)).unwrap();
        let mut plan = WorldMutationPlan::begin(world).unwrap();
        plan.stage_write(&layer, valid_elevation_bytes(&bounds, 2))
            .unwrap();
        crate::world_io::SIMULATE_LAYER_WRITE_FAILURE
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let err = plan.commit_unchecked().unwrap_err();
        crate::world_io::SIMULATE_LAYER_WRITE_FAILURE
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(err.contains("simulated"));
        assert_eq!(
            std::fs::read(&layer).unwrap(),
            valid_elevation_bytes(&bounds, 1)
        );
    }

    #[test]
    fn orphan_committing_txn_rolls_back_on_recovery() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        seed_world(world, "txn-rec");
        let layer = layer_file_path(world, "elevation");
        std::fs::write(&layer, b"stable").unwrap();
        let txn_dir = world.join(STAGING_ROOT).join("orphan-1");
        std::fs::create_dir_all(txn_dir.join("backups")).unwrap();
        std::fs::write(txn_dir.join("backups/b0"), b"stable").unwrap();
        std::fs::write(&layer, b"partial").unwrap();
        let manifest = TxnManifest {
            txn_id: "orphan-1".to_string(),
            status: TxnStatus::Committing,
            targets: vec![TxnTarget {
                rel_path: "map/layers/elevation.json".to_string(),
                backup_name: Some("b0".to_string()),
                staged_name: Some("s0".to_string()),
                action: TxnAction::Write,
            }],
            committed_count: 1,
        };
        std::fs::write(
            txn_dir.join("txn.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let reports = recover_orphan_transactions(world).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(
            reports[0].action,
            RecoveryAction::RolledBackFromCommitting
        );
        assert_eq!(std::fs::read(&layer).unwrap(), b"stable");
        assert!(!txn_dir.exists());
    }

    #[test]
    fn orphan_staging_txn_removed_without_touching_active() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        seed_world(world, "txn-stg");
        let layer = layer_file_path(world, "elevation");
        std::fs::write(&layer, b"stable").unwrap();
        let txn_dir = world.join(STAGING_ROOT).join("orphan-2");
        std::fs::create_dir_all(txn_dir.join("staged")).unwrap();
        let manifest = TxnManifest {
            txn_id: "orphan-2".to_string(),
            status: TxnStatus::Staging,
            targets: vec![TxnTarget {
                rel_path: "map/layers/elevation.json".to_string(),
                backup_name: Some("b0".to_string()),
                staged_name: Some("s0".to_string()),
                action: TxnAction::Write,
            }],
            committed_count: 0,
        };
        std::fs::write(
            txn_dir.join("txn.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let reports = recover_orphan_transactions(world).unwrap();
        assert_eq!(reports[0].action, RecoveryAction::RemovedStagingOnly);
        assert_eq!(std::fs::read(&layer).unwrap(), b"stable");
    }
}
