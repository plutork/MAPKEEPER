//! Stroke transaction: staging validation and durable apply (N-025 / N-031).

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use mapkeeper_core::spatial::{Axial, SpatialState};
use serde::Serialize;

use super::persist::write_spatial_state_timed;
use super::{conflict, load_state_timed, CellValue};
use crate::state::{RecentCommittedStroke, ServerState};

#[derive(Debug, Clone, Serialize)]
pub(super) struct StrokeServerTimings {
    pub read_ms: f64,
    pub parse_ms: f64,
    pub apply_ms: f64,
    pub serialize_ms: f64,
    pub atomic_write_ms: f64,
    pub replayed: bool,
}

pub(super) struct PersistStrokeResult {
    pub state: SpatialState,
    pub applied_cells: usize,
    pub server_timings: StrokeServerTimings,
}

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

pub(super) fn apply_stroke_cells(
    state: &mut SpatialState,
    cells: &HashMap<Axial, i32>,
) -> Result<(), String> {
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
) -> Result<PersistStrokeResult, axum::response::Response> {
    let lock = server.world_lock(world_key);
    let _guard = lock.lock().unwrap();
    let (mut state, load_timings) = match load_state_timed(path) {
        Ok(s) => s,
        Err(e) => return Err(e.into_response()),
    };
    if base_revision != state.revision {
        return Err(conflict(state));
    }
    let apply_started = Instant::now();
    if let Err(error) = apply_stroke_cells(&mut state, cells) {
        return Err((StatusCode::BAD_REQUEST, error).into_response());
    }
    let apply_ms = apply_started.elapsed().as_secs_f64() * 1000.0;
    state.bump_revision();
    let write_timings = write_spatial_state_timed(path, &state)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response())?;
    let applied_cells = cells.len();
    server.recent_committed_strokes.lock().unwrap().insert(
        stroke_id,
        RecentCommittedStroke {
            world_key: world_key.to_string(),
            applied_cells,
            recorded_at: Instant::now(),
        },
    );
    Ok(PersistStrokeResult {
        state,
        applied_cells,
        server_timings: StrokeServerTimings {
            read_ms: load_timings.read_ms,
            parse_ms: load_timings.parse_ms,
            apply_ms,
            serialize_ms: write_timings.serialize_ms,
            atomic_write_ms: write_timings.atomic_write_ms,
            replayed: false,
        },
    })
}

pub(super) fn replay_committed_view(
    server: &ServerState,
    path: &Path,
    world_key: &str,
    stroke_id: &str,
) -> Option<Result<PersistStrokeResult, (StatusCode, String)>> {
    let committed = server.recent_committed_strokes.lock().unwrap();
    let prev = committed.get(stroke_id)?;
    if prev.world_key != world_key {
        return None;
    }
    let applied_cells = prev.applied_cells;
    Some(
        load_state_timed(path).map(|(state, timings)| PersistStrokeResult {
            state,
            applied_cells,
            server_timings: StrokeServerTimings {
                read_ms: timings.read_ms,
                parse_ms: timings.parse_ms,
                apply_ms: 0.0,
                serialize_ms: 0.0,
                atomic_write_ms: 0.0,
                replayed: true,
            },
        }),
    )
}
