//! Coarse world revision — optimistic concurrency (agent-reliability map-revision).

use std::path::Path;

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use mapkeeper_core::layer::MapManifest;
use serde::{Deserialize, Serialize};

use crate::world_io::map_manifest_path;

pub const WORLD_BASE_REVISION_HEADER: &str = "X-World-Base-Revision";
pub const WORLD_RESULT_REVISION_HEADER: &str = "X-World-Result-Revision";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionError {
    Mismatch { current_revision: u64 },
    PreconditionRequired { current_revision: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevisionConflictBody {
    pub current_revision: u64,
    pub conflict_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(dead_code)]
pub struct MutationRevisionBody {
    pub result_revision: u64,
}

/// Read authoritative revision from `map/manifest.json` (missing => 0).
pub(crate) fn read_world_revision(world_path: &Path) -> Result<u64, String> {
    let path = map_manifest_path(world_path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(0);
    };
    let manifest = MapManifest::from_json(&raw).map_err(|e| e.to_string())?;
    Ok(manifest.revision)
}

/// Compare `base_revision` under caller-held world write guard.
pub(crate) fn require_base_revision(
    world_path: &Path,
    base_revision: Option<u64>,
) -> Result<(), RevisionError> {
    let current = read_world_revision(world_path).map_err(|_| RevisionError::Mismatch {
        current_revision: 0,
    })?;
    match base_revision {
        Some(base) if base == current => Ok(()),
        Some(_) => Err(RevisionError::Mismatch {
            current_revision: current,
        }),
        None if current == 0 => Ok(()),
        None => Err(RevisionError::PreconditionRequired {
            current_revision: current,
        }),
    }
}

/// Increment manifest revision after successful mutation.
pub(crate) fn bump_world_revision(world_path: &Path) -> Result<u64, String> {
    let path = map_manifest_path(world_path);
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut manifest = MapManifest::from_json(&raw).map_err(|e| e.to_string())?;
    manifest.revision = manifest.revision.saturating_add(1);
    let next = manifest.revision;
    std::fs::write(&path, manifest.to_json_pretty().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(next)
}

/// Header + optional JSON body field.
pub(crate) fn parse_base_revision(headers: &HeaderMap, body: Option<u64>) -> Option<u64> {
    if body.is_some() {
        return body;
    }
    headers
        .get(WORLD_BASE_REVISION_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

pub(crate) fn revision_error_response(err: RevisionError) -> Response {
    crate::op_log::note_revision_error(&err);
    match err {
        RevisionError::Mismatch { current_revision } => (
            StatusCode::CONFLICT,
            Json(RevisionConflictBody {
                current_revision,
                conflict_kind: "world_revision_mismatch".to_string(),
            }),
        )
            .into_response(),
        RevisionError::PreconditionRequired { current_revision } => (
            StatusCode::PRECONDITION_REQUIRED,
            Json(RevisionConflictBody {
                current_revision,
                conflict_kind: "base_revision_required".to_string(),
            }),
        )
            .into_response(),
    }
}

pub(crate) fn attach_result_revision(mut response: Response, revision: u64) -> Response {
    if let Ok(value) = revision.to_string().parse() {
        response
            .headers_mut()
            .insert(WORLD_RESULT_REVISION_HEADER, value);
    }
    response
}

pub(crate) fn no_content_with_revision(revision: u64) -> Response {
    attach_result_revision(StatusCode::NO_CONTENT.into_response(), revision)
}

pub(crate) fn json_with_revision<T: Serialize>(value: T, revision: u64) -> Response {
    attach_result_revision(Json(value).into_response(), revision)
}

/// Check → mutate → bump. Caller must hold world write guard.
pub(crate) fn mutate_map<T, F>(
    world_path: &Path,
    base_revision: Option<u64>,
    f: F,
) -> Result<(T, u64), RevisionMutationError>
where
    F: FnOnce() -> Result<T, String>,
{
    require_base_revision(world_path, base_revision).map_err(RevisionMutationError::Revision)?;
    let value = f().map_err(|msg| {
        crate::op_log::note_op_error(&msg);
        RevisionMutationError::Internal(msg)
    })?;
    let revision = bump_world_revision(world_path).map_err(|msg| {
        crate::op_log::note_op_error(&msg);
        RevisionMutationError::Internal(msg)
    })?;
    crate::op_log::note_direct_mutate_success(revision);
    Ok((value, revision))
}

#[derive(Debug)]
pub(crate) enum RevisionMutationError {
    Revision(RevisionError),
    Internal(String),
}

impl RevisionMutationError {
    pub(crate) fn into_response(self) -> Response {
        match self {
            Self::Revision(err) => revision_error_response(err),
            Self::Internal(msg) => {
                crate::op_log::note_op_error(&msg);
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mapkeeper_core::layer::MapManifest;
    use tempfile::tempdir;

    fn seed_manifest(path: &Path, revision: u64) {
        std::fs::create_dir_all(path.join("map")).unwrap();
        let mut manifest = MapManifest::default_v0(14, 8);
        manifest.revision = revision;
        std::fs::write(
            path.join("map/manifest.json"),
            manifest.to_json_pretty().unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn missing_revision_field_reads_as_zero() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        std::fs::create_dir_all(world.join("map")).unwrap();
        std::fs::write(
            world.join("map/manifest.json"),
            r#"{"schema_version":1,"bounds":{"kind":"hex-rectangle","width":14,"height":8},"layers":[]}"#,
        )
        .unwrap();
        assert_eq!(read_world_revision(world).unwrap(), 0);
    }

    #[test]
    fn bump_increments_and_persists() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        seed_manifest(world, 2);
        assert_eq!(bump_world_revision(world).unwrap(), 3);
        assert_eq!(read_world_revision(world).unwrap(), 3);
    }

    #[test]
    fn legacy_bootstrap_allows_omitted_base_at_zero() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        seed_manifest(world, 0);
        assert!(require_base_revision(world, None).is_ok());
    }

    #[test]
    fn omitted_base_rejected_after_first_bump() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        seed_manifest(world, 1);
        assert!(matches!(
            require_base_revision(world, None),
            Err(RevisionError::PreconditionRequired { current_revision: 1 })
        ));
    }

    #[test]
    fn failed_mutate_does_not_bump_revision() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        seed_manifest(world, 0);
        let err = mutate_map(world, Some(0), || -> Result<(), String> {
            Err("simulated failure".to_string())
        });
        assert!(err.is_err());
        assert_eq!(read_world_revision(world).unwrap(), 0);
    }
}
