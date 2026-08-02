use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use mapkeeper_core::projects::ProjectEntry;
use mapkeeper_core::world;
use serde::{Deserialize, Serialize};

use crate::state::{ActiveWorld, ServerState};
use crate::world_io;
use crate::world_layout;

mod create;
mod delete;

use create::{resolve_create_preset, transactional_create};

#[derive(Serialize)]
struct MapStatus {
    id: String,
    name: String,
    valid: bool,
}

#[derive(Serialize)]
struct ProjectStatus {
    id: String,
    path: String,
    valid: bool,
    #[serde(default)]
    maps: Vec<MapStatus>,
    /// Set when folder is pre-N-035 single-level (N-037).
    #[serde(skip_serializing_if = "Option::is_none")]
    legacy: Option<bool>,
}

#[derive(Serialize)]
struct ProjectsResponse {
    active: Option<ProjectEntry>,
    projects: Vec<ProjectStatus>,
    default_worlds_root: String,
}

#[derive(Deserialize)]
struct ProjectPathInput {
    path: String,
    /// Optional map id; default = first map in world (N-035).
    #[serde(default)]
    map_id: Option<String>,
}

#[derive(Deserialize)]
struct AddMapInput {
    path: String,
    id: String,
    #[serde(default)]
    preset_id: Option<String>,
}

#[derive(Serialize)]
struct OpenProjectResult {
    id: String,
    path: String,
    map_id: String,
    maps: Vec<MapStatus>,
}

#[derive(Deserialize)]
struct CreateProjectInput {
    id: String,
    path: String,
    /// Create catalog preset id (N-016). Absent → alpha default.
    #[serde(default)]
    preset_id: Option<String>,
}

#[derive(Deserialize)]
struct DeleteProjectInput {
    path: String,
    expected_id: String,
}

#[derive(Serialize)]
struct CreateProjectResult {
    id: String,
    path: String,
    /// Present when durable Create succeeded but marker clear failed (N-025).
    #[serde(skip_serializing_if = "Option::is_none")]
    create_recovery: Option<&'static str>,
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/open", post(open_project))
        .route("/api/projects/maps", post(add_map))
        .route("/api/projects/forget", post(forget_project))
        .route("/api/projects/delete", post(delete_project))
        .route("/api/projects/close", post(close_project))
        .route("/api/projects/restore-bak", post(restore_registry_bak))
}

fn map_statuses(world_path: &Path) -> Vec<MapStatus> {
    let Ok(manifest) = world_layout::read_world_manifest(world_path) else {
        return Vec::new();
    };
    manifest
        .maps
        .iter()
        .map(|m| {
            let map_path = world_layout::map_abs_path(world_path, m);
            MapStatus {
                id: m.id.clone(),
                name: m.name.clone(),
                valid: map_path.join("map.toml").is_file(),
            }
        })
        .collect()
}

fn registry_error(message: String) -> axum::response::Response {
    let status = if message.starts_with("corrupt_registry") {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, message).into_response()
}

