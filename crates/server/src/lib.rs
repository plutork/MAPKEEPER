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
use mapkeeper_core::build_state::{self, BUILD_STEP_LAND_SILHOUETTE};
use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::geology::{
    elevation_from_land_mask_and_geology, generate_geology, GeologyStyle, GEOLOGY_LAYER_ID,
};
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::land_mask::{
    elevation_from_land_mask, find_recipe, generate_land_mask, generate_land_mask_recipe,
    normalize_kind, LayoutClass, ShoreCharacter, LAND_MASK_INLAND_SEA, LAND_MASK_LAND,
    LAND_MASK_LAYER_ID, LAND_MASK_OCEAN,
};
use mapkeeper_core::layer::{
    Bounds, DenseLayer, DenseState, LayerCellWrite, LayerValue, MapManifest, ValueType,
    WireCellState, ELEVATION_LAYER_ID, RIVER_ID_LAYER_ID,
};
use mapkeeper_core::map_preset::{
    legacy_default_bounds, parse_map_preset, rect_cell_count, MapPreset,
};
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::projects::{projects_file_path, ProjectEntry, ProjectsFile};
use mapkeeper_core::river_flux::{generate_with_owners, sync_river_id_from_owners};
use mapkeeper_core::rivers::{
    append_cell, cell_index, create_river, delete_river, pop_last_cell, sync_river_id_layer,
    RiverCatalog, RiverError, RIVER_CATALOG_FILE,
};
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
    width: i32,
    height: i32,
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
    /// Build wizard draft (`[build] status = "draft"` in mapkeeper.toml).
    build_draft: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_step: Option<u32>,
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
    /// When true, seed `[build]` draft for wizard flow (home-build-draft-v1).
    #[serde(default)]
    build_wizard: Option<bool>,
}

#[derive(Deserialize)]
struct BuildStateInput {
    status: String,
    #[serde(default)]
    step: Option<u32>,
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
    #[allow(dead_code)]
    style: Option<String>,
}

