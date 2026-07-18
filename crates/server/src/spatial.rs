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
use mapkeeper_core::spatial::{
    cells_for_stub, default_spatial_state, Axial, GeometryStub, SpatialState,
    SPATIAL_STATE_RELATIVE,
};
use mapkeeper_core::world::{self, SpatialConfig};
use serde::{Deserialize, Serialize};

use crate::atomic_io;
use crate::state::{CommittedStroke, ServerState, StrokeStaging};
use crate::world_io;

#[derive(Serialize)]
struct SpatialView {
    state: SpatialState,
    /// Derived membership — not an independent SoT.
    stub_cells: Vec<Axial>,
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
    #[serde(default)]
    mode: Option<String>,
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
    #[serde(default)]
    mode: Option<String>,
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

fn spatial_bak_available(path: &Path) -> bool {
    atomic_io::bak_passes(path, |bytes| {
        let Ok(raw) = std::str::from_utf8(bytes) else {
            return false;
        };
        SpatialState::assert_no_screen_keys(raw).is_ok() && SpatialState::from_json(raw).is_ok()
    })
}

/// Load/init spatial state. Corrupt / interrupted on-disk state is never silently replaced with defaults.
pub fn ensure_spatial_state(world_path: &Path) -> anyhow::Result<SpatialState> {
    let config = ensure_spatial_config(world_path)?;
    let path = spatial_path(world_path);
    let (mut state, legacy_schema) = match atomic_io::classify_durable_open(&path) {
        atomic_io::DurableOpenKind::PrimaryPresent => {
            let raw = std::fs::read_to_string(&path)?;
            SpatialState::assert_no_screen_keys(&raw).map_err(anyhow::Error::msg)?;
            let legacy = raw.contains("cell_size") || raw.contains("unit_scale");
            match SpatialState::from_json(&raw) {
                Ok(state) => (state, legacy),
                Err(error) => {
                    anyhow::bail!(
                        "corrupt_spatial: {} (bak_available={})",
                        error,
                        spatial_bak_available(&path)
                    );
                }
            }
        }
        atomic_io::DurableOpenKind::InterruptedWrite => {
            anyhow::bail!(
                "corrupt_spatial: interrupted_write (bak_available={})",
                spatial_bak_available(&path)
            );
        }
        atomic_io::DurableOpenKind::AbsentClean => (default_spatial_state(), false),
    };

    let before = state.clone();
    state.apply_spatial_config(&config);
    if (before.frame != state.frame || before.grid != state.grid)
        && state.geometry_stub.id == "probe"
    {
        state.refresh_geometry_stub_from_probe();
    }

    let needs_write = !path.is_file() || legacy_schema || before != state;
    if needs_write {
        if state.revision == 0 {
            state.revision = 1;
        }
        write_spatial_state(world_path, &state)?;
    }
    Ok(state)
}

/// Explicit recovery: quarantine corrupt primary, restore from `.bak` (N-025).
pub fn restore_spatial_from_bak(world_path: &Path) -> anyhow::Result<SpatialState> {
    let path = spatial_path(world_path);
    let bak = atomic_io::bak_path(&path);
    if !bak.is_file() {
        anyhow::bail!("corrupt_spatial: no bak available");
    }
    let bak_raw = std::fs::read_to_string(&bak)?;
    SpatialState::assert_no_screen_keys(&bak_raw).map_err(anyhow::Error::msg)?;
    let restored = SpatialState::from_json(&bak_raw)
        .map_err(|e| anyhow::anyhow!("corrupt_spatial: invalid bak: {e}"))?;

    if path.is_file() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let diag = path.with_file_name(format!("state.json.corrupt-{stamp}"));
        std::fs::rename(&path, &diag).or_else(|_| {
            std::fs::copy(&path, &diag)?;
            std::fs::remove_file(&path)?;
            Ok::<(), std::io::Error>(())
        })?;
    }

    write_spatial_state(world_path, &restored)?;
    ensure_spatial_state(world_path)
}

