//! In-crate characterization: failpoints + direct FS-level RMW (no HTTP).

use std::sync::{Arc, Barrier};
use std::thread;

use mapkeeper_core::build_state::read_build;
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::layer::MapManifest;
use mapkeeper_core::map_preset::MapPreset;
use tempfile::tempdir;

use crate::world_io::{map_manifest_path, read_dense_layer, reset_build_bounds, write_dense_layer};

fn seed_world(path: &std::path::Path, world_id: &str) -> MapBounds {
    let bounds = MapBounds::new(14, 8);
    std::fs::create_dir_all(path.join("map/layers")).unwrap();
    std::fs::write(
        path.join("mapkeeper.toml"),
        mapkeeper_core::build_state::manifest_toml_with_build(world_id, false),
    )
    .unwrap();
    let manifest = MapManifest::default_v0(14, 8);
    std::fs::write(
        path.join("map/manifest.json"),
        manifest.to_json_pretty().unwrap(),
    )
    .unwrap();
    let ocean = mapkeeper_core::hydro::filled_elevation_layer(
        &bounds,
        mapkeeper_core::hydro::OCEAN_ELEVATION,
    );
    std::fs::write(
        path.join("map/layers/elevation.json"),
        ocean.to_json_pretty().unwrap(),
    )
    .unwrap();
    bounds
}

#[test]
fn direct_parallel_dense_layer_rmw_can_lose_updates() {
    let _lock = crate::world_io::failpoint_lock();
    let dir = tempdir().unwrap();
    let world = dir.path();
    let bounds = seed_world(world, "rmw-direct");
    let world = Arc::new(world.to_path_buf());
    let barrier = Arc::new(Barrier::new(2));

    let w1 = Arc::clone(&world);
    let b1 = Arc::clone(&barrier);
    let t1 = thread::spawn(move || {
        b1.wait();
        let mut layer = read_dense_layer(&w1, "elevation", &bounds);
        let i = bounds.index_of(Axial::new(3, 0)).unwrap();
        layer.set(i, mapkeeper_core::layer::DenseState::Value(
            mapkeeper_core::layer::LayerValue::Int(30),
        ));
        write_dense_layer(&w1, &layer).unwrap();
    });

    let w2 = Arc::clone(&world);
    let b2 = Arc::clone(&barrier);
    let t2 = thread::spawn(move || {
        b2.wait();
        let mut layer = read_dense_layer(&w2, "elevation", &bounds);
        let i = bounds.index_of(Axial::new(4, 0)).unwrap();
        layer.set(i, mapkeeper_core::layer::DenseState::Value(
            mapkeeper_core::layer::LayerValue::Int(40),
        ));
        write_dense_layer(&w2, &layer).unwrap();
    });

    t1.join().unwrap();
    t2.join().unwrap();

    let final_layer = read_dense_layer(&world, "elevation", &bounds);
    let v3 = final_layer.int_or(bounds.index_of(Axial::new(3, 0)).unwrap(), 0);
    let v4 = final_layer.int_or(bounds.index_of(Axial::new(4, 0)).unwrap(), 0);
    assert!(
        (v3 == 30 && v4 == 0) || (v3 == 0 && v4 == 40) || (v3 == 30 && v4 == 40),
        "unexpected parallel RMW outcome: cell3={v3}, cell4={v4}"
    );
}

#[test]
fn build_bounds_reset_rolls_back_when_draft_write_fails() {
    let _lock = crate::world_io::failpoint_lock();
    let dir = tempdir().unwrap();
    let world = dir.path();
    seed_world(world, "build-fp");
    std::fs::write(world.join("map/layers/land_mask.json"), b"{}").unwrap();
    let manifest_before = std::fs::read_to_string(map_manifest_path(world)).unwrap();

    std::env::set_var("MAPKEEPER_FAILPOINT", "build_draft");
    let err = reset_build_bounds(world, MapPreset::Small).unwrap_err();
    std::env::remove_var("MAPKEEPER_FAILPOINT");
    assert!(err.contains("simulated"));

    assert_eq!(
        std::fs::read_to_string(map_manifest_path(world)).unwrap(),
        manifest_before
    );
    assert!(read_build(world).is_none());
}
