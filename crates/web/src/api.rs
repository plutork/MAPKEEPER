//! HTTP fetch/load/post helpers (D-94 B1).

use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::lakes::LakeCatalog;
use mapkeeper_core::layer::DenseLayer;
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::rivers::{river_at_cell, RiverCatalog};
use mapkeeper_core::worldgen::hydrology::{
    NameMigrationReport, NamedRiverBinding, RiverRenderPaths,
};
use serde::{Deserialize, Serialize};

use crate::brush::{
    deactivate_paint_brush, reset_view_on_world_open, river_brush, sync_detach_tributary_ui,
    sync_manual_river_authoring_ui, sync_name_migration_warning, sync_river_status,
    RIVERS_READ_ONLY_MSG,
};
use crate::dom::{input, perf_now, set_text, set_world_label, show_view, textarea};
use crate::home::{refresh_suggested_path, render_project_list};
use crate::state::{
    bump_content_rev, draw_snapshot, fresh_elevation_layer, AppState, BuildStateInput,
    LayerCellWrite, MapResponse, PerfMetrics, ProjectsResponse, WaterGenTrace,
    PAINT_BATCH_MAX_CELLS, PAINT_SAVE_COOLDOWN_MS,
};
use crate::water_diag::{
    lake_catalog_stats, set_water_gen_trace, sync_water_diagnostics,
};
use crate::wizard::sync_wizard_actions;

pub(crate) async fn persist_build_draft(step: u32) -> bool {
    let body = BuildStateInput {
        status: "draft",
        step,
    };
    let Ok(resp) = gloo_net::http::Request::put("/api/build")
        .json(&body)
        .expect("serializing build state")
        .send()
        .await
    else {
        return false;
    };
    resp.ok()
}
pub(crate) async fn load_geology(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/layers/geology")
        .send()
        .await
    else {
        return;
    };
    if !resp.ok() {
        return;
    }
    let Ok(layer) = resp.json::<DenseLayer>().await else {
        return;
    };
    // world-pipeline--tectonics-v1: tint overlay depends on content_rev in DrawSnapshot
    let mut s = state.borrow_mut();
    s.geology = Some(layer);
    bump_content_rev(&mut s);
}
/// Fetch `/api/projects`; show the Home list or jump straight into the
/// editor if a world is already active (e.g. server started with `--world`).
pub(crate) async fn refresh_projects(state: Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/projects").send().await else {
        set_text("home-status", "Could not reach mapkeeper-server.");
        return;
    };
    let Ok(data) = resp.json::<ProjectsResponse>().await else {
        return;
    };

    {
        let mut state_mut = state.borrow_mut();
        state_mut.default_worlds_root = Some(data.default_worlds_root.clone());
    }
    refresh_suggested_path(&state);
    render_project_list(&data.projects, &state);

    if let Some(active) = data.active {
        let _ = active;
        show_view("editor");
        wasm_bindgen_futures::spawn_local(load_map(state));
    } else {
        show_view("home");
    }
}
pub(crate) async fn load_map(state: Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        s.perf = PerfMetrics::default();
        s.perf_timers.open_start = Some(perf_now());
    }
    if let Ok(resp) = gloo_net::http::Request::get("/api/map").send().await {
        if let Ok(map) = resp.json::<MapResponse>().await {
            let mut state_mut = state.borrow_mut();
            state_mut.cells = map
                .cells
                .into_iter()
                .map(|c| ((c.q, c.r), c.display_name))
                .collect();
            bump_content_rev(&mut state_mut);
            state_mut.map_bounds =
                MapBounds::new(map.bounds.width.max(1), map.bounds.height.max(1));
            state_mut.zoom = 1.0;
            state_mut.pan_x = 0.0;
            state_mut.pan_y = 0.0;
            state_mut.pending_paints.clear();
            state_mut.paint_flush_scheduled = false;
            state_mut.paint_flush_in_flight = false;
            state_mut.legacy_map = map.legacy_map;
            reset_view_on_world_open(&mut state_mut);
            sync_wizard_actions(&state_mut);
            set_world_label(&format!(
                "{} · {} cells",
                map.world_id, map.bounds.cell_count
            ));
            if map.legacy_map {
                set_text(
                    "legacy-map-note",
                    "Legacy folder — using default Small bounds until map/manifest.json exists.",
                );
            } else {
                set_text("legacy-map-note", "");
            }
        }
    }
    load_elevation(&state).await;
    probe_precip_layer(&state).await;
    load_lakes(&state).await;
    load_rivers(&state).await;
    let redraw_start = perf_now();
    let drawn = crate::canvas::redraw(&state.borrow());
    let first_redraw_ms = perf_now() - redraw_start;
    {
        let mut s = state.borrow_mut();
        s.perf.first_redraw_ms = Some(first_redraw_ms);
        s.perf.redraw_ms = Some(first_redraw_ms);
        s.perf.drawn_cells = Some(drawn);
        if let Some(t0) = s.perf_timers.open_start.take() {
            s.perf.open_ms = Some(perf_now() - t0);
        }
        s.last_draw_snapshot = Some(draw_snapshot(&s));
    }
    crate::perf_emit(&state.borrow().perf);
}

