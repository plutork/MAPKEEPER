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

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::hydro::{hydro_from_elevation, HydroKind, DEFAULT_LAND_ELEVATION};
use mapkeeper_core::layer::{
    Bounds, CellState, DenseLayer, DenseState, LayerValue, MapManifest, TERRAIN_LAYER_ID,
};
use mapkeeper_core::map_preset::{MapPreset, LEGACY_DEFAULT_RADIUS, hex_cell_count, parse_map_preset};
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::projects::{projects_file_path, ProjectEntry, ProjectsFile};
use mapkeeper_core::world;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

/// Where to bind + what to serve. `port: 0` binds an OS-assigned ephemeral
/// port — used by the desktop shell to avoid clashing with a dev server or
/// another mapkeeper instance; the CLI/dev binary keeps a fixed default.
pub struct ServerConfig {
    pub world: Option<PathBuf>,
    pub port: u16,
    pub web_dist: PathBuf,
}

struct AppState {
    active: Option<ActiveWorld>,
}

struct ActiveWorld {
    path: PathBuf,
    id: String,
}

#[derive(Deserialize)]
struct Manifest {
    world: WorldSection,
}

#[derive(Deserialize)]
struct WorldSection {
    id: String,
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
    radius: i32,
    cell_count: u32,
}

#[derive(Serialize)]
struct MapResponse {
    world_id: String,
    bounds: MapBoundsResponse,
    /// `true` when `map/manifest.json` is missing (pre-D-36 world) — not "outdated version".
    legacy_map: bool,
    cells: Vec<CellSummary>,
}

#[derive(Serialize)]
struct ElevationCellState {
    elevation: i16,
    hydro: HydroKind,
}

#[derive(Deserialize)]
struct ElevationCellWrite {
    q: i32,
    r: i32,
    elevation: i16,
}

#[derive(Deserialize)]
struct ProfileInput {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    notes: String,
}

#[derive(Serialize)]
struct ProjectStatus {
    id: String,
    path: String,
    /// `false` if the folder/`mapkeeper.toml` moved or was deleted since it was registered.
    valid: bool,
    /// `true` if `map/manifest.json` is missing — editor uses default Small bounds.
    legacy_map: bool,
}

#[derive(Serialize)]
struct ProjectsResponse {
    active: Option<ProjectEntry>,
    projects: Vec<ProjectStatus>,
    default_worlds_root: String,
}

#[derive(Deserialize)]
struct CreateProjectInput {
    id: String,
    path: String,
    /// `small` | `medium` | `large` — defaults to small.
    #[serde(default)]
    map_preset: Option<String>,
}

#[derive(Deserialize)]
struct OpenProjectInput {
    path: String,
}

#[derive(Deserialize)]
struct ForgetProjectInput {
    path: String,
}

#[derive(Deserialize)]
struct DeleteProjectInput {
    path: String,
}

fn read_manifest_id(world_path: &Path) -> Result<String> {
    let manifest_path = world_path.join("mapkeeper.toml");
    let raw = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "reading {} — is this a mapkeeper world? (see `mapkeeper init`)",
            manifest_path.display()
        )
    })?;
    let manifest: Manifest = toml::from_str(&raw).context("parsing mapkeeper.toml")?;
    Ok(manifest.world.id)
}

fn map_manifest_path(world_path: &Path) -> PathBuf {
    world_path.join("map/manifest.json")
}

fn legacy_map_folder(world_path: &Path) -> bool {
    !map_manifest_path(world_path).exists()
}

fn write_map_manifest(world_path: &Path, radius: i32) -> Result<()> {
    let manifest = MapManifest::default_v0(radius);
    let path = map_manifest_path(world_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, manifest.to_json_pretty()?)?;
    Ok(())
}

/// Read hex bounds from disk; missing manifest => legacy default (Small), not "outdated".
fn read_map_bounds(world_path: &Path) -> (i32, bool) {
    let path = map_manifest_path(world_path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return (LEGACY_DEFAULT_RADIUS, true);
    };
    let Ok(manifest) = MapManifest::from_json(&raw) else {
        return (LEGACY_DEFAULT_RADIUS, true);
    };
    match manifest.bounds {
        Bounds::HexRadius { radius } => (radius.max(0), false),
    }
}

