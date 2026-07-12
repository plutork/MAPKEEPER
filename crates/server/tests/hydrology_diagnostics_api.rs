//! Read-only Hydrology v2 diagnostics endpoint.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mapkeeper_core::hydro::{filled_elevation_layer, OCEAN_ELEVATION};
use mapkeeper_core::layer::MapManifest;
use mapkeeper_server::{build_router, ServerConfig};
use tempfile::tempdir;
use tower::ServiceExt;

fn seed_world(world: &std::path::Path) {
    std::fs::write(
        world.join("mapkeeper.toml"),
        "[world]\nid = \"hydrology-diagnostics-test\"\n",
    )
    .unwrap();
    let manifest = MapManifest::default_v0(14, 8);
    let manifest_path = world.join("map/manifest.json");
    std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    std::fs::write(&manifest_path, manifest.to_json_pretty().unwrap()).unwrap();
    let bounds = mapkeeper_core::hex::MapBounds::new(14, 8);
    let elevation = filled_elevation_layer(&bounds, OCEAN_ELEVATION);
    let elevation_path = world.join("map/layers/elevation.json");
    std::fs::create_dir_all(elevation_path.parent().unwrap()).unwrap();
    std::fs::write(&elevation_path, elevation.to_json_pretty().unwrap()).unwrap();
}

#[tokio::test]
async fn diagnostics_report_missing_snapshot_without_reading_legacy_catalog() {
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
    let request = Request::builder()
        .uri("/api/hydrology/diagnostics")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["snapshot_active"], false);
    assert_eq!(json["generated_river_count"], 0);
    assert_eq!(json["river_id_matches_snapshot"], false);
}