async fn list_projects(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    // Read-only: Delete inflight reconciliation runs at server startup (N-025).
    let file = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return registry_error(error),
    };
    let projects = file
        .projects
        .iter()
        .map(|entry| {
            let path = Path::new(&entry.path);
            let class = world_io::classify_create_marker_at(path, &file);
            let legacy = world_layout::is_legacy_world_dir(path);
            let maps = if legacy {
                Vec::new()
            } else {
                map_statuses(path)
            };
            let valid = path.join("mapkeeper.toml").is_file()
                && !legacy
                && !maps.is_empty()
                && maps.iter().any(|m| m.valid)
                && !matches!(
                    class,
                    world_io::CreateMarkerClass::SafeIncomplete
                        | world_io::CreateMarkerClass::Ambiguous { .. }
                );
            ProjectStatus {
                valid,
                id: entry.id.clone(),
                path: entry.path.clone(),
                maps,
                legacy: legacy.then_some(true),
            }
        })
        .collect();
    let active = server
        .app
        .lock()
        .unwrap()
        .active
        .as_ref()
        .map(|world| ProjectEntry {
            id: world.id.clone(),
            path: world.path.display().to_string(),
        });
    Json(ProjectsResponse {
        active,
        projects,
        default_worlds_root: world_io::default_worlds_root_path(),
    })
    .into_response()
}
async fn create_project(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<CreateProjectInput>,
) -> impl IntoResponse {
    let id = match world::normalize_world_id(&input.id) {
        Ok(id) => id,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    let path = world_io::normalize_world_path(Path::new(&input.path));
    let preset = match resolve_create_preset(input.preset_id.as_deref()) {
        Ok(preset) => preset,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    transactional_create(&server, id, path, preset)
}

async fn open_project(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<ProjectPathInput>,
) -> impl IntoResponse {
    let path = world_io::normalize_world_path(Path::new(&input.path));
    let registry = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return registry_error(error),
    };
    match world_io::classify_create_marker_at(&path, &registry) {
        world_io::CreateMarkerClass::NoMarker => {}
        world_io::CreateMarkerClass::SafeIncomplete => {
            match world_io::cleanup_incomplete_create(&path, &registry) {
                Ok(()) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        "create_incomplete: interrupted create cleaned; retry Create".to_string(),
                    )
                        .into_response();
                }
                Err(error) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
                }
            }
        }
        world_io::CreateMarkerClass::CompleteRegistered { .. }
        | world_io::CreateMarkerClass::CompleteUnregistered { .. } => {
            let _ = world_io::clear_incomplete_marker(&path);
        }
        world_io::CreateMarkerClass::Ambiguous { reason } => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("create_incomplete: recovery_required ({reason})"),
            )
                .into_response();
        }
    }
    activate_and_register(&server, path, input.map_id.as_deref())
}

