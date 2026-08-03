//! Spatial API + stroke transaction (N-008 / N-025).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use mapkeeper_core::spatial::{cells_for_stub, Axial, GeometryStub, SpatialState};
use serde::{Deserialize, Serialize};

use crate::state::{ServerState, StrokeStaging};
use crate::world_io;

mod persist;
mod stroke;

pub use persist::{ensure_spatial_state, restore_spatial_from_bak};
use persist::{ensure_spatial_state_timed, write_spatial_state};
use stroke::{
    cells_from_values, persist_stroke_cells, replay_committed_view, validate_stroke_id,
    PersistStrokeResult, StrokeServerTimings,
};

#[derive(Serialize)]
struct SpatialView {
    state: SpatialState,
    /// Derived membership — not an independent SoT.
    stub_cells: Vec<Axial>,
}

#[derive(Serialize)]
struct StrokeAck {
    ok: bool,
    revision: u64,
    applied_cells: usize,
    server_timings: StrokeServerTimings,
}

#[derive(Deserialize)]
struct CellValue {
    q: i32,
    r: i32,
    value: i32,
}

#[derive(Deserialize)]
struct StubUpdate {
    points: Vec<[f64; 2]>,
    #[serde(default)]
    base_revision: Option<u64>,
}

#[derive(Deserialize)]
struct StrokeBegin {
    stroke_id: String,
    base_revision: u64,
}

#[derive(Deserialize)]
struct StrokeChunk {
    stroke_id: String,
    chunk_id: String,
    cells: Vec<CellValue>,
}

#[derive(Deserialize)]
struct StrokeIdBody {
    stroke_id: String,
}

/// Single-shot stroke (typical small gestures) — begin+cells+commit.
#[derive(Deserialize)]
struct StrokeOneShot {
    stroke_id: String,
    base_revision: u64,
    cells: Vec<CellValue>,
}

#[derive(Serialize)]
struct OkMsg {
    ok: bool,
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/spatial", get(get_spatial))
        .route("/api/spatial/geometry-stub", put(put_stub))
        .route("/api/spatial/restore-bak", post(restore_bak))
        .route("/api/spatial/stroke", post(stroke_oneshot))
        .route("/api/spatial/stroke/begin", post(stroke_begin))
        .route("/api/spatial/stroke/chunk", post(stroke_chunk))
        .route("/api/spatial/stroke/commit", post(stroke_commit))
        .route("/api/spatial/stroke/abort", post(stroke_abort))
}

/// Active **map** path for spatial I/O (N-035). Lock key stays world folder.
fn active_world(server: &ServerState) -> Result<(PathBuf, String), (StatusCode, String)> {
    let app = server.app.lock().unwrap();
    match &app.active {
        Some(world) => Ok((world.map_path.clone(), world.id.clone())),
        None => Err((StatusCode::BAD_REQUEST, "no active world".into())),
    }
}

fn active_world_lock_key(server: &ServerState) -> Result<String, (StatusCode, String)> {
    let app = server.app.lock().unwrap();
    match &app.active {
        Some(world) => Ok(world_io::path_cmp_key(&world.path)),
        None => Err((StatusCode::BAD_REQUEST, "no active world".into())),
    }
}

fn load_state(path: &Path) -> Result<SpatialState, (StatusCode, String)> {
    load_state_timed(path).map(|(state, _)| state)
}

fn load_state_timed(
    path: &Path,
) -> Result<(SpatialState, persist::SpatialIoTimings), (StatusCode, String)> {
    match ensure_spatial_state_timed(path) {
        Ok(result) => Ok(result),
        Err(error) => {
            let message = error.to_string();
            let status =
                if message.contains("corrupt_spatial") || message.contains("corrupt_manifest") {
                    StatusCode::UNPROCESSABLE_ENTITY
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
            Err((status, message))
        }
    }
}

async fn restore_bak(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    let (path, _) = match active_world(&server) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let key = match active_world_lock_key(&server) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };
    let lock = server.world_lock(&key);
    let _guard = lock.lock().unwrap();
    match restore_spatial_from_bak(&path) {
        Ok(state) => Json(view_for(state)).into_response(),
        Err(error) => {
            let message = error.to_string();
            let status = if message.contains("corrupt_spatial") {
                StatusCode::UNPROCESSABLE_ENTITY
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, message).into_response()
        }
    }
}

