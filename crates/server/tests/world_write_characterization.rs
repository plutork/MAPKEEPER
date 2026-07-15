//! HTTP-level characterization tests (reuses `tests/support` harness).

mod support;

use std::sync::Arc;

use axum::http::StatusCode;
use mapkeeper_core::build_state::read_build;
use support::harness::{
    elevation_int_at, failpoint_test_lock, isolated_registry_home, real_projects_file_path,
    lake_catalog_with_cell_marker, layer_cell_write, read_lakes_json, registry_test_lock,
    seed_world, Harness,
};
use tempfile::tempdir;

#[tokio::test]
async fn open_project_writes_isolated_registry_not_real_appdata() {
    let _lock = registry_test_lock();
    let real_path = real_projects_file_path();
    let before = std::fs::read_to_string(&real_path).ok();

    let root = tempdir().unwrap();
    let world = root.path().join("registry-iso");
    seed_world(&world, "registry-iso", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let after = std::fs::read_to_string(&real_path).ok();
    assert_eq!(before, after);

    let isolated_path = isolated_registry_home()
        .path()
        .join("mapkeeper/projects.json");
    let isolated = std::fs::read_to_string(isolated_path).expect("isolated projects.json");
    assert!(isolated.contains("registry-iso"));
}

#[tokio::test]
async fn scoped_writes_ignore_active_world_switch() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world_a = root.path().join("world-a");
    let world_b = root.path().join("world-b");
    seed_world(&world_a, "scope-switch-a", 14, 8);
    seed_world(&world_b, "scope-switch-b", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world_a).await, StatusCode::OK);
    assert_eq!(
        harness
            .put_lakes_catalog_scoped(&lake_catalog_with_cell_marker(10), Some("scope-switch-a"))
            .await,
        StatusCode::OK,
    );

    assert_eq!(harness.open_project(&world_b).await, StatusCode::OK);
    assert_eq!(
        harness
            .put_lakes_catalog_scoped(&lake_catalog_with_cell_marker(20), Some("scope-switch-b"))
            .await,
        StatusCode::OK,
    );

    // Active is world-b, but scoped header still targets world-a.
    assert_eq!(
        harness
            .put_lakes_catalog_scoped(&lake_catalog_with_cell_marker(11), Some("scope-switch-a"))
            .await,
        StatusCode::OK,
    );

    assert_eq!(read_lakes_json(&world_a).lakes[0].cells, vec![11]);
    assert_eq!(read_lakes_json(&world_b).lakes[0].cells, vec![20]);
}

#[tokio::test]
async fn active_world_switch_redirects_implicit_writes() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world_a = root.path().join("world-a");
    let world_b = root.path().join("world-b");
    seed_world(&world_a, "world-a", 14, 8);
    seed_world(&world_b, "world-b", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world_a).await, StatusCode::OK);
    assert_eq!(
        harness
            .put_lakes_catalog(&lake_catalog_with_cell_marker(10))
            .await,
        StatusCode::OK,
    );

    assert_eq!(harness.open_project(&world_b).await, StatusCode::OK);
    assert_eq!(
        harness
            .put_lakes_catalog(&lake_catalog_with_cell_marker(20))
            .await,
        StatusCode::OK,
    );

    let lakes_a = read_lakes_json(&world_a);
    assert_eq!(lakes_a.lakes[0].cells, vec![10]);
    let lakes_b = read_lakes_json(&world_b);
    assert_eq!(lakes_b.lakes[0].cells, vec![20]);

    assert_eq!(harness.open_project(&world_a).await, StatusCode::OK);
    assert_eq!(
        harness
            .put_lakes_catalog(&lake_catalog_with_cell_marker(11))
            .await,
        StatusCode::OK,
    );
    assert_eq!(read_lakes_json(&world_a).lakes[0].cells, vec![11]);
    assert_eq!(read_lakes_json(&world_b).lakes[0].cells, vec![20]);
}

