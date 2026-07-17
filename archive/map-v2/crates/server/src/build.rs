//! Build wizard API — draft state, bounds, pipeline generate steps (D-96 S2).

use std::collections::VecDeque;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{Json, Router};
use mapkeeper_core::build_state::{self, BUILD_STEP_SIZE};
use mapkeeper_core::climate::{generate_climate_layers, PrecipitationStyle};
use mapkeeper_core::elevation_gen::{elevation_from_land_mask_and_geology, ElevationIntensity};
use mapkeeper_core::geology::{generate_geology, GeologyStyle, GEOLOGY_LAYER_ID};
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::land_mask::{
    elevation_from_land_mask, find_recipe, generate_land_mask, generate_land_mask_recipe,
    normalize_kind, LayoutClass, ShoreCharacter, LAND_MASK_INLAND_SEA, LAND_MASK_LAND,
    LAND_MASK_LAYER_ID, LAND_MASK_OCEAN,
};
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue, ELEVATION_LAYER_ID};
use mapkeeper_core::map_preset::parse_map_preset;
use serde::{Deserialize, Serialize};

use crate::state::ServerState;
use crate::world_io;
use crate::world_lock;
use crate::world_revision::{self, parse_base_revision};
use crate::{bounds_response, MapBoundsResponse};

#[derive(Deserialize)]
struct BuildStateInput {
    status: String,
    #[serde(default)]
    step: Option<u32>,
    #[serde(default)]
    base_revision: Option<u64>,
}

#[derive(Deserialize)]
struct BuildBoundsInput {
    map_preset: String,
    #[serde(default)]
    base_revision: Option<u64>,
}

#[derive(Serialize)]
struct BuildBoundsResponse {
    bounds: MapBoundsResponse,
    reset: bool,
}

#[derive(Deserialize)]
struct LandMaskGenerateInput {
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    recipe_id: Option<String>,
    #[serde(default)]
    character: Option<String>,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    regenerate_nonce: Option<u32>,
}

#[derive(Deserialize)]
struct GeologyGenerateInput {
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    regenerate_nonce: Option<u32>,
}

#[derive(Deserialize)]
struct ElevationGenerateInput {
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    regenerate_nonce: Option<u32>,
}

#[derive(Deserialize)]
struct ClimateGenerateInput {
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    regenerate_nonce: Option<u32>,
}

#[derive(Serialize)]
struct LandMaskGenerateResponse {
    seed: u64,
    recipe_id: String,
    layout_class: String,
    character: String,
    regenerate_nonce: u64,
}

#[derive(Deserialize)]
struct LandMaskCellInput {
    q: i32,
    r: i32,
    kind: String,
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/build", axum::routing::put(put_build_state))
        .route("/api/build/bounds", axum::routing::put(put_build_bounds))
        .route(
            "/api/build/land-mask/generate",
            axum::routing::post(generate_land_mask_handler),
        )
        .route(
            "/api/build/land-mask/cells",
            axum::routing::put(put_land_mask_cells),
        )
        .route(
            "/api/build/geology/generate",
            axum::routing::post(generate_geology_handler),
        )
        .route(
            "/api/build/elevation/generate",
            axum::routing::post(generate_elevation_handler),
        )
        .route(
            "/api/build/climate/generate",
            axum::routing::post(generate_climate_handler),
        )
}

async fn put_build_state(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(input): Json<BuildStateInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let base_revision = parse_base_revision(&headers, input.base_revision);
    match world_revision::mutate_map(&world.path, base_revision, || {
        match input.status.as_str() {
            "draft" => {
                let step = input.step.unwrap_or(BUILD_STEP_SIZE);
                build_state::write_build_draft(&world.path, step).map_err(|e| e.to_string())
            }
            "complete" => build_state::clear_build(&world.path).map_err(|e| e.to_string()),
            _ => Err("status must be draft or complete".to_string()),
        }
    }) {
        Ok(((), revision)) => world_revision::no_content_with_revision(revision).into_response(),
        Err(err) => match err {
            world_revision::RevisionMutationError::Internal(msg)
                if msg == "status must be draft or complete" =>
            {
                (StatusCode::BAD_REQUEST, msg).into_response()
            }
            other => other.into_response(),
        },
    }
}