#[derive(Deserialize)]
struct LandMaskCellInput {
    q: i32,
    r: i32,
    kind: String,
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

#[derive(Deserialize)]
struct OpenFixtureInput {
    slug: String,
}

#[derive(Serialize)]
struct FixtureWorldInfo {
    slug: String,
    label: String,
}

#[derive(Serialize)]
struct FixtureWorldsResponse {
    available: bool,
    worlds: Vec<FixtureWorldInfo>,
}

/// river-dogfood-fixture-worlds: slug -> Home button label.
const FIXTURE_WORLD_LABELS: &[(&str, &str)] = &[
    ("coastal-slope", "Coastal slope"),
    ("mountain-ridge", "Mountain ridge"),
    ("enclosed-basin", "Enclosed basin"),
    ("gentle-plain", "Gentle plain"),
    ("dual-watershed", "Dual watershed"),
];

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

fn write_map_manifest(world_path: &Path, preset: MapPreset) -> Result<()> {
    let (width, height) = preset.dimensions();
    let manifest = MapManifest::default_v0(width, height);
    let path = map_manifest_path(world_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, manifest.to_json_pretty()?)?;
    // elevation-authoring-v2: new worlds start as ocean (programmatic fill).
    let bounds = MapBounds::new(width, height);
    let ocean = mapkeeper_core::hydro::filled_elevation_layer(
        &bounds,
        mapkeeper_core::hydro::OCEAN_ELEVATION,
    );
    write_dense_layer(world_path, &ocean).map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

/// Read hex bounds from disk. Missing manifest => Small rectangle default.
fn read_map_bounds(world_path: &Path) -> (MapBounds, bool) {
    let path = map_manifest_path(world_path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return (legacy_default_bounds(), true);
    };
    let Ok(manifest) = MapManifest::from_json(&raw) else {
        return (legacy_default_bounds(), true);
    };
    match manifest.bounds {
        Bounds::HexRectangle { width, height } => (MapBounds::new(width, height), false),
    }
}

/// scale-layers (D-46): map bounds as the cell-index domain for dense layers.
fn map_bounds(world_path: &Path) -> MapBounds {
    read_map_bounds(world_path).0
}

fn bounds_response(bounds: &MapBounds) -> MapBoundsResponse {
    MapBoundsResponse {
        kind: "hex-rectangle".to_string(),
        width: bounds.width,
        height: bounds.height,
        cell_count: rect_cell_count(bounds.width, bounds.height),
    }
}

fn projects_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").ok();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();
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
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
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
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

fn dedupe_projects(mut file: ProjectsFile) -> ProjectsFile {
    let mut unique: Vec<ProjectEntry> = Vec::new();
    for p in file.projects.drain(..) {
        let normalized = normalize_world_path(Path::new(&p.path));
        let normalized_path = normalized.display().to_string();
        let key = path_cmp_key(&normalized);
        if let Some(existing) = unique
            .iter_mut()
            .find(|e| path_cmp_key(Path::new(&e.path)) == key)
        {
            *existing = ProjectEntry {
                id: p.id,
                path: normalized_path,
            };
        } else {
            unique.push(ProjectEntry {
                id: p.id,
                path: normalized_path,
            });
        }
    }
    file.projects = unique;
    file
}

fn default_worlds_root_path() -> String {
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        return PathBuf::from(userprofile)
            .join("Documents")
            .join("MAPKEEPER Worlds")
            .display()
            .to_string();
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join("Documents")
            .join("MAPKEEPER Worlds")
            .display()
            .to_string();
    }
    "MAPKEEPER Worlds".to_string()
}

fn default_worlds_root() -> PathBuf {
    PathBuf::from(default_worlds_root_path())
}

fn is_valid_fixture_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Locate `fixtures/worlds` for river dogfood (dev / repo checkout).
fn fixture_worlds_root() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("MAPKEEPER_FIXTURE_WORLDS") {
        let path = PathBuf::from(raw);
        if path.is_dir() {
            return Some(path);
        }
    }
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..8 {
        let candidate = dir.join("fixtures").join("worlds");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn list_fixture_worlds() -> FixtureWorldsResponse {
    let Some(root) = fixture_worlds_root() else {
        return FixtureWorldsResponse {
            available: false,
            worlds: Vec::new(),
        };
    };
    let mut worlds = Vec::new();
    for (slug, label) in FIXTURE_WORLD_LABELS {
        let path = root.join(slug);
        if path.join("mapkeeper.toml").is_file() {
            worlds.push(FixtureWorldInfo {
                slug: (*slug).to_string(),
                label: (*label).to_string(),
            });
        }
    }
    FixtureWorldsResponse {
        available: !worlds.is_empty(),
        worlds,
    }
}

fn import_fixture_world(slug: &str) -> Result<PathBuf> {
    if !is_valid_fixture_slug(slug) {
        anyhow::bail!("invalid fixture slug");
    }
    let root = fixture_worlds_root().context(
        "fixture worlds not found (run from MAPKEEPER repo or set MAPKEEPER_FIXTURE_WORLDS)",
    )?;
    let src = root.join(slug);
    if !src.join("mapkeeper.toml").is_file() {
        anyhow::bail!("unknown fixture world: {slug}");
    }
    let dest = default_worlds_root().join(format!("fixture-{slug}"));
    if !dest.join("mapkeeper.toml").exists() {
        std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(".")))?;
        if dest.exists() {
            std::fs::remove_dir_all(&dest).context("replacing incomplete fixture import")?;
        }
        copy_dir_all(&src, &dest)?;
    }
    Ok(normalize_world_path(&dest))
}

