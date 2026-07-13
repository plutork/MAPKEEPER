//! Map revision optimistic concurrency API tests.

mod support;

use axum::http::StatusCode;
use support::harness::{
    lake_catalog_with_cell_marker, read_lakes_json, registry_test_lock, seed_world, Harness,
};
use tempfile::tempdir;

#[tokio::test]
async fn stale_base_revision_returns_409_without_mutating_files() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("world-a");
    seed_world(&world, "rev-conflict-a", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    assert_eq!(
        harness
            .put_lakes_catalog_scoped_with_revision(
                &lake_catalog_with_cell_marker(10),
                Some("rev-conflict-a"),
                Some(0),
            )
            .await,
        StatusCode::OK,
    );
    assert_eq!(read_lakes_json(&world).lakes[0].cells, vec![10]);

    let (status, body, _) = harness
        .put_lakes_catalog_scoped_with_revision_raw(
            &lake_catalog_with_cell_marker(99),
            Some("rev-conflict-a"),
            Some(0),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(String::from_utf8_lossy(&body).contains("world_revision_mismatch"));
    assert_eq!(read_lakes_json(&world).lakes[0].cells, vec![10]);
}

#[tokio::test]
async fn matching_base_revision_serial_writes_both_commit() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("world-b");
    seed_world(&world, "rev-ok-b", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    assert_eq!(
        harness
            .put_lakes_catalog_scoped_with_revision(
                &lake_catalog_with_cell_marker(1),
                Some("rev-ok-b"),
                Some(0),
            )
            .await,
        StatusCode::OK,
    );
    assert_eq!(
        harness
            .put_lakes_catalog_scoped_with_revision(
                &lake_catalog_with_cell_marker(2),
                Some("rev-ok-b"),
                Some(1),
            )
            .await,
        StatusCode::OK,
    );
    assert_eq!(read_lakes_json(&world).lakes[0].cells, vec![2]);
}

#[tokio::test]
async fn revision_persists_in_manifest_after_successful_write() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("world-d");
    seed_world(&world, "rev-persist-d", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);
    assert_eq!(
        harness
            .put_lakes_catalog_scoped_with_revision(
                &lake_catalog_with_cell_marker(5),
                Some("rev-persist-d"),
                Some(0),
            )
            .await,
        StatusCode::OK,
    );

    let manifest = std::fs::read_to_string(world.join("map/manifest.json")).unwrap();
    assert!(manifest.contains("\"revision\": 1"));

    let harness2 = Harness::launcher();
    assert_eq!(harness2.open_project(&world).await, StatusCode::OK);
    let (status, _) = harness2
        .send_scoped_with_revision("GET", "/api/map", None, Some("rev-persist-d"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}
