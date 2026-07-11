//! Read-only legacy hydrology diagnostics (hydrology-v2--diagnostics-baseline).

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::lakes::sync_lake_id_layer;
use mapkeeper_core::layer::{ELEVATION_LAYER_ID, LAKE_ID_LAYER_ID, RIVER_ID_LAYER_ID};
use mapkeeper_core::rivers::sync_river_id_layer;
use mapkeeper_core::worldgen::hydrology::{analyze_depressions, diagnose_legacy_hydrology};
use serde::Serialize;

use crate::state::AppState;
use crate::world_io;

#[derive(Serialize)]
struct HydrologyDiagnosticsResponse {
    #[serde(flatten)]
    diagnostics: mapkeeper_core::worldgen::hydrology::LegacyHydrologyDiagnostics,
    river_id_matches_catalog: bool,
    lake_id_matches_catalog: bool,
}

async fn get_hydrology_diagnostics(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    let bounds = world_io::map_bounds(&active.path);
    let elevation = world_io::read_dense_layer(&active.path, ELEVATION_LAYER_ID, &bounds);
    let analysis = analyze_depressions(&elevation, &bounds);
    let rivers = world_io::read_river_catalog(&active.path);
    let lakes = world_io::read_lake_catalog(&active.path);
    let diagnostics = diagnose_legacy_hydrology(&analysis, &rivers, &lakes, &bounds);
    let river_id = world_io::read_dense_layer(&active.path, RIVER_ID_LAYER_ID, &bounds);
    let lake_id = world_io::read_dense_layer(&active.path, LAKE_ID_LAYER_ID, &bounds);
    let response = HydrologyDiagnosticsResponse {
        diagnostics,
        river_id_matches_catalog: river_id == sync_river_id_layer(&rivers, &bounds),
        lake_id_matches_catalog: lake_id == sync_lake_id_layer(&lakes, &bounds),
    };
    Json(response).into_response()
}

pub(crate) fn routes() -> Router<Arc<Mutex<AppState>>> {
    Router::new().route("/api/hydrology/diagnostics", get(get_hydrology_diagnostics))
}
