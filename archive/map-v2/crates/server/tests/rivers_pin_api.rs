//! Legacy manual river pin API (source→mouth).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::hydro::SEA_LEVEL;
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue, MapManifest};
use mapkeeper_server::{build_router, ServerConfig};
use tempfile::tempdir;
use tower::ServiceExt;

fn seed_world(world: &std::path::Path) {
    std::fs::write(
        world.join("mapkeeper.toml"),
        "[world]\nid = \"river-pin-test\"\n",
    )
    .unwrap();
    let bounds = MapBounds::new(8, 4);
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
    for index in 0..bounds.len() {
        let cell = bounds.from_index(index).unwrap();
        let height = SEA_LEVEL + 20 - cell.q;
        elevation.set(index, DenseState::Value(LayerValue::Int(height)));
    }
    let elevation_path = world.join("map/layers/elevation.json");
    std::fs::create_dir_all(elevation_path.parent().unwrap()).unwrap();
    std::fs::write(elevation_path, elevation.to_json_pretty().unwrap()).unwrap();
}

#[tokio::test]
async fn pin_river_creates_legacy_catalog_without_snapshot() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world);
    let web_dist = tempdir().unwrap();
    let app = build_router(&ServerConfig {
        world: Some(world),
        port: 0,
        web_dist: web_dist.path().to_path_buf(),
    })
    .unwrap();

    let response = app
        .oneshot(
            Request::post("/api/rivers/pin")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"source_q":1,"source_r":0,"mouth_q":3,"mouth_r":0}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["river_id"], 1);
    assert!(json["rivers"].as_array().is_some_and(|rivers| !rivers.is_empty()));
}

#[tokio::test]
async fn pin_river_rejected_when_hydrology_snapshot_active() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world);
    let web_dist = tempdir().unwrap();
    let app = build_router(&ServerConfig {
        world: Some(world.clone()),
        port: 0,
        web_dist: web_dist.path().to_path_buf(),
    })
    .unwrap();

    let generate = app
        .clone()
        .oneshot(
            Request::post("/api/rivers/generate")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"river_density":"balanced"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(generate.status(), StatusCode::OK);

    let pin = app
        .oneshot(
            Request::post("/api/rivers/pin")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"source_q":1,"source_r":0,"mouth_q":3,"mouth_r":0}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pin.status(), StatusCode::CONFLICT);
}
