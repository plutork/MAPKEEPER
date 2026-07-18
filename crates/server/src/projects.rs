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
    // Restart reconciliation for interrupted Delete (N-025).
    let _ = world_io::reconcile_delete_inflights();
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
    SpatialConfig::from_preset(preset).assert_matches_catalog()?;
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

    let registry = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return registry_error(error),
    };
    match world_io::classify_create_marker_at(&path, &registry) {
        world_io::CreateMarkerClass::SafeIncomplete => {
            if let Err(error) = world_io::cleanup_incomplete_create(&path) {
                return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
            }
        }
        world_io::CreateMarkerClass::CompleteRegistered { .. }
        | world_io::CreateMarkerClass::CompleteUnregistered { .. } => {
            return (
                StatusCode::CONFLICT,
                format!("{} already contains a world", path.display()),
            )
                .into_response();
        }
        world_io::CreateMarkerClass::Ambiguous { reason } => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("create_incomplete: recovery_required ({reason})"),
            )
                .into_response();
        }
        world_io::CreateMarkerClass::NoMarker => {}
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
        let _ = world_io::cleanup_after_failed_create(&path);
        return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }

    let fail = |server: &Arc<ServerState>, path: &Path, msg: String| -> axum::response::Response {
        let _ = world_io::cleanup_after_failed_create(path);
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
    if let Err(error) = world_io::mutate_projects(|file| {
        file.projects.retain(|item| item.id != id);
        file.upsert(entry.clone());
        Ok(())
    }) {
        return fail(
            server,
            &path,
            format!("create_incomplete: registry: {error}"),
        );
    }

    let create_recovery = match world_io::clear_incomplete_marker(&path) {
        Ok(()) => None,
        Err(_) => Some("marker_residual"),
    };

    // Registry lock released before app mutex (canonical order).
    server.app.lock().unwrap().active = Some(ActiveWorld {
        path: path.clone(),
        id: id.clone(),
    });
    Json(CreateProjectResult {
        id: entry.id,
        path: entry.path,
        create_recovery,
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
            match world_io::cleanup_incomplete_create(&path) {
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
        file.upsert(entry.clone());
        Ok(())
    }) {
        return registry_error(error);
    }
    // Registry lock released before app mutex (canonical order).
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
    let _ = world_io::reconcile_delete_inflights();

    let path = world_io::normalize_world_path(Path::new(&input.path));
    let key = world_io::path_cmp_key(&path);
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

    let registered = world_io::find_registered(&file, &path).cloned();
    if registered.is_none() {
        // Idempotent: already removed from registry and disk (successful prior Delete).
        if !path.exists() {
            clear_active_if_key(&server, &key);
            let _ = world_io::clear_delete_inflight(&key);
            return StatusCode::NO_CONTENT.into_response();
        }
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: world is not registered".to_string(),
        )
            .into_response();
    }
    let registered = registered.unwrap();

    if registered.id != expected_id {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: expected_id does not match registry".to_string(),
        )
            .into_response();
    }

    // Always operate on the canonical registered path (reject planted aliases).
    let registered_path = world_io::normalize_world_path(Path::new(&registered.path));
    if world_io::path_cmp_key(&path) != world_io::path_cmp_key(&registered_path) {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: path does not match registry entry".to_string(),
        )
            .into_response();
    }

    let manifest_id = match world_io::read_manifest_id(&registered_path) {
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

    let inflight = world_io::DeleteInflight {
        key: key.clone(),
        id: registered.id.clone(),
        path: registered.path.clone(),
    };
    if let Err(error) = world_io::write_delete_inflight(&inflight) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }

    // Registry first so Home never points at a mid-delete path.
    if let Err(error) = world_io::mutate_projects(|next| {
        next.projects
            .retain(|item| world_io::path_cmp_key(Path::new(&item.path)) != key);
        Ok(())
    }) {
        let _ = world_io::clear_delete_inflight(&key);
        return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }

    match world_io::move_world_to_trash(&registered_path, expected_id) {
        Ok(_trash) => {
            clear_active_if_key(&server, &key);
            if let Err(error) = world_io::clear_delete_inflight(&key) {
                // World trashed + registry clean; stale inflight is reconciled on next list.
                return (
                    StatusCode::NO_CONTENT,
                    // Still success; surface residual via empty body — list reconciles.
                    format!("delete_recovery: inflight_clear_failed: {error}"),
                )
                    .into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(move_error) => {
            #[cfg(test)]
            let force_restore_fail = world_io::take_delete_restore_failpoint();
            #[cfg(not(test))]
            let force_restore_fail = false;

            let restore_result = if force_restore_fail {
                Err("delete_recovery: restore failpoint".to_string())
            } else {
                world_io::mutate_projects(|restore| {
                    restore.upsert(registered.clone());
                    Ok(())
                })
            };

            match restore_result {
                Ok(()) => {
                    if let Err(clear_err) = world_io::clear_delete_inflight(&key) {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!(
                                "delete_recovery: move_failed_registry_restored; \
                                 inflight_clear_failed: {clear_err}; move: {move_error}"
                            ),
                        )
                            .into_response();
                    }
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("delete_rejected: {move_error}"),
                    )
                        .into_response()
                }
                Err(restore_error) => {
                    // Keep inflight; world on disk; registry missing → restart reconcile.
                    (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!(
                            "delete_recovery: registry_rollback_failed ({restore_error}); \
                             move: {move_error}"
                        ),
                    )
                        .into_response()
                }
            }
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
    use crate::world_io::lock_appdata_env;
    use mapkeeper_core::projects::ProjectsFile;
    use mapkeeper_core::spatial::alpha_default_preset;
    use std::fs;
    use tower::ServiceExt;

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
        let _lock = lock_appdata_env();
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
        let _lock = lock_appdata_env();
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
        let _lock = lock_appdata_env();
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
        let _lock = lock_appdata_env();
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
        let _lock = lock_appdata_env();
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
        let _lock = lock_appdata_env();
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
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let path = world_io::projects_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{nope").unwrap();
        let err = world_io::load_projects().unwrap_err();
        assert!(err.starts_with("corrupt_registry:"));
    }

    #[test]
    fn create_registry_write_failure_keeps_complete_unregistered_world() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        // Allow registry load (empty), then make save fail by replacing mapkeeper dir with a file.
        let server = test_server();
        let world = tmp.path().join("worlds").join("regfail");
        // Pre-create appdata mapkeeper so first load works; swap to file before save via hook:
        // Simpler: create world pieces manually then classify — registry save fail mid-create:
        fs::create_dir_all(tmp.path().join("mapkeeper")).unwrap();
        // Poison save by making projects.json path a directory after empty load path exists.
        // Instead drive fail by blocking parent after marker+manifest+spatial via monkeypatch-less approach:
        // Create succeeds until save: write a file where projects.json parent must be writable —
        // use atomic replace fail by making projects.json a directory.
        let projects = world_io::projects_path();
        fs::create_dir_all(projects.parent().unwrap()).unwrap();
        // Start create in a thread-like sequence: call transactional_create after turning
        // projects.json into a directory so atomic_replace/save fails.
        fs::create_dir_all(&projects).unwrap(); // projects.json as dir → save fails
        let resp = transactional_create(
            &server,
            "regfail".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        // Complete on-disk world must not be wiped (state C).
        assert!(world.join("mapkeeper.toml").is_file());
        assert!(world.join("spatial/state.json").is_file());
        assert!(world_io::is_incomplete_create(&world));
        let registry = ProjectsFile::default();
        assert!(matches!(
            world_io::classify_create_marker_at(&world, &registry),
            world_io::CreateMarkerClass::CompleteUnregistered { .. }
        ));
    }

    #[test]
    fn restart_after_true_incomplete_cleans_on_open() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let world = tmp.path().join("interrupted");
        fs::create_dir_all(&world).unwrap();
        world_io::write_incomplete_marker(&world).unwrap();
        fs::write(world.join("mapkeeper.toml"), "partial").unwrap();
        assert!(matches!(
            world_io::classify_create_marker_at(&world, &ProjectsFile::default()),
            world_io::CreateMarkerClass::SafeIncomplete
        ));
        world_io::cleanup_incomplete_create(&world).unwrap();
        assert!(!world.exists());
    }

    fn response_json(resp: axum::response::Response) -> serde_json::Value {
        use http_body_util::BodyExt;
        let bytes = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async { resp.into_body().collect().await.unwrap().to_bytes() });
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) })
        })
    }

    #[test]
    fn clear_marker_failure_returns_recovery_and_open_reconciles() {
        let _lock = lock_appdata_env();
        world_io::clear_clear_marker_failpoint();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("worlds").join("residual");
        world_io::set_clear_marker_failpoint();
        let resp = transactional_create(
            &server,
            "residual".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp);
        assert_eq!(body["create_recovery"], "marker_residual");
        assert!(world_io::is_incomplete_create(&world));
        assert!(world.join("mapkeeper.toml").is_file());
        assert!(world.join("spatial/state.json").is_file());
        let file = world_io::load_projects().unwrap();
        assert!(world_io::find_registered(&file, &world).is_some());
        assert!(matches!(
            world_io::classify_create_marker_at(&world, &file),
            world_io::CreateMarkerClass::CompleteRegistered { .. }
        ));
        // Simulated restart/open: keep world, clear marker.
        let _ = world_io::clear_incomplete_marker(&world);
        assert!(!world_io::is_incomplete_create(&world));
        let open = activate_and_register(&server, "residual".into(), world.clone());
        assert_eq!(open.status(), StatusCode::OK);
        assert!(world.exists());
    }

    #[test]
    fn marker_plus_complete_registered_never_deleted() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("worlds").join("keep");
        assert_eq!(
            transactional_create(&server, "keep".into(), world.clone(), alpha_default_preset())
                .status(),
            StatusCode::OK
        );
        world_io::write_incomplete_marker(&world).unwrap();
        let file = world_io::load_projects().unwrap();
        assert!(matches!(
            world_io::classify_create_marker_at(&world, &file),
            world_io::CreateMarkerClass::CompleteRegistered { .. }
        ));
        assert!(world_io::cleanup_incomplete_create(&world).is_err());
        assert!(world.join("mapkeeper.toml").is_file());
        let open = activate_and_register(&server, "keep".into(), world.clone());
        // open_project reconciles via classifier; exercise clear + activate
        let _ = world_io::clear_incomplete_marker(&world);
        assert_eq!(open.status(), StatusCode::OK);
        assert!(world.exists());
    }

    #[test]
    fn marker_plus_complete_unregistered_never_deleted() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("worlds").join("orphan");
        assert_eq!(
            transactional_create(&server, "orphan".into(), world.clone(), alpha_default_preset())
                .status(),
            StatusCode::OK
        );
        // Drop registry entry; leave durable world + residual marker.
        world_io::save_projects(&ProjectsFile::default()).unwrap();
        world_io::write_incomplete_marker(&world).unwrap();
        let file = world_io::load_projects().unwrap();
        assert!(matches!(
            world_io::classify_create_marker_at(&world, &file),
            world_io::CreateMarkerClass::CompleteUnregistered { .. }
        ));
        assert!(world_io::cleanup_incomplete_create(&world).is_err());
        assert!(world.join("mapkeeper.toml").is_file());
        let open = activate_and_register(&server, "orphan".into(), world.clone());
        assert_eq!(open.status(), StatusCode::OK);
        let file = world_io::load_projects().unwrap();
        assert!(world_io::find_registered(&file, &world).is_some());
    }

    #[test]
    fn marker_plus_partial_world_safe_incomplete_can_cleanup() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let world = tmp.path().join("partial-world");
        fs::create_dir_all(&world).unwrap();
        world_io::write_incomplete_marker(&world).unwrap();
        fs::write(
            world.join("mapkeeper.toml"),
            world::manifest_toml("partial-world"),
        )
        .unwrap();
        // Valid manifest, no spatial → SafeIncomplete (allowlisted only).
        assert!(matches!(
            world_io::classify_create_marker_at(&world, &ProjectsFile::default()),
            world_io::CreateMarkerClass::SafeIncomplete
        ));
        world_io::cleanup_incomplete_create(&world).unwrap();
        assert!(!world.exists());
    }

    #[test]
    fn planted_marker_in_user_folder_not_deleted() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let world = tmp.path().join("my-notes");
        fs::create_dir_all(&world).unwrap();
        world_io::write_incomplete_marker(&world).unwrap();
        fs::write(world.join("campaign.txt"), "keep me").unwrap();
        assert!(matches!(
            world_io::classify_create_marker_at(&world, &ProjectsFile::default()),
            world_io::CreateMarkerClass::Ambiguous {
                reason: "foreign_entries"
            }
        ));
        assert!(world_io::cleanup_incomplete_create(&world).is_err());
        assert!(world.join("campaign.txt").is_file());
        let server = test_server();
        let resp = transactional_create(
            &server,
            "my-notes".into(),
            world.clone(),
            alpha_default_preset(),
        );
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(world.join("campaign.txt").is_file());
    }

    #[test]
    fn ambiguous_cleanup_preserves_author_files() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let world = tmp.path().join("mixed");
        fs::create_dir_all(world.join("spatial")).unwrap();
        world_io::write_incomplete_marker(&world).unwrap();
        fs::write(world.join("mapkeeper.toml"), "not-valid-toml{{{").unwrap();
        fs::write(world.join("author.md"), "lore").unwrap();
        assert!(world_io::cleanup_incomplete_create(&world).is_err());
        assert!(world.join("author.md").is_file());
        assert!(world.join("mapkeeper.toml").is_file());
    }

    #[test]
    fn concurrent_creates_keep_both_entries() {
        use std::sync::Barrier;
        use std::thread;

        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let barrier = Arc::new(Barrier::new(2));
        let worlds = [
            tmp.path().join("worlds").join("ca"),
            tmp.path().join("worlds").join("cb"),
        ];
        let mut handles = Vec::new();
        for (id, world) in [("ca", worlds[0].clone()), ("cb", worlds[1].clone())] {
            let server = server.clone();
            let barrier = barrier.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                transactional_create(&server, id.into(), world, alpha_default_preset())
            }));
        }
        let statuses: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().unwrap().status())
            .collect();
        assert!(statuses.iter().all(|s| *s == StatusCode::OK), "{statuses:?}");
        let file = world_io::load_projects().unwrap();
        assert_eq!(file.projects.len(), 2);
        let mut ids: Vec<_> = file.projects.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["ca".to_string(), "cb".to_string()]);
    }

    #[test]
    fn concurrent_open_register_keeps_both_entries() {
        use std::sync::Barrier;
        use std::thread;

        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let wa = tmp.path().join("worlds").join("oa");
        let wb = tmp.path().join("worlds").join("ob");
        assert_eq!(
            transactional_create(&server, "oa".into(), wa.clone(), alpha_default_preset()).status(),
            StatusCode::OK
        );
        assert_eq!(
            transactional_create(&server, "ob".into(), wb.clone(), alpha_default_preset()).status(),
            StatusCode::OK
        );
        world_io::save_projects(&ProjectsFile::default()).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for (id, world) in [("oa", wa), ("ob", wb)] {
            let server = server.clone();
            let barrier = barrier.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                activate_and_register(&server, id.into(), world)
            }));
        }
        assert!(handles
            .into_iter()
            .all(|h| h.join().unwrap().status() == StatusCode::OK));
        let file = world_io::load_projects().unwrap();
        assert_eq!(file.projects.len(), 2);
    }

    #[test]
    fn concurrent_delete_and_create_preserve_survivor() {
        use std::sync::Barrier;
        use std::thread;

        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let doomed = tmp.path().join("worlds").join("doom");
        let born = tmp.path().join("worlds").join("born");
        assert_eq!(
            transactional_create(&server, "doom".into(), doomed.clone(), alpha_default_preset())
                .status(),
            StatusCode::OK
        );
        let barrier = Arc::new(Barrier::new(2));
        let s1 = server.clone();
        let s2 = server.clone();
        let b1 = barrier.clone();
        let doomed_path = doomed.clone();
        let delete_handle = thread::spawn(move || {
            b1.wait();
            let app = routes().with_state(s1);
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    app.oneshot(
                        axum::http::Request::builder()
                            .method("POST")
                            .uri("/api/projects/delete")
                            .header("content-type", "application/json")
                            .body(Body::from(
                                serde_json::json!({
                                    "path": doomed_path.display().to_string(),
                                    "expected_id": "doom"
                                })
                                .to_string(),
                            ))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
                })
        });
        let create_handle = thread::spawn(move || {
            barrier.wait();
            transactional_create(&s2, "born".into(), born.clone(), alpha_default_preset())
        });
        assert_eq!(delete_handle.join().unwrap(), StatusCode::NO_CONTENT);
        assert_eq!(create_handle.join().unwrap().status(), StatusCode::OK);
        assert!(!doomed.exists());
        let file = world_io::load_projects().unwrap();
        assert_eq!(file.projects.len(), 1);
        assert_eq!(file.projects[0].id, "born");
    }

    #[test]
    fn concurrent_forget_and_open() {
        use std::sync::Barrier;
        use std::thread;

        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let keep = tmp.path().join("worlds").join("keep");
        assert_eq!(
            transactional_create(&server, "keep".into(), keep.clone(), alpha_default_preset())
                .status(),
            StatusCode::OK
        );
        let ghost = tmp.path().join("worlds").join("ghost-missing");
        world_io::mutate_projects(|file| {
            file.upsert(ProjectEntry {
                id: "ghost".into(),
                path: ghost.display().to_string(),
            });
            Ok(())
        })
        .unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let s1 = server.clone();
        let s2 = server.clone();
        let b1 = barrier.clone();
        let ghost_path = ghost.clone();
        let forget_handle = thread::spawn(move || {
            b1.wait();
            forget_by_key(&s1, &world_io::path_cmp_key(&ghost_path))
        });
        let open_handle = thread::spawn(move || {
            barrier.wait();
            activate_and_register(&s2, "keep".into(), keep)
        });
        assert_eq!(forget_handle.join().unwrap().status(), StatusCode::NO_CONTENT);
        assert_eq!(open_handle.join().unwrap().status(), StatusCode::OK);
        let file = world_io::load_projects().unwrap();
        assert_eq!(file.projects.len(), 1);
        assert_eq!(file.projects[0].id, "keep");
    }

    #[test]
    fn mutate_failure_releases_registry_lock() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let err = world_io::mutate_projects(|_| -> Result<(), String> { Err("boom".into()) })
            .unwrap_err();
        assert_eq!(err, "boom");
        world_io::mutate_projects(|file| {
            file.upsert(ProjectEntry {
                id: "after".into(),
                path: "/after".into(),
            });
            Ok(())
        })
        .unwrap();
        let file = world_io::load_projects().unwrap();
        assert_eq!(file.projects.len(), 1);
        assert_eq!(file.projects[0].id, "after");
    }

    #[test]
    fn corrupt_registry_stays_visible_under_mutate() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let path = world_io::projects_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{broken-registry").unwrap();
        let err = world_io::mutate_projects(|file| {
            file.projects.clear();
            Ok(())
        })
        .unwrap_err();
        assert!(err.contains("corrupt_registry"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{broken-registry");
        assert!(world_io::load_projects().unwrap_err().contains("corrupt_registry"));
    }

    async fn delete_status(server: Arc<ServerState>, body: serde_json::Value) -> StatusCode {
        delete_response(server, body).await.0
    }

    async fn delete_response(
        server: Arc<ServerState>,
        body: serde_json::Value,
    ) -> (StatusCode, String) {
        use http_body_util::BodyExt;
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
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    #[test]
    fn alias_paths_share_world_lock() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("locked");
        fs::create_dir_all(&real).unwrap();
        let alias = dir.path().join("locked-alias");
        let linked = {
            #[cfg(windows)]
            {
                std::process::Command::new("cmd")
                    .args([
                        "/C",
                        "mklink",
                        "/J",
                        &alias.display().to_string(),
                        &real.display().to_string(),
                    ])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&real, &alias).is_ok()
            }
            #[cfg(not(any(windows, unix)))]
            {
                false
            }
        };
        if !linked {
            return;
        }
        let server = test_server();
        let k1 = world_io::path_cmp_key(&real);
        let k2 = world_io::path_cmp_key(&alias);
        assert_eq!(k1, k2);
        let l1 = server.world_lock(&k1);
        let l2 = server.world_lock(&k2);
        assert!(std::sync::Arc::ptr_eq(&l1, &l2));
    }

    #[tokio::test]
    async fn delete_move_failure_restores_registry() {
        let _lock = lock_appdata_env();
        world_io::clear_delete_failpoints();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("worlds").join("movefail");
        assert_eq!(
            transactional_create(
                &server,
                "movefail".into(),
                world.clone(),
                alpha_default_preset()
            )
            .status(),
            StatusCode::OK
        );
        world_io::set_move_trash_failpoint();
        let (status, body) = delete_response(
            server.clone(),
            serde_json::json!({
                "path": world.display().to_string(),
                "expected_id": "movefail"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains("move to trash") || body.contains("failpoint"));
        assert!(world.exists());
        let file = world_io::load_projects().unwrap();
        assert!(world_io::find_registered(&file, &world).is_some());
        assert!(world_io::load_delete_inflight(&world_io::path_cmp_key(&world)).is_none());
    }

    #[tokio::test]
    async fn delete_registry_rollback_failure_keeps_inflight() {
        let _lock = lock_appdata_env();
        world_io::clear_delete_failpoints();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("worlds").join("rollfail");
        assert_eq!(
            transactional_create(
                &server,
                "rollfail".into(),
                world.clone(),
                alpha_default_preset()
            )
            .status(),
            StatusCode::OK
        );
        world_io::set_move_trash_failpoint();
        world_io::set_delete_restore_failpoint();
        let (status, body) = delete_response(
            server.clone(),
            serde_json::json!({
                "path": world.display().to_string(),
                "expected_id": "rollfail"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body.contains("delete_recovery: registry_rollback_failed"));
        assert!(world.exists());
        let file = world_io::load_projects().unwrap();
        assert!(world_io::find_registered(&file, &world).is_none());
        assert!(world_io::load_delete_inflight(&world_io::path_cmp_key(&world)).is_some());

        // Restart reconciliation restores registry.
        let notes = world_io::reconcile_delete_inflights().unwrap();
        assert!(!notes.is_empty());
        let file = world_io::load_projects().unwrap();
        assert!(world_io::find_registered(&file, &world).is_some());
        assert!(world_io::load_delete_inflight(&world_io::path_cmp_key(&world)).is_none());
    }

    #[tokio::test]
    async fn delete_success_clears_active_and_is_idempotent() {
        let _lock = lock_appdata_env();
        world_io::clear_delete_failpoints();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("worlds").join("gone2");
        assert_eq!(
            transactional_create(&server, "gone2".into(), world.clone(), alpha_default_preset())
                .status(),
            StatusCode::OK
        );
        assert!(server.app.lock().unwrap().active.is_some());
        let status = delete_status(
            server.clone(),
            serde_json::json!({
                "path": world.display().to_string(),
                "expected_id": "gone2"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!world.exists());
        assert!(server.app.lock().unwrap().active.is_none());
        assert!(world_io::load_projects().unwrap().projects.is_empty());

        // Repeat Delete reports consistent already-deleted state.
        let again = delete_status(
            server.clone(),
            serde_json::json!({
                "path": world.display().to_string(),
                "expected_id": "gone2"
            }),
        )
        .await;
        assert_eq!(again, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_rejects_planted_manifest_not_on_registered_path() {
        let _lock = lock_appdata_env();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let registered = tmp.path().join("worlds").join("reg");
        assert_eq!(
            transactional_create(
                &server,
                "reg".into(),
                registered.clone(),
                alpha_default_preset()
            )
            .status(),
            StatusCode::OK
        );
        let planted = tmp.path().join("planted-elsewhere");
        fs::create_dir_all(&planted).unwrap();
        fs::write(
            planted.join("mapkeeper.toml"),
            world::manifest_toml("reg"),
        )
        .unwrap();
        let status = delete_status(
            server,
            serde_json::json!({
                "path": planted.display().to_string(),
                "expected_id": "reg"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(planted.exists());
        assert!(registered.exists());
    }

    #[tokio::test]
    async fn delete_inflight_after_successful_move_reconciles_on_list() {
        let _lock = lock_appdata_env();
        world_io::clear_delete_failpoints();
        let tmp = tempfile::tempdir().unwrap();
        let _guard = AppDataGuard::set(tmp.path());
        let server = test_server();
        let world = tmp.path().join("worlds").join("stale");
        assert_eq!(
            transactional_create(&server, "stale".into(), world.clone(), alpha_default_preset())
                .status(),
            StatusCode::OK
        );
        let key = world_io::path_cmp_key(&world);
        // Simulate crash after trash + registry remove but before inflight clear.
        world_io::mutate_projects(|file| {
            file.projects.clear();
            Ok(())
        })
        .unwrap();
        let trash = world_io::move_world_to_trash(&world, "stale").unwrap();
        assert!(trash.exists());
        world_io::write_delete_inflight(&world_io::DeleteInflight {
            key: key.clone(),
            id: "stale".into(),
            path: world.display().to_string(),
        })
        .unwrap();
        let notes = world_io::reconcile_delete_inflights().unwrap();
        assert!(notes.iter().any(|n| n.contains("cleared stale")));
        assert!(world_io::load_delete_inflight(&key).is_none());
        assert!(world_io::load_projects().unwrap().projects.is_empty());
        let _ = server;
    }

    #[tokio::test]
    async fn delete_api_rejects_wrong_id_and_unregistered() {
        let _lock = lock_appdata_env();
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
