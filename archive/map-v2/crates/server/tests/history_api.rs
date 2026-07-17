//! History API integration tests (D-107 tracks A–C).

mod support;

use std::fs;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mapkeeper_core::build_state::manifest_toml_with_build;
use mapkeeper_core::history::{read_history_manifest, BASELINE_STATE_ID, CANONICAL_DOMAIN_REF, DOMAIN_LAND};
use support::harness::{registry_test_lock, seed_world, Harness};
use tower::ServiceExt;
use tempfile::tempdir;

#[tokio::test]
async fn unlock_history_creates_baseline_without_domain_bundles() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("hist-world");
    seed_world(&world, "hist-world", 8, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let resp = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/unlock")
                .header("X-World-Id", "hist-world")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let manifest = read_history_manifest(&world);
    assert!(manifest.enabled);
    assert_eq!(manifest.selected_state_id, BASELINE_STATE_ID);
    assert_eq!(manifest.domain_bundles.len(), 0);
    assert_eq!(
        manifest.states[0].domain_refs.get(DOMAIN_LAND).map(String::as_str),
        Some(CANONICAL_DOMAIN_REF)
    );
}

#[tokio::test]
async fn unlock_rejected_while_build_draft_on_disk() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("draft-world");
    seed_world(&world, "draft-world", 8, 8);
    std::fs::write(
        world.join("mapkeeper.toml"),
        manifest_toml_with_build("draft-world", true),
    )
    .unwrap();

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let resp = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/unlock")
                .header("X-World-Id", "draft-world")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn lore_event_without_new_state() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("evt-world");
    seed_world(&world, "evt-world", 8, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let unlock = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/unlock")
                .header("X-World-Id", "evt-world")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unlock.status(), StatusCode::OK);

    let body = serde_json::json!({
        "time_key": 112,
        "display_date": "0112",
        "name": "Founding",
        "description": "Lore only",
        "anchor_state_id": BASELINE_STATE_ID,
    });
    let resp = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/events")
                .header("X-World-Id", "evt-world")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let manifest = read_history_manifest(&world);
    assert_eq!(manifest.states.len(), 1);
    assert_eq!(manifest.events.len(), 1);
    assert!(manifest.events[0].result_state_id.is_none());
}

#[tokio::test]
async fn cataclysm_creates_event_changeset_and_state() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("cat-world");
    seed_world(&world, "cat-world", 8, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let _ = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/unlock")
                .header("X-World-Id", "cat-world")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = serde_json::json!({
        "time_key": 427,
        "display_date": "0427",
        "event_name": "The Sundering",
        "description": "Cataclysm lore",
        "result_state_name": "After the Sundering",
        "based_on": BASELINE_STATE_ID,
        "changed_domains": ["land"],
        "notes": "land reshaped",
    });
    let resp = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/cataclysm")
                .header("X-World-Id", "cat-world")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let manifest = read_history_manifest(&world);
    assert_eq!(manifest.states.len(), 2);
    assert_eq!(manifest.events.len(), 1);
    assert_eq!(manifest.change_sets.len(), 1);
    let evt = &manifest.events[0];
    assert_eq!(evt.change_set_id.as_deref(), Some(manifest.change_sets[0].id.as_str()));
    assert!(evt.result_state_id.is_some());
}

#[tokio::test]
async fn divergence_ack_via_api() {
    use mapkeeper_core::history::{
        add_world_state, fork_domain_bundle, write_history_manifest, CreateStateInput, DOMAIN_LAND,
    };

    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("div-world");
    seed_world(&world, "div-world", 8, 8);
    fs::create_dir_all(world.join("map/layers")).unwrap();
    fs::write(world.join("map/layers/land_mask.json"), "{}").unwrap();
    fs::write(world.join("map/layers/elevation.json"), "{}").unwrap();

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let _ = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/unlock")
                .header("X-World-Id", "div-world")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let mut manifest = read_history_manifest(&world);
    let later = add_world_state(
        &mut manifest,
        &CreateStateInput {
            time_key: 100,
            display_date: "0100".to_string(),
            name: "Later".to_string(),
            based_on: BASELINE_STATE_ID.to_string(),
            direction: "later".to_string(),
        },
    )
    .unwrap();
    write_history_manifest(&world, &manifest).unwrap();
    fork_domain_bundle(&world, &mut manifest, BASELINE_STATE_ID, DOMAIN_LAND).unwrap();
    write_history_manifest(&world, &manifest).unwrap();

    let ack = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/history/states/{}/divergence/ack", later.id))
                .header("X-World-Id", "div-world")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"domains":["land"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ack.status(), StatusCode::OK);

    let after_ack = read_history_manifest(&world);
    let later_state = after_ack.state_by_id(&later.id).unwrap();
    assert!(later_state.history_divergence.is_empty());
    assert_eq!(
        later_state.domain_refs.get(DOMAIN_LAND).map(String::as_str),
        Some(CANONICAL_DOMAIN_REF)
    );
}

#[tokio::test]
async fn divergence_rebase_via_api() {
    use mapkeeper_core::history::{
        add_world_state, fork_domain_bundle, write_history_manifest, CreateStateInput, DOMAIN_LAND,
    };

    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("reb-world");
    seed_world(&world, "reb-world", 8, 8);
    fs::create_dir_all(world.join("map/layers")).unwrap();
    fs::write(world.join("map/layers/land_mask.json"), "{}").unwrap();
    fs::write(world.join("map/layers/elevation.json"), "{}").unwrap();

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);
    let _ = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/unlock")
                .header("X-World-Id", "reb-world")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let mut manifest = read_history_manifest(&world);
    let later = add_world_state(
        &mut manifest,
        &CreateStateInput {
            time_key: 100,
            display_date: "0100".to_string(),
            name: "Later".to_string(),
            based_on: BASELINE_STATE_ID.to_string(),
            direction: "later".to_string(),
        },
    )
    .unwrap();
    write_history_manifest(&world, &manifest).unwrap();
    fork_domain_bundle(&world, &mut manifest, BASELINE_STATE_ID, DOMAIN_LAND).unwrap();
    write_history_manifest(&world, &manifest).unwrap();

    let rebase = harness
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/history/states/{}/rebase", later.id))
                .header("X-World-Id", "reb-world")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"domain":"land"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rebase.status(), StatusCode::OK);

    let after_rebase = read_history_manifest(&world);
    let later_state = after_rebase.state_by_id(&later.id).unwrap();
    assert!(later_state.history_divergence.is_empty());
    assert_ne!(
        later_state.domain_refs.get(DOMAIN_LAND).map(String::as_str),
        Some(CANONICAL_DOMAIN_REF)
    );
}