/// Fetch the dense elevation layer (scale-layers, D-46) into index buffers.
pub(crate) async fn load_elevation(state: &Rc<RefCell<AppState>>) {
    let fetch_start = perf_now();
    let Ok(resp) = gloo_net::http::Request::get("/api/layers/elevation")
        .send()
        .await
    else {
        return;
    };
    let fetch_ms = perf_now() - fetch_start;
    let parse_start = perf_now();
    let Ok(layer) = resp.json::<DenseLayer>().await else {
        return;
    };
    let parse_ms = perf_now() - parse_start;
    let mirror_start = perf_now();
    let bounds = state.borrow().map_bounds;
    // perf-100k--web-dense-client: adopt layer wholesale — no scan-to-HashMap.
    let adopted = if layer.cell_count == bounds.len() {
        layer
    } else {
        fresh_elevation_layer(bounds)
    };
    let mirror_ms = perf_now() - mirror_start;
    let mut state_mut = state.borrow_mut();
    state_mut.elevation = adopted;
    bump_content_rev(&mut state_mut);
    state_mut.perf.layer_fetch_ms = Some(fetch_ms);
    state_mut.perf.layer_parse_or_decode_ms = Some(parse_ms);
    state_mut.perf.client_mirror_ms = Some(mirror_ms);
}

/// Fetch lake catalog (hydrology-lake-domain-v1).
pub(crate) async fn load_lakes(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/lakes").send().await else {
        return;
    };
    let Ok(catalog) = resp.json::<LakeCatalog>().await else {
        return;
    };
    let mut s = state.borrow_mut();
    s.lakes = catalog;
    bump_content_rev(&mut s);
    sync_water_diagnostics(&s);
}

async fn probe_precip_layer(state: &Rc<RefCell<AppState>>) {
    let present = gloo_net::http::Request::get("/api/layers/precipitation")
        .send()
        .await
        .map(|r| r.ok())
        .unwrap_or(false);
    {
        let mut s = state.borrow_mut();
        s.precip_layer_present = Some(present);
        if !present {
            s.precip_input_state = Some("missing".to_string());
        }
    }
    if present {
        sync_precip_input_from_diagnostics(state).await;
    }
    sync_water_diagnostics(&state.borrow());
}

async fn sync_precip_input_from_diagnostics(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/hydrology/diagnostics")
        .send()
        .await
    else {
        return;
    };
    if !resp.ok() {
        return;
    }
    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return;
    };
    if let Some(value) = body.get("precip_input_state").and_then(|v| v.as_str()) {
        state.borrow_mut().precip_input_state = Some(value.to_string());
        sync_water_diagnostics(&state.borrow());
    }
}

