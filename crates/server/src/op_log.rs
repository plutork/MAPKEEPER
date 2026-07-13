//! Structured tracing for mutating HTTP operations (agent-reliability op-log).

use std::sync::{Arc, Mutex, Once};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use serde::Serialize;
use tracing::info;

use crate::world_revision::{parse_base_revision, WORLD_RESULT_REVISION_HEADER};
use crate::world_scope::WORLD_ID_HEADER;
use crate::world_transaction::CommitReport;

pub const REQUEST_ID_HEADER: &str = "X-Request-Id";

tokio::task_local! {
    static MUTATE_OP_SLOT: Arc<Mutex<MutateOpSlot>>;
}

#[derive(Debug, Clone, Default)]
struct MutateOpSlot {
    result_revision: Option<u64>,
    error_code: Option<String>,
    commit: Option<CommitReportSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommitReportSummary {
    pub txn_id: String,
    pub files_written: usize,
    pub files_deleted: usize,
    pub invalidations: Vec<String>,
}

impl From<&CommitReport> for CommitReportSummary {
    fn from(report: &CommitReport) -> Self {
        Self {
            txn_id: report.txn_id.clone(),
            files_written: report.files_written,
            files_deleted: report.files_deleted,
            invalidations: report.post_commit.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogMode {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutateOutcome {
    Success,
    Conflict,
    PreconditionRequired,
    ClientError,
    ServerError,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MutateOpEvent {
    pub op_id: String,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_revision: Option<u64>,
    pub status: u16,
    pub outcome: MutateOutcome,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitReportSummary>,
}

impl MutateOpEvent {
    fn emit(&self) {
        match log_mode_from_env() {
            LogMode::Json => {
                if let Ok(line) = serde_json::to_string(self) {
                    info!(target: "mapkeeper_server::mutate_op", "{line}");
                }
            }
            LogMode::Text => info!(target: "mapkeeper_server::mutate_op", "{}", self.as_text_line()),
        }
    }

    fn as_text_line(&self) -> String {
        let mut parts = vec![
            format!("op_id={}", self.op_id),
            format!("operation={}", self.operation),
            format!("outcome={}", outcome_label(self.outcome)),
            format!("status={}", self.status),
            format!("duration_ms={}", self.duration_ms),
        ];
        if let Some(world_id) = &self.world_id {
            parts.push(format!("world_id={world_id}"));
        }
        if let Some(base) = self.base_revision {
            parts.push(format!("base_revision={base}"));
        }
        if let Some(result) = self.result_revision {
            parts.push(format!("result_revision={result}"));
        }
        if let Some(code) = &self.error_code {
            parts.push(format!("error_code={code}"));
        }
        if let Some(commit) = &self.commit {
            parts.push(format!("txn_id={}", commit.txn_id));
            parts.push(format!("files_written={}", commit.files_written));
            parts.push(format!("files_deleted={}", commit.files_deleted));
            if !commit.invalidations.is_empty() {
                parts.push(format!("invalidations={}", commit.invalidations.join(",")));
            }
        }
        parts.join(" ")
    }
}

fn outcome_label(outcome: MutateOutcome) -> &'static str {
    match outcome {
        MutateOutcome::Success => "success",
        MutateOutcome::Conflict => "conflict",
        MutateOutcome::PreconditionRequired => "precondition_required",
        MutateOutcome::ClientError => "client_error",
        MutateOutcome::ServerError => "server_error",
    }
}

fn log_mode_from_env() -> LogMode {
    match std::env::var("MAPKEEPER_LOG")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => LogMode::Json,
        _ => LogMode::Text,
    }
}

pub fn init_tracing() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "mapkeeper_server=info".into());
        match log_mode_from_env() {
            LogMode::Json => {
                tracing_subscriber::fmt()
                    .json()
                    .flatten_event(true)
                    .with_target(true)
                    .with_env_filter(filter)
                    .init();
            }
            LogMode::Text => {
                tracing_subscriber::fmt()
                    .compact()
                    .with_target(false)
                    .with_env_filter(filter)
                    .init();
            }
        }
    });
}

fn is_mutating_api(method: &Method, path: &str) -> bool {
    if !path.starts_with("/api/") {
        return false;
    }
    matches!(
        *method,
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    )
}

