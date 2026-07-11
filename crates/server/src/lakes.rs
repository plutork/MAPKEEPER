//! Lake catalog HTTP API (hydrology-lake-domain-v1).

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::lakes::{validate_catalog, LakeCatalog, LakeError};

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

pub(crate) fn routes() -> Router<Arc<Mutex<AppState>>> {
    Router::new().route("/api/lakes", get(get_lakes).put(put_lakes))
}
