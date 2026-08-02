//! Stroke transaction: staging validation and durable apply (N-025 / N-031).

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use mapkeeper_core::spatial::{Axial, SpatialState};

use super::persist::write_spatial_state;
use super::{conflict, load_state, CellValue};
use crate::state::{RecentCommittedStroke, ServerState};

pub(super) fn validate_stroke_id(id: &str) -> Result<(), (StatusCode, String)> {
    if id.is_empty() || id.len() > 128 {
        return Err((StatusCode::BAD_REQUEST, "invalid stroke_id".into()));
    }
    Ok(())
}

pub(super) fn cells_from_values(cells: &[CellValue]) -> HashMap<Axial, i32> {
    let mut map = HashMap::with_capacity(cells.len());
    for cell in cells {
        map.insert(
            Axial {
                q: cell.q,
                r: cell.r,
            },
            cell.value,
        );
    }
    map
}

pub(super) fn apply_stroke_cells(state: &mut SpatialState, cells: &HashMap<Axial, i32>) -> Result<(), String> {
    let updates: Vec<(Axial, i32)> = cells
        .iter()
        .map(|(&axial, &value)| (axial, value))
        .collect();
    state.field.set_cells(&state.grid, &updates)
}

/// Shared durable apply for oneshot and staged commit (N-025).
/// Registers process-local replay only after a successful atomic write.
#[allow(clippy::result_large_err)]
pub(super) fn persist_stroke_cells(
    server: &ServerState,
    path: &Path,
    world_key: &str,
    stroke_id: String,
    base_revision: u64,
    cells: &HashMap<Axial, i32>,
) -> Result<SpatialState, axum::response::Response> {
    let lock = server.world_lock(world_key);
    let _guard = lock.lock().unwrap();
    let mut state = match load_state(path) {
        Ok(s) => s,
        Err(e) => return Err(e.into_response()),
    };
    if base_revision != state.revision {
        return Err(conflict(state));
    }
    if let Err(error) = apply_stroke_cells(&mut state, cells) {
        return Err((StatusCode::BAD_REQUEST, error).into_response());
    }
    state.bump_revision();
    if let Err(error) = write_spatial_state(path, &state) {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response());
    }
    server.recent_committed_strokes.lock().unwrap().insert(
        stroke_id,
        RecentCommittedStroke {
            world_key: world_key.to_string(),
            recorded_at: Instant::now(),
        },
    );
    Ok(state)
}

pub(super) fn replay_committed_view(
    server: &ServerState,
    path: &Path,
    world_key: &str,
    stroke_id: &str,
) -> Option<Result<SpatialState, (StatusCode, String)>> {
    let committed = server.recent_committed_strokes.lock().unwrap();
    let prev = committed.get(stroke_id)?;
    if prev.world_key != world_key {
        return None;
    }
    Some(load_state(path))
}