fn view_for(state: SpatialState) -> SpatialView {
    let stub_cells = cells_for_stub(&state.frame, &state.grid, &state.geometry_stub);
    SpatialView { state, stub_cells }
}

fn stroke_ack_for(result: PersistStrokeResult) -> StrokeAck {
    StrokeAck {
        ok: true,
        revision: result.state.revision,
        applied_cells: result.applied_cells,
        server_timings: result.server_timings,
    }
}

fn conflict(state: SpatialState) -> axum::response::Response {
    (StatusCode::CONFLICT, Json(view_for(state))).into_response()
}

async fn get_spatial(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    let (path, _) = match active_world(&server) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    match load_state(&path) {
        Ok(state) => Json(view_for(state)).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn put_stub(
    State(server): State<Arc<ServerState>>,
    Json(update): Json<StubUpdate>,
) -> impl IntoResponse {
    let (path, _) = match active_world(&server) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let key = match active_world_lock_key(&server) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };
    let lock = server.world_lock(&key);
    let _guard = lock.lock().unwrap();
    let mut state = match load_state(&path) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if let Some(base) = update.base_revision {
        if base != state.revision {
            return conflict(state);
        }
    }
    state.geometry_stub = GeometryStub {
        id: state.geometry_stub.id.clone(),
        points: update.points,
    };
    state.bump_revision();
    if let Err(error) = write_spatial_state(&path, &state) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    Json(view_for(state)).into_response()
}

async fn stroke_begin(
    State(server): State<Arc<ServerState>>,
    Json(body): Json<StrokeBegin>,
) -> impl IntoResponse {
    if let Err(e) = validate_stroke_id(&body.stroke_id) {
        return e.into_response();
    }
    server.purge_stale_strokes();
    let (path, _) = match active_world(&server) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let key = match active_world_lock_key(&server) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };
    {
        let committed = server.recent_committed_strokes.lock().unwrap();
        if committed.contains_key(&body.stroke_id) {
            return (StatusCode::CONFLICT, "stroke_id already committed").into_response();
        }
    }
    let state = match load_state(&path) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if body.base_revision != state.revision {
        return conflict(state);
    }
    let mut strokes = server.strokes.lock().unwrap();
    if let Some(existing) = strokes.get(&body.stroke_id) {
        if existing.world_key != key {
            return (StatusCode::CONFLICT, "stroke_id in use").into_response();
        }
        // Idempotent begin replay.
        return Json(OkMsg { ok: true }).into_response();
    }
    let _ = path;
    strokes.insert(
        body.stroke_id,
        StrokeStaging {
            world_key: key,
            base_revision: body.base_revision,
            cells: HashMap::new(),
            chunk_ids: Default::default(),
            created_at: Instant::now(),
        },
    );
    Json(OkMsg { ok: true }).into_response()
}

