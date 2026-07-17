//! History timeline HTTP API (D-107 tracks A–C).

use std::sync::Arc;

use axum::extract::{Path as AxPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use mapkeeper_core::build_state::{is_draft, read_build};
use mapkeeper_core::history::{
    ack_divergence, add_lore_event, add_world_state, build_divergence_review, create_cataclysm,
    delete_world_state, read_history_manifest, rebase_domain, state_has_descendants, unlock_history,
    write_history_manifest, CreateCataclysmInput, CreateEventInput, CreateStateInput,
    DivergenceStateReview, DomainRefSummary, HistoricalEventRecord, HistoryManifest,
    WorldStateRecord,
};
use serde::{Deserialize, Serialize};

use crate::state::ServerState;
use crate::world_lock;
use crate::world_scope::{self, ScopeMode};

#[derive(Serialize)]
struct HistoryResponse {
    enabled: bool,
    unlock_available: bool,
    selected_state_id: String,
    states: Vec<WorldStateSummary>,
    events: Vec<EventSummary>,
    history_divergence: Vec<String>,
    divergence_review: Vec<DivergenceReviewWire>,
    selected_can_delete: bool,
}

#[derive(Serialize, Clone)]
struct DivergenceReviewWire {
    state_id: String,
    display_date: String,
    name: String,
    domains: Vec<DomainRefWire>,
}

#[derive(Serialize, Clone)]
struct DomainRefWire {
    domain: String,
    local_ref: String,
    fork_source_state_id: Option<String>,
    fork_source_state_name: Option<String>,
    message: String,
}

#[derive(Serialize, Clone)]
struct WorldStateSummary {
    id: String,
    time_key: i64,
    display_date: String,
    name: String,
    based_on: Option<String>,
    locked: bool,
    history_divergence: Vec<String>,
}

#[derive(Serialize, Clone)]
struct EventSummary {
    id: String,
    time_key: i64,
    display_date: String,
    name: String,
    description: String,
    anchor_state_id: Option<String>,
    change_set_id: Option<String>,
    result_state_id: Option<String>,
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/history", get(get_history))
        .route("/api/history/unlock", post(post_unlock))
        .route("/api/history/states", post(post_state))
        .route("/api/history/events", post(post_event))
        .route("/api/history/cataclysm", post(post_cataclysm))
        .route(
            "/api/history/states/:id/select",
            post(post_select_state),
        )
        .route(
            "/api/history/states/:id/meta",
            put(put_state_meta),
        )
        .route(
            "/api/history/states/:id/divergence/ack",
            post(post_ack_divergence),
        )
        .route(
            "/api/history/states/:id/rebase",
            post(post_rebase_domain),
        )
        .route("/api/history/states/:id", delete(delete_state))
}

fn to_summary(s: &WorldStateRecord) -> WorldStateSummary {
    WorldStateSummary {
        id: s.id.clone(),
        time_key: s.time_key,
        display_date: s.display_date.clone(),
        name: s.name.clone(),
        based_on: s.based_on.clone(),
        locked: s.locked,
        history_divergence: s.history_divergence.clone(),
    }
}

fn to_event_summary(e: &HistoricalEventRecord) -> EventSummary {
    EventSummary {
        id: e.id.clone(),
        time_key: e.time_key,
        display_date: e.display_date.clone(),
        name: e.name.clone(),
        description: e.description.clone(),
        anchor_state_id: e.anchor_state_id.clone(),
        change_set_id: e.change_set_id.clone(),
        result_state_id: e.result_state_id.clone(),
    }
}

fn to_domain_ref(d: &DomainRefSummary) -> DomainRefWire {
    DomainRefWire {
        domain: d.domain.clone(),
        local_ref: d.local_ref.clone(),
        fork_source_state_id: d.fork_source_state_id.clone(),
        fork_source_state_name: d.fork_source_state_name.clone(),
        message: d.message.clone(),
    }
}

fn to_divergence_review(r: &DivergenceStateReview) -> DivergenceReviewWire {
    DivergenceReviewWire {
        state_id: r.state_id.clone(),
        display_date: r.display_date.clone(),
        name: r.name.clone(),
        domains: r.domains.iter().map(to_domain_ref).collect(),
    }
}

fn unlock_available(world_path: &std::path::Path) -> bool {
    !read_build(world_path)
        .map(|b| is_draft(&b))
        .unwrap_or(false)
}

fn selected_can_delete(manifest: &HistoryManifest) -> bool {
    let id = manifest.selected_state_id.as_str();
    if id == mapkeeper_core::history::BASELINE_STATE_ID {
        return false;
    }
    if state_has_descendants(manifest, id) {
        return false;
    }
    if manifest.change_sets.iter().any(|c| c.from_state_id == id || c.to_state_id == id) {
        return false;
    }
    if manifest.events.iter().any(|e| {
        e.anchor_state_id.as_deref() == Some(id) || e.result_state_id.as_deref() == Some(id)
    }) {
        return false;
    }
    true
}

fn build_response(manifest: &HistoryManifest, world_path: &std::path::Path) -> HistoryResponse {
    let divergence = manifest
        .selected_state()
        .map(|s| s.history_divergence.clone())
        .unwrap_or_default();
    HistoryResponse {
        enabled: manifest.enabled,
        unlock_available: unlock_available(world_path),
        selected_state_id: manifest.selected_state_id.clone(),
        states: manifest.states.iter().map(to_summary).collect(),
        events: manifest.events.iter().map(to_event_summary).collect(),
        history_divergence: divergence,
        divergence_review: build_divergence_review(manifest)
            .iter()
            .map(to_divergence_review)
            .collect(),
        selected_can_delete: selected_can_delete(manifest),
    }
}

async fn get_history(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let world = match world_scope::resolve_world(&server.app, &headers, ScopeMode::Read) {
        Ok(world) => world,
        Err(err) => return err.into_response(),
    };
    let manifest = read_history_manifest(&world.path);
    Json(build_response(&manifest, &world.path)).into_response()
}

async fn post_unlock(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    if !unlock_available(&world.path) {
        return (
            StatusCode::CONFLICT,
            "complete build wizard before unlocking history",
        )
            .into_response();
    }
    match unlock_history(&world.path) {
        Ok(manifest) => Json(build_response(&manifest, &world.path)).into_response(),
        Err(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateStateBody {
    time_key: i64,
    display_date: String,
    name: String,
    based_on: String,
    direction: String,
}

async fn post_state(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<CreateStateBody>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let mut manifest = read_history_manifest(&world.path);
    if !manifest.enabled {
        return (StatusCode::CONFLICT, "history not enabled").into_response();
    }
    match add_world_state(
        &mut manifest,
        &CreateStateInput {
            time_key: body.time_key,
            display_date: body.display_date,
            name: body.name,
            based_on: body.based_on,
            direction: body.direction,
        },
    ) {
        Ok(state) => {
            if let Err(msg) = write_history_manifest(&world.path, &manifest) {
                return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
            }
            Json(to_summary(&state)).into_response()
        }
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

async fn post_event(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<CreateEventBody>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let mut manifest = read_history_manifest(&world.path);
    if !manifest.enabled {
        return (StatusCode::CONFLICT, "history not enabled").into_response();
    }
    match add_lore_event(
        &mut manifest,
        &CreateEventInput {
            time_key: body.time_key,
            display_date: body.display_date,
            name: body.name,
            description: body.description,
            anchor_state_id: body.anchor_state_id,
        },
    ) {
        Ok(event) => {
            if let Err(msg) = write_history_manifest(&world.path, &manifest) {
                return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
            }
            Json(to_event_summary(&event)).into_response()
        }
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateEventBody {
    time_key: i64,
    display_date: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    anchor_state_id: Option<String>,
}

async fn post_cataclysm(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<CreateCataclysmBody>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let mut manifest = read_history_manifest(&world.path);
    if !manifest.enabled {
        return (StatusCode::CONFLICT, "history not enabled").into_response();
    }
    match create_cataclysm(
        &mut manifest,
        &CreateCataclysmInput {
            time_key: body.time_key,
            display_date: body.display_date,
            event_name: body.event_name,
            description: body.description,
            result_state_name: body.result_state_name,
            based_on: body.based_on,
            changed_domains: body.changed_domains,
            notes: body.notes,
        },
    ) {
        Ok(result) => {
            if let Err(msg) = write_history_manifest(&world.path, &manifest) {
                return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
            }
            let _ = result;
            Json(build_response(&manifest, &world.path)).into_response()
        }
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateCataclysmBody {
    time_key: i64,
    display_date: String,
    event_name: String,
    #[serde(default)]
    description: String,
    result_state_name: String,
    based_on: String,
    #[serde(default)]
    changed_domains: Vec<String>,
    #[serde(default)]
    notes: String,
}

async fn post_select_state(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let mut manifest = read_history_manifest(&world.path);
    if !manifest.enabled {
        return (StatusCode::CONFLICT, "history not enabled").into_response();
    }
    if manifest.state_by_id(&id).is_none() {
        return (StatusCode::NOT_FOUND, "state not found").into_response();
    }
    manifest.selected_state_id = id;
    if let Err(msg) = write_history_manifest(&world.path, &manifest) {
        return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
    }
    Json(build_response(&manifest, &world.path)).into_response()
}

async fn put_state_meta(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
    Json(body): Json<MetaInput>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let mut manifest = read_history_manifest(&world.path);
    let Some(state) = manifest.state_by_id_mut(&id) else {
        return (StatusCode::NOT_FOUND, "state not found").into_response();
    };
    if let Some(name) = body.name {
        state.name = name;
    }
    if let Some(display_date) = body.display_date {
        state.display_date = display_date;
    }
    if let Some(locked) = body.locked {
        state.locked = locked;
    }
    if let Err(msg) = write_history_manifest(&world.path, &manifest) {
        return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
    }
    Json(build_response(&manifest, &world.path)).into_response()
}

#[derive(Deserialize)]
struct MetaInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_date: Option<String>,
    #[serde(default)]
    locked: Option<bool>,
}

#[derive(Deserialize)]
struct AckDivergenceBody {
    #[serde(default)]
    domains: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RebaseBody {
    domain: String,
}

async fn post_ack_divergence(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
    Json(body): Json<AckDivergenceBody>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let mut manifest = read_history_manifest(&world.path);
    if !manifest.enabled {
        return (StatusCode::CONFLICT, "history not enabled").into_response();
    }
    match ack_divergence(
        &mut manifest,
        &id,
        body.domains.as_deref(),
    ) {
        Ok(()) => {
            if let Err(msg) = write_history_manifest(&world.path, &manifest) {
                return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
            }
            Json(build_response(&manifest, &world.path)).into_response()
        }
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

async fn post_rebase_domain(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
    Json(body): Json<RebaseBody>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let mut manifest = read_history_manifest(&world.path);
    if !manifest.enabled {
        return (StatusCode::CONFLICT, "history not enabled").into_response();
    }
    match rebase_domain(&mut manifest, &id, &body.domain) {
        Ok(()) => {
            if let Err(msg) = write_history_manifest(&world.path, &manifest) {
                return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
            }
            Json(build_response(&manifest, &world.path)).into_response()
        }
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

async fn delete_state(
    State(server): State<Arc<ServerState>>,
    headers: HeaderMap,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    let (world, _write) = match world_lock::resolve_mutate_and_guard(&server, &headers).await {
        Ok(pair) => pair,
        Err(err) => return err.into_response(),
    };
    let mut manifest = read_history_manifest(&world.path);
    if !manifest.enabled {
        return (StatusCode::CONFLICT, "history not enabled").into_response();
    }
    match delete_world_state(&mut manifest, &id) {
        Ok(()) => {
            if let Err(msg) = write_history_manifest(&world.path, &manifest) {
                return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
            }
            Json(build_response(&manifest, &world.path)).into_response()
        }
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use mapkeeper_core::history::BASELINE_STATE_ID;

    #[test]
    fn baseline_state_id_constant() {
        assert_eq!(BASELINE_STATE_ID, "ws-0000");
    }
}
