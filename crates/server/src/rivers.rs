//! River catalog and auto-generate HTTP API (D-96 S4).

use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::layer::{DenseLayer, ELEVATION_LAYER_ID, LAKE_ID_LAYER_ID};
use mapkeeper_core::river_detach::{detach_tributary, RiverDetachError};
use mapkeeper_core::river_pin::{pin_cell_index, upsert_river_pin, RiverPinError};
use mapkeeper_core::rivers::{
    append_cell, cell_index, create_river, delete_river, pop_last_cell, RiverCatalog, RiverError,
};
use mapkeeper_core::worldgen::hydrology::{
    analyze_depressions, build_channel_graph, build_drainage_graph, classify_precip_input,
    derive_effective_seed, hydrology_policy_version, legacy_river_render_paths,
    river_render_paths, ChannelPolicy, HydrologyCatalog, HydrologySnapshot, NameMigrationReport,
    NamedRiverBinding, NamedRiverStore, HYDROLOGY_GENERATOR_VERSION, RiverDensity, RiverRenderPaths,
};
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::world_io;

fn river_error_status(err: RiverError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}

fn ensure_catalog_writable(world_path: &std::path::Path) -> Result<(), (StatusCode, String)> {
    match world_io::read_current_hydrology_snapshot(world_path) {
        Ok(Some(_)) => Err((
            StatusCode::CONFLICT,
            "legacy river catalog is read-only; generated rivers come from Hydrology v2".to_string(),
        )),
        Ok(None) => Ok(()),
        Err(err) => Err((StatusCode::CONFLICT, err)),
    }
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
    let bounds = world_io::map_bounds(&active.path);
    let elevation = world_io::read_dense_layer(&active.path, ELEVATION_LAYER_ID, &bounds);
    let lake_id = world_io::read_dense_layer(&active.path, LAKE_ID_LAYER_ID, &bounds);
    match world_io::read_current_hydrology_snapshot(&active.path) {
        Ok(Some(snapshot)) => Json(RiversResponse::from_snapshot(
            &snapshot, &bounds, &elevation, &lake_id,
        ))
        .into_response(),
        Ok(None) => Json(RiversResponse::from_catalog(
            world_io::read_river_catalog(&active.path),
            &bounds,
            &elevation,
            &lake_id,
        ))
        .into_response(),
        Err(err) => (StatusCode::CONFLICT, err).into_response(),
    }
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
    if let Err((status, message)) = ensure_catalog_writable(&active.path) {
        return (status, message).into_response();
    }
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
    if let Err((status, message)) = ensure_catalog_writable(&active.path) {
        return (status, message).into_response();
    }
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
    if let Err((status, message)) = ensure_catalog_writable(&active.path) {
        return (status, message).into_response();
    }
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

fn river_pin_error_status(err: RiverPinError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}

fn river_detach_error_status(err: RiverDetachError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}

async fn detach_river_handler(
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
    if let Err((status, message)) = ensure_catalog_writable(&active.path) {
        return (status, message).into_response();
    }
    let bounds = world_io::map_bounds(&active.path);
    let mut catalog = world_io::read_river_catalog(&active.path);
    if let Err(err) = detach_tributary(&mut catalog, river_id) {
        return river_detach_error_status(err).into_response();
    }
    if let Err(err) = world_io::persist_rivers(&active.path, &catalog, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(catalog).into_response()
}

async fn pin_river_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<RiverPinInput>,
) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    if let Err((status, message)) = ensure_catalog_writable(&active.path) {
        return (status, message).into_response();
    }
    let bounds = world_io::map_bounds(&active.path);
    let elevation = world_io::read_dense_layer(&active.path, ELEVATION_LAYER_ID, &bounds);
    let source = match pin_cell_index(&bounds, input.source_q, input.source_r) {
        Ok(index) => index,
        Err(err) => return river_pin_error_status(err).into_response(),
    };
    let mouth = match pin_cell_index(&bounds, input.mouth_q, input.mouth_r) {
        Ok(index) => index,
        Err(err) => return river_pin_error_status(err).into_response(),
    };
    let mut catalog = world_io::read_river_catalog(&active.path);
    match upsert_river_pin(
        &mut catalog,
        &bounds,
        &elevation,
        source,
        mouth,
        input.river_id,
    ) {
        Ok(river_id) => {
            if let Err(err) = world_io::persist_rivers(&active.path, &catalog, &bounds) {
                return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
            }
            Json(RiverPinResponse {
                river_id,
                catalog,
            })
            .into_response()
        }
        Err(err) => river_pin_error_status(err).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct RiverPinInput {
    source_q: i32,
    source_r: i32,
    mouth_q: i32,
    mouth_r: i32,
    river_id: Option<u32>,
}

#[derive(Serialize)]
struct RiverPinResponse {
    river_id: u32,
    #[serde(flatten)]
    catalog: RiverCatalog,
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
    if let Err((status, message)) = ensure_catalog_writable(&active.path) {
        return (status, message).into_response();
    }
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
struct RiversResponse {
    #[serde(flatten)]
    catalog: RiverCatalog,
    render_paths: RiverRenderPaths,
    read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_segment_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_cell_count: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    named_rivers: Vec<NamedRiverBinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    name_migration: Vec<NameMigrationReport>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    compatibility_projection: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    named_river_count: Option<usize>,
}

impl RiversResponse {
    fn from_snapshot(
        snapshot: &HydrologySnapshot,
        bounds: &MapBounds,
        elevation: &DenseLayer,
        lake_id: &DenseLayer,
    ) -> Self {
        let graph = &snapshot.channels.river_graph;
        Self {
            catalog: snapshot.catalog.compatibility_river_catalog(),
            render_paths: river_render_paths(graph, bounds, elevation, Some(lake_id)),
            read_only: true,
            channel_segment_count: Some(snapshot.catalog.physical_segments.len()),
            channel_cell_count: Some(graph.channel_mask.iter().filter(|&&c| c).count()),
            named_rivers: snapshot.catalog.named_rivers.clone(),
            name_migration: snapshot.catalog.migration.clone(),
            compatibility_projection: true,
            named_river_count: Some(snapshot.catalog.named_river_count()),
        }
    }

    fn from_catalog(
        catalog: RiverCatalog,
        bounds: &MapBounds,
        elevation: &DenseLayer,
        _lake_id: &DenseLayer,
    ) -> Self {
        let paths = catalog
            .rivers
            .iter()
            .filter(|river| !river.cells.is_empty())
            .map(|river| river.cells.clone())
            .collect();
        Self {
            catalog,
            render_paths: legacy_river_render_paths(paths, bounds, elevation),
            read_only: false,
            channel_segment_count: None,
            channel_cell_count: None,
            named_rivers: Vec::new(),
            name_migration: Vec::new(),
            compatibility_projection: false,
            named_river_count: None,
        }
    }
}

#[derive(Serialize)]
struct RiversGenerateResponse {
    #[serde(flatten)]
    response: RiversResponse,
    precip_input_state: &'static str,
    precip_source: &'static str,
    river_density: &'static str,
    name_migration_ambiguous_count: usize,
    deterministic: bool,
    input_fingerprint: String,
    regenerate_nonce_ignored: bool,
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
    let lake_id = world_io::read_dense_layer(&active.path, LAKE_ID_LAYER_ID, &bounds);
    let precipitation = world_io::read_optional_precip_layer(&active.path, &bounds);
    let precip_state = classify_precip_input(&elevation, precipitation.as_ref());
    let lakes = world_io::read_lake_catalog(&active.path);
    let analysis = analyze_depressions(&elevation, &bounds);
    let density = input
        .river_density
        .as_deref()
        .map(RiverDensity::parse)
        .unwrap_or(RiverDensity::Balanced);
    let drainage = match build_drainage_graph(&analysis, &lakes, &bounds) {
        Ok(graph) => graph,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid drainage graph: {err:?}"),
            )
                .into_response()
        }
    };
    let channels = match build_channel_graph(
        &drainage,
        &analysis,
        precipitation.as_ref(),
        precip_state,
        channel_policy(density),
    ) {
        Ok(graph) => graph,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid channel graph: {err:?}"),
            )
                .into_response()
        }
    };
    let legacy = world_io::read_river_catalog(&active.path);
    let prior_snapshot = world_io::read_current_hydrology_snapshot(&active.path).ok().flatten();
    let prior_store = world_io::read_named_river_store(&active.path);
    let catalog = HydrologyCatalog::from_river_graph(
        &channels.river_graph,
        &legacy,
        Some(&prior_store),
        prior_snapshot.as_ref().map(|snapshot| &snapshot.catalog),
    );
    let name_migration_ambiguous_count = catalog
        .migration
        .iter()
        .filter(|report| report.ambiguous)
        .count();
    let (base_revision, fingerprint) = world_io::hydrology_base_fingerprint(&active.path);
    let policy_version = hydrology_policy_version(density.id());
    let effective_seed = derive_effective_seed(base_revision, &policy_version);
    let snapshot = HydrologySnapshot::new(
        base_revision,
        fingerprint.clone(),
        HYDROLOGY_GENERATOR_VERSION.to_string(),
        policy_version,
        effective_seed,
        drainage,
        channels,
    )
    .with_catalog(catalog);
    if let Err(err) = world_io::persist_hydrology_snapshot(&active.path, &snapshot) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    if let Err(err) = world_io::write_named_river_store(
        &active.path,
        &NamedRiverStore::from_catalog(&snapshot.catalog),
    ) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(RiversGenerateResponse {
        response: RiversResponse::from_snapshot(&snapshot, &bounds, &elevation, &lake_id),
        precip_input_state: precip_state.id(),
        precip_source: precip_state.legacy_precip_source(),
        river_density: density.id(),
        name_migration_ambiguous_count,
        deterministic: true,
        input_fingerprint: fingerprint,
        regenerate_nonce_ignored: input.regenerate_nonce.is_some(),
    })
    .into_response()
}

fn channel_policy(density: RiverDensity) -> ChannelPolicy {
    let base = ChannelPolicy::default();
    match density {
        RiverDensity::Few => ChannelPolicy {
            min_flow: base.min_flow.saturating_mul(2),
            min_contributing_area: base.min_contributing_area.saturating_add(1),
        },
        RiverDensity::Balanced => base,
        RiverDensity::Many => ChannelPolicy {
            min_flow: (base.min_flow / 2).max(1),
            min_contributing_area: 1,
        },
    }
}

pub(crate) fn routes() -> Router<Arc<Mutex<AppState>>> {
    Router::new()
        .route("/api/rivers", get(get_rivers).put(put_rivers))
        .route("/api/rivers/pin", post(pin_river_handler))
        .route("/api/rivers/:id/detach", post(detach_river_handler))
        .route("/api/rivers/append", post(append_river_cell))
        .route("/api/rivers/:id/pop", post(pop_river_cell))
        .route("/api/rivers/:id", delete(delete_river_handler))
        .route("/api/rivers/generate", post(generate_rivers_handler))
}