fn ensure_spatial_config(world_path: &Path) -> anyhow::Result<SpatialConfig> {
    let manifest_path = world_path.join("mapkeeper.toml");
    match atomic_io::classify_durable_open(&manifest_path) {
        atomic_io::DurableOpenKind::InterruptedWrite => {
            let bak_available = atomic_io::bak_passes(&manifest_path, |bytes| {
                std::str::from_utf8(bytes)
                    .ok()
                    .and_then(|raw| world::parse_manifest(raw).ok())
                    .is_some()
            });
            anyhow::bail!("corrupt_manifest: interrupted_write (bak_available={bak_available})");
        }
        atomic_io::DurableOpenKind::AbsentClean => {
            anyhow::bail!(
                "corrupt_manifest: missing mapkeeper.toml at {}",
                manifest_path.display()
            );
        }
        atomic_io::DurableOpenKind::PrimaryPresent => {}
    }

    let raw = std::fs::read_to_string(&manifest_path)?;
    let mut manifest = match world::parse_manifest(&raw) {
        Ok(m) => m,
        Err(error) => {
            let bak_available = atomic_io::bak_passes(&manifest_path, |bytes| {
                std::str::from_utf8(bytes)
                    .ok()
                    .and_then(|b| world::parse_manifest(b).ok())
                    .is_some()
            });
            anyhow::bail!("corrupt_manifest: {error} (bak_available={bak_available})");
        }
    };
    if let Some(spatial) = manifest.spatial.clone() {
        spatial
            .assert_matches_catalog()
            .map_err(anyhow::Error::msg)?;
        return Ok(spatial);
    }
    let spatial = SpatialConfig::alpha_default();
    manifest.spatial = Some(spatial.clone());
    let rendered = world::render_manifest(&manifest)?;
    atomic_io::atomic_replace(&manifest_path, rendered.as_bytes())?;
    Ok(spatial)
}

fn spatial_path(world_path: &Path) -> PathBuf {
    world_path.join(SPATIAL_STATE_RELATIVE)
}

