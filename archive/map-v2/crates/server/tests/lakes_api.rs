//! Lake catalog HTTP roundtrip (hydrology-lake-domain-v1).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mapkeeper_core::lakes::{Lake, LakeCatalog};
use mapkeeper_core::layer::MapManifest;
use mapkeeper_server::{build_router, ServerConfig};
use tempfile::tempdir;
use tower::ServiceExt;

fn seed_world(world: &std::path::Path) {
    std::fs::write(
        world.join("mapkeeper.toml"),
        "[world]\nid = \"lake-api-test\"\n",
    )
    .unwrap();
    let manifest = MapManifest::default_v0(14, 8);
    let manifest_path = world.join("map/manifest.json");
    std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    std::fs::write(&manifest_path, manifest.to_json_pretty().unwrap()).unwrap();
}

#[tokio::test]
async fn lakes_put_get_roundtrip() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world);
    let web_dist = tempdir().unwrap();

    let config = ServerConfig {
        world: Some(world.clone()),
        port: 0,
        web_dist: web_dist.path().to_path_buf(),
    };
    let app = build_router(&config).unwrap();

    let catalog = LakeCatalog {
        schema_version: 1,
        next_id: 2,
        lakes: vec![Lake {
            id: 1,
            cells: vec![4, 5],
            outlet_cell: Some(4),
            endorheic: false,
            name: None,
        }],
    };

    let put_body = serde_json::to_vec(&catalog).unwrap();
    let put = Request::builder()
        .method("PUT")
        .uri("/api/lakes")
        .header("content-type", "application/json")
        .body(Body::from(put_body))
        .unwrap();
    let put_resp = app.clone().oneshot(put).await.unwrap();
    assert_eq!(put_resp.status(), StatusCode::OK);

    let get = Request::builder()
        .uri("/api/lakes")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = get_resp.into_body().collect().await.unwrap().to_bytes();
    let read: LakeCatalog = serde_json::from_slice(&body).unwrap();
    assert_eq!(read, catalog);

    let layer_path = world.join("map/layers/lake_id.json");
    assert!(layer_path.exists());
}

#[tokio::test]
async fn lakes_get_without_active_world_is_conflict() {
    let web_dist = tempdir().unwrap();
    let config = ServerConfig {
        world: None,
        port: 0,
        web_dist: web_dist.path().to_path_buf(),
    };
    let app = build_router(&config).unwrap();
    let get = Request::builder()
        .uri("/api/lakes")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(get).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn lakes_generate_clears_rivers() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world);
    let web_dist = tempdir().unwrap();
    let bounds = mapkeeper_core::hex::MapBounds::new(14, 8);
    let ocean = mapkeeper_core::hydro::filled_elevation_layer(
        &bounds,
        mapkeeper_core::hydro::OCEAN_ELEVATION,
    );
    let elev_path = world.join("map/layers/elevation.json");
    std::fs::create_dir_all(elev_path.parent().unwrap()).unwrap();
    std::fs::write(&elev_path, ocean.to_json_pretty().unwrap()).unwrap();

    let mut rivers = mapkeeper_core::rivers::RiverCatalog::default();
    rivers.rivers.push(mapkeeper_core::rivers::River {
        id: 1,
        cells: vec![10, 11],
        source: 10,
        mouth: 11,
        parent: 1,
        basin: 1,
        name: None,
    });
    rivers.next_id = 2;
    std::fs::write(
        world.join("map/rivers.json"),
        rivers.to_json_pretty().unwrap(),
    )
    .unwrap();
    let river_layer = mapkeeper_core::rivers::sync_river_id_layer(&rivers, &bounds);
    std::fs::write(
        world.join("map/layers/river_id.json"),
        river_layer.to_json_pretty().unwrap(),
    )
    .unwrap();

    let config = ServerConfig {
        world: Some(world.clone()),
        port: 0,
        web_dist: web_dist.path().to_path_buf(),
    };
    let app = build_router(&config).unwrap();
    let body = serde_json::json!({"density": "balanced", "seed": 3});
    let post = Request::builder()
        .method("POST")
        .uri("/api/lakes/generate")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(post).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed.get("rivers_cleared"), Some(&serde_json::json!(true)));

    let rivers_raw = std::fs::read_to_string(world.join("map/rivers.json")).unwrap();
    let cleared: mapkeeper_core::rivers::RiverCatalog =
        mapkeeper_core::rivers::RiverCatalog::from_json(&rivers_raw).unwrap();
    assert!(cleared.rivers.is_empty());
}