async fn stroke_chunk(
    State(server): State<Arc<ServerState>>,
    Json(body): Json<StrokeChunk>,
) -> impl IntoResponse {
    if let Err(e) = validate_stroke_id(&body.stroke_id) {
        return e.into_response();
    }
    if body.chunk_id.is_empty() || body.chunk_id.len() > 64 {
        return (StatusCode::BAD_REQUEST, "invalid chunk_id").into_response();
    }
    server.purge_stale_strokes();
    let (path, _) = match active_world(&server) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let key = match active_world_lock_key(&server) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };
    let mut strokes = server.strokes.lock().unwrap();
    let Some(staging) = strokes.get_mut(&body.stroke_id) else {
        return (StatusCode::BAD_REQUEST, "unknown stroke_id — begin first").into_response();
    };
    if staging.world_key != key {
        return (StatusCode::CONFLICT, "stroke belongs to another world").into_response();
    }
    // Duplicate chunk: no double-apply.
    if staging.chunk_ids.contains(&body.chunk_id) {
        return Json(OkMsg { ok: true }).into_response();
    }
    let state = match load_state(&path) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    for cell in &body.cells {
        if !state.grid.contains_axial(cell.q, cell.r) {
            return (
                StatusCode::BAD_REQUEST,
                format!("cell {},{} outside grid", cell.q, cell.r),
            )
                .into_response();
        }
        if !(mapkeeper_core::spatial::RELIEF_MIN..=mapkeeper_core::spatial::RELIEF_MAX)
            .contains(&cell.value)
        {
            return (StatusCode::BAD_REQUEST, "relief out of range").into_response();
        }
        staging.cells.insert(
            Axial {
                q: cell.q,
                r: cell.r,
            },
            cell.value,
        );
    }
    staging.chunk_ids.insert(body.chunk_id);
    Json(OkMsg { ok: true }).into_response()
}

async fn stroke_commit(
    State(server): State<Arc<ServerState>>,
    Json(body): Json<StrokeIdBody>,
) -> impl IntoResponse {
    if let Err(e) = validate_stroke_id(&body.stroke_id) {
        return e.into_response();
    }
    server.purge_stale_strokes();
    let (path, _) = match active_world(&server) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let key = match active_world_lock_key(&server) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };

    if let Some(replay) = replay_committed_view(&server, &path, &key, &body.stroke_id) {
        return match replay {
            Ok(result) => Json(stroke_ack_for(result)).into_response(),
            Err(e) => e.into_response(),
        };
    }

    let staging = {
        let mut strokes = server.strokes.lock().unwrap();
        match strokes.remove(&body.stroke_id) {
            Some(s) => s,
            None => {
                return (StatusCode::BAD_REQUEST, "unknown stroke_id — begin first")
                    .into_response();
            }
        }
    };
    if staging.world_key != key {
        return (StatusCode::CONFLICT, "stroke belongs to another world").into_response();
    }

    match persist_stroke_cells(
        &server,
        &path,
        &key,
        body.stroke_id,
        staging.base_revision,
        &staging.cells,
    ) {
        Ok(result) => Json(stroke_ack_for(result)).into_response(),
        Err(resp) => resp,
    }
}

async fn stroke_abort(
    State(server): State<Arc<ServerState>>,
    Json(body): Json<StrokeIdBody>,
) -> impl IntoResponse {
    if let Err(e) = validate_stroke_id(&body.stroke_id) {
        return e.into_response();
    }
    let mut strokes = server.strokes.lock().unwrap();
    strokes.remove(&body.stroke_id);
    // Abort is idempotent even if already gone / committed.
    Json(OkMsg { ok: true }).into_response()
}

async fn stroke_oneshot(
    State(server): State<Arc<ServerState>>,
    Json(body): Json<StrokeOneShot>,
) -> impl IntoResponse {
    if let Err(e) = validate_stroke_id(&body.stroke_id) {
        return e.into_response();
    }
    server.purge_stale_strokes();
    let (path, _) = match active_world(&server) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let key = match active_world_lock_key(&server) {
        Ok(k) => k,
        Err(e) => return e.into_response(),
    };

    if let Some(replay) = replay_committed_view(&server, &path, &key, &body.stroke_id) {
        return match replay {
            Ok(result) => Json(stroke_ack_for(result)).into_response(),
            Err(e) => e.into_response(),
        };
    }

    let cells = cells_from_values(&body.cells);
    let stroke_id = body.stroke_id.clone();
    match persist_stroke_cells(
        &server,
        &path,
        &key,
        body.stroke_id,
        body.base_revision,
        &cells,
    ) {
        Ok(result) => {
            // Drop any partial staging with same id.
            server.strokes.lock().unwrap().remove(&stroke_id);
            Json(stroke_ack_for(result)).into_response()
        }
        Err(resp) => resp,
    }
}

#[cfg(test)]
mod tests;