pub(crate) fn operation_kind(method: &Method, path: &str) -> String {
    let verb = method.as_str().to_ascii_lowercase();
    let route = classify_route(path);
    format!("{verb}.{route}")
}

fn classify_route(path: &str) -> &'static str {
    match path {
        "/api/projects" => "projects",
        "/api/projects/open" => "projects.open",
        "/api/projects/forget" => "projects.forget",
        "/api/projects/delete" => "projects.delete",
        "/api/projects/close" => "projects.close",
        "/api/fixture-worlds/open" => "fixture_worlds.open",
        "/api/build" => "build.state",
        "/api/build/bounds" => "build.bounds",
        "/api/build/land-mask/generate" => "build.land_mask.generate",
        "/api/build/land-mask/cells" => "build.land_mask.cells",
        "/api/build/geology/generate" => "build.geology.generate",
        "/api/build/elevation/generate" => "build.elevation.generate",
        "/api/build/climate/generate" => "build.climate.generate",
        "/api/lakes" => "lakes",
        "/api/lakes/generate" => "lakes.generate",
        "/api/rivers" => "rivers",
        "/api/rivers/pin" => "rivers.pin",
        "/api/rivers/append" => "rivers.append",
        "/api/rivers/generate" => "rivers.generate",
        _ if path.starts_with("/api/cells/") && path.ends_with("/profile") => "cells.profile",
        _ if path.starts_with("/api/layers/") && path.ends_with("/batch") => "layers.batch",
        _ if path.starts_with("/api/layers/") && path.contains("/cells/") => "layers.cell",
        _ if path.starts_with("/api/rivers/") && path.ends_with("/detach") => "rivers.detach",
        _ if path.starts_with("/api/rivers/") && path.ends_with("/pop") => "rivers.pop",
        _ if path.starts_with("/api/rivers/") => "rivers.id",
        _ => "unknown",
    }
}

fn new_op_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn outcome_from(status: StatusCode, slot: &MutateOpSlot) -> MutateOutcome {
    if status.is_success() {
        return MutateOutcome::Success;
    }
    if let Some(code) = slot.error_code.as_deref() {
        return match code {
            "world_revision_mismatch" => MutateOutcome::Conflict,
            "base_revision_required" => MutateOutcome::PreconditionRequired,
            _ if status == StatusCode::CONFLICT => MutateOutcome::Conflict,
            _ if status == StatusCode::PRECONDITION_REQUIRED => MutateOutcome::PreconditionRequired,
            _ if status.is_client_error() => MutateOutcome::ClientError,
            _ => MutateOutcome::ServerError,
        };
    }
    match status {
        StatusCode::CONFLICT => MutateOutcome::Conflict,
        StatusCode::PRECONDITION_REQUIRED => MutateOutcome::PreconditionRequired,
        _ if status.is_client_error() => MutateOutcome::ClientError,
        _ => MutateOutcome::ServerError,
    }
}

pub(crate) fn note_commit_success(report: &CommitReport, result_revision: u64) {
    let _ = MUTATE_OP_SLOT.try_with(|slot| {
        let mut slot = slot.lock().expect("mutate op slot");
        slot.commit = Some(report.into());
        slot.result_revision = Some(result_revision);
    });
}

pub(crate) fn note_direct_mutate_success(result_revision: u64) {
    let _ = MUTATE_OP_SLOT.try_with(|slot| {
        slot.lock().expect("mutate op slot").result_revision = Some(result_revision);
    });
}

pub(crate) fn note_revision_error(err: &crate::world_revision::RevisionError) {
    let code = match err {
        crate::world_revision::RevisionError::Mismatch { .. } => "world_revision_mismatch",
        crate::world_revision::RevisionError::PreconditionRequired { .. } => {
            "base_revision_required"
        }
    };
    note_error_code(code);
}

pub(crate) fn note_error_code(code: &str) {
    let _ = MUTATE_OP_SLOT.try_with(|slot| {
        slot.lock().expect("mutate op slot").error_code = Some(code.to_string());
    });
}

pub(crate) fn note_op_error(_message: &str) {
    let _ = MUTATE_OP_SLOT.try_with(|slot| {
        let mut slot = slot.lock().expect("mutate op slot");
        if slot.error_code.is_none() {
            slot.error_code = Some("operation_failed".to_string());
        }
    });
}

