//! Resolve explicit world scope from `X-World-Id` (agent-reliability).

use std::path::PathBuf;
use std::sync::Mutex;

use axum::http::{HeaderMap, StatusCode};
use mapkeeper_core::world;

use crate::state::AppState;
use crate::world_io;

pub const WORLD_ID_HEADER: &str = "x-world-id";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedWorld {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeMode {
    /// Read/navigation: header preferred, `active` fallback.
    Read,
    /// Mutating: header preferred; `active` fallback during migration window.
    Mutate,
}

pub(crate) fn resolve_world(
    state: &Mutex<AppState>,
    headers: &HeaderMap,
    mode: ScopeMode,
) -> Result<ResolvedWorld, (StatusCode, String)> {
    if let Some(world_id) = header_world_id(headers) {
        return resolve_registered_world(&world_id);
    }
    let active = state.lock().unwrap().active.clone();
    let Some(active) = active else {
        let msg = match mode {
            ScopeMode::Mutate => {
                "missing X-World-Id header and no active world (open via /api/projects)".to_string()
            }
            ScopeMode::Read => {
                "no active world — open via /api/projects or send X-World-Id".to_string()
            }
        };
        return Err((StatusCode::CONFLICT, msg));
    };
    Ok(ResolvedWorld {
        id: active.id,
        path: active.path,
    })
}

pub(crate) fn resolve_registered_world(
    world_id: &str,
) -> Result<ResolvedWorld, (StatusCode, String)> {
    if !world::is_valid_world_id(world_id) {
        return Err((StatusCode::BAD_REQUEST, "invalid X-World-Id format".into()));
    }
    let file = world_io::load_projects();
    let mut matches: Vec<_> = file
        .projects
        .iter()
        .filter(|entry| entry.id == world_id)
        .collect();
    matches.sort_by(|left, right| left.path.cmp(&right.path));
    for entry in matches {
        let path = world_io::normalize_world_path(std::path::Path::new(&entry.path));
        if !path.join("mapkeeper.toml").is_file() {
            continue;
        }
        let manifest_id = match world_io::read_manifest_id(&path) {
            Ok(id) => id,
            Err(_) => continue,
        };
        if manifest_id != world_id {
            continue;
        }
        return Ok(ResolvedWorld {
            id: world_id.to_string(),
            path,
        });
    }
    Err((
        StatusCode::NOT_FOUND,
        format!("unknown world id: {world_id}"),
    ))
}

fn header_world_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(WORLD_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn rejects_unknown_world_id() {
        let err = resolve_registered_world("no-such-world").unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }

    #[test]
    fn rejects_invalid_world_id_format() {
        let err = resolve_registered_world("../escape").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }
}
