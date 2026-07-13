//! Read-only diagnostics for the active Hydrology v2 snapshot.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::lakes::sync_lake_id_layer;
use mapkeeper_core::layer::{ELEVATION_LAYER_ID, LAKE_ID_LAYER_ID, RIVER_ID_LAYER_ID};
use mapkeeper_core::worldgen::hydrology::{
    analyze_depressions, classify_precip_input, compatibility_river_id_layer, diagnose_hydrology,
};
use serde::Serialize;

use crate::state::ServerState;
use crate::world_io;
use crate::world_scope::{self, ScopeMode};

#[derive(Serialize)]
struct HydrologyDiagnosticsResponse {
    #[serde(flatten)]
    diagnostics: mapkeeper_core::worldgen::hydrology::HydrologyDiagnostics,
    river_id_matches_snapshot: bool,
    lake_id_matches_catalog: bool,
}

async fn get_hydrology_diagnostics(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let world = match world_scope::resolve_world(&server.app, &headers, ScopeMode::Read) {
        Ok(world) => world,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    let elevation = world_io::read_dense_layer(&world.path, ELEVATION_LAYER_ID, &bounds);
    let precipitation = world_io::read_optional_precip_layer(&world.path, &bounds);
    let precip_state = classify_precip_input(&elevation, precipitation.as_ref());
    let analysis = analyze_depressions(&elevation, &bounds);
    let lakes = world_io::read_lake_catalog(&world.path);
    let snapshot = match world_io::read_current_hydrology_snapshot(&world.path) {
        Ok(snapshot) => snapshot,
        Err(err) => return (StatusCode::CONFLICT, err).into_response(),
    };
    let diagnostics = diagnose_hydrology(&analysis, &lakes, snapshot.as_ref(), precip_state);
    let river_id = world_io::read_dense_layer(&world.path, RIVER_ID_LAYER_ID, &bounds);
    let lake_id = world_io::read_dense_layer(&world.path, LAKE_ID_LAYER_ID, &bounds);
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

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new().route("/api/hydrology/diagnostics", get(get_hydrology_diagnostics))
}
