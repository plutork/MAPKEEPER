//! River catalog and auto-generate HTTP API (D-96 S4).

use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use mapkeeper_core::layer::ELEVATION_LAYER_ID;
use mapkeeper_core::river_flux::{generate_with_owners, RiverFluxParams};
use mapkeeper_core::rivers::{
    append_cell, cell_index, create_river, delete_river, pop_last_cell, RiverCatalog, RiverError,
};
use mapkeeper_core::worldgen::hydrology::{analyze_depressions, RiverDensity};
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::world_io;

fn river_error_status(err: RiverError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}

async fn get_rivers(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    Json(world_io::read_river_catalog(&active.path)).into_response()
}

async fn put_rivers(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(catalog): Json<RiverCatalog>,
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
    if let Err(err) = world_io::persist_rivers(&active.path, &catalog, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(catalog).into_response()
}

#[derive(Debug, Deserialize)]
struct RiverAppendInput {
    river_id: Option<u32>,
    q: i32,
    r: i32,
}

async fn append_river_cell(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<RiverAppendInput>,
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
    let index = match cell_index(&bounds, input.q, input.r) {
        Ok(i) => i,
        Err(err) => return river_error_status(err).into_response(),
    };
    let mut catalog = world_io::read_river_catalog(&active.path);
    let result = match input.river_id {
        Some(id) => append_cell(&mut catalog, &bounds, id, index).map(|_| id),
        None => create_river(&mut catalog, &bounds, index),
    };
    match result {
        Ok(_) => {
            if let Err(err) = world_io::persist_rivers(&active.path, &catalog, &bounds) {
                return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
            }
            Json(catalog).into_response()
        }
        Err(err) => river_error_status(err).into_response(),
    }
}

async fn pop_river_cell(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath(river_id): AxPath<u32>,
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
    let mut catalog = world_io::read_river_catalog(&active.path);
    if let Err(err) = pop_last_cell(&mut catalog, river_id) {
        return river_error_status(err).into_response();
    }
    if let Err(err) = world_io::persist_rivers(&active.path, &catalog, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(catalog).into_response()
}

async fn delete_river_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath(river_id): AxPath<u32>,
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
    let mut catalog = world_io::read_river_catalog(&active.path);
    if let Err(err) = delete_river(&mut catalog, river_id) {
        return river_error_status(err).into_response();
    }
    if let Err(err) = world_io::persist_rivers(&active.path, &catalog, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(catalog).into_response()
}

#[derive(Serialize)]
struct RiversGenerateResponse {
    #[serde(flatten)]
    catalog: RiverCatalog,
    precip_source: &'static str,
    river_density: &'static str,
    rejected_river_count: u32,
}

#[derive(Debug, Deserialize, Default)]
struct RiversGenerateInput {
    river_density: Option<String>,
    regenerate_nonce: Option<u32>,
}

async fn generate_rivers_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    body: Option<Json<RiversGenerateInput>>,
) -> impl IntoResponse {
    let input = body.map(|Json(b)| b).unwrap_or_default();
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
    let lakes = world_io::read_lake_catalog(&active.path);
    let analysis = analyze_depressions(&elevation, &bounds);
    let density = input
        .river_density
        .as_deref()
        .map(RiverDensity::parse)
        .unwrap_or(RiverDensity::Balanced);
    let _nonce = input.regenerate_nonce.unwrap_or(0);
    let lakes_ref = if lakes.lakes.is_empty() {
        None
    } else {
        Some(&lakes)
    };
    let out = generate_with_owners(
        &elevation,
        &bounds,
        precipitation.as_ref(),
        RiverFluxParams {
            analysis: Some(&analysis),
            lakes: lakes_ref,
            density,
        },
    );
    if let Err(err) = world_io::persist_generated_rivers(&active.path, &out.catalog, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    let precip_source = if out.used_climate {
        "climate"
    } else {
        "uniform_fallback"
    };
    Json(RiversGenerateResponse {
        catalog: out.catalog,
        precip_source,
        river_density: density.id(),
        rejected_river_count: out.rejected_rivers,
    })
    .into_response()
}

pub(crate) fn routes() -> Router<Arc<Mutex<AppState>>> {
    Router::new()
        .route("/api/rivers", get(get_rivers).put(put_rivers))
        .route("/api/rivers/append", post(append_river_cell))
        .route("/api/rivers/:id/pop", post(pop_river_cell))
        .route("/api/rivers/:id", delete(delete_river_handler))
        .route("/api/rivers/generate", post(generate_rivers_handler))
}
