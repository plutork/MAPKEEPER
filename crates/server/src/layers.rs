//! Map, profile, and generic layer HTTP API (D-96 S3).

use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::hex::Axial;
use mapkeeper_core::layer::{LayerCellWrite, WireCellState};
use mapkeeper_core::profile::CellProfile;
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::world_io;
use crate::{bounds_response, MapBoundsResponse};

#[derive(Serialize)]
struct CellSummary {
    cell_id: String,
    q: i32,
    r: i32,
    display_name: String,
}

#[derive(Serialize)]
struct MapResponse {
    world_id: String,
    bounds: MapBoundsResponse,
    legacy_map: bool,
    cells: Vec<CellSummary>,
}

#[derive(Deserialize)]
struct ProfileInput {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    notes: String,
}

pub(crate) fn routes() -> Router<Arc<Mutex<AppState>>> {
    Router::new()
        .route("/api/map", get(get_map))
        .route(
            "/api/cells/:q/:r/profile",
            get(get_profile).put(put_profile),
        )
        .route("/api/layers/:id", get(get_layer))
        .route("/api/layers/:id/batch", axum::routing::put(put_layer_batch))
        .route(
            "/api/layers/:id/cells/:q/:r",
            axum::routing::put(put_layer_cell),
        )
}

async fn get_map(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    let dir = world_io::profiles_dir(&active.path);
    let mut cells = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(profile) = serde_json::from_str::<CellProfile>(&raw) else {
                continue;
            };
            let Some(id) = CellId::parse(&profile.cell_id) else {
                continue;
            };
            cells.push(CellSummary {
                cell_id: profile.cell_id,
                q: id.q,
                r: id.r,
                display_name: profile.display_name,
            });
        }
    }
    let (bounds, legacy_map) = world_io::read_map_bounds(&active.path);
    Json(MapResponse {
        world_id: active.id.clone(),
        bounds: bounds_response(&bounds),
        legacy_map,
        cells,
    })
    .into_response()
}

async fn get_profile(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath((q, r)): AxPath<(i32, i32)>,
) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    let id = CellId::new(&active.id, q, r);
    let path = world_io::profile_path(&active.path, &active.id, q, r);
    let profile = match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(profile) => profile,
            Err(err) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            }
        },
        Err(_) => CellProfile::new(&id, ""),
    };
    Json(profile).into_response()
}

async fn put_profile(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath((q, r)): AxPath<(i32, i32)>,
    Json(input): Json<ProfileInput>,
) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    let id = CellId::new(&active.id, q, r);
    let mut profile = CellProfile::new(&id, input.display_name);
    profile.notes = input.notes;

    let issues = profile.validate();
    if issues
        .iter()
        .any(|i| matches!(i, mapkeeper_core::profile::ValidationIssue::Error(_)))
    {
        return (StatusCode::BAD_REQUEST, format!("{issues:?}")).into_response();
    }

    let dir = world_io::profiles_dir(&active.path);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }
    let path = world_io::profile_path(&active.path, &active.id, q, r);
    let body = match serde_json::to_string_pretty(&profile) {
        Ok(body) => body,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };
    if let Err(err) = std::fs::write(&path, body) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }
    Json(profile).into_response()
}

async fn get_layer(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath(layer_id): AxPath<String>,
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
    Json(world_io::read_dense_layer(&active.path, &layer_id, &bounds)).into_response()
}

async fn put_layer_batch(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath(layer_id): AxPath<String>,
    Json(updates): Json<Vec<LayerCellWrite>>,
) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    if updates.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let bounds = world_io::map_bounds(&active.path);
    let mut dense = world_io::read_dense_layer(&active.path, &layer_id, &bounds);
    for item in updates {
        let Some(index) = bounds.index_of(Axial::new(item.q, item.r)) else {
            continue;
        };
        if let Some(new_state) = item.state.to_dense(dense.value_type) {
            dense.set(index, new_state);
        }
    }
    if let Err(err) = world_io::write_dense_layer(&active.path, &dense) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn put_layer_cell(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath((layer_id, q, r)): AxPath<(String, i32, i32)>,
    Json(new_state): Json<WireCellState>,
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
    let Some(index) = bounds.index_of(Axial::new(q, r)) else {
        return (StatusCode::BAD_REQUEST, "cell out of map bounds").into_response();
    };
    let mut dense = world_io::read_dense_layer(&active.path, &layer_id, &bounds);
    let Some(resolved) = new_state.to_dense(dense.value_type) else {
        return (
            StatusCode::BAD_REQUEST,
            "value kind does not match layer value_type",
        )
            .into_response();
    };
    dense.set(index, resolved);
    if let Err(err) = world_io::write_dense_layer(&active.path, &dense) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(WireCellState::from_dense(dense.state(index))).into_response()
}