/// scale-layers (D-46): map bounds as the cell-index domain for dense layers.
fn map_bounds(world_path: &Path) -> MapBounds {
    let (radius, _) = read_map_bounds(world_path);
    MapBounds::new(radius)
}

fn bounds_response(radius: i32) -> MapBoundsResponse {
    MapBoundsResponse {
        kind: "hex-radius".to_string(),
        radius,
        cell_count: hex_cell_count(radius),
    }
}

fn projects_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").ok();
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
    PathBuf::from(projects_file_path(appdata.as_deref(), home.as_deref()))
}

fn load_projects() -> ProjectsFile {
    let parsed = match std::fs::read_to_string(projects_path()) {
        Ok(raw) => ProjectsFile::parse(&raw),
        Err(_) => ProjectsFile::default(),
    };
    dedupe_projects(parsed)
}

fn save_projects(file: &ProjectsFile) -> Result<()> {
    let path = projects_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, file.to_json_pretty())?;
    Ok(())
}

fn normalize_world_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|cwd| cwd.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    let normalized = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    strip_windows_verbatim_prefix(normalized)
}

fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{}", rest));
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path
}

fn path_cmp_key(path: &Path) -> String {
    let normalized = normalize_world_path(path);
    let key = normalized.to_string_lossy().replace('\\', "/");
    if cfg!(windows) { key.to_lowercase() } else { key }
}

fn dedupe_projects(mut file: ProjectsFile) -> ProjectsFile {
    let mut unique: Vec<ProjectEntry> = Vec::new();
    for p in file.projects.drain(..) {
        let normalized = normalize_world_path(Path::new(&p.path));
        let normalized_path = normalized.display().to_string();
        let key = path_cmp_key(&normalized);
        if let Some(existing) = unique.iter_mut().find(|e| path_cmp_key(Path::new(&e.path)) == key) {
            *existing = ProjectEntry { id: p.id, path: normalized_path };
        } else {
            unique.push(ProjectEntry { id: p.id, path: normalized_path });
        }
    }
    file.projects = unique;
    file
}

fn default_worlds_root_path() -> String {
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        return PathBuf::from(userprofile).join("Documents").join("MAPKEEPER Worlds").display().to_string();
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join("Documents").join("MAPKEEPER Worlds").display().to_string();
    }
    "MAPKEEPER Worlds".to_string()
}

/// Build the router + `AppState`, without binding a socket — split out so
/// callers (desktop shell) can bind first and read back the OS-assigned
/// port before starting to serve.
pub fn build_router(config: &ServerConfig) -> Result<Router> {
    let active = match &config.world {
        Some(world_path) => {
            let id = read_manifest_id(world_path)?;
            Some(ActiveWorld { path: world_path.clone(), id })
        }
        None => None,
    };
    let state = Arc::new(Mutex::new(AppState { active }));

    Ok(Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/open", axum::routing::post(open_project))
        .route("/api/projects/forget", axum::routing::post(forget_project))
        .route("/api/projects/delete", axum::routing::post(delete_project))
        .route("/api/projects/close", axum::routing::post(close_project))
        .route("/api/map", get(get_map))
        .route("/api/cells/:q/:r/profile", get(get_profile).put(put_profile))
        .route("/api/layers/terrain", get(get_terrain_layer))
        .route("/api/cells/:q/:r/terrain", axum::routing::put(put_cell_terrain))
        .route("/api/layers/elevation", get(get_elevation_layer))
        // save-batch--http-endpoint-v1: one request -> one layer write.
        .route("/api/layers/elevation/batch", axum::routing::put(put_elevation_batch))
        .route("/api/cells/:q/:r/elevation", axum::routing::put(put_cell_elevation))
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
        Some(world) => println!("mapkeeper-server: world '{}' at http://{addr}", world.display()),
        None => println!("mapkeeper-server: launcher mode at http://{addr}"),
    }
    axum::serve(listener, app).await?;
    Ok(())
}