/// Fetch river topology projection.
pub(crate) async fn load_rivers(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/rivers").send().await else {
        return;
    };
    let Ok(body) = resp.json::<RiversResponse>().await else {
        return;
    };
    let mut s = state.borrow_mut();
    apply_rivers_response(&mut s, body);
    sync_river_status(&s);
    sync_name_migration_warning(&s);
    sync_manual_river_authoring_ui(&s);
    sync_water_diagnostics(&s);
}
#[derive(Serialize)]
pub(crate) struct LakesGenerateInput {
    pub(crate) density: String,
    pub(crate) seed: u64,
}

#[derive(Deserialize)]
pub(crate) struct LakesGenerateResponse {
    #[serde(flatten)]
    pub(crate) catalog: LakeCatalog,
    pub(crate) rivers_cleared: bool,
    pub(crate) precip_input_state: String,
}

#[derive(Serialize)]
pub(crate) struct RiversGenerateInput {
    pub(crate) river_density: String,
}

#[derive(Deserialize)]
pub(crate) struct RiversResponse {
    #[serde(flatten)]
    pub(crate) catalog: RiverCatalog,
    #[serde(default)]
    pub(crate) render_paths: RiverRenderPaths,
    #[serde(default)]
    pub(crate) read_only: bool,
    #[serde(default)]
    pub(crate) channel_segment_count: Option<usize>,
    #[serde(default)]
    pub(crate) channel_cell_count: Option<usize>,
    #[serde(default)]
    pub(crate) named_rivers: Vec<NamedRiverBinding>,
    #[serde(default)]
    pub(crate) name_migration: Vec<NameMigrationReport>,
    #[serde(default)]
    pub(crate) compatibility_projection: bool,
    #[serde(default)]
    pub(crate) named_river_count: Option<usize>,
}

fn apply_rivers_response(s: &mut AppState, body: RiversResponse) {
    s.rivers = body.catalog;
    s.river_render_paths = body.render_paths;
    s.rivers_read_only = body.read_only;
    s.channel_segment_count = body.channel_segment_count;
    s.channel_cell_count = body.channel_cell_count;
    s.named_rivers = body.named_rivers;
    s.name_migration = body.name_migration;
    s.rivers_compatibility_projection = body.compatibility_projection;
    s.active_river_id = None;
    if body.read_only && river_brush(&s.brush) {
        deactivate_paint_brush(s);
    }
}

#[derive(Deserialize)]
pub(crate) struct RiversGenerateResponse {
    #[serde(flatten)]
    pub(crate) response: RiversResponse,
    pub(crate) precip_input_state: String,
    #[serde(default)]
    pub(crate) precip_source: String,
    #[serde(default)]
    pub(crate) river_density: String,
    #[serde(default)]
    pub(crate) name_migration_ambiguous_count: usize,
    #[serde(default = "default_true")]
    pub(crate) deterministic: bool,
    #[serde(default)]
    pub(crate) input_fingerprint: String,
    #[serde(default)]
    pub(crate) regenerate_nonce_ignored: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize)]
pub(crate) struct RiverAppendBody {
    pub(crate) river_id: Option<u32>,
    pub(crate) q: i32,
    pub(crate) r: i32,
}

pub(crate) async fn post_river_pin(
    state: Rc<RefCell<AppState>>,
    source: (i32, i32),
    mouth: (i32, i32),
) {
    if state.borrow().rivers_read_only {
        set_text("river-status", RIVERS_READ_ONLY_MSG);
        return;
    }
    #[derive(Serialize)]
    struct RiverPinBody {
        source_q: i32,
        source_r: i32,
        mouth_q: i32,
        mouth_r: i32,
        river_id: Option<u32>,
    }
    let body = RiverPinBody {
        source_q: source.0,
        source_r: source.1,
        mouth_q: mouth.0,
        mouth_r: mouth.1,
        river_id: None,
    };
    let Ok(resp) = gloo_net::http::Request::post("/api/rivers/pin")
        .json(&body)
        .expect("serialize river pin")
        .send()
        .await
    else {
        set_text("river-status", "Pin river failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Pin river rejected".into());
        set_text("river-status", &msg);
        return;
    }
    #[derive(Deserialize)]
    struct RiverPinResponse {
        river_id: u32,
        #[serde(flatten)]
        catalog: RiverCatalog,
    }
    let Ok(body) = resp.json::<RiverPinResponse>().await else {
        set_text("river-status", "Pin river failed (parse)");
        return;
    };
    {
        let mut s = state.borrow_mut();
        s.rivers = body.catalog;
        s.active_river_id = Some(body.river_id);
        s.river_pin_source = None;
        bump_content_rev(&mut s);
        sync_river_status(&s);
        sync_detach_tributary_ui(&s);
    }
    crate::canvas::schedule_redraw(state);
}

