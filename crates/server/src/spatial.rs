use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, put};
use axum::{Json, Router};
use mapkeeper_core::spatial::{
    cells_for_stub, default_spatial_state, Axial, GeometryStub, SpatialState,
    SPATIAL_STATE_RELATIVE,
};
use mapkeeper_core::world::{self, SpatialConfig};
use serde::{Deserialize, Serialize};

use crate::state::ServerState;

#[derive(Serialize)]
struct SpatialView {
    state: SpatialState,
    /// Derived membership — not an independent SoT.
    stub_cells: Vec<Axial>,
}

#[derive(Deserialize)]
struct FieldUpdate {
    cells: Vec<CellValue>,
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
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/spatial", get(get_spatial))
        .route("/api/spatial/field", put(put_field))
        .route("/api/spatial/geometry-stub", put(put_stub))
}

pub fn ensure_spatial_state(world_path: &Path) -> anyhow::Result<SpatialState> {
    let config = ensure_spatial_config(world_path)?;
    let path = spatial_path(world_path);
    let (mut state, legacy_schema) = if path.is_file() {
        let raw = std::fs::read_to_string(&path)?;
        SpatialState::assert_no_screen_keys(&raw).map_err(anyhow::Error::msg)?;
        let legacy = raw.contains("cell_size") || raw.contains("unit_scale");
        (SpatialState::from_json(&raw)?, legacy)
    } else {
        (default_spatial_state(), false)
    };

    let before = state.clone();
    state.apply_spatial_config(&config);
    // Keep authored relief; only refresh probe stub when config-driven frame/grid changed.
    if (before.frame != state.frame || before.grid != state.grid)
        && state.geometry_stub.id == "probe"
    {
        state.refresh_geometry_stub_from_probe();
    }

    let needs_write = !path.is_file() || legacy_schema || before != state;
    if needs_write {
        write_spatial_state(world_path, &state)?;
    }
    Ok(state)
}

fn ensure_spatial_config(world_path: &Path) -> anyhow::Result<SpatialConfig> {
    let manifest_path = world_path.join("mapkeeper.toml");
    let raw = std::fs::read_to_string(&manifest_path)?;
    let mut manifest = world::parse_manifest(&raw)?;
    if let Some(spatial) = manifest.spatial.clone() {
        // Reject unknown preset ids early.
        SpatialConfig::from_preset_id(&spatial.preset_id).map_err(anyhow::Error::msg)?;
        return Ok(spatial);
    }
    let spatial = SpatialConfig::alpha_default();
    manifest.spatial = Some(spatial.clone());
    let rendered = world::render_manifest(&manifest)?;
    std::fs::write(manifest_path, rendered)?;
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
    std::fs::write(path, raw)?;
    Ok(())
}

fn load_active(server: &ServerState) -> Result<(PathBuf, SpatialState), (StatusCode, String)> {
    let path = {
        let app = server.app.lock().unwrap();
        match &app.active {
            Some(world) => world.path.clone(),
            None => return Err((StatusCode::BAD_REQUEST, "no active world".into())),
        }
    };
    match ensure_spatial_state(&path) {
        Ok(state) => Ok((path, state)),
        Err(error) => Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
    }
}

fn view_for(state: SpatialState) -> SpatialView {
    let stub_cells = cells_for_stub(&state.frame, &state.grid, &state.geometry_stub);
    SpatialView { state, stub_cells }
}

async fn get_spatial(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    match load_active(&server) {
        Ok((_path, state)) => Json(view_for(state)).into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn put_field(
    State(server): State<Arc<ServerState>>,
    Json(update): Json<FieldUpdate>,
) -> impl IntoResponse {
    let (path, mut state) = match load_active(&server) {
        Ok(v) => v,
        Err((status, message)) => return (status, message).into_response(),
    };
    let updates: Vec<_> = update
        .cells
        .into_iter()
        .map(|c| (Axial { q: c.q, r: c.r }, c.value))
        .collect();
    if let Err(error) = state.field.set_cells(&state.grid, &updates) {
        return (StatusCode::BAD_REQUEST, error).into_response();
    }
    if let Err(error) = write_spatial_state(&path, &state) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    Json(view_for(state)).into_response()
}

async fn put_stub(
    State(server): State<Arc<ServerState>>,
    Json(update): Json<StubUpdate>,
) -> impl IntoResponse {
    let (path, mut state) = match load_active(&server) {
        Ok(v) => v,
        Err((status, message)) => return (status, message).into_response(),
    };
    state.geometry_stub = GeometryStub {
        id: state.geometry_stub.id.clone(),
        points: update.points,
    };
    if let Err(error) = write_spatial_state(&path, &state) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    Json(view_for(state)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mapkeeper_core::world;
    use tempfile::tempdir;

    #[test]
    fn ensure_writes_spatial_config_and_metric_grid() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mapkeeper.toml"), world::manifest_toml("t")).unwrap();
        let state = ensure_spatial_state(dir.path()).unwrap();
        assert_eq!(state.grid.neighbor_center_distance_m, 1000.0);
        assert_eq!(state.grid.width, 55);
        assert_eq!(state.grid.height, 36);
        let again = ensure_spatial_state(dir.path()).unwrap();
        assert_eq!(again, state);
        let manifest = world::parse_manifest(
            &std::fs::read_to_string(dir.path().join("mapkeeper.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.spatial.unwrap().preset_id, "wide_2000");
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
}