fn profiles_dir(world_path: &Path) -> PathBuf {
    world_path.join("profiles")
}

fn profile_path(world_path: &Path, world_id: &str, q: i32, r: i32) -> PathBuf {
    let id = CellId::new(world_id, q, r);
    profiles_dir(world_path).join(id.filename())
}

async fn list_projects(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let file = load_projects();
    let projects = file
        .projects
        .into_iter()
        .map(|p| {
            let world_path = Path::new(&p.path);
            let valid = world_path.join("mapkeeper.toml").exists();
            let legacy_map = valid && legacy_map_folder(world_path);
            ProjectStatus { id: p.id, path: p.path, valid, legacy_map }
        })
        .collect();
    let active = state.lock().unwrap().active.as_ref().map(|a| ProjectEntry {
        id: a.id.clone(),
        path: a.path.display().to_string(),
    });
    Json(ProjectsResponse {
        active,
        projects,
        default_worlds_root: default_worlds_root_path(),
    })
    .into_response()
}

async fn create_project(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<CreateProjectInput>,
) -> impl IntoResponse {
    if !world::is_valid_world_id(&input.id) {
        return (
            StatusCode::BAD_REQUEST,
            "invalid world name format: use lowercase letters, digits, '-', '_' only",
        )
            .into_response();
    }
    let path = normalize_world_path(Path::new(&input.path));
    let manifest = path.join("mapkeeper.toml");
    if manifest.exists() {
        return (
            StatusCode::CONFLICT,
            format!("{} already has a mapkeeper.toml", path.display()),
        )
            .into_response();
    }
    if let Err(err) = std::fs::create_dir_all(&path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }
    for dir in world::SCAFFOLD_DIRS {
        if let Err(err) = std::fs::create_dir_all(path.join(dir)) {
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    }
    for file in world::SCAFFOLD_FILES {
        let file_path = path.join(file.rel_path);
        if let Some(parent) = file_path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            }
        }
        if let Err(err) = std::fs::write(&file_path, file.contents) {
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    }
    if let Err(err) = std::fs::write(&manifest, world::manifest_toml(&input.id)) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let preset = input
        .map_preset
        .as_deref()
        .and_then(parse_map_preset)
        .unwrap_or(MapPreset::Small);
    if let Err(err) = write_map_manifest(&path, preset.radius()) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut file = load_projects();
    file.upsert(ProjectEntry { id: input.id.clone(), path: path.display().to_string() });
    if let Err(err) = save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    state.lock().unwrap().active = Some(ActiveWorld { path: path.clone(), id: input.id.clone() });
    Json(ProjectEntry { id: input.id, path: path.display().to_string() }).into_response()
}

async fn open_project(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<OpenProjectInput>,
) -> impl IntoResponse {
    let path = normalize_world_path(Path::new(&input.path));
    let id = match read_manifest_id(&path) {
        Ok(id) => id,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };

    let mut file = load_projects();
    file.upsert(ProjectEntry { id: id.clone(), path: path.display().to_string() });
    if let Err(err) = save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    state.lock().unwrap().active = Some(ActiveWorld { path: path.clone(), id: id.clone() });
    Json(ProjectEntry { id, path: path.display().to_string() }).into_response()
}

