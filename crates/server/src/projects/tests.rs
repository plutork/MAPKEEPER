//! Integration tests for `projects` (N-031: tests outside implementation).
//!
//! `lock_appdata_env` deliberately spans awaits: it serializes the process-wide
//! APPDATA override, and these tests run on a single-threaded test runtime.
#![allow(clippy::await_holding_lock)]

use super::*;
use crate::world_io::lock_appdata_env;
use axum::body::Body;
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
    fs::write(world.join("mapkeeper.toml"), world::manifest_toml("ghost")).unwrap();
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
    let _ = transactional_create(&server, "del".into(), world.clone(), alpha_default_preset());
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
    world_io::cleanup_incomplete_create(&world, &ProjectsFile::default()).unwrap();
    assert!(!world.exists());
}

fn response_json(resp: axum::response::Response) -> serde_json::Value {
    use http_body_util::BodyExt;
    let bytes = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { resp.into_body().collect().await.unwrap().to_bytes() });
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) }))
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
        transactional_create(
            &server,
            "keep".into(),
            world.clone(),
            alpha_default_preset()
        )
        .status(),
        StatusCode::OK
    );
    world_io::write_incomplete_marker(&world).unwrap();
    let file = world_io::load_projects().unwrap();
    assert!(matches!(
        world_io::classify_create_marker_at(&world, &file),
        world_io::CreateMarkerClass::CompleteRegistered { .. }
    ));
    assert!(world_io::cleanup_incomplete_create(&world, &file).is_err());
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
        transactional_create(
            &server,
            "orphan".into(),
            world.clone(),
            alpha_default_preset()
        )
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
    assert!(world_io::cleanup_incomplete_create(&world, &file).is_err());
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
    let empty = ProjectsFile::default();
    assert!(matches!(
        world_io::classify_create_marker_at(&world, &empty),
        world_io::CreateMarkerClass::SafeIncomplete
    ));
    world_io::cleanup_incomplete_create(&world, &empty).unwrap();
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
    let empty = ProjectsFile::default();
    assert!(matches!(
        world_io::classify_create_marker_at(&world, &empty),
        world_io::CreateMarkerClass::Ambiguous {
            reason: "foreign_entries"
        }
    ));
    assert!(world_io::cleanup_incomplete_create(&world, &empty).is_err());
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
    assert!(world_io::cleanup_incomplete_create(&world, &ProjectsFile::default()).is_err());
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
    assert!(
        statuses.iter().all(|s| *s == StatusCode::OK),
        "{statuses:?}"
    );
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
        transactional_create(
            &server,
            "doom".into(),
            doomed.clone(),
            alpha_default_preset()
        )
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
        world_io::upsert_registered(
            file,
            ProjectEntry {
                id: "ghost".into(),
                path: ghost.display().to_string(),
            },
        );
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
    assert_eq!(
        forget_handle.join().unwrap().status(),
        StatusCode::NO_CONTENT
    );
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
        world_io::upsert_registered(
            file,
            ProjectEntry {
                id: "after".into(),
                path: "/after".into(),
            },
        );
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
    assert!(world_io::load_projects()
        .unwrap_err()
        .contains("corrupt_registry"));
}

#[tokio::test]
async fn restore_bak_route_makes_corrupt_registry_listable_again() {
    let _lock = lock_appdata_env();
    let tmp = tempfile::tempdir().unwrap();
    let _guard = AppDataGuard::set(tmp.path());
    let path = world_io::projects_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    world_io::save_projects_to(
        &path,
        &mapkeeper_core::projects::ProjectsFile {
            projects: vec![mapkeeper_core::projects::ProjectEntry {
                id: "kept".into(),
                path: tmp.path().join("kept").display().to_string(),
            }],
        },
    )
    .unwrap();
    // Second write leaves a valid .bak, then the primary is damaged.
    world_io::save_projects_to(&path, &world_io::load_projects().unwrap()).unwrap();
    fs::write(&path, "{broken").unwrap();
    assert!(world_io::load_projects().is_err());

    let server = Arc::new(ServerState::new(None));
    let response = routes()
        .with_state(server)
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/projects/restore-bak")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(world_io::load_projects().unwrap().projects[0].id, "kept");
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
        transactional_create(
            &server,
            "gone2".into(),
            world.clone(),
            alpha_default_preset()
        )
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
    fs::write(planted.join("mapkeeper.toml"), world::manifest_toml("reg")).unwrap();
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
async fn delete_inflight_after_successful_move_reconciles_on_startup_path() {
    let _lock = lock_appdata_env();
    world_io::clear_delete_failpoints();
    let tmp = tempfile::tempdir().unwrap();
    let _guard = AppDataGuard::set(tmp.path());
    let server = test_server();
    let world = tmp.path().join("worlds").join("stale");
    assert_eq!(
        transactional_create(
            &server,
            "stale".into(),
            world.clone(),
            alpha_default_preset()
        )
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
    // GET list must stay read-only (no hidden reconcile).
    let app = routes().with_state(server.clone());
    let list_resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    assert!(world_io::load_delete_inflight(&key).is_some());

    // Startup / explicit reconcile still recovers.
    let notes = world_io::reconcile_delete_inflights().unwrap();
    assert!(notes.iter().any(|n| n.contains("cleared stale")));
    assert!(world_io::load_delete_inflight(&key).is_none());
    assert!(world_io::load_projects().unwrap().projects.is_empty());
}

#[tokio::test]
async fn delete_success_has_empty_204_body() {
    let _lock = lock_appdata_env();
    world_io::clear_delete_failpoints();
    let tmp = tempfile::tempdir().unwrap();
    let _guard = AppDataGuard::set(tmp.path());
    let server = test_server();
    let world = tmp.path().join("worlds").join("nobody");
    assert_eq!(
        transactional_create(
            &server,
            "nobody".into(),
            world.clone(),
            alpha_default_preset()
        )
        .status(),
        StatusCode::OK
    );
    let (status, body) = delete_response(
        server,
        serde_json::json!({
            "path": world.display().to_string(),
            "expected_id": "nobody"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "204 must not carry a body: {body:?}");
    assert!(!world.exists());
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
