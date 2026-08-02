//! Integration tests for `spatial` (N-031: tests outside implementation).

use super::*;
use axum::body::Body;
use http_body_util::BodyExt;
use crate::state::RecentCommittedStroke;
use tempfile::tempdir;
use tower::ServiceExt;

fn app_with_world(world_path: &Path, map_path: &Path) -> axum::Router {
    let state = Arc::new(ServerState::new(Some(crate::state::ActiveWorld {
        path: world_path.to_path_buf(),
        id: "t".into(),
        map_path: map_path.to_path_buf(),
        map_id: "main".into(),
    })));
    routes().with_state(state)
}

async fn json_request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = if bytes.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) }))
    };
    (status, value)
}

fn seed_world() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let map_path = crate::world_layout::write_world_skeleton(
        dir.path(),
        "t",
        mapkeeper_core::spatial::alpha_default_preset(),
    )
    .unwrap();
    ensure_spatial_state(&map_path).unwrap();
    (dir, map_path)
}

#[test]
fn ensure_writes_spatial_config_and_metric_grid() {
    let dir = tempdir().unwrap();
    let map_path = crate::world_layout::write_world_skeleton(
        dir.path(),
        "t",
        mapkeeper_core::spatial::alpha_default_preset(),
    )
    .unwrap();
    let state = ensure_spatial_state(&map_path).unwrap();
    assert_eq!(state.grid.neighbor_center_distance_m, 1000.0);
    assert!(state.revision >= 1);
    let again = ensure_spatial_state(&map_path).unwrap();
    assert_eq!(again.grid, state.grid);
}

#[test]
fn ensure_requires_map_toml_not_world_spatial() {
    let dir = tempdir().unwrap();
    let legacy = "# mapkeeper world workspace\n\n[world]\nid = \"old\"\nname = \"old\"\nversion = \"0.3.0\"\n";
    std::fs::write(dir.path().join("mapkeeper.toml"), legacy).unwrap();
    let err = ensure_spatial_state(dir.path()).unwrap_err().to_string();
    assert!(err.contains("missing map.toml"), "{err}");
}