async fn forget_project(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<ForgetProjectInput>,
) -> impl IntoResponse {
    let forget_key = path_cmp_key(Path::new(&input.path));

    let mut file = load_projects();
    let before = file.projects.len();
    file.projects.retain(|p| path_cmp_key(Path::new(&p.path)) != forget_key);

    if file.projects.len() == before {
        return StatusCode::NO_CONTENT.into_response();
    }
    if let Err(err) = save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut app = state.lock().unwrap();
    if let Some(active) = app.active.as_ref() {
        if path_cmp_key(&active.path) == forget_key {
            app.active = None;
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn delete_project(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<DeleteProjectInput>,
) -> impl IntoResponse {
    let target = normalize_world_path(Path::new(&input.path));
    let target_key = path_cmp_key(&target);
    let manifest = target.join("mapkeeper.toml");
    if !manifest.exists() {
        return (
            StatusCode::BAD_REQUEST,
            "target path has no mapkeeper.toml — use Forget to remove a stale entry",
        )
            .into_response();
    }

    if let Err(err) = std::fs::remove_dir_all(&target) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut file = load_projects();
    file.projects.retain(|p| path_cmp_key(Path::new(&p.path)) != target_key);
    if let Err(err) = save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut app = state.lock().unwrap();
    if let Some(active) = app.active.as_ref() {
        if path_cmp_key(&active.path) == target_key {
            app.active = None;
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn close_project(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    state.lock().unwrap().active = None;
    StatusCode::NO_CONTENT
}

async fn get_map(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (StatusCode::CONFLICT, "no active world — open one via /api/projects").into_response();
    };
    let dir = profiles_dir(&active.path);
    let mut cells = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else { continue };
            let Ok(profile) = serde_json::from_str::<CellProfile>(&raw) else { continue };
            let Some(id) = CellId::parse(&profile.cell_id) else { continue };
            cells.push(CellSummary {
                cell_id: profile.cell_id,
                q: id.q,
                r: id.r,
                display_name: profile.display_name,
            });
        }
    }
    let (radius, legacy_map) = read_map_bounds(&active.path);
    Json(MapResponse {
        world_id: active.id.clone(),
        bounds: bounds_response(radius),
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
        return (StatusCode::CONFLICT, "no active world — open one via /api/projects").into_response();
    };
    let id = CellId::new(&active.id, q, r);
    let path = profile_path(&active.path, &active.id, q, r);
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
        return (StatusCode::CONFLICT, "no active world — open one via /api/projects").into_response();
    };
    let id = CellId::new(&active.id, q, r);
    let mut profile = CellProfile::new(&id, input.display_name);
    profile.notes = input.notes;

    let issues = profile.validate();
    if issues.iter().any(|i| matches!(i, mapkeeper_core::profile::ValidationIssue::Error(_))) {
        return (StatusCode::BAD_REQUEST, format!("{issues:?}")).into_response();
    }

    let dir = profiles_dir(&active.path);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }
    let path = profile_path(&active.path, &active.id, q, r);
    let body = match serde_json::to_string_pretty(&profile) {
        Ok(body) => body,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };
    if let Err(err) = std::fs::write(&path, body) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }
    Json(profile).into_response()
}

// --- Map state layers (Hex Map Model Foundation D-36; scale-layers D-46) ---
// Map state lives under `map/layers/`, separate from author `profiles/`.
// scale-layers (D-46): on-disk truth is now the dense, index-addressed
// `DenseLayer` (migrate-on-read from the old sparse v1). The server projects
// dense -> sparse for the HTTP responses/requests the current web still
// consumes, so this adapter change is byte-compatible for the browser (the web
// switch to dense/generic is the next slice). FS stays here (D-20); index and
// model live in core.

fn layer_file_path(world_path: &Path, layer_id: &str) -> PathBuf {
    world_path.join("map").join("layers").join(format!("{layer_id}.json"))
}

fn write_dense_layer(world_path: &Path, layer: &DenseLayer) -> Result<(), String> {
    let path = layer_file_path(world_path, &layer.layer_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = layer.to_json_pretty().map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

fn read_terrain_dense(world_path: &Path, bounds: &MapBounds) -> DenseLayer {
    let raw = std::fs::read_to_string(layer_file_path(world_path, TERRAIN_LAYER_ID)).ok();
    DenseLayer::categorical_from_disk(raw.as_deref(), TERRAIN_LAYER_ID, bounds)
}

fn read_elevation_dense(world_path: &Path, bounds: &MapBounds) -> DenseLayer {
    let raw = std::fs::read_to_string(layer_file_path(world_path, "elevation")).ok();
    DenseLayer::elevation_from_disk(raw.as_deref(), bounds)
}

fn cellstate_to_dense(state: CellState) -> DenseState {
    match state {
        CellState::Unknown => DenseState::Unknown,
        CellState::None => DenseState::None,
        CellState::Value { value } => DenseState::Value(LayerValue::Text(value)),
    }
}

fn dense_to_cellstate(state: DenseState) -> CellState {
    match state {
        DenseState::Unknown => CellState::Unknown,
        DenseState::None => CellState::None,
        DenseState::Value(LayerValue::Text(v)) => CellState::Value { value: v },
        DenseState::Value(LayerValue::Int(i)) => CellState::Value { value: i.to_string() },
    }
}

/// Keep default-land sparse (old semantics): painting the default elevation
/// clears the cell rather than storing it.
fn set_dense_elevation(dense: &mut DenseLayer, index: usize, elevation: i16) {
    if elevation == DEFAULT_LAND_ELEVATION {
        dense.set(index, DenseState::Unknown);
    } else {
        dense.set(index, DenseState::Value(LayerValue::Int(elevation as i32)));
    }
}

async fn get_terrain_layer(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (StatusCode::CONFLICT, "no active world — open one via /api/projects").into_response();
    };
    let bounds = map_bounds(&active.path);
    let dense = read_terrain_dense(&active.path, &bounds);
    // Project back to the sparse shape the current web/CLI consume.
    Json(dense.to_sparse_layer(&bounds, &active.id)).into_response()
}

async fn put_cell_terrain(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath((q, r)): AxPath<(i32, i32)>,
    Json(new_state): Json<CellState>,
) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (StatusCode::CONFLICT, "no active world — open one via /api/projects").into_response();
    };
    let bounds = map_bounds(&active.path);
    let Some(index) = bounds.index_of(Axial::new(q, r)) else {
        return (StatusCode::BAD_REQUEST, "cell out of map bounds").into_response();
    };
    let mut dense = read_terrain_dense(&active.path, &bounds);
    dense.set(index, cellstate_to_dense(new_state));
    if let Err(err) = write_dense_layer(&active.path, &dense) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(dense_to_cellstate(dense.state(index))).into_response()
}

