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

#[tokio::test]
async fn land_mask_generate_requires_base_revision_after_draft_bump() {
    use mapkeeper_core::build_state::manifest_toml_with_build;

    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("wizard-draft");
    seed_world(&world, "wizard-draft", 14, 8);
    std::fs::write(
        world.join("mapkeeper.toml"),
        manifest_toml_with_build("wizard-draft", true),
    )
    .unwrap();

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let draft_body = serde_json::json!({ "status": "draft", "step": 1 }).to_string();
    assert_eq!(
        harness
            .send_scoped_with_revision(
                "PUT",
                "/api/build",
                Some(draft_body.into_bytes()),
                Some("wizard-draft"),
                Some(0),
            )
            .await
            .0,
        StatusCode::NO_CONTENT,
    );

    let gen_body = serde_json::json!({
        "variant": "pangea",
        "character": "smooth",
        "regenerate_nonce": 0
    })
    .to_string();

    let (status, body) = harness
        .send_scoped(
            "POST",
            "/api/build/land-mask/generate",
            Some(gen_body.as_bytes().to_vec()),
            Some("wizard-draft"),
        )
        .await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    assert!(String::from_utf8_lossy(&body).contains("base_revision_required"));

    let (status, _) = harness
        .send_scoped_with_revision(
            "POST",
            "/api/build/land-mask/generate",
            Some(gen_body.into_bytes()),
            Some("wizard-draft"),
            Some(1),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(world.join("map/layers/land_mask.json").is_file());
}