#[tokio::test]
async fn stroke_oneshot_atomic_gesture() {
    let (dir, map_path) = seed_world();
    let app = app_with_world(dir.path(), &map_path);
    let (status, view) = json_request(
        app,
        "POST",
        "/api/spatial/stroke",
        serde_json::json!({
            "stroke_id": "s1",
            "base_revision": 1,
            "cells": [{"q": 0, "r": 0, "value": 4}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["state"]["field"]["cells"]["0,0"], 4);
    assert_eq!(view["state"]["revision"], 2);
    let raw = std::fs::read_to_string(map_path.join("spatial/state.json")).unwrap();
    assert!(raw.contains("\"0,0\": 4") || raw.contains("\"0,0\":4"));
    assert!(!raw.contains("\"0,0\": 2")); // no partial mid-values
}

#[tokio::test]
async fn multi_chunk_commit_is_one_revision() {
    let (dir, map_path) = seed_world();
    let app = app_with_world(dir.path(), &map_path);
    let (st, _) = json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/begin",
        serde_json::json!({ "stroke_id": "big", "base_revision": 1 }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/chunk",
        serde_json::json!({
            "stroke_id": "big", "chunk_id": "0",
            "cells": [{"q": 0, "r": 0, "value": 1}, {"q": 1, "r": 0, "value": 2}]
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // Disk unchanged mid-staging.
    let mid = SpatialState::from_json(
        &std::fs::read_to_string(map_path.join("spatial/state.json")).unwrap(),
    )
    .unwrap();
    assert!(mid.field.cells.is_empty());
    assert_eq!(mid.revision, 1);

    let (st, _) = json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/chunk",
        serde_json::json!({
            "stroke_id": "big", "chunk_id": "1",
            "cells": [{"q": 2, "r": 0, "value": 3}]
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, view) = json_request(
        app,
        "POST",
        "/api/spatial/stroke/commit",
        serde_json::json!({ "stroke_id": "big" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(view["state"]["revision"], 2);
    assert_eq!(view["state"]["field"]["cells"]["0,0"], 1);
    assert_eq!(view["state"]["field"]["cells"]["2,0"], 3);
}

#[tokio::test]
async fn abort_leaves_disk_untouched() {
    let (dir, map_path) = seed_world();
    let app = app_with_world(dir.path(), &map_path);
    json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/begin",
        serde_json::json!({ "stroke_id": "a", "base_revision": 1 }),
    )
    .await;
    json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/chunk",
        serde_json::json!({
            "stroke_id": "a", "chunk_id": "0",
            "cells": [{"q": 0, "r": 0, "value": 9}]
        }),
    )
    .await;
    let (st, _) = json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/abort",
        serde_json::json!({ "stroke_id": "a" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // Second abort safe.
    let (st, _) = json_request(
        app,
        "POST",
        "/api/spatial/stroke/abort",
        serde_json::json!({ "stroke_id": "a" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let state = SpatialState::from_json(
        &std::fs::read_to_string(map_path.join("spatial/state.json")).unwrap(),
    )
    .unwrap();
    assert!(state.field.cells.is_empty());
    assert_eq!(state.revision, 1);
}

#[tokio::test]
async fn duplicate_chunk_and_duplicate_commit() {
    let (dir, map_path) = seed_world();
    let app = app_with_world(dir.path(), &map_path);
    json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/begin",
        serde_json::json!({ "stroke_id": "d", "base_revision": 1 }),
    )
    .await;
    let chunk = serde_json::json!({
        "stroke_id": "d", "chunk_id": "x",
        "cells": [{"q": 0, "r": 0, "value": 5}]
    });
    assert_eq!(
        json_request(
            app.clone(),
            "POST",
            "/api/spatial/stroke/chunk",
            chunk.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        json_request(app.clone(), "POST", "/api/spatial/stroke/chunk", chunk)
            .await
            .0,
        StatusCode::OK
    );
    let (st, v1) = json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/commit",
        serde_json::json!({ "stroke_id": "d" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let rev = v1["state"]["revision"].as_u64().unwrap();
    let (st, v2) = json_request(
        app,
        "POST",
        "/api/spatial/stroke/commit",
        serde_json::json!({ "stroke_id": "d" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v2["state"]["revision"], rev);
    assert_eq!(v2["state"]["field"]["cells"]["0,0"], 5);
}

#[tokio::test]
async fn stale_base_revision_conflicts() {
    let (dir, map_path) = seed_world();
    let app = app_with_world(dir.path(), &map_path);
    let (st, _) = json_request(
        app,
        "POST",
        "/api/spatial/stroke",
        serde_json::json!({
            "stroke_id": "stale",
            "base_revision": 0,
            "cells": [{"q": 0, "r": 0, "value": 1}]
        }),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn cells_outside_grid_rejected() {
    let (dir, map_path) = seed_world();
    let app = app_with_world(dir.path(), &map_path);
    let (st, _) = json_request(
        app,
        "POST",
        "/api/spatial/stroke",
        serde_json::json!({
            "stroke_id": "out",
            "base_revision": 1,
            "cells": [{"q": 9999, "r": 9999, "value": 1}]
        }),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let state = SpatialState::from_json(
        &std::fs::read_to_string(map_path.join("spatial/state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state.revision, 1);
    assert!(state.field.cells.is_empty());
}

#[tokio::test]
async fn failed_write_does_not_register_committed() {
    crate::atomic_io::clear_failpoint();
    let (dir, map_path) = seed_world();
    let server = Arc::new(ServerState::new(Some(crate::state::ActiveWorld {
        path: dir.path().to_path_buf(),
        id: "t".into(),
        map_path: map_path.clone(),
        map_id: "main".into(),
    })));
    let app = routes().with_state(server.clone());
    crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::FinalRename);
    let (st, _) = json_request(
        app,
        "POST",
        "/api/spatial/stroke",
        serde_json::json!({
            "stroke_id": "fw",
            "base_revision": 1,
            "cells": [{"q": 0, "r": 0, "value": 3}]
        }),
    )
    .await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!server
        .recent_committed_strokes
        .lock()
        .unwrap()
        .contains_key("fw"));
    crate::atomic_io::clear_failpoint();
}

#[test]
fn recent_committed_strokes_ttl_purge() {
    let server = ServerState::new(None);
    {
        let mut map = server.recent_committed_strokes.lock().unwrap();
        map.insert(
            "old".into(),
            RecentCommittedStroke {
                world_key: "w".into(),
                recorded_at: Instant::now()
                    - crate::state::COMMITTED_STROKE_TTL
                    - std::time::Duration::from_secs(1),
            },
        );
        map.insert(
            "fresh".into(),
            RecentCommittedStroke {
                world_key: "w".into(),
                recorded_at: Instant::now(),
            },
        );
    }
    server.purge_stale_strokes();
    let map = server.recent_committed_strokes.lock().unwrap();
    assert!(!map.contains_key("old"));
    assert!(map.contains_key("fresh"));
}

#[tokio::test]
async fn failed_before_commit_no_partial_on_disk() {
    let (dir, map_path) = seed_world();
    let app = app_with_world(dir.path(), &map_path);
    json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/begin",
        serde_json::json!({ "stroke_id": "fail", "base_revision": 1 }),
    )
    .await;
    json_request(
        app.clone(),
        "POST",
        "/api/spatial/stroke/chunk",
        serde_json::json!({
            "stroke_id": "fail", "chunk_id": "0",
            "cells": [{"q": 0, "r": 0, "value": 7}]
        }),
    )
    .await;
    // Simulate "failure before commit" by aborting.
    json_request(
        app,
        "POST",
        "/api/spatial/stroke/abort",
        serde_json::json!({ "stroke_id": "fail" }),
    )
    .await;
    let state = SpatialState::from_json(
        &std::fs::read_to_string(map_path.join("spatial/state.json")).unwrap(),
    )
    .unwrap();
    assert!(!state.field.cells.contains_key("0,0"));
}

#[test]
fn corrupt_state_does_not_rewrite_defaults() {
    let (_dir, map_path) = seed_world();
    let path = map_path.join("spatial/state.json");
    let good = ensure_spatial_state(&map_path).unwrap();
    write_spatial_state(&map_path, &good).unwrap(); // creates *.bak
    std::fs::write(&path, "{truncated").unwrap();
    let err = ensure_spatial_state(&map_path).unwrap_err().to_string();
    assert!(err.contains("corrupt_spatial"));
    assert!(err.contains("bak_available=true"));
    assert_eq!(std::fs::read(&path).unwrap(), b"{truncated");
    assert!(crate::atomic_io::bak_path(&path).is_file());
}

#[test]
fn restart_missing_primary_valid_bak_is_recovery_not_default() {
    let (_dir, map_path) = seed_world();
    let path = map_path.join("spatial/state.json");
    let good = ensure_spatial_state(&map_path).unwrap();
    write_spatial_state(&map_path, &good).unwrap();
    let bak = crate::atomic_io::bak_path(&path);
    let bak_bytes = std::fs::read(&bak).unwrap();
    std::fs::remove_file(&path).unwrap();
    // Simulated restart: open with missing primary + valid bak.
    let err = ensure_spatial_state(&map_path).unwrap_err().to_string();
    assert!(err.contains("interrupted_write"));
    assert!(err.contains("bak_available=true"));
    assert!(!path.is_file());
    assert_eq!(std::fs::read(&bak).unwrap(), bak_bytes);
    // Explicit restore recovers author state; never silent default.
    let restored = restore_spatial_from_bak(&map_path).unwrap();
    assert!(restored.revision >= 1);
    assert!(path.is_file());
}

#[test]
fn restart_missing_primary_invalid_bak_never_defaults() {
    let (_dir, map_path) = seed_world();
    let path = map_path.join("spatial/state.json");
    let bak = crate::atomic_io::bak_path(&path);
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&bak, "{bad-bak").unwrap();
    let err = ensure_spatial_state(&map_path).unwrap_err().to_string();
    assert!(err.contains("interrupted_write"));
    assert!(err.contains("bak_available=false"));
    assert!(!path.is_file());
    assert_eq!(std::fs::read(&bak).unwrap(), b"{bad-bak");
}

#[test]
fn invalid_primary_valid_bak_never_defaults() {
    let (_dir, map_path) = seed_world();
    let path = map_path.join("spatial/state.json");
    let good = ensure_spatial_state(&map_path).unwrap();
    write_spatial_state(&map_path, &good).unwrap();
    let bak_bytes = std::fs::read(crate::atomic_io::bak_path(&path)).unwrap();
    std::fs::write(&path, "{truncated").unwrap();
    let err = ensure_spatial_state(&map_path).unwrap_err().to_string();
    assert!(err.contains("corrupt_spatial"));
    assert!(err.contains("bak_available=true"));
    assert_eq!(std::fs::read(&path).unwrap(), b"{truncated");
    assert_eq!(
        std::fs::read(crate::atomic_io::bak_path(&path)).unwrap(),
        bak_bytes
    );
}

#[test]
fn failpoint_crash_after_bak_survives_restart_as_recovery() {
    crate::atomic_io::clear_failpoint();
    let (_dir, map_path) = seed_world();
    let path = map_path.join("spatial/state.json");
    let good = ensure_spatial_state(&map_path).unwrap();
    let good_json = good.to_json_pretty().unwrap();
    write_spatial_state(&map_path, &good).unwrap();
    // Force a second replace so bak holds last-good author bytes.
    let mut next = good.clone();
    next.revision = good.revision + 1;
    crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::AfterPrimaryToBak);
    assert!(write_spatial_state(&map_path, &next).is_err());
    assert!(!path.is_file());
    let bak = crate::atomic_io::bak_path(&path);
    assert!(SpatialState::from_json(&std::fs::read_to_string(&bak).unwrap()).is_ok());
    assert!(!crate::atomic_io::leftover_temp_paths(&path).is_empty());
    // Simulated process restart.
    let err = ensure_spatial_state(&map_path).unwrap_err().to_string();
    assert!(err.contains("interrupted_write"));
    assert!(err.contains("bak_available=true"));
    assert!(!path.is_file());
    let restored = restore_spatial_from_bak(&map_path).unwrap();
    assert!(restored.revision >= 1);
    let _ = good_json;
}

#[test]
fn failpoint_final_rename_restores_primary_before_error() {
    crate::atomic_io::clear_failpoint();
    let (_dir, map_path) = seed_world();
    let path = map_path.join("spatial/state.json");
    let good = ensure_spatial_state(&map_path).unwrap();
    write_spatial_state(&map_path, &good).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut next = good.clone();
    next.revision = good.revision + 1;
    crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::FinalRename);
    assert!(write_spatial_state(&map_path, &next).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Restart sees healthy primary (restored), not defaults.
    let loaded = ensure_spatial_state(&map_path).unwrap();
    assert_eq!(loaded.revision, good.revision);
}

#[test]
fn restore_bak_quarantines_corrupt_and_recovers() {
    let (_dir, map_path) = seed_world();
    let path = map_path.join("spatial/state.json");
    // Force a known bak by rewriting once more.
    let good = ensure_spatial_state(&map_path).unwrap();
    write_spatial_state(&map_path, &good).unwrap();
    std::fs::write(&path, "{truncated").unwrap();
    let restored = restore_spatial_from_bak(&map_path).unwrap();
    assert!(restored.revision >= 1);
    assert!(SpatialState::from_json(&std::fs::read_to_string(&path).unwrap()).is_ok());
    let diag_count = std::fs::read_dir(map_path.join("spatial"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .ok()
                .and_then(|f| f.file_name().into_string().ok())
                .is_some_and(|n| n.starts_with("state.json.corrupt-"))
        })
        .count();
    assert!(diag_count >= 1);
}

#[test]
fn invalid_bak_restore_errors() {
    let (_dir, map_path) = seed_world();
    let path = map_path.join("spatial/state.json");
    let bak = crate::atomic_io::bak_path(&path);
    std::fs::write(&path, "{truncated").unwrap();
    std::fs::write(&bak, "{also-bad").unwrap();
    let err = restore_spatial_from_bak(&map_path)
        .unwrap_err()
        .to_string();
    assert!(err.contains("corrupt_spatial"));
    assert!(err.contains("invalid bak") || err.contains("no bak"));
}

#[test]
fn missing_map_toml_with_bak_is_recovery_not_rewrite() {
    let (_dir, map_path) = seed_world();
    let manifest = map_path.join("map.toml");
    let bak = crate::atomic_io::bak_path(&manifest);
    std::fs::copy(&manifest, &bak).unwrap();
    let bak_bytes = std::fs::read(&bak).unwrap();
    std::fs::remove_file(&manifest).unwrap();
    let err = ensure_spatial_state(&map_path).unwrap_err().to_string();
    assert!(err.contains("corrupt_manifest"));
    assert!(err.contains("interrupted_write"));
    assert!(err.contains("bak_available=true"));
    assert!(!manifest.is_file());
    assert_eq!(std::fs::read(&bak).unwrap(), bak_bytes);
}

#[test]
fn map_manifest_preset_mismatch_rejected() {
    let dir = tempdir().unwrap();
    let map_path = crate::world_layout::write_world_skeleton(
        dir.path(),
        "t",
        mapkeeper_core::spatial::alpha_default_preset(),
    )
    .unwrap();
    let bad = r#"# mapkeeper map

[map]
id = "main"
name = "main"
version = "0.4.0"

[spatial]
preset_id = "wide_2000"
grid_id = "primary"
width_m = 100.0
height_m = 100.0
cols = 1
rows = 1
neighbor_center_distance_m = 1000.0
origin_x_m = 0.0
origin_y_m = 0.0
orientation = "pointy-top"
"#;
    std::fs::write(map_path.join("map.toml"), bad).unwrap();
    let err = ensure_spatial_state(&map_path).unwrap_err().to_string();
    assert!(err.contains("manifest/preset mismatch"));
}
