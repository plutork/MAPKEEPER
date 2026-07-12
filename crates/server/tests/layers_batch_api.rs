//! `PUT /api/layers/:id/batch` characterization (agent-reliability phase 1).

mod support;

use axum::http::StatusCode;
use support::harness::{elevation_int_at, layer_cell_write, seed_world, Harness};
use tempfile::tempdir;

#[tokio::test]
async fn layer_batch_put_writes_multiple_cells() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "batch-api", 14, 8);
    let harness = Harness::with_active_world(Some(world.clone()));

    let updates = vec![
        layer_cell_write(0, 0, 5),
        layer_cell_write(1, 0, 12),
        layer_cell_write(2, 0, -3),
    ];
    assert_eq!(
        harness.put_layer_batch("elevation", &updates).await,
        StatusCode::NO_CONTENT
    );

    assert_eq!(elevation_int_at(&world, 0, 0), 5);
    assert_eq!(elevation_int_at(&world, 1, 0), 12);
    assert_eq!(elevation_int_at(&world, 2, 0), -3);

    let raw = std::fs::read_to_string(world.join("map/layers/elevation.json")).unwrap();
    let bounds = mapkeeper_core::hex::MapBounds::new(14, 8);
    let layer = mapkeeper_core::layer::DenseLayer::read_or_empty(
        Some(&raw),
        mapkeeper_core::layer::ELEVATION_LAYER_ID,
        mapkeeper_core::layer::ValueType::Integer,
        &bounds,
    );
    let index = bounds
        .index_of(mapkeeper_core::hex::Axial::new(0, 0))
        .expect("in bounds");
    assert_eq!(layer.int_or(index, 0), 5);
}

#[tokio::test]
async fn layer_batch_put_empty_body_is_no_content() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "batch-empty", 14, 8);
    let harness = Harness::with_active_world(Some(world));

    assert_eq!(
        harness.put_layer_batch("elevation", &[]).await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn layer_batch_put_without_active_world_is_conflict() {
    let harness = Harness::launcher();
    assert_eq!(
        harness
            .put_layer_batch("elevation", &[layer_cell_write(0, 0, 1)])
            .await,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn layer_batch_put_rejects_derived_hydrology_layer() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world, "batch-forbidden", 14, 8);
    let harness = Harness::with_active_world(Some(world));

    assert_eq!(
        harness
            .put_layer_batch(
                mapkeeper_core::worldgen::hydrology::CHANNEL_NODE_LAYER_ID,
                &[layer_cell_write(0, 0, 1)],
            )
            .await,
        StatusCode::FORBIDDEN
    );
}
