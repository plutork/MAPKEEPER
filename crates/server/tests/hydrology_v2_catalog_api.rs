//! Generated river catalog is a projection of the active Hydrology v2 snapshot.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::hydro::SEA_LEVEL;
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue, MapManifest};
use mapkeeper_core::rivers::RiverCatalog;
use mapkeeper_server::{build_router, ServerConfig, WORLD_BASE_REVISION_HEADER};
use tempfile::tempdir;
use tower::ServiceExt;

fn manifest_revision(world: &std::path::Path) -> u64 {
    let raw = std::fs::read_to_string(world.join("map/manifest.json")).unwrap();
    MapManifest::from_json(&raw).unwrap().revision
}

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
    assert_eq!(response["precip_input_state"], "missing");
    assert_eq!(response["precip_source"], "uniform_fallback");
    assert_eq!(response["deterministic"], true);
    assert_eq!(response["read_only"], true);
    assert!(response["channel_segment_count"].as_u64().is_some_and(|n| n > 0));
}

#[tokio::test]
async fn get_rivers_read_only_when_snapshot_active() {
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

    let response = app
        .oneshot(Request::get("/api/rivers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["read_only"], true);
    assert!(json["channel_segment_count"].as_u64().is_some_and(|n| n > 0));
}

#[tokio::test]
async fn get_rivers_writable_without_snapshot() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world);
    std::fs::write(
        world.join("map/rivers.json"),
        RiverCatalog::default().to_json_pretty().unwrap(),
    )
    .unwrap();
    let web_dist = tempdir().unwrap();
    let app = build_router(&ServerConfig {
        world: Some(world),
        port: 0,
        web_dist: web_dist.path().to_path_buf(),
    })
    .unwrap();

    let response = app
        .oneshot(Request::get("/api/rivers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["read_only"], false);
    assert!(json.get("channel_segment_count").is_none());
}

#[tokio::test]
async fn generate_rivers_reports_valid_dry_precip_state() {
    let dir = tempdir().unwrap();
    let world = dir.path().to_path_buf();
    seed_world(&world);
    let bounds = MapBounds::new(14, 8);
    let mut precip = DenseLayer::new_integer("precipitation", bounds.len());
    for cell in 0..bounds.len() {
        precip.set(cell, DenseState::Value(LayerValue::Int(4)));
    }
    let precip_path = world.join("map/layers/precipitation.json");
    std::fs::write(precip_path, precip.to_json_pretty().unwrap()).unwrap();

    let web_dist = tempdir().unwrap();
    let app = build_router(&ServerConfig {
        world: Some(world),
        port: 0,
        web_dist: web_dist.path().to_path_buf(),
    })
    .unwrap();

    let response = app
        .oneshot(
            Request::post("/api/rivers/generate")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"river_density":"balanced"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["precip_input_state"], "valid");
    assert_eq!(json["precip_source"], "climate");
    assert_eq!(json["deterministic"], true);
}

#[tokio::test]
async fn double_river_generate_produces_identical_snapshot() {
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

    let post_generate = |nonce: u32| {
        let app = app.clone();
        let world = world.clone();
        async move {
            let revision = manifest_revision(&world);
            app.oneshot(
                Request::post("/api/rivers/generate")
                    .header("content-type", "application/json")
                    .header(WORLD_BASE_REVISION_HEADER, revision.to_string())
                    .body(Body::from(format!(
                        r#"{{"river_density":"balanced","regenerate_nonce":{nonce}}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    assert_eq!(post_generate(0).await.status(), StatusCode::OK);
    let first = std::fs::read_to_string(world.join("map/hydrology-v2.json")).unwrap();
    assert_eq!(post_generate(7).await.status(), StatusCode::OK);
    let second = std::fs::read_to_string(world.join("map/hydrology-v2.json")).unwrap();
    assert_eq!(first, second);
}

#[tokio::test]
async fn regenerate_nonce_is_ignored_in_generate_response() {
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
            Request::post("/api/rivers/generate")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"river_density":"balanced","regenerate_nonce":42}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["regenerate_nonce_ignored"], true);
    assert_eq!(json["deterministic"], true);
}

#[tokio::test]
async fn named_river_id_differs_from_physical_segment_id() {
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

    let bootstrap = app
        .clone()
        .oneshot(
            Request::post("/api/rivers/generate")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"river_density":"balanced"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), StatusCode::OK);
    let bootstrap_body = bootstrap.into_body().collect().await.unwrap().to_bytes();
    let bootstrap_json: serde_json::Value = serde_json::from_slice(&bootstrap_body).unwrap();
    let segment_cells = bootstrap_json["rivers"][0]["cells"]
        .as_array()
        .expect("projected river cells")
        .iter()
        .map(|v| v.as_u64().unwrap() as usize)
        .collect::<Vec<_>>();

    std::fs::remove_file(world.join("map/hydrology-v2.json")).unwrap();
    std::fs::remove_file(world.join("map/named-rivers.json")).ok();
    std::fs::write(
        world.join("map/rivers.json"),
        serde_json::json!({
            "schema_version": 1,
            "next_id": 2,
            "rivers": [{
                "id": 99,
                "cells": segment_cells,
                "source": segment_cells[0],
                "mouth": segment_cells[segment_cells.len() - 1],
                "parent": 99,
                "basin": 99,
                "name": "Silver"
            }]
        })
        .to_string(),
    )
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/rivers/generate")
                .header("content-type", "application/json")
                .header(
                    WORLD_BASE_REVISION_HEADER,
                    manifest_revision(&world).to_string(),
                )
                .body(Body::from(r#"{"river_density":"balanced"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["compatibility_projection"], true);
    assert_eq!(json["named_river_count"], 1);
    let named_id = json["named_rivers"][0]["id"].as_u64().unwrap();
    let segment_id = json["named_rivers"][0]["segment_ids"][0].as_u64().unwrap();
    assert_ne!(named_id, segment_id);
    assert_eq!(json["named_rivers"][0]["name"], "Silver");
    assert!(world.join("map/named-rivers.json").is_file());

    let regen = app
        .clone()
        .oneshot(
            Request::post("/api/rivers/generate")
                .header("content-type", "application/json")
                .header(
                    WORLD_BASE_REVISION_HEADER,
                    manifest_revision(&world).to_string(),
                )
                .body(Body::from(r#"{"river_density":"balanced"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(regen.status(), StatusCode::OK);
    let regen_body = regen.into_body().collect().await.unwrap().to_bytes();
    let regen_json: serde_json::Value = serde_json::from_slice(&regen_body).unwrap();
    assert_eq!(regen_json["named_rivers"][0]["id"], named_id);
    assert_eq!(regen_json["named_rivers"][0]["name"], "Silver");
}