/// Build the router + `AppState`, without binding a socket — split out so
/// callers (desktop shell) can bind first and read back the OS-assigned
/// port before starting to serve.
pub fn build_router(config: &ServerConfig) -> Result<Router> {
    let active = match &config.world {
        Some(world_path) => {
            let id = read_manifest_id(world_path)?;
            Some(ActiveWorld {
                path: world_path.clone(),
                id,
            })
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
        .route("/api/build", axum::routing::put(put_build_state))
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
        .route("/api/fixture-worlds", get(list_fixture_worlds_handler))
        .route(
            "/api/fixture-worlds/open",
            axum::routing::post(open_fixture_world),
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
            let (build_draft, build_step) = if valid {
                match build_state::read_build(world_path) {
                    Some(b) if build_state::is_draft(&b) => (true, Some(b.step)),
                    _ => (false, None),
                }
            } else {
                (false, None)
            };
            ProjectStatus {
                id: p.id,
                path: p.path,
                valid,
                legacy_map,
                build_draft,
                build_step,
            }
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
    if let Err(err) = std::fs::write(
        &manifest,
        build_state::manifest_toml_with_build(&input.id, input.build_wizard == Some(true)),
    ) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let preset = input
        .map_preset
        .as_deref()
        .and_then(parse_map_preset)
        .unwrap_or(MapPreset::Small);
    if let Err(err) = write_map_manifest(&path, preset) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut file = load_projects();
    file.upsert(ProjectEntry {
        id: input.id.clone(),
        path: path.display().to_string(),
    });
    if let Err(err) = save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    state.lock().unwrap().active = Some(ActiveWorld {
        path: path.clone(),
        id: input.id.clone(),
    });
    Json(ProjectEntry {
        id: input.id,
        path: path.display().to_string(),
    })
    .into_response()
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
    file.upsert(ProjectEntry {
        id: id.clone(),
        path: path.display().to_string(),
    });
    if let Err(err) = save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    state.lock().unwrap().active = Some(ActiveWorld {
        path: path.clone(),
        id: id.clone(),
    });
    Json(ProjectEntry {
        id,
        path: path.display().to_string(),
    })
    .into_response()
}

async fn list_fixture_worlds_handler() -> impl IntoResponse {
    Json(list_fixture_worlds()).into_response()
}

async fn open_fixture_world(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<OpenFixtureInput>,
) -> impl IntoResponse {
    let path = match import_fixture_world(&input.slug) {
        Ok(path) => path,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };
    let id = match read_manifest_id(&path) {
        Ok(id) => id,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };

    let mut file = load_projects();
    file.upsert(ProjectEntry {
        id: id.clone(),
        path: path.display().to_string(),
    });
    if let Err(err) = save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    state.lock().unwrap().active = Some(ActiveWorld {
        path: path.clone(),
        id: id.clone(),
    });
    Json(ProjectEntry {
        id,
        path: path.display().to_string(),
    })
    .into_response()
}

async fn forget_project(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(input): Json<ForgetProjectInput>,
) -> impl IntoResponse {
    let forget_key = path_cmp_key(Path::new(&input.path));

    let mut file = load_projects();
    let before = file.projects.len();
    file.projects
        .retain(|p| path_cmp_key(Path::new(&p.path)) != forget_key);

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
    file.projects
        .retain(|p| path_cmp_key(Path::new(&p.path)) != target_key);
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
            let step = input.step.unwrap_or(BUILD_STEP_LAND_SILHOUETTE);
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
    let bounds = map_bounds(&world_path);
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
    let recipe = input
        .recipe_id
        .as_deref()
        .and_then(find_recipe);
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
    let seed = silhouette_seed(
        &world_id,
        style,
        character,
        variant,
        nonce,
        recipe.map(|r| r.id).unwrap_or(""),
    );
    let mask = if let Some(recipe) = recipe {
        generate_land_mask_recipe(&bounds, recipe, character, seed)
    } else {
        generate_land_mask(&bounds, style, character, seed)
    };
    let elevation = elevation_from_land_mask(&bounds, &mask);
    if let Err(err) = write_dense_layer(&world_path, &mask) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    if let Err(err) = write_dense_layer(&world_path, &elevation) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
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
    let bounds = map_bounds(&world_path);
    let mask = read_dense_layer(&world_path, LAND_MASK_LAYER_ID, &bounds);
    let style = GeologyStyle::parse(input.style.as_deref().unwrap_or("belts"));
    let nonce = input.regenerate_nonce.unwrap_or(0) as u64;
    let seed = geology_seed(&world_id, style, nonce);
    let geology = generate_geology(&bounds, &mask, style, seed);
    if let Err(err) = write_dense_layer(&world_path, &geology) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Step 5: elevation from land_mask + geology (bridge).
async fn generate_elevation_handler(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(_input): Json<ElevationGenerateInput>,
) -> impl IntoResponse {
    let world_path = {
        let guard = state.lock().unwrap();
        let Some(active) = guard.active.as_ref() else {
            return (StatusCode::CONFLICT, "no active world").into_response();
        };
        active.path.clone()
    };
    let bounds = map_bounds(&world_path);
    let mask = read_dense_layer(&world_path, LAND_MASK_LAYER_ID, &bounds);
    let geology = read_dense_layer(&world_path, GEOLOGY_LAYER_ID, &bounds);
    let elevation = elevation_from_land_mask_and_geology(&bounds, &mask, &geology);
    if let Err(err) = write_dense_layer(&world_path, &elevation) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
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
    let bounds = map_bounds(&world_path);
    let mut mask = read_dense_layer(&world_path, LAND_MASK_LAYER_ID, &bounds);
    for cell in cells {
        let Some(index) = bounds.index_of(Axial::new(cell.q, cell.r)) else {
            continue;
        };
        let kind = normalize_kind(&cell.kind);
        mask.set(index, DenseState::Value(LayerValue::Text(kind.to_string())));
    }
    mark_inland_for_unknown_pools(&bounds, &mut mask);
    let elevation = elevation_from_land_mask(&bounds, &mask);
    if let Err(err) = write_dense_layer(&world_path, &mask) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    if let Err(err) = write_dense_layer(&world_path, &elevation) {
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
    let dir = profiles_dir(&active.path);
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
    let (bounds, legacy_map) = read_map_bounds(&active.path);
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

// --- Generic map-state layer API (scale-layers, D-46) ----------------------
// Map state lives under `map/layers/<id>.json`, separate from author
// `profiles/`. On-disk truth is the dense, index-addressed `DenseLayer`; the
// server is a filesystem adapter (D-20) and addresses cells by `(q,r)` externally
// while storing them by linear index internally. Any layer id is reachable
// generically — new layers need no new routes.

fn layer_file_path(world_path: &Path, layer_id: &str) -> PathBuf {
    world_path
        .join("map")
        .join("layers")
        .join(format!("{layer_id}.json"))
}

/// Default value kind for a not-yet-created layer. Only `elevation` is integer
/// today; everything else defaults to categorical.
fn default_value_type(layer_id: &str) -> ValueType {
    if layer_id == ELEVATION_LAYER_ID || layer_id == RIVER_ID_LAYER_ID {
        ValueType::Integer
    } else {
        ValueType::Categorical
    }
}

fn read_dense_layer(world_path: &Path, layer_id: &str, bounds: &MapBounds) -> DenseLayer {
    let raw = std::fs::read_to_string(layer_file_path(world_path, layer_id)).ok();
    DenseLayer::read_or_empty(
        raw.as_deref(),
        layer_id,
        default_value_type(layer_id),
        bounds,
    )
}

fn write_dense_layer(world_path: &Path, layer: &DenseLayer) -> Result<(), String> {
    let path = layer_file_path(world_path, &layer.layer_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = layer.to_json_pretty().map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
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
    let bounds = map_bounds(&active.path);
    Json(read_dense_layer(&active.path, &layer_id, &bounds)).into_response()
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
    let bounds = map_bounds(&active.path);
    let mut dense = read_dense_layer(&active.path, &layer_id, &bounds);
    for item in updates {
        let Some(index) = bounds.index_of(Axial::new(item.q, item.r)) else {
            continue;
        };
        if let Some(new_state) = item.state.to_dense(dense.value_type) {
            dense.set(index, new_state);
        }
    }
    if let Err(err) = write_dense_layer(&active.path, &dense) {
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
    let bounds = map_bounds(&active.path);
    let Some(index) = bounds.index_of(Axial::new(q, r)) else {
        return (StatusCode::BAD_REQUEST, "cell out of map bounds").into_response();
    };
    let mut dense = read_dense_layer(&active.path, &layer_id, &bounds);
    let Some(resolved) = new_state.to_dense(dense.value_type) else {
        return (
            StatusCode::BAD_REQUEST,
            "value kind does not match layer value_type",
        )
            .into_response();
    };
    dense.set(index, resolved);
    if let Err(err) = write_dense_layer(&active.path, &dense) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(WireCellState::from_dense(dense.state(index))).into_response()
}

// --- River catalog (river-overlay-layer-v1, D-54) ---------------------------

fn rivers_file_path(world_path: &Path) -> PathBuf {
    world_path.join("map").join(RIVER_CATALOG_FILE)
}

fn read_river_catalog(world_path: &Path) -> RiverCatalog {
    std::fs::read_to_string(rivers_file_path(world_path))
        .ok()
        .and_then(|raw| RiverCatalog::from_json(&raw).ok())
        .unwrap_or_default()
}

fn write_river_catalog(world_path: &Path, catalog: &RiverCatalog) -> Result<(), String> {
    let path = rivers_file_path(world_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = catalog.to_json_pretty().map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

fn persist_rivers(
    world_path: &Path,
    catalog: &RiverCatalog,
    bounds: &MapBounds,
) -> Result<(), String> {
    write_river_catalog(world_path, catalog)?;
    let layer = sync_river_id_layer(catalog, bounds);
    write_dense_layer(world_path, &layer)
}

fn persist_generated_rivers(
    world_path: &Path,
    catalog: &RiverCatalog,
    owners: &[u32],
    bounds: &MapBounds,
) -> Result<(), String> {
    write_river_catalog(world_path, catalog)?;
    let layer = sync_river_id_from_owners(owners, bounds);
    write_dense_layer(world_path, &layer)
}

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
    Json(read_river_catalog(&active.path)).into_response()
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
    let bounds = map_bounds(&active.path);
    if let Err(err) = persist_rivers(&active.path, &catalog, &bounds) {
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
    let bounds = map_bounds(&active.path);
    let index = match cell_index(&bounds, input.q, input.r) {
        Ok(i) => i,
        Err(err) => return river_error_status(err).into_response(),
    };
    let mut catalog = read_river_catalog(&active.path);
    let result = match input.river_id {
        Some(id) => append_cell(&mut catalog, &bounds, id, index).map(|_| id),
        None => create_river(&mut catalog, &bounds, index),
    };
    match result {
        Ok(_) => {
            if let Err(err) = persist_rivers(&active.path, &catalog, &bounds) {
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
    let bounds = map_bounds(&active.path);
    let mut catalog = read_river_catalog(&active.path);
    if let Err(err) = pop_last_cell(&mut catalog, river_id) {
        return river_error_status(err).into_response();
    }
    if let Err(err) = persist_rivers(&active.path, &catalog, &bounds) {
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
    let bounds = map_bounds(&active.path);
    let mut catalog = read_river_catalog(&active.path);
    if let Err(err) = delete_river(&mut catalog, river_id) {
        return river_error_status(err).into_response();
    }
    if let Err(err) = persist_rivers(&active.path, &catalog, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(catalog).into_response()
}

/// rivers-auto-from-elevation-v1 (D-55): replace-all catalog from elevation flux.
async fn generate_rivers_handler(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let guard = state.lock().unwrap();
    let Some(active) = guard.active.as_ref() else {
        return (
            StatusCode::CONFLICT,
            "no active world — open one via /api/projects",
        )
            .into_response();
    };
    let bounds = map_bounds(&active.path);
    let elevation = read_dense_layer(&active.path, ELEVATION_LAYER_ID, &bounds);
    let (catalog, owners) = generate_with_owners(&elevation, &bounds);
    if let Err(err) = persist_generated_rivers(&active.path, &catalog, &owners, &bounds) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    Json(catalog).into_response()
}
