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

mod create;
mod delete;

use create::{resolve_create_preset, transactional_create};

#[derive(Serialize)]
struct ProjectStatus {
    id: String,
    path: String,
    valid: bool,
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
        .route("/api/projects/forget", post(forget_project))
        .route("/api/projects/delete", post(delete_project))
        .route("/api/projects/close", post(close_project))
        .route("/api/projects/restore-bak", post(restore_registry_bak))
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
            let valid = path.join("mapkeeper.toml").is_file()
                && !matches!(
                    class,
                    world_io::CreateMarkerClass::SafeIncomplete
                        | world_io::CreateMarkerClass::Ambiguous { .. }
                );
            ProjectStatus {
                valid,
                id: entry.id.clone(),
                path: entry.path.clone(),
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
        world_io::CreateMarkerClass::CompleteRegistered { world_id }
        | world_io::CreateMarkerClass::CompleteUnregistered { world_id } => {
            // Durable Create with residual marker — keep world, clear marker, open.
            let _ = world_io::clear_incomplete_marker(&path);
            return activate_and_register(&server, world_id, path);
        }
        world_io::CreateMarkerClass::Ambiguous { reason } => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("create_incomplete: recovery_required ({reason})"),
            )
                .into_response();
        }
    }
    let id = match world_io::read_manifest_id(&path) {
        Ok(id) => id,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    activate_and_register(&server, id, path)
}

fn activate_and_register(
    server: &Arc<ServerState>,
    id: String,
    path: PathBuf,
) -> axum::response::Response {
    if let Err(error) = crate::spatial::ensure_spatial_state(&path) {
        let message = error.to_string();
        let status = if message.contains("corrupt_spatial") {
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
    // Registry lock released before app mutex (canonical order).
    server.app.lock().unwrap().active = Some(ActiveWorld { path, id });
    Json(entry).into_response()
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
