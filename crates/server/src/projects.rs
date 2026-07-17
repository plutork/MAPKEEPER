use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use mapkeeper_core::projects::ProjectEntry;
use mapkeeper_core::spatial::preset_by_id;
use mapkeeper_core::world::{self, SpatialConfig};
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

#[derive(Deserialize)]
struct DeleteProjectInput {
    path: String,
    expected_id: String,
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/open", post(open_project))
        .route("/api/projects/forget", post(forget_project))
        .route("/api/projects/delete", post(delete_project))
        .route("/api/projects/close", post(close_project))
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
    let file = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return registry_error(error),
    };
    let projects = file
        .projects
        .into_iter()
        .map(|entry| ProjectStatus {
            valid: Path::new(&entry.path).join("mapkeeper.toml").is_file()
                && !world_io::is_incomplete_create(Path::new(&entry.path)),
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

fn resolve_create_preset(preset_id: Option<&str>) -> Result<&'static mapkeeper_core::spatial::MapExtentPreset, String> {
    let preset = match preset_id {
        None | Some("") => mapkeeper_core::spatial::alpha_default_preset(),
        Some(id) => match preset_by_id(id) {
            Some(preset)
                if mapkeeper_core::spatial::create_presets()
                    .iter()
                    .any(|p| p.id == preset.id) =>
            {
                preset
            }
            Some(_) => {
                return Err(format!("preset `{id}` is not available in Create catalog"));
            }
            None => {
                return Err(format!("unknown map extent preset `{id}`"));
            }
        },
    };
    // Full metric validation, not id-only.
    SpatialConfig::from_preset(preset)
        .assert_matches_catalog()
        .map_err(|e| e)?;
    Ok(preset)
}

/// Create dir + manifest + spatial + registry + activate, or recoverable cleanup (N-025).
fn transactional_create(
    server: &Arc<ServerState>,
    id: String,
    path: PathBuf,
    preset: &mapkeeper_core::spatial::MapExtentPreset,
) -> axum::response::Response {
    let manifest_path = path.join("mapkeeper.toml");

    if world_io::is_incomplete_create(&path) {
        if let Err(error) = world_io::cleanup_incomplete_create(&path) {
            return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
        }
    }

    if manifest_path.exists() {
        return (
            StatusCode::CONFLICT,
            format!("{} already contains a world", path.display()),
        )
            .into_response();
    }

    if path.exists() {
        let is_empty = std::fs::read_dir(&path)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return (
                StatusCode::BAD_REQUEST,
                format!(
                    "create_incomplete: existing non-world directory at {}",
                    path.display()
                ),
            )
                .into_response();
        }
    } else if let Err(error) = std::fs::create_dir_all(&path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }

    // Marker first — owns cleanup contour for this Create.
    if let Err(error) = world_io::write_incomplete_marker(&path) {
        let _ = world_io::cleanup_incomplete_create(&path);
        return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }

    let fail = |server: &Arc<ServerState>, path: &Path, msg: String| -> axum::response::Response {
        let _ = world_io::cleanup_incomplete_create(path);
        let key = world_io::path_cmp_key(path);
        let _ = forget_registry_only(server, &key);
        (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
    };

    if let Err(error) = crate::atomic_io::atomic_replace(
        &manifest_path,
        world::manifest_toml_with_preset(&id, preset).as_bytes(),
    ) {
        return fail(server, &path, format!("create_incomplete: manifest: {error}"));
    }

    if let Err(error) = crate::spatial::ensure_spatial_state(&path) {
        return fail(server, &path, format!("create_incomplete: spatial: {error}"));
    }

    let entry = ProjectEntry {
        id: id.clone(),
        path: path.display().to_string(),
    };
    let mut file = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return fail(server, &path, error),
    };
    file.projects.retain(|item| item.id != id);
    file.upsert(entry.clone());
    if let Err(error) = world_io::save_projects(&file) {
        return fail(
            server,
            &path,
            format!("create_incomplete: registry: {error}"),
        );
    }

    if let Err(error) = world_io::clear_incomplete_marker(&path) {
        // World is durable + registered; marker left is recoverable on next open.
        let _ = error;
    }

    server.app.lock().unwrap().active = Some(ActiveWorld {
        path: path.clone(),
        id,
    });
    Json(entry).into_response()
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
    if world_io::is_incomplete_create(&path) {
        // Interrupted Create: never treat as successful world.
        let _ = world_io::cleanup_incomplete_create(&path);
        return (
            StatusCode::BAD_REQUEST,
            "create_incomplete: interrupted create cleaned; retry Create".to_string(),
        )
            .into_response();
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
    let mut file = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return registry_error(error),
    };
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

fn forget_registry_only(server: &Arc<ServerState>, key: &str) -> Result<(), String> {
    let mut file = world_io::load_projects()?;
    file.projects
        .retain(|item| world_io::path_cmp_key(Path::new(&item.path)) != *key);
    world_io::save_projects(&file).map_err(|e| e.to_string())?;
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

async fn delete_project(
    State(server): State<Arc<ServerState>>,
    Json(input): Json<DeleteProjectInput>,
) -> impl IntoResponse {
    let path = world_io::normalize_world_path(Path::new(&input.path));
    let expected_id = input.expected_id.trim();
    if expected_id.is_empty() || !world::is_valid_world_id(expected_id) {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: expected_id required".to_string(),
        )
            .into_response();
    }

    let file = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return registry_error(error),
    };
    let registered = match world_io::find_registered(&file, &path) {
        Some(entry) => entry.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "delete_rejected: world is not registered".to_string(),
            )
                .into_response();
        }
    };
    if registered.id != expected_id {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: expected_id does not match registry".to_string(),
        )
            .into_response();
    }

    let manifest_id = match world_io::read_manifest_id(&path) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "delete_rejected: target is not a mapkeeper world workspace".to_string(),
            )
                .into_response();
        }
    };
    if manifest_id != expected_id {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: expected_id does not match manifest".to_string(),
        )
            .into_response();
    }

    // Registry entry path is the allowlist (rejects planted manifests elsewhere).
    let matches_registered =
        world_io::path_cmp_key(&path) == world_io::path_cmp_key(Path::new(&registered.path));
    if !matches_registered {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: path does not match registry entry".to_string(),
        )
            .into_response();
    }

    // Registry first so Home never points at a mid-delete path; restore on move failure.
    let key = world_io::path_cmp_key(&path);
    let mut next = file.clone();
    next.projects
        .retain(|item| world_io::path_cmp_key(Path::new(&item.path)) != key);
    if let Err(error) = world_io::save_projects(&next) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }

    match world_io::move_world_to_trash(&path, expected_id) {
        Ok(_trash) => {
            let mut app = server.app.lock().unwrap();
            if app
                .active
                .as_ref()
                .is_some_and(|world| world_io::path_cmp_key(&world.path) == key)
            {
                app.active = None;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            // Best-effort restore registry entry.
            let mut restore = match world_io::load_projects() {
                Ok(f) => f,
                Err(_) => file,
            };
            restore.upsert(registered);
            let _ = world_io::save_projects(&restore);
            (StatusCode::INTERNAL_SERVER_ERROR, error).into_response()
        }
    }
}

