//! Launcher projects API + fixture worlds import (D-96 S1).

use std::path::Path;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::build_state;
use mapkeeper_core::map_preset::{parse_map_preset, MapPreset};
use mapkeeper_core::projects::ProjectEntry;
use mapkeeper_core::world;
use serde::{Deserialize, Serialize};

use crate::state::{ActiveWorld, ServerState};
use crate::world_io;

#[derive(Serialize)]
struct ProjectStatus {
    id: String,
    path: String,
    valid: bool,
    legacy_map: bool,
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
    #[serde(default)]
    map_preset: Option<String>,
    #[serde(default)]
    build_wizard: Option<bool>,
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

const FIXTURE_WORLD_LABELS: &[(&str, &str)] = &[
    ("coastal-slope", "Coastal slope"),
    ("mountain-ridge", "Mountain ridge"),
    ("enclosed-basin", "Enclosed basin"),
    ("gentle-plain", "Gentle plain"),
    ("dual-watershed", "Dual watershed"),
];

fn list_fixture_worlds() -> FixtureWorldsResponse {
    let Some(root) = world_io::fixture_worlds_root() else {
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

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/open", axum::routing::post(open_project))
        .route("/api/projects/forget", axum::routing::post(forget_project))
        .route("/api/projects/delete", axum::routing::post(delete_project))
        .route("/api/projects/close", axum::routing::post(close_project))
        .route("/api/fixture-worlds", get(list_fixture_worlds_handler))
        .route(
            "/api/fixture-worlds/open",
            axum::routing::post(open_fixture_world),
        )
}

async fn list_projects(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    let file = world_io::load_projects();
    let projects = file
        .projects
        .into_iter()
        .map(|p| {
            let world_path = Path::new(&p.path);
            let valid = world_path.join("mapkeeper.toml").exists();
            let legacy_map = valid && world_io::legacy_map_folder(world_path);
            let (build_draft, build_step) = if valid {
                match build_state::read_build(world_path) {
                    Some(b) if build_state::is_draft(&b) => {
                        (true, Some(build_state::normalize_wizard_step(&b)))
                    }
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
    let active = server.app.lock().unwrap().active.as_ref().map(|a| ProjectEntry {
        id: a.id.clone(),
        path: a.path.display().to_string(),
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
    if !world::is_valid_world_id(&input.id) {
        return (
            StatusCode::BAD_REQUEST,
            "invalid world name format: use lowercase letters, digits, '-', '_' only",
        )
            .into_response();
    }
    let path = world_io::normalize_world_path(Path::new(&input.path));
    let manifest = path.join("mapkeeper.toml");
    if manifest.exists() {
        return (
            StatusCode::CONFLICT,
            format!("{} already has a mapkeeper.toml", path.display()),
        )
            .into_response();
    }
    let _write = server.world_locks.acquire_write(&input.id).await;
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
    if let Err(err) = world_io::write_map_manifest(&path, preset) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut file = world_io::load_projects();
    file.projects.retain(|entry| entry.id != input.id);
    file.upsert(ProjectEntry {
        id: input.id.clone(),
        path: path.display().to_string(),
    });
    if let Err(err) = world_io::save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    server.app.lock().unwrap().active = Some(ActiveWorld {
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
    State(server): State<Arc<ServerState>>,
    Json(input): Json<OpenProjectInput>,
) -> impl IntoResponse {
    let path = world_io::normalize_world_path(Path::new(&input.path));
    let id = match world_io::read_manifest_id(&path) {
        Ok(id) => id,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };

    let mut file = world_io::load_projects();
    file.projects.retain(|entry| entry.id != id);
    file.upsert(ProjectEntry {
        id: id.clone(),
        path: path.display().to_string(),
    });
    if let Err(err) = world_io::save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    server.app.lock().unwrap().active = Some(ActiveWorld {
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
    State(server): State<Arc<ServerState>>,
    Json(input): Json<OpenFixtureInput>,
) -> impl IntoResponse {
    let src_id = match world_io::read_fixture_manifest_id(&input.slug) {
        Ok(id) => id,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };
    let _write = server.world_locks.acquire_write(&src_id).await;
    let path = match world_io::import_fixture_world(&input.slug) {
        Ok(path) => path,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };
    let id = match world_io::read_manifest_id(&path) {
        Ok(id) => id,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };

    let mut file = world_io::load_projects();
    file.projects.retain(|entry| entry.id != id);
    file.upsert(ProjectEntry {
        id: id.clone(),
        path: path.display().to_string(),
    });
    if let Err(err) = world_io::save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    server.app.lock().unwrap().active = Some(ActiveWorld {
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
    State(server): State<Arc<ServerState>>,
    Json(input): Json<ForgetProjectInput>,
) -> impl IntoResponse {
    let forget_key = world_io::path_cmp_key(Path::new(&input.path));

    let mut file = world_io::load_projects();
    let before = file.projects.len();
    file.projects
        .retain(|p| world_io::path_cmp_key(Path::new(&p.path)) != forget_key);

    if file.projects.len() == before {
        return StatusCode::NO_CONTENT.into_response();
    }
    if let Err(err) = world_io::save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut app = server.app.lock().unwrap();
    if let Some(active) = app.active.as_ref() {
        if world_io::path_cmp_key(&active.path) == forget_key {
            app.active = None;
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn delete_project(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<DeleteProjectInput>,
) -> impl IntoResponse {
    let target = world_io::normalize_world_path(Path::new(&input.path));
    let target_key = world_io::path_cmp_key(&target);
    let manifest = target.join("mapkeeper.toml");
    if !manifest.exists() {
        return (
            StatusCode::BAD_REQUEST,
            "target path has no mapkeeper.toml — use Forget to remove a stale entry",
        )
            .into_response();
    }
    let id = match world_io::read_manifest_id(&target) {
        Ok(id) => id,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };
    let _write = server.world_locks.acquire_write(&id).await;

    if let Err(err) = std::fs::remove_dir_all(&target) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut file = world_io::load_projects();
    file.projects
        .retain(|p| world_io::path_cmp_key(Path::new(&p.path)) != target_key);
    if let Err(err) = world_io::save_projects(&file) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let mut app = server.app.lock().unwrap();
    if let Some(active) = app.active.as_ref() {
        if world_io::path_cmp_key(&active.path) == target_key {
            app.active = None;
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn close_project(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    server.app.lock().unwrap().active = None;
    StatusCode::NO_CONTENT
}
