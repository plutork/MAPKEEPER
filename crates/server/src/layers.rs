//! Map, profile, and generic layer HTTP API (D-96 S3).

use std::sync::Arc;

use axum::extract::{Path as AxPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::hex::Axial;
use mapkeeper_core::layer::{LayerCellWrite, WireCellState};
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::worldgen::hydrology::is_derived_hydrology_layer_id;
use serde::{Deserialize, Serialize};

use crate::state::ServerState;
use crate::world_io;
use crate::world_lock;
use crate::world_revision::{self, parse_base_revision};
use crate::world_scope::{self, ScopeMode};
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
    revision: u64,
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
    #[serde(default)]
    base_revision: Option<u64>,
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
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

async fn get_map(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let world = match world_scope::resolve_world(&server.app, &headers, ScopeMode::Read) {
        Ok(world) => world,
        Err(err) => return err.into_response(),
    };
    let dir = world_io::profiles_dir(&world.path);
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
    let (bounds, legacy_map) = world_io::read_map_bounds(&world.path);
    let revision = world_revision::read_world_revision(&world.path).unwrap_or(0);
    Json(MapResponse {
        world_id: world.id.clone(),
        revision,
        bounds: bounds_response(&bounds),
        legacy_map,
        cells,
    })
    .into_response()
}

async fn get_profile(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath((q, r)): AxPath<(i32, i32)>,
) -> impl IntoResponse {
    let world = match world_scope::resolve_world(&server.app, &headers, ScopeMode::Read) {
        Ok(world) => world,
        Err(err) => return err.into_response(),
    };
    let id = CellId::new(&world.id, q, r);
    let path = world_io::profile_path(&world.path, &world.id, q, r);
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
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath((q, r)): AxPath<(i32, i32)>,
    Json(input): Json<ProfileInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let id = CellId::new(&world.id, q, r);
    let mut profile = CellProfile::new(&id, input.display_name);
    profile.notes = input.notes;

    let issues = profile.validate();
    if issues
        .iter()
        .any(|i| matches!(i, mapkeeper_core::profile::ValidationIssue::Error(_)))
    {
        return (StatusCode::BAD_REQUEST, format!("{issues:?}")).into_response();
    }

    let base_revision = parse_base_revision(&headers, input.base_revision);
    match world_revision::mutate_map(&world.path, base_revision, || {
        let dir = world_io::profiles_dir(&world.path);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = world_io::profile_path(&world.path, &world.id, q, r);
        let body = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        Ok(profile)
    }) {
        Ok((profile, revision)) => world_revision::json_with_revision(profile, revision).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn get_layer(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath(layer_id): AxPath<String>,
) -> impl IntoResponse {
    let world = match world_scope::resolve_world(&server.app, &headers, ScopeMode::Read) {
        Ok(world) => world,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    Json(world_io::read_dense_layer(&world.path, &layer_id, &bounds)).into_response()
}

async fn put_layer_batch(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath(layer_id): AxPath<String>,
    Json(updates): Json<Vec<LayerCellWrite>>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    if updates.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    if is_derived_hydrology_layer_id(&layer_id) {
        return (
            StatusCode::FORBIDDEN,
            "derived Hydrology v2 layers are activated only as a snapshot",
        )
            .into_response();
    }
    let base_revision = parse_base_revision(&headers, None);
    match world_revision::mutate_map(&world.path, base_revision, || {
        let bounds = world_io::map_bounds(&world.path);
        let mut dense = world_io::read_dense_layer(&world.path, &layer_id, &bounds);
        for item in &updates {
            let Some(index) = bounds.index_of(Axial::new(item.q, item.r)) else {
                continue;
            };
            if let Some(new_state) = item.state.to_dense(dense.value_type) {
                dense.set(index, new_state);
            }
        }
        world_io::write_dense_layer(&world.path, &dense)
    }) {
        Ok(((), revision)) => world_revision::no_content_with_revision(revision).into_response(),
        Err(err) => match err {
            world_revision::RevisionMutationError::Internal(msg)
                if msg == "world state is locked" =>
            {
                (StatusCode::FORBIDDEN, msg).into_response()
            }
            other => other.into_response(),
        },
    }
}

async fn put_layer_cell(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath((layer_id, q, r)): AxPath<(String, i32, i32)>,
    Json(new_state): Json<WireCellState>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    if is_derived_hydrology_layer_id(&layer_id) {
        return (
            StatusCode::FORBIDDEN,
            "derived Hydrology v2 layers are activated only as a snapshot",
        )
            .into_response();
    }
    let base_revision = parse_base_revision(&headers, None);
    match world_revision::mutate_map(&world.path, base_revision, || {
        let bounds = world_io::map_bounds(&world.path);
        let Some(index) = bounds.index_of(Axial::new(q, r)) else {
            return Err("cell out of map bounds".to_string());
        };
        let mut dense = world_io::read_dense_layer(&world.path, &layer_id, &bounds);
        let Some(resolved) = new_state.to_dense(dense.value_type) else {
            return Err("value kind does not match layer value_type".to_string());
        };
        dense.set(index, resolved);
        world_io::write_dense_layer(&world.path, &dense)?;
        Ok(WireCellState::from_dense(dense.state(index)))
    }) {
        Ok((wire, revision)) => world_revision::json_with_revision(wire, revision).into_response(),
        Err(err) => match err {
            world_revision::RevisionMutationError::Internal(msg)
                if msg == "world state is locked" =>
            {
                (StatusCode::FORBIDDEN, msg).into_response()
            }
            world_revision::RevisionMutationError::Internal(msg)
                if msg == "cell out of map bounds" =>
            {
                (StatusCode::BAD_REQUEST, msg).into_response()
            }
            world_revision::RevisionMutationError::Internal(msg)
                if msg.contains("value kind") =>
            {
                (StatusCode::BAD_REQUEST, msg).into_response()
            }
            other => other.into_response(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mapkeeper_core::worldgen::hydrology::CHANNEL_NODE_LAYER_ID;
    use tempfile::tempdir;

    use axum::http::HeaderMap;

    #[tokio::test]
    async fn generic_writes_reject_derived_hydrology_layers() {
        let dir = tempdir().unwrap();
        let server = Arc::new(ServerState::new(Some(crate::state::ActiveWorld {
            path: dir.path().to_path_buf(),
            id: "test".to_string(),
        })));
        let response = put_layer_cell(
            State(server),
            HeaderMap::new(),
            AxPath((CHANNEL_NODE_LAYER_ID.to_string(), 0, 0)),
            Json(WireCellState::None),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
