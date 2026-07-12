//! HTTP-level characterization tests (reuses `tests/support` harness).

mod support;

use std::sync::Arc;

use axum::http::StatusCode;
use mapkeeper_core::build_state::read_build;
use support::harness::{
    elevation_int_at, lake_catalog_with_cell_marker, layer_cell_write, read_lakes_json, seed_world,
    Harness,
};
use tempfile::tempdir;

#[tokio::test]
async fn active_world_switch_redirects_implicit_writes() {
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
async fn parallel_layer_batch_writes_serialize_via_app_state_mutex() {
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
    let h2 = {
        let h = harness.clone();
        tokio::spawn(async move {
            h.put_layer_batch("elevation", &[layer_cell_write(2, 0, 20)])
                .await
        })
    };
    assert_eq!(h1.await.unwrap(), StatusCode::NO_CONTENT);
    assert_eq!(h2.await.unwrap(), StatusCode::NO_CONTENT);

    assert_eq!(elevation_int_at(&world, 1, 0), 10);
    assert_eq!(elevation_int_at(&world, 2, 0), 20);
}

#[tokio::test]
async fn build_bounds_api_ignores_draft_write_failure() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "build-api-fp", 14, 8);
    std::fs::write(world.join("map/layers/land_mask.json"), b"{}").unwrap();

    std::env::set_var("MAPKEEPER_FAILPOINT", "build_draft");
    let harness = Harness::with_active_world(Some(world.clone()));
    let status = harness.put_build_bounds("small").await;
    std::env::remove_var("MAPKEEPER_FAILPOINT");

    assert_eq!(status, StatusCode::OK);
    assert!(
        read_build(&world).is_none(),
        "PUT /api/build/bounds uses `let _ = write_build_draft` — bounds change without draft marker"
    );
}

#[test]
#[ignore = "future build-lifecycle-tx: bounds reset and build draft share one transaction"]
fn build_bounds_api_fails_when_draft_write_fails() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "build-api-fp-future", 14, 8);
    std::fs::write(world.join("map/layers/land_mask.json"), b"{}").unwrap();
    let manifest_before =
        std::fs::read_to_string(world.join("map/manifest.json")).expect("manifest");

    let rt = tokio::runtime::Runtime::new().unwrap();
    std::env::set_var("MAPKEEPER_FAILPOINT", "build_draft");
    let status = rt.block_on(async {
        let harness = Harness::with_active_world(Some(world.clone()));
        harness.put_build_bounds("small").await
    });
    std::env::remove_var("MAPKEEPER_FAILPOINT");

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let manifest_after =
        std::fs::read_to_string(world.join("map/manifest.json")).expect("manifest");
    assert_eq!(manifest_after, manifest_before);
}
