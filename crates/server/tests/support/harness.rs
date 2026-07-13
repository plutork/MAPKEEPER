//! HTTP harness helpers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mapkeeper_core::build_state::manifest_toml_with_build;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::hydro::{filled_elevation_layer, OCEAN_ELEVATION};
use mapkeeper_core::layer::MapManifest;
use mapkeeper_server::{build_router, ServerConfig, WORLD_BASE_REVISION_HEADER, WORLD_ID_HEADER, WORLD_RESULT_REVISION_HEADER};
use tempfile::TempDir;
use tower::ServiceExt;

static PROJECTS_REGISTRY_LOCK: Mutex<()> = Mutex::new(());

/// Serialize tests that set process-wide `MAPKEEPER_FAILPOINT`.
pub fn failpoint_test_lock() -> std::sync::MutexGuard<'static, ()> {
    mapkeeper_server::failpoint_test_lock()
}

/// Serialize tests that read/write the shared projects registry file.
pub fn registry_test_lock() -> std::sync::MutexGuard<'static, ()> {
    PROJECTS_REGISTRY_LOCK.lock().unwrap()
}

pub struct Harness {
    pub app: Router,
    _web_dist: TempDir,
    revisions: std::sync::Mutex<HashMap<String, u64>>,
    active_revision: std::sync::Mutex<u64>,
}

fn manifest_revision_at(world_path: &Path) -> u64 {
    let raw = std::fs::read_to_string(world_path.join("map/manifest.json")).unwrap_or_default();
    MapManifest::from_json(&raw).map(|m| m.revision).unwrap_or(0)
}

