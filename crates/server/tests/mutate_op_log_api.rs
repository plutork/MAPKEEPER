//! Mutating operation structured tracing — correlation id API tests.

mod support;

use axum::http::StatusCode;
use mapkeeper_server::REQUEST_ID_HEADER;
use support::harness::{lake_catalog_with_cell_marker, registry_test_lock, seed_world, Harness};
use tempfile::tempdir;

#[tokio::test]
async fn successful_mutate_returns_request_id_header() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("world");
    seed_world(&world, "op-log-ok", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let (status, _, headers) = harness
        .put_lakes_catalog_scoped_with_revision_raw(
            &lake_catalog_with_cell_marker(1),
            Some("op-log-ok"),
            Some(0),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let request_id = headers
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(!request_id.is_empty());
    assert!(uuid::Uuid::parse_str(request_id).is_ok());
}

#[tokio::test]
async fn client_request_id_is_propagated() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("world");
    seed_world(&world, "op-log-prop", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    let client_id = "client-correlation-42";
    let (status, _, headers) = harness
        .put_lakes_catalog_scoped_with_revision_raw_and_request_id(
            &lake_catalog_with_cell_marker(2),
            Some("op-log-prop"),
            Some(0),
            Some(client_id),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok()),
        Some(client_id)
    );
}

#[tokio::test]
async fn conflict_mutate_still_returns_request_id() {
    let _lock = registry_test_lock();
    let root = tempdir().unwrap();
    let world = root.path().join("world");
    seed_world(&world, "op-log-conflict", 14, 8);

    let harness = Harness::launcher();
    assert_eq!(harness.open_project(&world).await, StatusCode::OK);

    assert_eq!(
        harness
            .put_lakes_catalog_scoped_with_revision(
                &lake_catalog_with_cell_marker(1),
                Some("op-log-conflict"),
                Some(0),
            )
            .await,
        StatusCode::OK,
    );

    let (status, body, headers) = harness
        .put_lakes_catalog_scoped_with_revision_raw(
            &lake_catalog_with_cell_marker(99),
            Some("op-log-conflict"),
            Some(0),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(String::from_utf8_lossy(&body).contains("world_revision_mismatch"));
    let request_id = headers
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(!request_id.is_empty());
}
