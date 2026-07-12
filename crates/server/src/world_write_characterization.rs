//! In-crate characterization: failpoints + direct FS-level RMW (no HTTP).

use std::sync::atomic::Ordering;
use std::sync::{Arc, Barrier};
use std::thread;

use mapkeeper_core::build_state::{read_build, write_build_draft, BUILD_STEP_SIZE};
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::lakes::{Lake, LakeCatalog};
use mapkeeper_core::layer::{DenseState, LayerValue, Bounds, MapManifest};
use mapkeeper_core::map_preset::MapPreset;
use mapkeeper_core::rivers::{sync_river_id_layer, River, RiverCatalog};
use tempfile::tempdir;

use crate::world_io::{
    failpoint_lock, persist_lake_generation, persist_rivers, read_dense_layer, read_lake_catalog,
    read_river_catalog, rewrite_world_bounds, write_dense_layer, SIMULATE_CLEAR_RIVERS_FAILURE,
    SIMULATE_LAYER_WRITE_FAILURE,
};

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
        layer.set(i, DenseState::Value(LayerValue::Int(30)));
        write_dense_layer(&w1, &layer).unwrap();
    });

    let w2 = Arc::clone(&world);
    let b2 = Arc::clone(&barrier);
    let t2 = thread::spawn(move || {
        b2.wait();
        let mut layer = read_dense_layer(&w2, "elevation", &bounds);
        let i = bounds.index_of(Axial::new(4, 0)).unwrap();
        layer.set(i, DenseState::Value(LayerValue::Int(40)));
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
fn persist_rivers_leaves_catalog_ahead_of_layer_on_layer_failpoint() {
    let _lock = failpoint_lock();
    let dir = tempdir().unwrap();
    let world = dir.path();
    let bounds = seed_world(world, "rivers-fp");

    let mut catalog = RiverCatalog::default();
    catalog.rivers.push(River {
        id: 1,
        cells: vec![5, 6],
        source: 5,
        mouth: 6,
        parent: 1,
        basin: 1,
        name: None,
    });
    catalog.next_id = 2;

    SIMULATE_LAYER_WRITE_FAILURE.store(true, Ordering::SeqCst);
    let err = persist_rivers(world, &catalog, &bounds).unwrap_err();
    SIMULATE_LAYER_WRITE_FAILURE.store(false, Ordering::SeqCst);
    assert!(err.contains("simulated"));

    let on_disk = read_river_catalog(world);
    assert_eq!(on_disk.rivers.len(), 1, "catalog written before layer");
    let layer = read_dense_layer(world, mapkeeper_core::layer::RIVER_ID_LAYER_ID, &bounds);
    let synced = sync_river_id_layer(&on_disk, &bounds);
    assert_ne!(
        layer, synced,
        "river_id layer not synced with catalog — known partial-write defect"
    );
}

#[test]
#[ignore = "future transactional-io: rollback catalog when river_id layer write fails"]
fn persist_rivers_rolls_back_catalog_on_layer_failure() {
    let _lock = failpoint_lock();
    let dir = tempdir().unwrap();
    let world = dir.path();
    let bounds = seed_world(world, "rivers-fp-future");
    let before = read_river_catalog(world);

    let mut catalog = RiverCatalog::default();
    catalog.rivers.push(River {
        id: 1,
        cells: vec![1],
        source: 1,
        mouth: 1,
        parent: 1,
        basin: 1,
        name: None,
    });
    catalog.next_id = 2;

    SIMULATE_LAYER_WRITE_FAILURE.store(true, Ordering::SeqCst);
    let _ = persist_rivers(world, &catalog, &bounds).unwrap_err();
    SIMULATE_LAYER_WRITE_FAILURE.store(false, Ordering::SeqCst);

    assert_eq!(read_river_catalog(world), before);
}

#[test]
fn persist_lake_generation_leaves_new_lakes_when_clear_rivers_fails() {
    let _lock = failpoint_lock();
    let dir = tempdir().unwrap();
    let world = dir.path();
    let bounds = seed_world(world, "lake-gen-fp");

    let mut rivers = RiverCatalog::default();
    rivers.rivers.push(River {
        id: 1,
        cells: vec![2, 3],
        source: 2,
        mouth: 3,
        parent: 1,
        basin: 1,
        name: None,
    });
    rivers.next_id = 2;
    persist_rivers(world, &rivers, &bounds).unwrap();

    let mut lakes = LakeCatalog::default();
    lakes.lakes.push(Lake {
        id: 1,
        cells: vec![7],
        outlet_cell: None,
        endorheic: false,
        name: None,
    });
    lakes.next_id = 2;

    SIMULATE_CLEAR_RIVERS_FAILURE.store(true, Ordering::SeqCst);
    let err = persist_lake_generation(world, &lakes, &bounds).unwrap_err();
    SIMULATE_CLEAR_RIVERS_FAILURE.store(false, Ordering::SeqCst);
    assert!(err.contains("simulated"));

    assert_eq!(read_lake_catalog(world).lakes.len(), 1, "lakes committed");
    assert_eq!(
        read_river_catalog(world).rivers.len(),
        1,
        "rivers not cleared — known partial water-bundle defect"
    );
}

#[test]
#[ignore = "future transactional-io: lake generation commits as one water bundle or full rollback"]
fn persist_lake_generation_rolls_back_lakes_when_clear_rivers_fails() {
    let _lock = failpoint_lock();
    let dir = tempdir().unwrap();
    let world = dir.path();
    let bounds = seed_world(world, "lake-gen-fp-future");
    let lakes_before = read_lake_catalog(world);

    let mut lakes = LakeCatalog::default();
    lakes.lakes.push(Lake {
        id: 1,
        cells: vec![1],
        outlet_cell: None,
        endorheic: false,
        name: None,
    });
    lakes.next_id = 2;

    SIMULATE_CLEAR_RIVERS_FAILURE.store(true, Ordering::SeqCst);
    let _ = persist_lake_generation(world, &lakes, &bounds).unwrap_err();
    SIMULATE_CLEAR_RIVERS_FAILURE.store(false, Ordering::SeqCst);

    assert_eq!(read_lake_catalog(world), lakes_before);
}

#[test]
fn bounds_reset_succeeds_when_build_draft_write_fails() {
    let dir = tempdir().unwrap();
    let world = dir.path();
    seed_world(world, "build-fp");
    std::fs::write(world.join("map/layers/land_mask.json"), b"{}").unwrap();

    rewrite_world_bounds(world, MapPreset::Small, true).unwrap();
    std::env::set_var("MAPKEEPER_FAILPOINT", "build_draft");
    let draft_err = write_build_draft(world, BUILD_STEP_SIZE).unwrap_err();
    std::env::remove_var("MAPKEEPER_FAILPOINT");
    assert!(draft_err.contains("simulated"));

    let manifest: MapManifest =
        serde_json::from_str(&std::fs::read_to_string(world.join("map/manifest.json")).unwrap())
            .unwrap();
    let (w, h) = MapPreset::Small.dimensions();
    assert_eq!(manifest.bounds, Bounds::HexRectangle { width: w, height: h });
    assert!(
        read_build(world).is_none(),
        "build draft missing after simulated failure — known lifecycle divergence"
    );
}