pub async fn mutate_op_middleware(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    if !is_mutating_api(&method, &path) {
        return next.run(request).await;
    }

    let op_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(new_op_id);

    let world_id = request
        .headers()
        .get(WORLD_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let base_revision = parse_base_revision(request.headers(), None);
    let operation = operation_kind(&method, &path);
    let started = std::time::Instant::now();
    let slot = Arc::new(Mutex::new(MutateOpSlot::default()));
    let slot_for_request = slot.clone();

    let response = MUTATE_OP_SLOT
        .scope(slot_for_request, async move { next.run(request).await })
        .await;

    let status = response.status();
    let duration_ms = started.elapsed().as_millis() as u64;
    let header_result_revision = response
        .headers()
        .get(WORLD_RESULT_REVISION_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let slot_snap = slot.lock().expect("mutate op slot").clone();

    let event = MutateOpEvent {
        op_id: op_id.clone(),
        operation,
        world_id,
        base_revision,
        result_revision: slot_snap.result_revision.or(header_result_revision),
        status: status.as_u16(),
        outcome: outcome_from(status, &slot_snap),
        duration_ms,
        error_code: slot_snap.error_code,
        commit: slot_snap.commit,
    };
    event.emit();

    let mut response = response;
    if let Ok(value) = HeaderValue::from_str(&op_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_event_has_stable_field_names() {
        let event = MutateOpEvent {
            op_id: "req-1".to_string(),
            operation: "put.lakes".to_string(),
            world_id: Some("world-a".to_string()),
            base_revision: Some(2),
            result_revision: Some(3),
            status: 204,
            outcome: MutateOutcome::Success,
            duration_ms: 12,
            error_code: None,
            commit: Some(CommitReportSummary {
                txn_id: "txn-1".to_string(),
                files_written: 2,
                files_deleted: 0,
                invalidations: vec!["hydrology".to_string()],
            }),
        };
        let json = serde_json::to_string(&event).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = value.as_object().unwrap();
        for key in [
            "op_id",
            "operation",
            "world_id",
            "base_revision",
            "result_revision",
            "status",
            "outcome",
            "duration_ms",
            "commit",
        ] {
            assert!(obj.contains_key(key), "missing key {key}");
        }
        assert!(!obj.contains_key("error_code"));
        let commit = obj.get("commit").unwrap().as_object().unwrap();
        for key in ["txn_id", "files_written", "files_deleted", "invalidations"] {
            assert!(commit.contains_key(key), "missing commit.{key}");
        }
    }

    #[test]
    fn optional_revision_omitted_from_json() {
        let event = MutateOpEvent {
            op_id: "req-2".to_string(),
            operation: "post.projects.open".to_string(),
            world_id: None,
            base_revision: None,
            result_revision: None,
            status: 200,
            outcome: MutateOutcome::Success,
            duration_ms: 5,
            error_code: None,
            commit: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("base_revision"));
        assert!(!json.contains("result_revision"));
        assert!(!json.contains("world_id"));
        assert!(!json.contains("commit"));
    }

    #[test]
    fn text_line_excludes_absolute_paths_and_notes() {
        let event = MutateOpEvent {
            op_id: "req-3".to_string(),
            operation: "put.layers.batch".to_string(),
            world_id: Some("demo".to_string()),
            base_revision: Some(0),
            result_revision: Some(1),
            status: 204,
            outcome: MutateOutcome::Success,
            duration_ms: 8,
            error_code: None,
            commit: Some(CommitReportSummary {
                txn_id: "123-456".to_string(),
                files_written: 1,
                files_deleted: 0,
                invalidations: vec!["hydrology".to_string()],
            }),
        };
        let line = event.as_text_line();
        assert!(!line.contains(":\\"));
        assert!(!line.contains("notes"));
        assert!(!line.contains("C:/"));
        assert!(line.contains("world_id=demo"));
        assert!(line.contains("invalidations=hydrology"));
    }

    #[test]
    fn operation_kind_normalizes_layer_routes() {
        assert_eq!(
            operation_kind(&Method::PUT, "/api/layers/elevation/batch"),
            "put.layers.batch"
        );
        assert_eq!(
            operation_kind(&Method::PUT, "/api/layers/land_mask/cells/3/4"),
            "put.layers.cell"
        );
        assert_eq!(
            operation_kind(&Method::PUT, "/api/cells/1/2/profile"),
            "put.cells.profile"
        );
    }
}