fn write_spatial_state(world_path: &Path, state: &SpatialState) -> anyhow::Result<()> {
    let path = spatial_path(world_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = state.to_json_pretty()?;
    SpatialState::assert_no_screen_keys(&raw).map_err(anyhow::Error::msg)?;
    atomic_io::atomic_replace(&path, raw.as_bytes())?;
    Ok(())
}

fn active_world(server: &ServerState) -> Result<(PathBuf, String), (StatusCode, String)> {
    let app = server.app.lock().unwrap();
    match &app.active {
        Some(world) => Ok((world.path.clone(), world.id.clone())),
        None => Err((StatusCode::BAD_REQUEST, "no active world".into())),
    }
}

fn load_state(path: &Path) -> Result<SpatialState, (StatusCode, String)> {
    match ensure_spatial_state(path) {
        Ok(state) => Ok(state),
        Err(error) => {
            let message = error.to_string();
            let status = if message.contains("corrupt_spatial")
                || message.contains("corrupt_manifest")
            {
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
    let key = world_io::path_cmp_key(&path);
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
    let key = world_io::path_cmp_key(&path);
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

fn validate_stroke_id(id: &str) -> Result<(), (StatusCode, String)> {
    if id.is_empty() || id.len() > 128 {
        return Err((StatusCode::BAD_REQUEST, "invalid stroke_id".into()));
    }
    Ok(())
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
    let key = world_io::path_cmp_key(&path);
    {
        let committed = server.committed_strokes.lock().unwrap();
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
    let _ = body.mode;
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
    let key = world_io::path_cmp_key(&path);
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
        staging
            .cells
            .insert(format!("{},{}", cell.q, cell.r), cell.value);
    }
    staging.chunk_ids.insert(body.chunk_id);
    Json(OkMsg { ok: true }).into_response()
}

fn apply_staged_cells(
    state: &mut SpatialState,
    cells: &HashMap<String, i32>,
) -> Result<(), String> {
    let updates: Vec<(Axial, i32)> = cells
        .iter()
        .filter_map(|(key, value)| {
            let mut parts = key.split(',');
            let q = parts.next()?.parse().ok()?;
            let r = parts.next()?.parse().ok()?;
            Some((Axial { q, r }, *value))
        })
        .collect();
    state.field.set_cells(&state.grid, &updates)
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
    let key = world_io::path_cmp_key(&path);

    // Idempotent commit replay.
    {
        let committed = server.committed_strokes.lock().unwrap();
        if let Some(prev) = committed.get(&body.stroke_id) {
            if prev.world_key == key {
                let state = match load_state(&path) {
                    Ok(s) => s,
                    Err(e) => return e.into_response(),
                };
                return Json(view_for(state)).into_response();
            }
        }
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

    let lock = server.world_lock(&key);
    let _guard = lock.lock().unwrap();
    let mut state = match load_state(&path) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if staging.base_revision != state.revision {
        // Staging dropped; client must reload.
        return conflict(state);
    }
    if let Err(error) = apply_staged_cells(&mut state, &staging.cells) {
        return (StatusCode::BAD_REQUEST, error).into_response();
    }
    state.bump_revision();
    if let Err(error) = write_spatial_state(&path, &state) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    server.committed_strokes.lock().unwrap().insert(
        body.stroke_id,
        CommittedStroke { world_key: key },
    );
    Json(view_for(state)).into_response()
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
    let key = world_io::path_cmp_key(&path);

    {
        let committed = server.committed_strokes.lock().unwrap();
        if let Some(prev) = committed.get(&body.stroke_id) {
            if prev.world_key == key {
                let state = match load_state(&path) {
                    Ok(s) => s,
                    Err(e) => return e.into_response(),
                };
                return Json(view_for(state)).into_response();
            }
        }
    }

    let lock = server.world_lock(&key);
    let _guard = lock.lock().unwrap();
    let mut state = match load_state(&path) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if body.base_revision != state.revision {
        return conflict(state);
    }
    let mut cells = HashMap::new();
    for cell in &body.cells {
        cells.insert(format!("{},{}", cell.q, cell.r), cell.value);
    }
    if let Err(error) = apply_staged_cells(&mut state, &cells) {
        return (StatusCode::BAD_REQUEST, error).into_response();
    }
    let _ = body.mode;
    state.bump_revision();
    if let Err(error) = write_spatial_state(&path, &state) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    let stroke_id = body.stroke_id;
    server.committed_strokes.lock().unwrap().insert(
        stroke_id.clone(),
        CommittedStroke { world_key: key },
    );
    // Drop any partial staging with same id.
    server.strokes.lock().unwrap().remove(&stroke_id);
    Json(view_for(state)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use mapkeeper_core::world;
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn app_with_world(path: &Path) -> axum::Router {
        let state = Arc::new(ServerState::new(Some(crate::state::ActiveWorld {
            path: path.to_path_buf(),
            id: "t".into(),
        })));
        routes().with_state(state)
    }

    async fn json_request(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = if bytes.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) })
            })
        };
        (status, value)
    }

    fn seed_world() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mapkeeper.toml"), world::manifest_toml("t")).unwrap();
        ensure_spatial_state(dir.path()).unwrap();
        dir
    }

    #[test]
    fn ensure_writes_spatial_config_and_metric_grid() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mapkeeper.toml"), world::manifest_toml("t")).unwrap();
        let state = ensure_spatial_state(dir.path()).unwrap();
        assert_eq!(state.grid.neighbor_center_distance_m, 1000.0);
        assert!(state.revision >= 1);
        let again = ensure_spatial_state(dir.path()).unwrap();
        assert_eq!(again.grid, state.grid);
    }

    #[test]
    fn ensure_backfills_spatial_section_on_legacy_manifest() {
        let dir = tempdir().unwrap();
        let legacy = "# mapkeeper world workspace\n\n[world]\nid = \"old\"\nname = \"old\"\nversion = \"0.3.0\"\n";
        std::fs::write(dir.path().join("mapkeeper.toml"), legacy).unwrap();
        let state = ensure_spatial_state(dir.path()).unwrap();
        assert_eq!(state.grid.neighbor_center_distance_m, 1000.0);
        let manifest = world::parse_manifest(
            &std::fs::read_to_string(dir.path().join("mapkeeper.toml")).unwrap(),
        )
        .unwrap();
        assert!(manifest.spatial.is_some());
    }

    #[tokio::test]
    async fn stroke_oneshot_atomic_gesture() {
        let dir = seed_world();
        let app = app_with_world(dir.path());
        let (status, view) = json_request(
            app,
            "POST",
            "/api/spatial/stroke",
            serde_json::json!({
                "stroke_id": "s1",
                "base_revision": 1,
                "cells": [{"q": 0, "r": 0, "value": 4}]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["state"]["field"]["cells"]["0,0"], 4);
        assert_eq!(view["state"]["revision"], 2);
        let raw = std::fs::read_to_string(dir.path().join("spatial/state.json")).unwrap();
        assert!(raw.contains("\"0,0\": 4") || raw.contains("\"0,0\":4"));
        assert!(!raw.contains("\"0,0\": 2")); // no partial mid-values
    }

    #[tokio::test]
    async fn multi_chunk_commit_is_one_revision() {
        let dir = seed_world();
        let app = app_with_world(dir.path());
        let (st, _) = json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/begin",
            serde_json::json!({ "stroke_id": "big", "base_revision": 1, "mode": "stamp" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/chunk",
            serde_json::json!({
                "stroke_id": "big", "chunk_id": "0",
                "cells": [{"q": 0, "r": 0, "value": 1}, {"q": 1, "r": 0, "value": 2}]
            }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        // Disk unchanged mid-staging.
        let mid = SpatialState::from_json(
            &std::fs::read_to_string(dir.path().join("spatial/state.json")).unwrap(),
        )
        .unwrap();
        assert!(mid.field.cells.is_empty());
        assert_eq!(mid.revision, 1);

        let (st, _) = json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/chunk",
            serde_json::json!({
                "stroke_id": "big", "chunk_id": "1",
                "cells": [{"q": 2, "r": 0, "value": 3}]
            }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let (st, view) = json_request(
            app,
            "POST",
            "/api/spatial/stroke/commit",
            serde_json::json!({ "stroke_id": "big" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(view["state"]["revision"], 2);
        assert_eq!(view["state"]["field"]["cells"]["0,0"], 1);
        assert_eq!(view["state"]["field"]["cells"]["2,0"], 3);
    }

    #[tokio::test]
    async fn abort_leaves_disk_untouched() {
        let dir = seed_world();
        let app = app_with_world(dir.path());
        json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/begin",
            serde_json::json!({ "stroke_id": "a", "base_revision": 1 }),
        )
        .await;
        json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/chunk",
            serde_json::json!({
                "stroke_id": "a", "chunk_id": "0",
                "cells": [{"q": 0, "r": 0, "value": 9}]
            }),
        )
        .await;
        let (st, _) = json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/abort",
            serde_json::json!({ "stroke_id": "a" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        // Second abort safe.
        let (st, _) = json_request(
            app,
            "POST",
            "/api/spatial/stroke/abort",
            serde_json::json!({ "stroke_id": "a" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let state = SpatialState::from_json(
            &std::fs::read_to_string(dir.path().join("spatial/state.json")).unwrap(),
        )
        .unwrap();
        assert!(state.field.cells.is_empty());
        assert_eq!(state.revision, 1);
    }

    #[tokio::test]
    async fn duplicate_chunk_and_duplicate_commit() {
        let dir = seed_world();
        let app = app_with_world(dir.path());
        json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/begin",
            serde_json::json!({ "stroke_id": "d", "base_revision": 1 }),
        )
        .await;
        let chunk = serde_json::json!({
            "stroke_id": "d", "chunk_id": "x",
            "cells": [{"q": 0, "r": 0, "value": 5}]
        });
        assert_eq!(
            json_request(app.clone(), "POST", "/api/spatial/stroke/chunk", chunk.clone())
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            json_request(app.clone(), "POST", "/api/spatial/stroke/chunk", chunk)
                .await
                .0,
            StatusCode::OK
        );
        let (st, v1) = json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/commit",
            serde_json::json!({ "stroke_id": "d" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let rev = v1["state"]["revision"].as_u64().unwrap();
        let (st, v2) = json_request(
            app,
            "POST",
            "/api/spatial/stroke/commit",
            serde_json::json!({ "stroke_id": "d" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v2["state"]["revision"], rev);
        assert_eq!(v2["state"]["field"]["cells"]["0,0"], 5);
    }

    #[tokio::test]
    async fn stale_base_revision_conflicts() {
        let dir = seed_world();
        let app = app_with_world(dir.path());
        let (st, _) = json_request(
            app,
            "POST",
            "/api/spatial/stroke",
            serde_json::json!({
                "stroke_id": "stale",
                "base_revision": 0,
                "cells": [{"q": 0, "r": 0, "value": 1}]
            }),
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn failed_before_commit_no_partial_on_disk() {
        let dir = seed_world();
        let app = app_with_world(dir.path());
        json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/begin",
            serde_json::json!({ "stroke_id": "fail", "base_revision": 1 }),
        )
        .await;
        json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/chunk",
            serde_json::json!({
                "stroke_id": "fail", "chunk_id": "0",
                "cells": [{"q": 0, "r": 0, "value": 7}]
            }),
        )
        .await;
        // Simulate "failure before commit" by aborting.
        json_request(
            app,
            "POST",
            "/api/spatial/stroke/abort",
            serde_json::json!({ "stroke_id": "fail" }),
        )
        .await;
        let state = SpatialState::from_json(
            &std::fs::read_to_string(dir.path().join("spatial/state.json")).unwrap(),
        )
        .unwrap();
        assert!(!state.field.cells.contains_key("0,0"));
    }

    #[test]
    fn corrupt_state_does_not_rewrite_defaults() {
        let dir = seed_world();
        let path = dir.path().join("spatial/state.json");
        let good = ensure_spatial_state(dir.path()).unwrap();
        write_spatial_state(dir.path(), &good).unwrap(); // creates *.bak
        std::fs::write(&path, "{truncated").unwrap();
        let err = ensure_spatial_state(dir.path()).unwrap_err().to_string();
        assert!(err.contains("corrupt_spatial"));
        assert!(err.contains("bak_available=true"));
        assert_eq!(std::fs::read(&path).unwrap(), b"{truncated");
        assert!(crate::atomic_io::bak_path(&path).is_file());
    }

    #[test]
    fn restart_missing_primary_valid_bak_is_recovery_not_default() {
        let dir = seed_world();
        let path = dir.path().join("spatial/state.json");
        let good = ensure_spatial_state(dir.path()).unwrap();
        write_spatial_state(dir.path(), &good).unwrap();
        let bak = crate::atomic_io::bak_path(&path);
        let bak_bytes = std::fs::read(&bak).unwrap();
        std::fs::remove_file(&path).unwrap();
        // Simulated restart: open with missing primary + valid bak.
        let err = ensure_spatial_state(dir.path()).unwrap_err().to_string();
        assert!(err.contains("interrupted_write"));
        assert!(err.contains("bak_available=true"));
        assert!(!path.is_file());
        assert_eq!(std::fs::read(&bak).unwrap(), bak_bytes);
        // Explicit restore recovers author state; never silent default.
        let restored = restore_spatial_from_bak(dir.path()).unwrap();
        assert!(restored.revision >= 1);
        assert!(path.is_file());
    }

    #[test]
    fn restart_missing_primary_invalid_bak_never_defaults() {
        let dir = seed_world();
        let path = dir.path().join("spatial/state.json");
        let bak = crate::atomic_io::bak_path(&path);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&bak, "{bad-bak").unwrap();
        let err = ensure_spatial_state(dir.path()).unwrap_err().to_string();
        assert!(err.contains("interrupted_write"));
        assert!(err.contains("bak_available=false"));
        assert!(!path.is_file());
        assert_eq!(std::fs::read(&bak).unwrap(), b"{bad-bak");
    }

    #[test]
    fn invalid_primary_valid_bak_never_defaults() {
        let dir = seed_world();
        let path = dir.path().join("spatial/state.json");
        let good = ensure_spatial_state(dir.path()).unwrap();
        write_spatial_state(dir.path(), &good).unwrap();
        let bak_bytes = std::fs::read(crate::atomic_io::bak_path(&path)).unwrap();
        std::fs::write(&path, "{truncated").unwrap();
        let err = ensure_spatial_state(dir.path()).unwrap_err().to_string();
        assert!(err.contains("corrupt_spatial"));
        assert!(err.contains("bak_available=true"));
        assert_eq!(std::fs::read(&path).unwrap(), b"{truncated");
        assert_eq!(
            std::fs::read(crate::atomic_io::bak_path(&path)).unwrap(),
            bak_bytes
        );
    }

    #[test]
    fn failpoint_crash_after_bak_survives_restart_as_recovery() {
        crate::atomic_io::clear_failpoint();
        let dir = seed_world();
        let path = dir.path().join("spatial/state.json");
        let good = ensure_spatial_state(dir.path()).unwrap();
        let good_json = good.to_json_pretty().unwrap();
        write_spatial_state(dir.path(), &good).unwrap();
        // Force a second replace so bak holds last-good author bytes.
        let mut next = good.clone();
        next.revision = good.revision + 1;
        crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::AfterPrimaryToBak);
        assert!(write_spatial_state(dir.path(), &next).is_err());
        assert!(!path.is_file());
        let bak = crate::atomic_io::bak_path(&path);
        assert!(SpatialState::from_json(&std::fs::read_to_string(&bak).unwrap()).is_ok());
        assert!(!crate::atomic_io::leftover_temp_paths(&path).is_empty());
        // Simulated process restart.
        let err = ensure_spatial_state(dir.path()).unwrap_err().to_string();
        assert!(err.contains("interrupted_write"));
        assert!(err.contains("bak_available=true"));
        assert!(!path.is_file());
        let restored = restore_spatial_from_bak(dir.path()).unwrap();
        assert!(restored.revision >= 1);
        let _ = good_json;
    }

    #[test]
    fn failpoint_final_rename_restores_primary_before_error() {
        crate::atomic_io::clear_failpoint();
        let dir = seed_world();
        let path = dir.path().join("spatial/state.json");
        let good = ensure_spatial_state(dir.path()).unwrap();
        write_spatial_state(dir.path(), &good).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut next = good.clone();
        next.revision = good.revision + 1;
        crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::FinalRename);
        assert!(write_spatial_state(dir.path(), &next).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // Restart sees healthy primary (restored), not defaults.
        let loaded = ensure_spatial_state(dir.path()).unwrap();
        assert_eq!(loaded.revision, good.revision);
    }

    #[test]
    fn restore_bak_quarantines_corrupt_and_recovers() {
        let dir = seed_world();
        let path = dir.path().join("spatial/state.json");
        // Force a known bak by rewriting once more.
        let good = ensure_spatial_state(dir.path()).unwrap();
        write_spatial_state(dir.path(), &good).unwrap();
        std::fs::write(&path, "{truncated").unwrap();
        let restored = restore_spatial_from_bak(dir.path()).unwrap();
        assert!(restored.revision >= 1);
        assert!(SpatialState::from_json(&std::fs::read_to_string(&path).unwrap()).is_ok());
        let diag_count = std::fs::read_dir(dir.path().join("spatial"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .ok()
                    .and_then(|f| f.file_name().into_string().ok())
                    .is_some_and(|n| n.starts_with("state.json.corrupt-"))
            })
            .count();
        assert!(diag_count >= 1);
    }

    #[test]
    fn invalid_bak_restore_errors() {
        let dir = seed_world();
        let path = dir.path().join("spatial/state.json");
        let bak = crate::atomic_io::bak_path(&path);
        std::fs::write(&path, "{truncated").unwrap();
        std::fs::write(&bak, "{also-bad").unwrap();
        let err = restore_spatial_from_bak(dir.path()).unwrap_err().to_string();
        assert!(err.contains("corrupt_spatial"));
        assert!(err.contains("invalid bak") || err.contains("no bak"));
    }

    #[test]
    fn missing_manifest_with_bak_is_recovery_not_rewrite() {
        let dir = seed_world();
        let manifest = dir.path().join("mapkeeper.toml");
        let bak = crate::atomic_io::bak_path(&manifest);
        std::fs::copy(&manifest, &bak).unwrap();
        let bak_bytes = std::fs::read(&bak).unwrap();
        std::fs::remove_file(&manifest).unwrap();
        let err = ensure_spatial_state(dir.path()).unwrap_err().to_string();
        assert!(err.contains("corrupt_manifest"));
        assert!(err.contains("interrupted_write"));
        assert!(err.contains("bak_available=true"));
        assert!(!manifest.is_file());
        assert_eq!(std::fs::read(&bak).unwrap(), bak_bytes);
    }

    #[test]
    fn manifest_preset_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let bad = r#"# mapkeeper world workspace

[world]
id = "t"
name = "t"
version = "0.3.0"

[spatial]
preset_id = "wide_2000"
grid_id = "primary"
width_m = 100.0
height_m = 100.0
cols = 1
rows = 1
neighbor_center_distance_m = 1000.0
origin_x_m = 0.0
origin_y_m = 0.0
orientation = "pointy-top"
"#;
        std::fs::write(dir.path().join("mapkeeper.toml"), bad).unwrap();
        let err = ensure_spatial_state(dir.path()).unwrap_err().to_string();
        assert!(err.contains("manifest/preset mismatch"));
    }
}