async fn put_build_bounds(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(input): Json<BuildBoundsInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let Some(preset) = parse_map_preset(&input.map_preset) else {
        return (StatusCode::BAD_REQUEST, "unknown map_preset").into_response();
    };
    let reset = world_io::pipeline_has_downstream(&world.path);
    let base_revision = parse_base_revision(&headers, input.base_revision);
    match world_revision::mutate_map(&world.path, base_revision, || {
        world_io::reset_build_bounds(&world.path, preset).map_err(|e| e.to_string())
    }) {
        Ok((bounds, revision)) => world_revision::json_with_revision(
            BuildBoundsResponse {
                bounds: bounds_response(&bounds),
                reset,
            },
            revision,
        )
        .into_response(),
        Err(err) => err.into_response(),
    }
}

async fn generate_land_mask_handler(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(input): Json<LandMaskGenerateInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    let character = ShoreCharacter::parse(input.character.as_deref().unwrap_or("smooth"));
    let variant = input
        .variant
        .as_deref()
        .unwrap_or("A")
        .trim()
        .chars()
        .next()
        .unwrap_or('A')
        .to_ascii_uppercase();
    let nonce = input.regenerate_nonce.unwrap_or(0) as u64;
    let recipe = input.recipe_id.as_deref().and_then(find_recipe);
    let style = recipe
        .map(|r| r.layout_class)
        .or_else(|| {
            input
                .style
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(LayoutClass::parse)
        })
        .unwrap_or(LayoutClass::Pangea);
    let recipe_id = recipe.map(|r| r.id).unwrap_or("").to_string();
    let seed = silhouette_seed(&world.id, style, character, variant, nonce, &recipe_id);
    let mask = if let Some(recipe) = recipe {
        generate_land_mask_recipe(&bounds, recipe, character, seed)
    } else {
        generate_land_mask(&bounds, style, character, seed)
    };
    let elevation = elevation_from_land_mask(&bounds, &mask);
    let base_revision = parse_base_revision(&headers, None);
    match world_io::persist_land_mask_bundle(&world.path, &mask, &elevation, base_revision) {
        Ok(revision) => world_revision::json_with_revision(
            LandMaskGenerateResponse {
                seed,
                recipe_id,
                layout_class: style.id().to_string(),
                character: match character {
                    ShoreCharacter::Smooth => "smooth".to_string(),
                    ShoreCharacter::Jagged => "jagged".to_string(),
                },
                regenerate_nonce: nonce,
            },
            revision,
        )
        .into_response(),
        Err(err) => err.into_revision_response(),
    }
}