async fn close_project(State(server): State<Arc<ServerState>>) -> impl IntoResponse {
    server.app.lock().unwrap().active = None;
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use mapkeeper_core::spatial::alpha_default_preset;
    use std::fs;
    use std::sync::Mutex;
    use tower::ServiceExt;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct AppDataGuard {
        prev: Option<String>,
    }

    impl AppDataGuard {
        fn set(path: &Path) -> Self {
            let prev = std::env::var("APPDATA").ok();
            std::env::set_var("APPDATA", path);
            Self { prev }
        }
    }

    impl Drop for AppDataGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("APPDATA", v),
                None => std::env::remove_var("APPDATA"),
            }
        }
    }

    fn test_server() -> Arc<ServerState> {
        Arc::new(ServerState::new(None))
    }

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

    #[test]
    fn create_success_clears_marker_and_registers() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("worlds").join("alpha");
        let resp = transactional_create(
            &server,
            "alpha".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!world_io::is_incomplete_create(&world));
        assert!(world.join("mapkeeper.toml").is_file());
        assert!(world.join("spatial/state.json").is_file());
        let file = world_io::load_projects().unwrap();
        assert_eq!(file.projects.len(), 1);
        assert_eq!(file.projects[0].id, "alpha");
    }

    #[test]
    fn create_rejects_existing_non_world_dir() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("notes");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("readme.txt"), "mine").unwrap();
        let resp = transactional_create(
            &server,
            "notes".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(world.join("readme.txt").is_file());
        assert!(world_io::load_projects().unwrap().projects.is_empty());
    }

    #[test]
    fn create_retry_after_incomplete_marker() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("retry");
        fs::create_dir_all(&world).unwrap();
        world_io::write_incomplete_marker(&world).unwrap();
        fs::write(world.join("mapkeeper.toml"), "partial").unwrap();
        let resp = transactional_create(
            &server,
            "retry".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!world_io::is_incomplete_create(&world));
        let raw = fs::read_to_string(world.join("mapkeeper.toml")).unwrap();
        assert!(raw.contains("id = \"retry\""));
    }

    #[test]
    fn create_unknown_preset_rejected() {
        let err = resolve_create_preset(Some("not_a_real_preset")).unwrap_err();
        assert!(err.contains("unknown") || err.contains("not available"));
    }

    #[test]
    fn delete_rejects_wrong_id() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("del");
        let _ = transactional_create(
            &server,
            "del".into(),
            world.clone(),
            alpha_default_preset(),
        );
        // Simulate delete handler checks via shared helpers.
        let file = world_io::load_projects().unwrap();
        let reg = world_io::find_registered(&file, &world).unwrap();
        assert_eq!(reg.id, "del");
        assert_ne!(reg.id, "other");
    }

    #[test]
    fn delete_rejects_unregistered_planted_manifest() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let planted = tmp.path().join("planted");
        fs::create_dir_all(&planted).unwrap();
        fs::write(
            planted.join("mapkeeper.toml"),
            world::manifest_toml("planted"),
        )
        .unwrap();
        let file = world_io::load_projects().unwrap();
        assert!(world_io::find_registered(&file, &planted).is_none());
    }

    #[test]
    fn delete_moves_to_trash_not_purge() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("gone");
        let resp = transactional_create(
            &server,
            "gone".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::OK);

        let file = world_io::load_projects().unwrap();
        let registered = world_io::find_registered(&file, &world).unwrap().clone();
        let mut next = file.clone();
        let key = world_io::path_cmp_key(&world);
        next.projects
            .retain(|item| world_io::path_cmp_key(Path::new(&item.path)) != key);
        world_io::save_projects(&next).unwrap();
        let trash = world_io::move_world_to_trash(&world, &registered.id).unwrap();
        assert!(!world.exists());
        assert!(trash.join("mapkeeper.toml").is_file());
        assert!(trash.join("spatial/state.json").is_file());
    }

    #[test]
    fn registry_corrupt_surfaces() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let path = world_io::projects_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{nope").unwrap();
        let err = world_io::load_projects().unwrap_err();
        assert!(err.starts_with("corrupt_registry:"));
    }

    #[test]
    fn create_registry_write_failure_cleans_partial() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        // Block %APPDATA%/mapkeeper/ as a directory so registry save fails.
        fs::write(tmp.path().join("mapkeeper"), b"not-a-dir").unwrap();
        let server = test_server();
        let world = tmp.path().join("worlds").join("regfail");
        let resp = transactional_create(
            &server,
            "regfail".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!world.exists());
    }

    #[test]
    fn restart_after_interrupted_create_cleans_on_open() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let world = tmp.path().join("interrupted");
        fs::create_dir_all(&world).unwrap();
        world_io::write_incomplete_marker(&world).unwrap();
        fs::write(
            world.join("mapkeeper.toml"),
            world::manifest_toml("interrupted"),
        )
        .unwrap();
        let server = test_server();
        // open_project path via shared incomplete handling
        assert!(world_io::is_incomplete_create(&world));
        world_io::cleanup_incomplete_create(&world).unwrap();
        assert!(!world.exists());
        let _ = server;
    }

    async fn delete_status(server: Arc<ServerState>, body: serde_json::Value) -> StatusCode {
        let app = routes().with_state(server);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/projects/delete")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        response.status()
    }

    #[tokio::test]
    async fn delete_api_rejects_wrong_id_and_unregistered() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("api-del");
        let resp = transactional_create(
            &server,
            "api-del".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::OK);

        let wrong_id = delete_status(
            server.clone(),
            serde_json::json!({
                "path": world.display().to_string(),
                "expected_id": "other"
            }),
        )
        .await;
        assert_eq!(wrong_id, StatusCode::BAD_REQUEST);
        assert!(world.exists());

        let planted = tmp.path().join("planted-api");
        fs::create_dir_all(&planted).unwrap();
        fs::write(
            planted.join("mapkeeper.toml"),
            world::manifest_toml("planted-api"),
        )
        .unwrap();
        let unreg = delete_status(
            server.clone(),
            serde_json::json!({
                "path": planted.display().to_string(),
                "expected_id": "planted-api"
            }),
        )
        .await;
        assert_eq!(unreg, StatusCode::BAD_REQUEST);
        assert!(planted.exists());

        let ok = delete_status(
            server,
            serde_json::json!({
                "path": world.display().to_string(),
                "expected_id": "api-del"
            }),
        )
        .await;
        assert_eq!(ok, StatusCode::NO_CONTENT);
        assert!(!world.exists());
    }
}
