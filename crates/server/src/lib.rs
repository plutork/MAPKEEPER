//! Local server — owns filesystem, world folder, HTTP API, projects list.
//! Calls into mapkeeper-core for rules; HTTP framework choice (axum) is an
//! open implementation detail (not blocking repo layout, D-20) picked here.
//!
//! V0 flow-first slice (roadmap D-21): serves the WASM web UI as static
//! files and a small JSON API so a browser can paint hex cells and save
//! placeholder profiles into one world folder.
//!
//! Launcher slice (roadmap D-12/5.7): with no active world the server
//! starts with no active world; the web UI shows a Home screen backed by
//! `/api/projects` (list/create/open/close) instead of a hex map.
//!
//! Extracted to a library (roadmap 5.9, D-29) so `mapkeeper-desktop` (Tauri)
//! can embed the exact same router in-process instead of re-implementing
//! the API — "Tauri wraps the same frontend build" means it also reuses
//! this same backend, just swaps how the window is opened (native window
//! vs. `open http://localhost` instructions).

mod projects;
mod state;
mod world_io;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::build_state::{self, BUILD_STEP_SIZE};
use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::climate::{generate_climate_layers, PrecipitationStyle};
use mapkeeper_core::elevation_gen::{elevation_from_land_mask_and_geology, ElevationIntensity};
use mapkeeper_core::geology::{generate_geology, GeologyStyle, GEOLOGY_LAYER_ID};
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::land_mask::{
    elevation_from_land_mask, find_recipe, generate_land_mask, generate_land_mask_recipe,
    normalize_kind, LayoutClass, ShoreCharacter, LAND_MASK_INLAND_SEA, LAND_MASK_LAND,
    LAND_MASK_LAYER_ID, LAND_MASK_OCEAN,
};
use mapkeeper_core::layer::{
    DenseLayer, DenseState, LayerCellWrite, LayerValue, WireCellState, ELEVATION_LAYER_ID,
};
use mapkeeper_core::map_preset::{parse_map_preset, rect_cell_count};
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::river_flux::generate_with_owners;
use mapkeeper_core::rivers::{
    append_cell, cell_index, create_river, delete_river, pop_last_cell, RiverCatalog, RiverError,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

/// Where to bind + what to serve. `port: 0` binds an OS-assigned ephemeral
/// port — used by the desktop shell to avoid clashing with a dev server or
/// another mapkeeper instance; the CLI/dev binary keeps a fixed default.
use state::{ActiveWorld, AppState};

pub struct ServerConfig {
    pub world: Option<PathBuf>,
    pub port: u16,
    pub web_dist: PathBuf,
}

#[derive(Serialize)]
struct CellSummary {
    cell_id: String,
    q: i32,
    r: i32,
    display_name: String,
}

#[derive(Serialize)]
struct MapBoundsResponse {
    kind: String,
    width: i32,
    height: i32,
    cell_count: u32,
}

fn bounds_response(bounds: &MapBounds) -> MapBoundsResponse {
    MapBoundsResponse {
        kind: "hex-rectangle".to_string(),
        width: bounds.width,
        height: bounds.height,
        cell_count: rect_cell_count(bounds.width, bounds.height),
    }
}

#[derive(Serialize)]
struct MapResponse {
    world_id: String,
    bounds: MapBoundsResponse,
    /// `true` when `map/manifest.json` is missing (pre-D-36 world) — not "outdated version".
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

#[derive(Deserialize)]
struct BuildStateInput {
    status: String,
    #[serde(default)]
    step: Option<u32>,
}

/// wizard-size-grid-step (D-69): change preset mid-draft; hard-resets dense layers.
#[derive(Deserialize)]
struct BuildBoundsInput {
    map_preset: String,
}

#[derive(Serialize)]
struct BuildBoundsResponse {
    bounds: MapBoundsResponse,
    reset: bool,
}

#[derive(Deserialize)]
struct LandMaskGenerateInput {
    /// Explicit layout class (optional if recipe_id present).
    #[serde(default)]
    style: Option<String>,
    /// Recipe id from pattern bank (preferred).
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

/// step3-recipe-seed-debug-line (D-68): generation identity for wizard dogfood.
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

pub fn build_router(config: &ServerConfig) -> Result<Router> {
    let active = match &config.world {
        Some(world_path) => {
            let id = world_io::read_manifest_id(world_path)?;
            Some(ActiveWorld {
                path: world_path.clone(),
                id,
            })
        }
        None => None,
    };
    let state = Arc::new(Mutex::new(AppState { active }));

    Ok(projects::routes()
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
        .route("/api/map", get(get_map))
        .route(
            "/api/cells/:q/:r/profile",
            get(get_profile).put(put_profile),
        )
        // scale-layers (D-46): generic layer API by id (dense). Replaces the old
        // per-layer terrain/elevation routes.
        .route("/api/layers/:id", get(get_layer))
        // save-batch--http-endpoint-v1: one request -> one layer write.
        .route("/api/layers/:id/batch", axum::routing::put(put_layer_batch))
        .route(
            "/api/layers/:id/cells/:q/:r",
            axum::routing::put(put_layer_cell),
        )
        // river-overlay-layer-v1 (D-54): catalog API + derived river_id sync.
        .route("/api/rivers", get(get_rivers).put(put_rivers))
        .route("/api/rivers/append", axum::routing::post(append_river_cell))
        .route("/api/rivers/:id/pop", axum::routing::post(pop_river_cell))
        .route(
            "/api/rivers/:id",
            axum::routing::delete(delete_river_handler),
        )
        .route(
            "/api/rivers/generate",
            axum::routing::post(generate_rivers_handler),
        )
        .with_state(state)
        .fallback_service(ServeDir::new(&config.web_dist)))
}

/// Bind a `TcpListener` for `config.port` (0 = OS-assigned) and build the
/// router. Returns the listener so the caller can read `local_addr()`
/// before calling `axum::serve`.
pub async fn bind(config: ServerConfig) -> Result<(TcpListener, Router)> {
    let app = build_router(&config)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = TcpListener::bind(addr).await?;
    Ok((listener, app))
}

/// Bind + serve, blocking until the server stops. Used by the `mapkeeper-server`
/// CLI binary; the desktop shell calls `bind` directly instead so it can read
/// back the bound port first.
pub async fn run(config: ServerConfig) -> Result<()> {
    let world = config.world.clone();
    let (listener, app) = bind(config).await?;
    let addr = listener.local_addr()?;
    match &world {
        Some(world) => println!(
            "mapkeeper-server: world '{}' at http://{addr}",
            world.display()
        ),
        None => println!("mapkeeper-server: launcher mode at http://{addr}"),
    }
    axum::serve(listener, app).await?;
    Ok(())
}

/// home-build-draft-v1: persist or clear `[build]` on the active world.
async fn put_build_state(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<BuildStateInput>,
) -> impl IntoResponse {
    let world_path = {
        let guard = state.lock().unwrap();
        let Some(active) = guard.active.as_ref() else {
            return (StatusCode::CONFLICT, "no active world").into_response();
        };
        active.path.clone()
    };
    let result = match input.status.as_str() {
        "draft" => {
            let step = input.step.unwrap_or(BUILD_STEP_SIZE);
            build_state::write_build_draft(&world_path, step)
        }
        "complete" => build_state::clear_build(&world_path),
        _ => return (StatusCode::BAD_REQUEST, "status must be draft or complete").into_response(),
    };
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
    }
}

/// wizard-size-grid-step (D-69): set map preset; hard-reset Geo layers when present.
async fn put_build_bounds(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<BuildBoundsInput>,
) -> impl IntoResponse {
    let world_path = {
        let guard = state.lock().unwrap();
        let Some(active) = guard.active.as_ref() else {
            return (StatusCode::CONFLICT, "no active world").into_response();
        };
        active.path.clone()
    };
    let Some(preset) = parse_map_preset(&input.map_preset) else {
        return (StatusCode::BAD_REQUEST, "unknown map_preset").into_response();
    };
    let reset = world_io::pipeline_has_downstream(&world_path);
    match world_io::rewrite_world_bounds(&world_path, preset, true) {
        Ok(bounds) => {
            let _ = build_state::write_build_draft(&world_path, BUILD_STEP_SIZE);
            Json(BuildBoundsResponse {
                bounds: bounds_response(&bounds),
                reset,
            })
            .into_response()
        }
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

/// world-pipeline--land-silhouette-v1 + step3-layout-pattern-bank-v1:
/// generate step-3 `land_mask` from recipe bank (macroform) + shore character.
async fn generate_land_mask_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<LandMaskGenerateInput>,
) -> impl IntoResponse {
    let (world_path, world_id) = {
        let guard = state.lock().unwrap();
        let Some(active) = guard.active.as_ref() else {
            return (StatusCode::CONFLICT, "no active world").into_response();
        };
        (active.path.clone(), active.id.clone())
    };
    let bounds = world_io::map_bounds(&world_path);
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
    let seed = silhouette_seed(&world_id, style, character, variant, nonce, &recipe_id);
    let mask = if let Some(recipe) = recipe {
        generate_land_mask_recipe(&bounds, recipe, character, seed)
    } else {
        generate_land_mask(&bounds, style, character, seed)
    };
    let elevation = elevation_from_land_mask(&bounds, &mask);
    if let Err(err) = world_io::write_dense_layer(&world_path, &mask) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    if let Err(err) = world_io::write_dense_layer(&world_path, &elevation) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    // D-68: return identity so wizard can show recipe/seed without rehashing.
    Json(LandMaskGenerateResponse {
        seed,
        recipe_id,
        layout_class: style.id().to_string(),
        character: match character {
            ShoreCharacter::Smooth => "smooth".to_string(),
            ShoreCharacter::Jagged => "jagged".to_string(),
        },
        regenerate_nonce: nonce,
    })
    .into_response()
}

/// world-pipeline--tectonics-v1: generate step-4 `geology` from accepted land_mask.
/// Does not write elevation.
async fn generate_geology_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<GeologyGenerateInput>,
) -> impl IntoResponse {
    let (world_path, world_id) = {
        let guard = state.lock().unwrap();
        let Some(active) = guard.active.as_ref() else {
            return (StatusCode::CONFLICT, "no active world").into_response();
        };
        (active.path.clone(), active.id.clone())
    };
    let bounds = world_io::map_bounds(&world_path);
    let mask = world_io::read_dense_layer(&world_path, LAND_MASK_LAYER_ID, &bounds);
    let style = GeologyStyle::parse(input.style.as_deref().unwrap_or("belts"));
    let nonce = input.regenerate_nonce.unwrap_or(0) as u64;
    let seed = geology_seed(&world_id, style, nonce);
    let geology = generate_geology(&bounds, &mask, style, seed);
    if let Err(err) = world_io::write_dense_layer(&world_path, &geology) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Step 5: elevation from land_mask + geology (bridge).
async fn generate_elevation_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<ElevationGenerateInput>,
) -> impl IntoResponse {
    let (world_path, world_id) = {
        let guard = state.lock().unwrap();
        let Some(active) = guard.active.as_ref() else {
            return (StatusCode::CONFLICT, "no active world").into_response();
        };
        (active.path.clone(), active.id.clone())
    };
    let bounds = world_io::map_bounds(&world_path);
    let mask = world_io::read_dense_layer(&world_path, LAND_MASK_LAYER_ID, &bounds);
    let geology = world_io::read_dense_layer(&world_path, GEOLOGY_LAYER_ID, &bounds);
    let nonce = input.regenerate_nonce.unwrap_or(0) as u64;
    let intensity = ElevationIntensity::parse(input.style.as_deref().unwrap_or("standard"));
    let seed = elevation_seed(&world_id, intensity, nonce);
    let elevation = elevation_from_land_mask_and_geology(&bounds, &mask, &geology, seed, intensity);
    if let Err(err) = world_io::write_dense_layer(&world_path, &elevation) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Step 7–10 climate T2 (D-90): zonal heuristic layers from land_mask + elevation.
async fn generate_climate_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<ClimateGenerateInput>,
) -> impl IntoResponse {
    let (world_path, world_id) = {
        let guard = state.lock().unwrap();
        let Some(active) = guard.active.as_ref() else {
            return (StatusCode::CONFLICT, "no active world").into_response();
        };
        (active.path.clone(), active.id.clone())
    };
    let bounds = world_io::map_bounds(&world_path);
    let mask = world_io::read_dense_layer(&world_path, LAND_MASK_LAYER_ID, &bounds);
    let elevation = world_io::read_dense_layer(&world_path, ELEVATION_LAYER_ID, &bounds);
    let style = PrecipitationStyle::parse(input.style.as_deref().unwrap_or("balanced"));
    let nonce = input.regenerate_nonce.unwrap_or(0) as u64;
    let seed = climate_seed(&world_id, style, nonce);
    let layers = generate_climate_layers(&bounds, &mask, &elevation, style, seed);
    for layer in [layers.temperature, layers.precipitation, layers.ice] {
        if let Err(err) = world_io::write_dense_layer(&world_path, &layer) {
            return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Step-3 edit brush writes land/ocean into `land_mask` and keeps elevation in sync.
async fn put_land_mask_cells(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(cells): Json<Vec<LandMaskCellInput>>,
) -> impl IntoResponse {
    if cells.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let world_path = {
        let guard = state.lock().unwrap();
        let Some(active) = guard.active.as_ref() else {
            return (StatusCode::CONFLICT, "no active world").into_response();
        };
        active.path.clone()
    };
    let bounds = world_io::map_bounds(&world_path);
    let mut mask = world_io::read_dense_layer(&world_path, LAND_MASK_LAYER_ID, &bounds);
    let mut elevation = world_io::read_dense_layer(&world_path, "elevation", &bounds);
    // Incremental edit: patch touched cells only. Skip full-map inland BFS +
    // elevation rebuild (those run on generate) so wizard paint stays responsive.
    for cell in cells {
        let Some(index) = bounds.index_of(Axial::new(cell.q, cell.r)) else {
            continue;
        };
        let kind = normalize_kind(&cell.kind);
        mask.set(index, DenseState::Value(LayerValue::Text(kind.to_string())));
        let elev = if kind == LAND_MASK_LAND { 1 } else { 0 };
        elevation.set(index, DenseState::Value(LayerValue::Int(elev)));
    }
    if let Err(err) = world_io::write_dense_layer(&world_path, &mask) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    if let Err(err) = world_io::write_dense_layer(&world_path, &elevation) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
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

// --- Generic map-state layer API (scale-layers, D-46) ----------------------
// Map state lives under `map/layers/<id>.json`, separate from author
// `profiles/`. On-disk truth is the dense, index-addressed `DenseLayer`; the
// server is a filesystem adapter (D-20) and addresses cells by `(q,r)` externally
// while storing them by linear index internally. Any layer id is reachable
// generically — new layers need no new routes.

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
    hash ^ 0xC1AA_7E ^ regenerate_nonce
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
    hash ^ 0xE1E8_01 ^ regenerate_nonce
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

#[allow(dead_code)] // kept for generate-time inland refresh if edit path needs it later
fn mark_inland_for_unknown_pools(bounds: &MapBounds, mask: &mut DenseLayer) {
    let mut seen = vec![false; bounds.len()];
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for index in 0..bounds.len() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        if !cell.neighbors().iter().any(|n| !bounds.contains(*n)) {
            continue;
        }
        if !is_water_like(mask, index) {
            continue;
        }
        seen[index] = true;
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

// --- River catalog (river-overlay-layer-v1, D-54) ---------------------------

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

/// rivers-auto-from-elevation-v1 (D-55); D-91 climate precipitation when layer exists.
#[derive(serde::Serialize)]
struct RiversGenerateResponse {
    #[serde(flatten)]
    catalog: RiverCatalog,
    precip_source: &'static str,
}

async fn generate_rivers_handler(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
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
    let (catalog, owners, used_climate) =
        generate_with_owners(&elevation, &bounds, precipitation.as_ref());
    if let Err(err) = world_io::persist_generated_rivers(&active.path, &catalog, &owners, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    let precip_source = if used_climate {
        "climate"
    } else {
        "uniform_fallback"
    };
    Json(RiversGenerateResponse {
        catalog,
        precip_source,
    })
    .into_response()
}
