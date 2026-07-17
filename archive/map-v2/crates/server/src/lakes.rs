//! Lake catalog and auto-generate HTTP API.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use mapkeeper_core::lakes::{validate_catalog, LakeCatalog, LakeError};
use mapkeeper_core::layer::ELEVATION_LAYER_ID;
use mapkeeper_core::worldgen::hydrology::{
    analyze_depressions, classify_precip_input, generate_lakes, LakeDensity,
};
use serde::{Deserialize, Serialize};

use crate::state::ServerState;
use crate::world_io;
use crate::world_lock;
use crate::world_revision::{self, parse_base_revision};
use crate::world_scope::{self, ScopeMode};

fn lake_error_status(err: LakeError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}

async fn get_lakes(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let world = match world_scope::resolve_world(&server.app, &headers, ScopeMode::Read) {
        Ok(world) => world,
        Err(err) => return err.into_response(),
    };
    Json(world_io::read_lake_catalog(&world.path)).into_response()
}

async fn put_lakes(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(catalog): Json<LakeCatalog>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    if let Err(err) = validate_catalog(&catalog, &bounds) {
        return lake_error_status(err).into_response();
    }
    let base_revision = parse_base_revision(&headers, None);
    match world_io::persist_lakes(&world.path, &catalog, &bounds, base_revision) {
        Ok(revision) => world_revision::json_with_revision(catalog, revision).into_response(),
        Err(err) => err.into_revision_response(),
    }
}

#[derive(Debug, Deserialize)]
struct LakesGenerateInput {
    density: Option<String>,
    seed: Option<u64>,
}

#[derive(Serialize)]
struct LakesGenerateResponse {
    #[serde(flatten)]
    catalog: LakeCatalog,
    rivers_cleared: bool,
    precip_input_state: &'static str,
    /// `seed` affects lake tie-break ordering only — not depression topology.
    seed_role: &'static str,
}

async fn generate_lakes_handler(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(input): Json<LakesGenerateInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    let elevation = world_io::read_dense_layer(&world.path, ELEVATION_LAYER_ID, &bounds);
    let precipitation = world_io::read_optional_precip_layer(&world.path, &bounds);
    let precip_state = classify_precip_input(&elevation, precipitation.as_ref());
    let analysis = analyze_depressions(&elevation, &bounds);
    let density = input
        .density
        .as_deref()
        .map(LakeDensity::parse)
        .unwrap_or(LakeDensity::Balanced);
    let seed = input.seed.unwrap_or(1);
    let catalog = generate_lakes(
        &analysis,
        &elevation,
        precipitation.as_ref(),
        &bounds,
        density,
        seed,
    );
    let base_revision = parse_base_revision(&headers, None);
    match world_io::persist_lake_generation(&world.path, &catalog, &bounds, base_revision) {
        Ok(revision) => world_revision::json_with_revision(
            LakesGenerateResponse {
                catalog,
                rivers_cleared: true,
                precip_input_state: precip_state.id(),
                seed_role: "tie_break_only",
            },
            revision,
        )
        .into_response(),
        Err(err) => err.into_revision_response(),
    }
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/lakes", get(get_lakes).put(put_lakes))
        .route("/api/lakes/generate", post(generate_lakes_handler))
}
