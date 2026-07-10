//! HTTP fetch/load/post helpers (D-94 B1).

use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::layer::DenseLayer;
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::rivers::{river_at_cell, RiverCatalog};
use serde::{Deserialize, Serialize};

use crate::dom::{input, perf_now, set_text, set_world_label, show_view, textarea};
use crate::state::{
    bump_content_rev, draw_snapshot, fresh_elevation_layer, AppState, BuildStateInput,
    LayerCellWrite, MapResponse, PerfMetrics, ProjectsResponse,
    PAINT_BATCH_MAX_CELLS, PAINT_SAVE_COOLDOWN_MS,
};

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
    crate::refresh_suggested_path(&state);
    crate::render_project_list(&data.projects, &state);

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
            crate::reset_view_on_world_open(&mut state_mut);
            crate::sync_wizard_actions(&state_mut);
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
    load_rivers(&state).await;
    let redraw_start = perf_now();
    let drawn = crate::redraw(&state.borrow());
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

/// Fetch river catalog (river-overlay-layer-v1).
pub(crate) async fn load_rivers(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/rivers").send().await else {
        return;
    };
    let Ok(catalog) = resp.json::<RiverCatalog>().await else {
        return;
    };
    let mut s = state.borrow_mut();
    s.rivers = catalog;
    s.active_river_id = None;
    crate::sync_river_status(&s);
}
#[derive(Deserialize)]
pub(crate) struct RiversGenerateResponse {
    #[serde(flatten)]
    pub(crate) catalog: RiverCatalog,
    pub(crate) precip_source: String,
}

#[derive(Serialize)]
pub(crate) struct RiverAppendBody {
    pub(crate) river_id: Option<u32>,
    pub(crate) q: i32,
    pub(crate) r: i32,
}

pub(crate) async fn post_river_append(state: Rc<RefCell<AppState>>, q: i32, r: i32) {
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
        crate::sync_river_status(&s);
    }
    crate::schedule_redraw(state);
}

pub(crate) async fn delete_river_at_cell(state: Rc<RefCell<AppState>>, q: i32, r: i32) {
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
        crate::sync_river_status(&s);
    }
    crate::schedule_redraw(state);
}

pub(crate) async fn post_river_pop(state: Rc<RefCell<AppState>>) {
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
        crate::sync_river_status(&s);
    }
    crate::schedule_redraw(state);
}

pub(crate) async fn post_river_generate(state: Rc<RefCell<AppState>>, status_id: &str) {
    set_text(status_id, "Generating rivers…");
    let Ok(resp) = gloo_net::http::Request::post("/api/rivers/generate")
        .send()
        .await
    else {
        set_text(status_id, "Generate failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Generate rejected".into());
        set_text(status_id, &msg);
        return;
    }
    let Ok(body) = resp.json::<RiversGenerateResponse>().await else {
        set_text(status_id, "Generate failed (parse)");
        return;
    };
    {
        let mut s = state.borrow_mut();
        s.rivers = body.catalog;
        s.active_river_id = None;
        bump_content_rev(&mut s);
        crate::sync_river_status(&s);
    }
    let source_note = if body.precip_source == "climate" {
        "from climate precipitation"
    } else {
        "uniform fallback (no precipitation layer)"
    };
    set_text(
        status_id,
        &format!(
            "Generated {} river(s) — {}",
            state.borrow().rivers.rivers.len(),
            source_note
        ),
    );
    crate::schedule_redraw(state);
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