async fn generate_geology_handler(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(input): Json<GeologyGenerateInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    let mask = world_io::read_dense_layer(&world.path, LAND_MASK_LAYER_ID, &bounds);
    let style = GeologyStyle::parse(input.style.as_deref().unwrap_or("belts"));
    let nonce = input.regenerate_nonce.unwrap_or(0) as u64;
    let seed = geology_seed(&world.id, style, nonce);
    let geology = generate_geology(&bounds, &mask, style, seed);
    let base_revision = parse_base_revision(&headers, None);
    match world_revision::mutate_map(&world.path, base_revision, || {
        world_io::write_dense_layer(&world.path, &geology)
    }) {
        Ok(((), revision)) => world_revision::no_content_with_revision(revision).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn generate_elevation_handler(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(input): Json<ElevationGenerateInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    let mask = world_io::read_dense_layer(&world.path, LAND_MASK_LAYER_ID, &bounds);
    let geology = world_io::read_dense_layer(&world.path, GEOLOGY_LAYER_ID, &bounds);
    let nonce = input.regenerate_nonce.unwrap_or(0) as u64;
    let intensity = ElevationIntensity::parse(input.style.as_deref().unwrap_or("standard"));
    let seed = elevation_seed(&world.id, intensity, nonce);
    let elevation = elevation_from_land_mask_and_geology(&bounds, &mask, &geology, seed, intensity);
    let base_revision = parse_base_revision(&headers, None);
    match world_revision::mutate_map(&world.path, base_revision, || {
        world_io::write_dense_layer(&world.path, &elevation)
    }) {
        Ok(((), revision)) => world_revision::no_content_with_revision(revision).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn generate_climate_handler(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(input): Json<ClimateGenerateInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    let mask = world_io::read_dense_layer(&world.path, LAND_MASK_LAYER_ID, &bounds);
    let elevation = world_io::read_dense_layer(&world.path, ELEVATION_LAYER_ID, &bounds);
    let style = PrecipitationStyle::parse(input.style.as_deref().unwrap_or("balanced"));
    let nonce = input.regenerate_nonce.unwrap_or(0) as u64;
    let seed = climate_seed(&world.id, style, nonce);
    let layers = generate_climate_layers(&bounds, &mask, &elevation, style, seed);
    let base_revision = parse_base_revision(&headers, None);
    match world_io::persist_climate_layers_bundle(
        &world.path,
        &layers.temperature,
        &layers.precipitation,
        &layers.ice,
        base_revision,
    ) {
        Ok(revision) => world_revision::no_content_with_revision(revision).into_response(),
        Err(err) => err.into_revision_response(),
    }
}

async fn put_land_mask_cells(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(cells): Json<Vec<LandMaskCellInput>>,
) -> impl IntoResponse {
    if cells.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let bounds = world_io::map_bounds(&world.path);
    let mut mask = world_io::read_dense_layer(&world.path, LAND_MASK_LAYER_ID, &bounds);
    let mut elevation = world_io::read_dense_layer(&world.path, "elevation", &bounds);
    for cell in cells {
        let Some(index) = bounds.index_of(Axial::new(cell.q, cell.r)) else {
            continue;
        };
        let kind = normalize_kind(&cell.kind);
        mask.set(index, DenseState::Value(LayerValue::Text(kind.to_string())));
        let elev = if kind == LAND_MASK_LAND { 1 } else { 0 };
        elevation.set(index, DenseState::Value(LayerValue::Int(elev)));
    }
    let base_revision = parse_base_revision(&headers, None);
    match world_io::persist_land_mask_bundle(&world.path, &mask, &elevation, base_revision) {
        Ok(revision) => world_revision::no_content_with_revision(revision).into_response(),
        Err(err) => err.into_revision_response(),
    }
}

fn geology_seed(world_id: &str, style: GeologyStyle, regenerate_nonce: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for b in world_id.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for b in style.id().bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^ regenerate_nonce
}

fn climate_seed(world_id: &str, style: PrecipitationStyle, regenerate_nonce: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for b in world_id.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for b in style.id().bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^ 0x00C1_AA7E ^ regenerate_nonce
}

fn elevation_seed(world_id: &str, intensity: ElevationIntensity, regenerate_nonce: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for b in world_id.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for b in intensity.id().bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^ 0x00E1_E801 ^ regenerate_nonce
}

fn silhouette_seed(
    world_id: &str,
    style: LayoutClass,
    character: ShoreCharacter,
    variant: char,
    regenerate_nonce: u64,
    recipe_id: &str,
) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for b in world_id.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for b in style.id().bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for b in recipe_id.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^= character as u8 as u64;
    hash = hash.wrapping_mul(0x100000001b3);
    hash ^= variant as u32 as u64;
    hash = hash.wrapping_mul(0x100000001b3);
    hash ^ regenerate_nonce
}

#[allow(dead_code)]
fn mark_inland_for_unknown_pools(bounds: &MapBounds, mask: &mut DenseLayer) {
    let mut seen = vec![false; bounds.len()];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (index, seen_cell) in seen.iter_mut().enumerate().take(bounds.len()) {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        if !cell.neighbors().iter().any(|n| !bounds.contains(*n)) {
            continue;
        }
        if !is_water_like(mask, index) {
            continue;
        }
        *seen_cell = true;
        queue.push_back(index);
    }
    while let Some(index) = queue.pop_front() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        for n in cell.neighbors() {
            let Some(next) = bounds.index_of(n) else {
                continue;
            };
            if seen[next] || !is_water_like(mask, next) {
                continue;
            }
            seen[next] = true;
            queue.push_back(next);
        }
    }
    for (index, ocean_connected) in seen.into_iter().enumerate() {
        if !is_water_like(mask, index) {
            continue;
        }
        let kind = if ocean_connected {
            LAND_MASK_OCEAN
        } else {
            LAND_MASK_INLAND_SEA
        };
        mask.set(index, DenseState::Value(LayerValue::Text(kind.to_string())));
    }
}

fn is_water_like(mask: &DenseLayer, index: usize) -> bool {
    !matches!(
        mask.state(index),
        DenseState::Value(LayerValue::Text(kind)) if kind == LAND_MASK_LAND
    )
}