fn world_id_from_path(world_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(world_path.join("mapkeeper.toml")).ok()?;
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(id) = trimmed.strip_prefix("id = ") {
            return Some(id.trim_matches('"').to_string());
        }
    }
    None
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
        let harness = Self {
            app,
            _web_dist: web_dist,
            revisions: std::sync::Mutex::new(HashMap::new()),
            active_revision: std::sync::Mutex::new(0),
        };
        if let Some(ref path) = config.world {
            harness.sync_revision_from_world(path);
        }
        harness
    }

    fn tracked_revision(&self, world_id: Option<&str>) -> u64 {
        if let Some(id) = world_id {
            return *self
                .revisions
                .lock()
                .unwrap()
                .get(id)
                .unwrap_or(&0);
        }
        *self.active_revision.lock().unwrap()
    }

    fn apply_result_revision(&self, world_id: Option<&str>, headers: &HeaderMap) {
        if let Some(value) = headers.get(WORLD_RESULT_REVISION_HEADER) {
            if let Ok(text) = value.to_str() {
                if let Ok(revision) = text.parse::<u64>() {
                    if let Some(id) = world_id {
                        self.revisions
                            .lock()
                            .unwrap()
                            .insert(id.to_string(), revision);
                    } else {
                        *self.active_revision.lock().unwrap() = revision;
                    }
                }
            }
        }
    }

    fn sync_revision_from_world(&self, world_path: &Path) {
        let revision = manifest_revision_at(world_path);
        *self.active_revision.lock().unwrap() = revision;
        if let Some(id) = world_id_from_path(world_path) {
            self.revisions.lock().unwrap().insert(id, revision);
        }
    }

    async fn send_scoped_with_revision_raw(
        &self,
        method: &str,
        uri: &str,
        body: Option<Vec<u8>>,
        world_id: Option<&str>,
        base_revision: Option<u64>,
        request_id: Option<&str>,
    ) -> (StatusCode, Vec<u8>, HeaderMap) {
        let effective_revision = base_revision.or_else(|| Some(self.tracked_revision(world_id)));
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(id) = world_id {
            builder = builder.header(WORLD_ID_HEADER, id);
        }
        if let Some(rev) = effective_revision {
            builder = builder.header(WORLD_BASE_REVISION_HEADER, rev.to_string());
        }
        if let Some(op_id) = request_id {
            builder = builder.header(mapkeeper_server::REQUEST_ID_HEADER, op_id);
        }
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
        let headers = resp.headers().clone();
        if status.is_success() {
            self.apply_result_revision(world_id, &headers);
        }
        let bytes = resp
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        (status, bytes, headers)
    }

    pub async fn send(
        &self,
        method: &str,
        uri: &str,
        body: Option<Vec<u8>>,
    ) -> (StatusCode, Vec<u8>) {
        self.send_scoped(method, uri, body, None).await
    }

    pub async fn send_scoped_with_revision(
        &self,
        method: &str,
        uri: &str,
        body: Option<Vec<u8>>,
        world_id: Option<&str>,
        base_revision: Option<u64>,
    ) -> (StatusCode, Vec<u8>) {
        let (status, bytes, _) = self
            .send_scoped_with_revision_raw(method, uri, body, world_id, base_revision, None)
            .await;
        (status, bytes)
    }

    pub async fn put_lakes_catalog_scoped_with_revision(
        &self,
        catalog: &mapkeeper_core::lakes::LakeCatalog,
        world_id: Option<&str>,
        base_revision: Option<u64>,
    ) -> StatusCode {
        self.put_lakes_catalog_scoped_with_revision_raw(catalog, world_id, base_revision)
            .await
            .0
    }

    pub async fn put_lakes_catalog_scoped_with_revision_raw(
        &self,
        catalog: &mapkeeper_core::lakes::LakeCatalog,
        world_id: Option<&str>,
        base_revision: Option<u64>,
    ) -> (StatusCode, Vec<u8>, HeaderMap) {
        let bytes = serde_json::to_vec(catalog).expect("catalog json");
        self.send_scoped_with_revision_raw("PUT", "/api/lakes", Some(bytes), world_id, base_revision, None)
            .await
    }

    pub async fn put_lakes_catalog_scoped_with_revision_raw_and_request_id(
        &self,
        catalog: &mapkeeper_core::lakes::LakeCatalog,
        world_id: Option<&str>,
        base_revision: Option<u64>,
        request_id: Option<&str>,
    ) -> (StatusCode, Vec<u8>, HeaderMap) {
        let bytes = serde_json::to_vec(catalog).expect("catalog json");
        self.send_scoped_with_revision_raw(
            "PUT",
            "/api/lakes",
            Some(bytes),
            world_id,
            base_revision,
            request_id,
        )
        .await
    }

    pub async fn put_layer_batch_with_revision(
        &self,
        layer_id: &str,
        updates: &[mapkeeper_core::layer::LayerCellWrite],
        base_revision: Option<u64>,
    ) -> StatusCode {
        let bytes = serde_json::to_vec(updates).expect("batch json");
        let (status, _) = self
            .send_scoped_with_revision(
                "PUT",
                &format!("/api/layers/{layer_id}/batch"),
                Some(bytes),
                None,
                base_revision,
            )
            .await;
        status
    }

    pub async fn send_scoped(
        &self,
        method: &str,
        uri: &str,
        body: Option<Vec<u8>>,
        world_id: Option<&str>,
    ) -> (StatusCode, Vec<u8>) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(id) = world_id {
            builder = builder.header(WORLD_ID_HEADER, id);
        }
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
        if status.is_success() {
            self.sync_revision_from_world(world_path);
        }
        status
    }

    pub async fn put_lakes_catalog(
        &self,
        catalog: &mapkeeper_core::lakes::LakeCatalog,
    ) -> StatusCode {
        self.put_lakes_catalog_scoped(catalog, None).await
    }

    pub async fn put_lakes_catalog_scoped(
        &self,
        catalog: &mapkeeper_core::lakes::LakeCatalog,
        world_id: Option<&str>,
    ) -> StatusCode {
        self.put_lakes_catalog_scoped_with_revision(catalog, world_id, None)
            .await
    }

    pub async fn put_layer_batch(
        &self,
        layer_id: &str,
        updates: &[mapkeeper_core::layer::LayerCellWrite],
    ) -> StatusCode {
        self.put_layer_batch_with_revision(layer_id, updates, None)
            .await
    }

    pub async fn put_build_bounds(&self, map_preset: &str) -> StatusCode {
        let body = serde_json::json!({ "map_preset": map_preset });
        let (status, _) = self
            .send_scoped_with_revision(
                "PUT",
                "/api/build/bounds",
                Some(body.to_string().into_bytes()),
                None,
                None,
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