async fn get_elevation_layer(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (StatusCode::CONFLICT, "no active world — open one via /api/projects").into_response();
    };
    let bounds = map_bounds(&active.path);
    let dense = read_elevation_dense(&active.path, &bounds);
    // Project to the sparse ElevationLayer shape the current web consumes.
    Json(dense.to_sparse_elevation(&bounds, &active.id)).into_response()
}

async fn put_cell_elevation(
    State(state): State<Arc<Mutex<AppState>>>,
    AxPath((q, r)): AxPath<(i32, i32)>,
    Json(new_elevation): Json<i16>,
) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (StatusCode::CONFLICT, "no active world — open one via /api/projects").into_response();
    };
    let bounds = map_bounds(&active.path);
    let Some(index) = bounds.index_of(Axial::new(q, r)) else {
        return (StatusCode::BAD_REQUEST, "cell out of map bounds").into_response();
    };
    let mut dense = read_elevation_dense(&active.path, &bounds);
    set_dense_elevation(&mut dense, index, new_elevation);
    if let Err(err) = write_dense_layer(&active.path, &dense) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    let elevation = dense.int_or(index, DEFAULT_LAND_ELEVATION as i32) as i16;
    Json(ElevationCellState { elevation, hydro: hydro_from_elevation(elevation) }).into_response()
}

async fn put_elevation_batch(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(updates): Json<Vec<ElevationCellWrite>>,
) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (StatusCode::CONFLICT, "no active world — open one via /api/projects").into_response();
    };
    if updates.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let bounds = map_bounds(&active.path);
    let mut dense = read_elevation_dense(&active.path, &bounds);
    for item in updates {
        if let Some(index) = bounds.index_of(Axial::new(item.q, item.r)) {
            set_dense_elevation(&mut dense, index, item.elevation);
        }
    }
    if let Err(err) = write_dense_layer(&active.path, &dense) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}
