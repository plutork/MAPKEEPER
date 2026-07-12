//! Legacy manual river detach tributary API.

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
        "[world]\nid = \"river-detach-test\"\n",
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

async fn pin_tributary(app: &axum::Router) -> (u32, u32) {
    let stem = app
        .clone()
        .oneshot(
            Request::post("/api/rivers/pin")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"source_q":0,"source_r":0,"mouth_q":3,"mouth_r":0}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stem.status(), StatusCode::OK);
    let stem_id = 1;

    let trib = app
        .clone()
        .oneshot(
            Request::post("/api/rivers/pin")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"source_q":0,"source_r":1,"mouth_q":3,"mouth_r":0}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(trib.status(), StatusCode::OK);
    let body = trib.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let trib_id = json["river_id"].as_u64().unwrap() as u32;
    assert_eq!(json["rivers"][1]["parent"], stem_id);
    (stem_id, trib_id)
}

#[tokio::test]
async fn detach_tributary_truncates_and_resets_parent() {
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

    let (stem_id, trib_id) = pin_tributary(&app).await;

    let detach = app
        .clone()
        .oneshot(
            Request::post(format!("/api/rivers/{trib_id}/detach"))
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detach.status(), StatusCode::OK);
    let body = detach.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let trib = &json["rivers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|river| river["id"] == trib_id)
        .unwrap();
    assert_eq!(trib["parent"], trib_id);
    assert_eq!(trib["basin"], trib_id);
    let stem = json["rivers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|river| river["id"] == stem_id)
        .unwrap();
    assert!(stem["cells"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn detach_rejected_when_hydrology_snapshot_active() {
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

    let detach = app
        .oneshot(
            Request::post("/api/rivers/1/detach")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detach.status(), StatusCode::CONFLICT);
}