#[tokio::test]
async fn same_world_writes_serialize_under_world_lock() {
    let _lock = failpoint_test_lock();
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "lock-serialize", 14, 8);
    let harness = Arc::new(Harness::with_active_world(Some(world.clone())));

    std::env::set_var("MAPKEEPER_FAILPOINT", "world_write_hold");
    let started = std::time::Instant::now();
    assert_eq!(
        harness
            .put_layer_batch("elevation", &[layer_cell_write(1, 0, 10)])
            .await,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(
        harness
            .put_layer_batch("elevation", &[layer_cell_write(2, 0, 20)])
            .await,
        StatusCode::NO_CONTENT,
    );
    std::env::remove_var("MAPKEEPER_FAILPOINT");

    assert!(
        started.elapsed() >= std::time::Duration::from_millis(140),
        "same-world writes should serialize under per-world lock"
    );
    assert_eq!(elevation_int_at(&world, 1, 0), 10);
    assert_eq!(elevation_int_at(&world, 2, 0), 20);
}

#[tokio::test]
async fn cross_world_writes_run_in_parallel() {
    let _lock = failpoint_test_lock();
    let _registry = registry_test_lock();
    let root = tempdir().unwrap();
    let world_a = root.path().join("lock-par-a");
    let world_b = root.path().join("lock-par-b");
    seed_world(&world_a, "lock-par-a", 14, 8);
    seed_world(&world_b, "lock-par-b", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world_a).await, StatusCode::OK);
    assert_eq!(harness.open_project(&world_b).await, StatusCode::OK);

    std::env::set_var("MAPKEEPER_FAILPOINT", "world_write_hold");
    let started = std::time::Instant::now();
    let cat_a = lake_catalog_with_cell_marker(10);
    let cat_b = lake_catalog_with_cell_marker(20);
    let (s1, s2) = tokio::join!(
        harness.put_lakes_catalog_scoped(&cat_a, Some("lock-par-a")),
        harness.put_lakes_catalog_scoped(&cat_b, Some("lock-par-b")),
    );
    std::env::remove_var("MAPKEEPER_FAILPOINT");

    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);
    assert!(
        started.elapsed() < std::time::Duration::from_millis(140),
        "different worlds should not block each other's writes"
    );
}

#[tokio::test]
async fn world_lock_releases_after_handler_error() {
    let _lock = failpoint_test_lock();
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "lock-error", 14, 8);

    let harness = Harness::with_active_world(Some(world.clone()));
    std::env::set_var("MAPKEEPER_FAILPOINT", "build_draft");
    assert_eq!(
        harness.put_build_bounds("small").await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    std::env::remove_var("MAPKEEPER_FAILPOINT");

    assert_eq!(
        harness
            .put_lakes_catalog(&lake_catalog_with_cell_marker(7))
            .await,
        StatusCode::OK,
    );
    assert_eq!(read_lakes_json(&world).lakes[0].cells, vec![7]);
}

#[tokio::test]
async fn parallel_layer_batch_writes_serialize_per_world_lock() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "rmw-batch", 14, 8);
    let harness = Arc::new(Harness::with_active_world(Some(world.clone())));

    let h1 = {
        let h = harness.clone();
        tokio::spawn(async move {
            h.put_layer_batch("elevation", &[layer_cell_write(1, 0, 10)])
                .await
        })
    };
    assert_eq!(h1.await.unwrap(), StatusCode::NO_CONTENT);
    let h2 = {
        let h = harness.clone();
        tokio::spawn(async move {
            h.put_layer_batch("elevation", &[layer_cell_write(2, 0, 20)])
                .await
        })
    };
    assert_eq!(h2.await.unwrap(), StatusCode::NO_CONTENT);

    assert_eq!(elevation_int_at(&world, 1, 0), 10);
    assert_eq!(elevation_int_at(&world, 2, 0), 20);
}

#[tokio::test]
async fn build_bounds_api_fails_when_draft_write_fails() {
    let _lock = failpoint_test_lock();
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "build-api-fp", 14, 8);
    std::fs::write(world.join("map/layers/land_mask.json"), b"{}").unwrap();
    let manifest_before =
        std::fs::read_to_string(world.join("map/manifest.json")).expect("manifest");

    std::env::set_var("MAPKEEPER_FAILPOINT", "build_draft");
    let harness = Harness::with_active_world(Some(world.clone()));
    let status = harness.put_build_bounds("small").await;
    std::env::remove_var("MAPKEEPER_FAILPOINT");

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        std::fs::read_to_string(world.join("map/manifest.json")).unwrap(),
        manifest_before
    );
    assert!(read_build(&world).is_none());
}
