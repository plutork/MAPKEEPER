//! Lake catalog and auto-generate HTTP API.

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use mapkeeper_core::layer::ELEVATION_LAYER_ID;
use mapkeeper_core::lakes::{validate_catalog, LakeCatalog, LakeError};
use mapkeeper_core::worldgen::hydrology::{analyze_depressions, generate_lakes, LakeDensity};
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::world_io;

fn lake_error_status(err: LakeError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}

async fn get_lakes(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    Json(world_io::read_lake_catalog(&active.path)).into_response()
}

async fn put_lakes(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(catalog): Json<LakeCatalog>,
) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    let bounds = world_io::map_bounds(&active.path);
    if let Err(err) = validate_catalog(&catalog, &bounds) {
        return lake_error_status(err).into_response();
    }
    if let Err(err) = world_io::persist_lakes(&active.path, &catalog, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(catalog).into_response()
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
}

async fn generate_lakes_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<LakesGenerateInput>,
) -> impl IntoResponse {
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
    if let Err(err) = world_io::persist_lake_generation(&active.path, &catalog, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(LakesGenerateResponse {
        catalog,
        rivers_cleared: true,
    })
    .into_response()
}

pub(crate) fn routes() -> Router<Arc<Mutex<AppState>>> {
    Router::new()
        .route("/api/lakes", get(get_lakes).put(put_lakes))
        .route("/api/lakes/generate", post(generate_lakes_handler))
}
