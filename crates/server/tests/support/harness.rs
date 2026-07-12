//! HTTP harness helpers.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mapkeeper_core::build_state::manifest_toml_with_build;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::hydro::{filled_elevation_layer, OCEAN_ELEVATION};
use mapkeeper_core::layer::MapManifest;
use mapkeeper_server::{build_router, ServerConfig};
use tempfile::TempDir;
use tower::ServiceExt;

pub struct Harness {
    pub app: Router,
    _web_dist: TempDir,
}

impl Harness {
    pub fn launcher() -> Self {
        Self::with_active_world(None)
    }

    pub fn with_active_world(world: Option<PathBuf>) -> Self {
        let web_dist = tempfile::tempdir().expect("web_dist tempdir");
        let config = ServerConfig {
            world,
            port: 0,
            web_dist: web_dist.path().to_path_buf(),
        };
        let app = build_router(&config).expect("build_router");
        Self {
            app,
            _web_dist: web_dist,
        }
    }

    pub async fn send(
        &self,
        method: &str,
        uri: &str,
        body: Option<Vec<u8>>,
    ) -> (StatusCode, Vec<u8>) {
        let mut builder = Request::builder().method(method).uri(uri);
        let req_body = match body {
            Some(bytes) => {
                builder = builder.header("content-type", "application/json");
                Body::from(bytes)
            }
            None => Body::empty(),
        };
        let req = builder.body(req_body).expect("request");
        let resp = self.app.clone().oneshot(req).await.expect("response");
        let status = resp.status();
        let bytes = resp
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        (status, bytes)
    }

    pub async fn open_project(&self, world_path: &Path) -> StatusCode {
        let body = serde_json::json!({ "path": world_path.display().to_string() });
        let (status, _) = self
            .send(
                "POST",
                "/api/projects/open",
                Some(body.to_string().into_bytes()),
            )
            .await;
        status
    }

    pub async fn put_lakes_catalog(
        &self,
        catalog: &mapkeeper_core::lakes::LakeCatalog,
    ) -> StatusCode {
        let bytes = serde_json::to_vec(catalog).expect("catalog json");
        let (status, _) = self.send("PUT", "/api/lakes", Some(bytes)).await;
        status
    }

    pub async fn put_layer_batch(
        &self,
        layer_id: &str,
        updates: &[mapkeeper_core::layer::LayerCellWrite],
    ) -> StatusCode {
        let bytes = serde_json::to_vec(updates).expect("batch json");
        let (status, _) = self
            .send(
                "PUT",
                &format!("/api/layers/{layer_id}/batch"),
                Some(bytes),
            )
            .await;
        status
    }

    pub async fn put_build_bounds(&self, map_preset: &str) -> StatusCode {
        let body = serde_json::json!({ "map_preset": map_preset });
        let (status, _) = self
            .send(
                "PUT",
                "/api/build/bounds",
                Some(body.to_string().into_bytes()),
            )
            .await;
        status
    }
}

pub fn seed_world(world: &Path, world_id: &str, width: i32, height: i32) -> MapBounds {
    std::fs::create_dir_all(world.join("map/layers")).expect("map dirs");
    std::fs::write(
        world.join("mapkeeper.toml"),
        manifest_toml_with_build(world_id, false),
    )
    .expect("mapkeeper.toml");
    let manifest = MapManifest::default_v0(width, height);
    std::fs::write(
        world.join("map/manifest.json"),
        manifest.to_json_pretty().expect("manifest json"),
    )
    .expect("manifest write");
    let bounds = MapBounds::new(width, height);
    let ocean = filled_elevation_layer(&bounds, OCEAN_ELEVATION);
    std::fs::write(
        world.join("map/layers/elevation.json"),
        ocean.to_json_pretty().expect("elevation json"),
    )
    .expect("elevation write");
    bounds
}

pub fn lake_catalog_with_cell_marker(cell: u32) -> mapkeeper_core::lakes::LakeCatalog {
    mapkeeper_core::lakes::LakeCatalog {
        schema_version: 1,
        next_id: 2,
        lakes: vec![mapkeeper_core::lakes::Lake {
            id: 1,
            cells: vec![cell as usize],
            outlet_cell: None,
            endorheic: false,
            name: None,
        }],
    }
}

pub fn read_lakes_json(world: &Path) -> mapkeeper_core::lakes::LakeCatalog {
    let raw = std::fs::read_to_string(world.join("map/lakes.json")).expect("lakes.json");
    mapkeeper_core::lakes::LakeCatalog::from_json(&raw).expect("lakes parse")
}

pub fn elevation_int_at(world: &Path, q: i32, r: i32) -> i32 {
    let bounds = MapBounds::new(14, 8);
    let raw = std::fs::read_to_string(world.join("map/layers/elevation.json")).expect("elevation");
    let layer = mapkeeper_core::layer::DenseLayer::read_or_empty(
        Some(&raw),
        mapkeeper_core::layer::ELEVATION_LAYER_ID,
        mapkeeper_core::layer::ValueType::Integer,
        &bounds,
    );
    let index = bounds
        .index_of(mapkeeper_core::hex::Axial::new(q, r))
        .expect("in bounds");
    layer.int_or(index, 0)
}

pub fn layer_cell_write(q: i32, r: i32, value: i32) -> mapkeeper_core::layer::LayerCellWrite {
    mapkeeper_core::layer::LayerCellWrite {
        q,
        r,
        state: mapkeeper_core::layer::WireCellState::Value {
            value: serde_json::json!(value),
        },
    }
}
