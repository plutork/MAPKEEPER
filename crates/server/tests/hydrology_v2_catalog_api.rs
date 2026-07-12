//! Generated river catalog is a projection of the active Hydrology v2 snapshot.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::hydro::SEA_LEVEL;
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue, MapManifest};
use mapkeeper_core::rivers::RiverCatalog;
use mapkeeper_server::{build_router, ServerConfig};
use tempfile::tempdir;
use tower::ServiceExt;

fn seed_world(world: &std::path::Path) {
    std::fs::write(
        world.join("mapkeeper.toml"),
        "[world]\nid = \"v2-catalog\"\n",
    )
    .unwrap();
    let bounds = MapBounds::new(14, 8);
    let manifest_path = world.join("map/manifest.json");
    std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    std::fs::write(
        manifest_path,
        MapManifest::default_v0(bounds.width, bounds.height)
            .to_json_pretty()
            .unwrap(),
    )
    .unwrap();
    let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
    for cell in 0..bounds.len() {
        let height = if cell < bounds.width as usize {
            SEA_LEVEL
        } else {
            SEA_LEVEL + 20
        };
        elevation.set(cell, DenseState::Value(LayerValue::Int(height)));
    }
    let elevation_path = world.join("map/layers/elevation.json");
    std::fs::create_dir_all(elevation_path.parent().unwrap()).unwrap();
    std::fs::write(elevation_path, elevation.to_json_pretty().unwrap()).unwrap();
}

#[tokio::test]
async fn generate_rivers_activates_v2_snapshot_not_legacy_catalog() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world);
    let legacy_path = world.join("map/rivers.json");
    std::fs::write(
        &legacy_path,
        RiverCatalog::default().to_json_pretty().unwrap(),
    )
    .unwrap();
    let legacy_before = std::fs::read_to_string(&legacy_path).unwrap();
    let web_dist = tempdir().unwrap();
    let app = build_router(&ServerConfig {
        world: Some(world.clone()),
        port: 0,
        web_dist: web_dist.path().to_path_buf(),
    })
    .unwrap();

    let response = app
        .oneshot(
            Request::post("/api/rivers/generate")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"river_density":"many"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(world.join("map/hydrology-v2.json").is_file());
    assert!(world.join("map/layers/river_id.json").is_file());
    assert_eq!(std::fs::read_to_string(legacy_path).unwrap(), legacy_before);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let generated: RiverCatalog = serde_json::from_slice(&body).unwrap();
    assert!(!generated.rivers.is_empty());
    let response: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(response["render_paths"]["paths"].is_array());
}
