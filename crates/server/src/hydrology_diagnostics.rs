//! Read-only diagnostics for the active Hydrology v2 snapshot.

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::lakes::sync_lake_id_layer;
use mapkeeper_core::layer::{ELEVATION_LAYER_ID, LAKE_ID_LAYER_ID, RIVER_ID_LAYER_ID};
use mapkeeper_core::worldgen::hydrology::{
    analyze_depressions, classify_precip_input, compatibility_river_id_layer, diagnose_hydrology,
};
use serde::Serialize;

use crate::state::AppState;
use crate::world_io;

#[derive(Serialize)]
struct HydrologyDiagnosticsResponse {
    #[serde(flatten)]
    diagnostics: mapkeeper_core::worldgen::hydrology::HydrologyDiagnostics,
    river_id_matches_snapshot: bool,
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
    let precipitation = world_io::read_optional_precip_layer(&active.path, &bounds);
    let precip_state = classify_precip_input(&elevation, precipitation.as_ref());
    let analysis = analyze_depressions(&elevation, &bounds);
    let lakes = world_io::read_lake_catalog(&active.path);
    let snapshot = match world_io::read_current_hydrology_snapshot(&active.path) {
        Ok(snapshot) => snapshot,
        Err(err) => return (StatusCode::CONFLICT, err).into_response(),
    };
    let diagnostics = diagnose_hydrology(&analysis, &lakes, snapshot.as_ref(), precip_state);
    let river_id = world_io::read_dense_layer(&active.path, RIVER_ID_LAYER_ID, &bounds);
    let lake_id = world_io::read_dense_layer(&active.path, LAKE_ID_LAYER_ID, &bounds);
    let response = HydrologyDiagnosticsResponse {
        diagnostics,
        river_id_matches_snapshot: snapshot.as_ref().is_some_and(|snapshot| {
            river_id
                == compatibility_river_id_layer(
                    &snapshot.channels.river_graph,
                    snapshot.channels.river_graph.channel_mask.len(),
                )
        }),
        lake_id_matches_catalog: lake_id == sync_lake_id_layer(&lakes, &bounds),
    };
    Json(response).into_response()
}

pub(crate) fn routes() -> Router<Arc<Mutex<AppState>>> {
    Router::new().route("/api/hydrology/diagnostics", get(get_hydrology_diagnostics))
}
