use std::path::Path;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use mapkeeper_core::projects::ProjectEntry;
use mapkeeper_core::spatial::preset_by_id;
use mapkeeper_core::world;
use serde::{Deserialize, Serialize};

use crate::state::{ActiveWorld, ServerState};
use crate::world_io;

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

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/open", post(open_project))
        .route("/api/projects/forget", post(forget_project))
        .route("/api/projects/delete", post(delete_project))
        .route("/api/projects/close", post(close_project))
}

async fn list_projects(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    let file = world_io::load_projects();
    let projects = file
        .projects
        .into_iter()
        .map(|entry| ProjectStatus {
            valid: Path::new(&entry.path).join("mapkeeper.toml").is_file(),
            id: entry.id,
            path: entry.path,
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
    let manifest_path = path.join("mapkeeper.toml");
    if manifest_path.exists() {
        return (
            StatusCode::CONFLICT,
            format!("{} already contains a world", path.display()),
        )
            .into_response();
    }
    let preset = match input.preset_id.as_deref() {
        None | Some("") => mapkeeper_core::spatial::alpha_default_preset(),
        Some(preset_id) => match preset_by_id(preset_id) {
            Some(preset)
                if mapkeeper_core::spatial::create_presets()
                    .iter()
                    .any(|p| p.id == preset.id) =>
            {
                preset
            }
            Some(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("preset `{preset_id}` is not available in Create catalog"),
                )
                    .into_response();
            }
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("unknown map extent preset `{preset_id}`"),
                )
                    .into_response();
            }
        },
    };
    if let Err(error) = std::fs::create_dir_all(&path).and_then(|_| {
        std::fs::write(
            &manifest_path,
            world::manifest_toml_with_preset(&id, preset),
        )
    }) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    if let Err(error) = crate::spatial::ensure_spatial_state(&path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    activate_and_register(&server, id, path)
}

async fn open_project(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<ProjectPathInput>,
) -> impl IntoResponse {
    let path = world_io::normalize_world_path(Path::new(&input.path));
    let id = match world_io::read_manifest_id(&path) {
        Ok(id) => id,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    activate_and_register(&server, id, path)
}

fn activate_and_register(
    server: &Arc<ServerState>,
    id: String,
    path: std::path::PathBuf,
) -> axum::response::Response {
    if let Err(error) = crate::spatial::ensure_spatial_state(&path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    let entry = ProjectEntry {
        id: id.clone(),
        path: path.display().to_string(),
    };
    let mut file = world_io::load_projects();
    file.projects.retain(|item| item.id != id);
    file.upsert(entry.clone());
    if let Err(error) = world_io::save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    server.app.lock().unwrap().active = Some(ActiveWorld { path, id });
    Json(entry).into_response()
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

fn forget_by_key(server: &Arc<ServerState>, key: &str) -> axum::response::Response {
    let mut file = world_io::load_projects();
    file.projects
        .retain(|item| world_io::path_cmp_key(Path::new(&item.path)) != *key);
    if let Err(error) = world_io::save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    let mut app = server.app.lock().unwrap();
    if app
        .active
        .as_ref()
        .is_some_and(|world| world_io::path_cmp_key(&world.path) == *key)
    {
        app.active = None;
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn delete_project(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<ProjectPathInput>,
) -> impl IntoResponse {
    let path = world_io::normalize_world_path(Path::new(&input.path));
    if world_io::read_manifest_id(&path).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            "target is not a mapkeeper world workspace",
        )
            .into_response();
    }
    if let Err(error) = std::fs::remove_dir_all(&path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }
    let key = world_io::path_cmp_key(&path);
    forget_by_key(&server, &key)
}

async fn close_project(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    server.app.lock().unwrap().active = None;
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn forget_allowed_only_when_manifest_missing() {
        let temp = tempfile::tempdir().unwrap();
        let world = temp.path().join("ghost");
        fs::create_dir_all(&world).unwrap();
        assert!(!world_exists_on_disk(&world));
        fs::write(
            world.join("mapkeeper.toml"),
            world::manifest_toml("ghost"),
        )
        .unwrap();
        assert!(world_exists_on_disk(&world));
    }
}