fn activate_and_register(
    server: &Arc<ServerState>,
    path: PathBuf,
    map_id: Option<&str>,
) -> axum::response::Response {
    let (id, map_path, resolved_map_id) = match world_layout::prepare_open(&path, map_id) {
        Ok(v) => v,
        Err(message) => {
            let status = if message.starts_with("legacy_world_format") {
                StatusCode::UNPROCESSABLE_ENTITY
            } else {
                StatusCode::BAD_REQUEST
            };
            return (status, message).into_response();
        }
    };
    if let Err(error) = crate::spatial::ensure_spatial_state(&map_path) {
        let message = error.to_string();
        let status = if message.contains("corrupt_spatial") || message.contains("corrupt_manifest")
        {
            StatusCode::UNPROCESSABLE_ENTITY
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        return (status, message).into_response();
    }
    let entry = ProjectEntry {
        id: id.clone(),
        path: path.display().to_string(),
    };
    if let Err(error) = world_io::mutate_projects(|file| {
        file.projects.retain(|item| item.id != id);
        world_io::upsert_registered(file, entry.clone());
        Ok(())
    }) {
        return registry_error(error);
    }
    let maps = map_statuses(&path);
    server.app.lock().unwrap().active = Some(ActiveWorld {
        path,
        id: id.clone(),
        map_path,
        map_id: resolved_map_id.clone(),
    });
    Json(OpenProjectResult {
        id,
        path: entry.path,
        map_id: resolved_map_id,
        maps,
    })
    .into_response()
}

async fn add_map(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<AddMapInput>,
) -> impl IntoResponse {
    let world_path = world_io::normalize_world_path(Path::new(&input.path));
    if world_layout::is_legacy_world_dir(&world_path) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            world_layout::LEGACY_REFUSE_MSG.to_string(),
        )
            .into_response();
    }
    let map_id = match world::normalize_world_id(&input.id) {
        Ok(id) => id,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    let preset = match resolve_create_preset(input.preset_id.as_deref()) {
        Ok(preset) => preset,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    let mut manifest = match world_layout::read_world_manifest(&world_path) {
        Ok(m) => m,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    if !world::is_two_level_world(&manifest) {
        return (
            StatusCode::BAD_REQUEST,
            "world_format: missing maps list".to_string(),
        )
            .into_response();
    }
    if manifest.maps.iter().any(|m| m.id == map_id) {
        return (
            StatusCode::CONFLICT,
            format!("map `{map_id}` already exists"),
        )
            .into_response();
    }
    let map_ref = mapkeeper_core::world::WorldMapRef {
        id: map_id.clone(),
        name: map_id.clone(),
        path: world::map_rel_path(&map_id),
    };
    let map_path = world_layout::map_abs_path(&world_path, &map_ref);
    if let Err(e) = std::fs::create_dir_all(&map_path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    if let Err(e) = crate::atomic_io::atomic_replace(
        &map_path.join("map.toml"),
        world::map_manifest_toml(&map_id, preset).as_bytes(),
    ) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    if let Err(e) = crate::spatial::ensure_spatial_state(&map_path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    manifest.maps.push(map_ref);
    let rendered = match world::render_manifest(&manifest) {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    if let Err(e) =
        crate::atomic_io::atomic_replace(&world_path.join("mapkeeper.toml"), rendered.as_bytes())
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    // Activate the new map when this world is already open.
    {
        let mut app = server.app.lock().unwrap();
        if app
            .active
            .as_ref()
            .is_some_and(|w| world_io::path_cmp_key(&w.path) == world_io::path_cmp_key(&world_path))
        {
            if let Some(active) = app.active.as_mut() {
                active.map_path = map_path;
                active.map_id = map_id.clone();
            }
        }
    }
    Json(serde_json::json!({
        "id": manifest.world.id,
        "path": world_path.display().to_string(),
        "map_id": map_id,
        "maps": map_statuses(&world_path),
    }))
    .into_response()
}

/// Explicit author-triggered recovery offered when the registry is corrupt (N-025).
async fn restore_registry_bak() -> impl IntoResponse {
    match world_io::restore_projects_from_bak() {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => registry_error(error),
    }
}

fn world_exists_on_disk(path: &Path) -> bool {
    path.join("mapkeeper.toml").is_file()
}

async fn forget_project(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<ProjectPathInput>,
) -> impl IntoResponse {
    let path = world_io::normalize_world_path(Path::new(&input.path));
    // Forget is for missing list entries only — never orphan an on-disk world.
    if world_exists_on_disk(&path) {
        return (
            StatusCode::BAD_REQUEST,
            "world still exists on disk; use Delete to remove it",
        )
            .into_response();
    }
    let key = world_io::path_cmp_key(&path);
    forget_by_key(&server, &key)
}

fn forget_registry_only(server: &Arc<ServerState>, key: &str) -> Result<(), String> {
    world_io::mutate_projects(|file| {
        file.projects
            .retain(|item| world_io::path_cmp_key(Path::new(&item.path)) != *key);
        Ok(())
    })?;
    // Registry lock released before app mutex (canonical order).
    let mut app = server.app.lock().unwrap();
    if app
        .active
        .as_ref()
        .is_some_and(|world| world_io::path_cmp_key(&world.path) == *key)
    {
        app.active = None;
    }
    Ok(())
}

fn forget_by_key(server: &Arc<ServerState>, key: &str) -> axum::response::Response {
    match forget_registry_only(server, key) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => registry_error(error),
    }
}

fn clear_active_if_key(server: &Arc<ServerState>, key: &str) {
    let mut app = server.app.lock().unwrap();
    if app
        .active
        .as_ref()
        .is_some_and(|world| world_io::path_cmp_key(&world.path) == *key)
    {
        app.active = None;
    }
}

async fn delete_project(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<DeleteProjectInput>,
) -> impl IntoResponse {
    delete::delete_world(&server, input)
}

async fn close_project(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    server.app.lock().unwrap().active = None;
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests;