pub(crate) async fn post_river_append(state: Rc<RefCell<AppState>>, q: i32, r: i32) {
    if state.borrow().rivers_read_only {
        set_text("river-status", RIVERS_READ_ONLY_MSG);
        return;
    }
    let river_id = state.borrow().active_river_id;
    let body = RiverAppendBody { river_id, q, r };
    let Ok(resp) = gloo_net::http::Request::post("/api/rivers/append")
        .json(&body)
        .expect("serialize river append")
        .send()
        .await
    else {
        set_text("river-status", "River save failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "River append rejected".into());
        set_text("river-status", &msg);
        return;
    }
    let Ok(catalog) = resp.json::<RiverCatalog>().await else {
        set_text("river-status", "River save failed (parse)");
        return;
    };
    let new_active = if river_id.is_some() {
        river_id
    } else {
        catalog.rivers.last().map(|r| r.id)
    };
    {
        let mut s = state.borrow_mut();
        s.rivers = catalog;
        s.active_river_id = new_active;
        bump_content_rev(&mut s);
        sync_river_status(&s);
        sync_detach_tributary_ui(&s);
    }
    crate::canvas::schedule_redraw(state);
}

pub(crate) async fn post_river_detach(state: Rc<RefCell<AppState>>) {
    if state.borrow().rivers_read_only {
        set_text("river-status", RIVERS_READ_ONLY_MSG);
        return;
    }
    let river_id = match state.borrow().active_river_id {
        Some(id) => id,
        None => {
            set_text("river-status", "Select a tributary to detach");
            return;
        }
    };
    let url = format!("/api/rivers/{river_id}/detach");
    let Ok(resp) = gloo_net::http::Request::post(&url)
        .header("content-type", "application/json")
        .body("")
        .expect("detach body")
        .send()
        .await
    else {
        set_text("river-status", "Detach failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Detach rejected".into());
        set_text("river-status", &msg);
        return;
    }
    let Ok(catalog) = resp.json::<RiverCatalog>().await else {
        set_text("river-status", "Detach failed (parse)");
        return;
    };
    {
        let mut s = state.borrow_mut();
        s.rivers = catalog;
        s.active_river_id = Some(river_id);
        bump_content_rev(&mut s);
        sync_river_status(&s);
        sync_detach_tributary_ui(&s);
    }
    crate::canvas::schedule_redraw(state);
}

pub(crate) async fn delete_river_at_cell(state: Rc<RefCell<AppState>>, q: i32, r: i32) {
    if state.borrow().rivers_read_only {
        set_text("river-status", RIVERS_READ_ONLY_MSG);
        return;
    }
    let river_id = {
        let s = state.borrow();
        let index = match s.map_bounds.index_of(Axial::new(q, r)) {
            Some(i) => i,
            None => return,
        };
        match river_at_cell(&s.rivers, index) {
            Some(id) => id,
            None => {
                set_text("river-status", "No river on this cell");
                return;
            }
        }
    };
    let url = format!("/api/rivers/{river_id}");
    let Ok(resp) = gloo_net::http::Request::delete(&url).send().await else {
        set_text("river-status", "River delete failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "River delete rejected".into());
        set_text("river-status", &msg);
        return;
    }
    let Ok(catalog) = resp.json::<RiverCatalog>().await else {
        set_text("river-status", "River delete failed (parse)");
        return;
    };
    {
        let mut s = state.borrow_mut();
        s.rivers = catalog;
        if s.active_river_id == Some(river_id) {
            s.active_river_id = None;
        }
        bump_content_rev(&mut s);
        sync_river_status(&s);
    }
    crate::canvas::schedule_redraw(state);
}

pub(crate) async fn post_river_pop(state: Rc<RefCell<AppState>>) {
    if state.borrow().rivers_read_only {
        set_text("river-status", RIVERS_READ_ONLY_MSG);
        return;
    }
    let river_id = match state.borrow().active_river_id {
        Some(id) => id,
        None => {
            set_text("river-status", "No active river to undo");
            return;
        }
    };
    let url = format!("/api/rivers/{river_id}/pop");
    let Ok(resp) = gloo_net::http::Request::post(&url).send().await else {
        set_text("river-status", "Undo failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp.text().await.unwrap_or_else(|_| "Undo rejected".into());
        set_text("river-status", &msg);
        return;
    }
    let Ok(catalog) = resp.json::<RiverCatalog>().await else {
        set_text("river-status", "Undo failed (parse)");
        return;
    };
    let still_active = catalog.rivers.iter().any(|r| r.id == river_id);
    {
        let mut s = state.borrow_mut();
        s.rivers = catalog;
        if !still_active {
            s.active_river_id = None;
        }
        bump_content_rev(&mut s);
        sync_river_status(&s);
    }
    crate::canvas::schedule_redraw(state);
}

pub(crate) async fn post_lake_generate(
    state: Rc<RefCell<AppState>>,
    status_id: &'static str,
    density: String,
) {
    set_text(status_id, "Generating lakes…");
    let req_line = format!("density={density} seed=1");
    let body = LakesGenerateInput {
        density: density.clone(),
        seed: 1,
    };
    let Ok(resp) = gloo_net::http::Request::post("/api/lakes/generate")
        .json(&body)
        .expect("serialize lakes generate")
        .send()
        .await
    else {
        let mut s = state.borrow_mut();
        set_water_gen_trace(
            &mut s,
            WaterGenTrace {
                action: "generate_lakes".into(),
                request: req_line,
                result: String::new(),
                error: "network failure".into(),
            },
        );
        set_text(status_id, "Generate failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Generate rejected".into());
        let mut s = state.borrow_mut();
        set_water_gen_trace(
            &mut s,
            WaterGenTrace {
                action: "generate_lakes".into(),
                request: req_line,
                result: String::new(),
                error: msg.clone(),
            },
        );
        set_text(status_id, &msg);
        return;
    }
    let Ok(body) = resp.json::<LakesGenerateResponse>().await else {
        let mut s = state.borrow_mut();
        set_water_gen_trace(
            &mut s,
            WaterGenTrace {
                action: "generate_lakes".into(),
                request: req_line,
                result: String::new(),
                error: "response parse failure".into(),
            },
        );
        set_text(status_id, "Generate failed (parse)");
        return;
    };
    let (lake_n, lake_cells, endorheic) = lake_catalog_stats(&body.catalog);
    let next_id = body.catalog.next_id;
    let rivers_cleared = body.rivers_cleared;
    {
        let mut s = state.borrow_mut();
        s.lakes = body.catalog;
        if rivers_cleared {
            s.rivers = RiverCatalog::default();
            s.river_render_paths = RiverRenderPaths::default();
            s.rivers_read_only = false;
            s.channel_segment_count = None;
            s.channel_cell_count = None;
            s.named_rivers.clear();
            s.name_migration.clear();
            s.rivers_compatibility_projection = false;
            s.precip_source = None;
            s.active_river_id = None;
            sync_river_status(&s);
            sync_manual_river_authoring_ui(&s);
        }
        bump_content_rev(&mut s);
        set_water_gen_trace(
            &mut s,
            WaterGenTrace {
                action: "generate_lakes".into(),
                request: req_line,
                result: format!(
                    "lakes={lake_n} cells={lake_cells} endorheic={endorheic} next_id={next_id} rivers_cleared={rivers_cleared} precip={}",
                    body.precip_input_state
                ),
                error: String::new(),
            },
        );
        s.precip_input_state = Some(body.precip_input_state);
    }
    if rivers_cleared {
        set_text(status_id, "Rivers cleared — regenerate rivers.");
    } else {
        set_text(status_id, &format!("Generated {lake_n} lake(s)."));
    }
    crate::canvas::schedule_redraw(state);
}

pub(crate) async fn post_river_generate(
    state: Rc<RefCell<AppState>>,
    status_id: &'static str,
    river_density: String,
) {
    set_text(status_id, "Rebuilding rivers…");
    let req_line = format!("river_density={river_density}");
    let body = RiversGenerateInput {
        river_density: river_density.clone(),
    };
    let Ok(resp) = gloo_net::http::Request::post("/api/rivers/generate")
        .json(&body)
        .expect("serialize rivers generate")
        .send()
        .await
    else {
        let mut s = state.borrow_mut();
        set_water_gen_trace(
            &mut s,
            WaterGenTrace {
                action: "generate_rivers".into(),
                request: req_line,
                result: String::new(),
                error: "network failure".into(),
            },
        );
        set_text(status_id, "Generate failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Generate rejected".into());
        let mut s = state.borrow_mut();
        set_water_gen_trace(
            &mut s,
            WaterGenTrace {
                action: "generate_rivers".into(),
                request: req_line,
                result: String::new(),
                error: msg.clone(),
            },
        );
        set_text(status_id, &msg);
        return;
    }
    let Ok(body) = resp.json::<RiversGenerateResponse>().await else {
        let mut s = state.borrow_mut();
        set_water_gen_trace(
            &mut s,
            WaterGenTrace {
                action: "generate_rivers".into(),
                request: req_line,
                result: String::new(),
                error: "response parse failure".into(),
            },
        );
        set_text(status_id, "Generate failed (parse)");
        return;
    };
    let ambiguous = body
        .response
        .name_migration
        .iter()
        .filter(|report| report.ambiguous)
        .count();
    let named_n = body
        .response
        .named_river_count
        .unwrap_or(body.response.named_rivers.len());
    let segments = body
        .response
        .channel_segment_count
        .unwrap_or(body.response.catalog.rivers.len());
    let channel_cells = body.response.channel_cell_count.unwrap_or_else(|| {
        body.response
            .catalog
            .rivers
            .iter()
            .map(|r| r.cells.len())
            .sum()
    });
    let lake_n = state.borrow().lakes.lakes.len();
    {
        let mut s = state.borrow_mut();
        apply_rivers_response(&mut s, body.response);
        bump_content_rev(&mut s);
        sync_river_status(&s);
        sync_name_migration_warning(&s);
        sync_manual_river_authoring_ui(&s);
        let density_note = if body.river_density.is_empty() {
            river_density.as_str()
        } else {
            body.river_density.as_str()
        };
        set_water_gen_trace(
            &mut s,
            WaterGenTrace {
                action: "generate_rivers".into(),
                request: req_line,
                result: format!(
                    "named_rivers={named_n} segments={segments} channel_cells={channel_cells} migration_ambiguous={ambiguous} deterministic={} fingerprint={} regenerate_nonce_ignored={} name_migration_ambiguous={} precip={} precip_source={} density={density_note} lakes_in_catalog={lake_n}",
                    body.deterministic,
                    body.input_fingerprint,
                    body.regenerate_nonce_ignored,
                    body.name_migration_ambiguous_count,
                    body.precip_input_state,
                    body.precip_source
                ),
                error: String::new(),
            },
        );
        s.precip_input_state = Some(body.precip_input_state.clone());
        s.precip_source = Some(body.precip_source.clone());
    }
    let source_note = match body.precip_source.as_str() {
        "climate" => "from climate precipitation",
        "uniform_fallback" => "uniform fallback runoff",
        other => other,
    };
    let density_note = if body.river_density.is_empty() {
        river_density.as_str()
    } else {
        body.river_density.as_str()
    };
    let migration_note = if ambiguous > 0 {
        format!(" · {ambiguous} ambiguous name(s) need review")
    } else {
        String::new()
    };
    set_text(
        status_id,
        &format!(
            "Rebuilt {named_n} named river(s) · {segments} physical segments — same inputs give the same network · {source_note} · density {density_note}{migration_note}",
        ),
    );
    crate::canvas::schedule_redraw(state);
}
pub(crate) fn schedule_paint_flush(state: Rc<RefCell<AppState>>) {
    let should_schedule = {
        let mut s = state.borrow_mut();
        if s.paint_flush_scheduled || s.pending_paints.is_empty() {
            false
        } else {
            s.paint_flush_scheduled = true;
            true
        }
    };
    if !should_schedule {
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        TimeoutFuture::new(PAINT_SAVE_COOLDOWN_MS).await;
        flush_pending_paints(state).await;
    });
}

pub(crate) async fn flush_pending_paints(state: Rc<RefCell<AppState>>) {
    let batch = {
        let mut s = state.borrow_mut();
        s.paint_flush_scheduled = false;
        if s.paint_flush_in_flight || s.pending_paints.is_empty() {
            return;
        }
        s.paint_flush_in_flight = true;
        s.pending_paints.drain().collect::<Vec<_>>()
    };

    let mut failed_cells: Vec<((i32, i32), i32)> = Vec::new();
    let mut measured_batch = false;
    // save-batch--http-endpoint-v1: send chunked batch writes.
    for chunk in batch.chunks(PAINT_BATCH_MAX_CELLS.max(1)) {
        let payload = chunk
            .iter()
            .map(|((q, r), elevation)| LayerCellWrite {
                q: *q,
                r: *r,
                state: "value",
                value: *elevation,
            })
            .collect::<Vec<_>>();
        let batch_start = perf_now();
        let sent = gloo_net::http::Request::put("/api/layers/elevation/batch")
            .json(&payload)
            .expect("serializing elevation batch body")
            .send()
            .await;
        if matches!(sent, Ok(resp) if resp.ok()) {
            state.borrow_mut().perf.batch_flush_ms = Some(perf_now() - batch_start);
            measured_batch = true;
        } else {
            failed_cells.extend(chunk.iter().copied());
        }
    }

    {
        let mut s = state.borrow_mut();
        for ((q, r), value) in failed_cells {
            s.pending_paints.insert((q, r), value);
        }
        s.paint_flush_in_flight = false;
    }

    if measured_batch {
        crate::perf_emit(&state.borrow().perf);
    }

    if !state.borrow().pending_paints.is_empty() {
        set_text("status", "Autosave retry…");
        schedule_paint_flush(state);
    } else {
        set_text("status", "");
    }
}
pub(crate) async fn load_profile_into_panel(state: Rc<RefCell<AppState>>, q: i32, r: i32) {
    let url = format!("/api/cells/{q}/{r}/profile");
    let profile = gloo_net::http::Request::get(&url)
        .send()
        .await
        .ok()
        .and_then(|resp| resp.ok().then_some(resp));
    // The author may have clicked a different cell while this was in
    // flight — don't clobber whatever panel is showing now.
    if state.borrow().selected != Some((q, r)) {
        return;
    }
    let Some(resp) = profile else {
        set_text("status", "Could not load profile");
        input("title").set_disabled(false);
        textarea("notes").set_disabled(false);
        return;
    };
    if let Ok(profile) = resp.json::<CellProfile>().await {
        if state.borrow().selected == Some((q, r)) {
            input("title").set_value(&profile.display_name);
            textarea("notes").set_value(&profile.notes);
        }
    }
    input("title").set_disabled(false);
    textarea("notes").set_disabled(false);
    set_text("status", "");
}
