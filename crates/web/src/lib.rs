//! WASM UI — calls mapkeeper-core for hex geometry + profile rules;
//! filesystem goes through the `mapkeeper-server` HTTP API, never direct
//! FS access. WASM framework choice: none — plain `wasm-bindgen` + `web-sys`
//! canvas, deliberately minimal for the flow-first pass (roadmap D-21).
//!
//! Flow: Home screen lists/creates worlds (roadmap D-12/5.7 launcher) ->
//! open a world -> render a blank hex grid -> click a cell -> edit a
//! placeholder profile (title + notes) -> save -> cell is "painted". No
//! real cell schema (roadmap 3.2) yet — that is the point of this pass.

mod elevation_view;

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use elevation_view::{
    draw_elevation_label, draw_mountain_glyph, labels_status_label, overlays_lod_ok,
    peaks_status_label, set_fill_rgb, ColorMode, MOUNTAIN_THRESHOLD,
};
use gloo_timers::future::TimeoutFuture;
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::hydro::{hydro_from_elevation, stamp_delta, HydroKind};
use mapkeeper_core::land_mask::{find_recipe, next_recipe, pick_recipe, LayoutClass};
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue};
use mapkeeper_core::map_preset::MapPreset;
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::rivers::{river_at_cell, RiverCatalog};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    CanvasRenderingContext2d, Element, HtmlCanvasElement, HtmlElement, HtmlInputElement,
    HtmlSelectElement, HtmlTextAreaElement,
};

const MIN_ZOOM: f64 = 0.6;
const MAX_ZOOM: f64 = 2.5;
const PAN_DRAG_THRESHOLD: f64 = 3.0;
// save-batch--http-endpoint-v1: tuneable write buffering.
const PAINT_SAVE_COOLDOWN_MS: u32 = 300;
const PAINT_BATCH_MAX_CELLS: usize = 512;
const MIN_BRUSH_RADIUS: i32 = 0;
const MAX_BRUSH_RADIUS: i32 = 3;
// perf-100k--canvas-lod-grid-markers: skip per-hex stroke when many cells visible.
const GRID_STROKE_CELL_THRESHOLD: usize = 10_000;
// perf-100k--canvas-lod-grid-markers: hide profile dots when zoomed out.
const PROFILE_MARKER_MIN_ZOOM: f64 = 1.0;
// view-cells-seamless-v1: fill scales — On = edge-to-edge; Off = overlap seamless map.
const FILL_SCALE_GRID_ON: f64 = 1.0;
const FILL_SCALE_GRID_OFF: f64 = 1.04;
const GRID_LINE_WIDTH: f64 = 1.0;
/// Brush hover preview inset (unchanged from legacy HEX_GAP).
const BRUSH_PREVIEW_GAP: f64 = 0.92;
/// Breathing room (px) between the map and the canvas edge.
const CANVAS_PAD: f64 = 20.0;
/// Default land elevation when a cell is unknown/none (hydro projection).
const DEFAULT_LAND_ELEVATION: i32 = 1;

// perf-100k--web-dense-client: index-addressed elevation buffer (no sparse mirror).
fn fresh_elevation_layer(bounds: MapBounds) -> DenseLayer {
    DenseLayer::new_integer("elevation", bounds.len())
}

fn geology_tint(geo: &DenseLayer, bounds: MapBounds, q: i32, r: i32) -> Option<&'static str> {
    let index = bounds.index_of(Axial::new(q, r))?;
    match geo.state(index) {
        DenseState::Value(LayerValue::Text(ref t)) => match t.as_str() {
            "ridge" => Some("rgba(180, 90, 60, 0.35)"),
            "rift" => Some("rgba(120, 60, 140, 0.30)"),
            "basin" => Some("rgba(60, 100, 160, 0.30)"),
            "volcanic_arc" => Some("rgba(200, 70, 50, 0.40)"),
            "stable" => Some("rgba(90, 140, 80, 0.18)"),
            _ => None,
        },
        _ => None,
    }
}

fn elevation_at(layer: &DenseLayer, bounds: MapBounds, q: i32, r: i32) -> i32 {
    let index = bounds.index_of(Axial::new(q, r)).unwrap_or(0);
    layer.int_or(index, DEFAULT_LAND_ELEVATION)
}

fn set_elevation_cell(layer: &mut DenseLayer, bounds: MapBounds, q: i32, r: i32, value: i32) {
    if let Some(index) = bounds.index_of(Axial::new(q, r)) {
        layer.set(index, DenseState::Value(LayerValue::Int(value)));
    }
}

fn count_visible_in_bounds(
    q_min: i32,
    q_max: i32,
    r_min: i32,
    r_max: i32,
    bounds: MapBounds,
) -> usize {
    let mut n = 0usize;
    for q in q_min..=q_max {
        for r in r_min..=r_max {
            if bounds.contains(Axial::new(q, r)) {
                n += 1;
            }
        }
    }
    n
}

fn stroke_grid_enabled(show_grid: bool, visible_cells: usize) -> bool {
    show_grid && visible_cells <= GRID_STROKE_CELL_THRESHOLD
}

fn show_profile_markers(zoom: f64) -> bool {
    zoom >= PROFILE_MARKER_MIN_ZOOM
}

fn grid_lines_stats_label(show_grid: bool, visible_cells: usize) -> &'static str {
    if !show_grid {
        "lines Off"
    } else if visible_cells > GRID_STROKE_CELL_THRESHOLD {
        "lines Auto-off"
    } else {
        "lines On"
    }
}

fn grid_lines_toggle_label(show_grid: bool) -> &'static str {
    if show_grid {
        "Grid lines: On"
    } else {
        "Grid lines: Off"
    }
}

// perf-100k--measurement-hooks: lightweight Step 0 timing (console + view pane).
#[derive(Default)]
struct PerfMetrics {
    open_ms: Option<f64>,
    layer_fetch_ms: Option<f64>,
    layer_parse_or_decode_ms: Option<f64>,
    client_mirror_ms: Option<f64>,
    first_redraw_ms: Option<f64>,
    redraw_ms: Option<f64>,
    drawn_cells: Option<usize>,
    batch_flush_ms: Option<f64>,
}

#[derive(Default)]
struct PerfTimers {
    open_start: Option<f64>,
}

fn perf_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or_else(js_sys::Date::now)
}

fn perf_ms_label(v: Option<f64>) -> String {
    match v {
        Some(ms) => format!("{ms:.0}ms"),
        None => "—".to_string(),
    }
}

impl PerfMetrics {
    fn console_line(&self) -> String {
        format!(
            "open={} fetch={} parse={} mirror={} 1st_redraw={} redraw={} drawn={} batch={}",
            perf_ms_label(self.open_ms),
            perf_ms_label(self.layer_fetch_ms),
            perf_ms_label(self.layer_parse_or_decode_ms),
            perf_ms_label(self.client_mirror_ms),
            perf_ms_label(self.first_redraw_ms),
            perf_ms_label(self.redraw_ms),
            self.drawn_cells
                .map(|n| n.to_string())
                .unwrap_or_else(|| "—".to_string()),
            perf_ms_label(self.batch_flush_ms),
        )
    }

    fn view_line(&self) -> String {
        format!(
            "Perf: open {} · fetch {} · parse {} · mirror {} · redraw {} · batch {} · drawn {}",
            perf_ms_label(self.open_ms),
            perf_ms_label(self.layer_fetch_ms),
            perf_ms_label(self.layer_parse_or_decode_ms),
            perf_ms_label(self.client_mirror_ms),
            perf_ms_label(self.redraw_ms),
            perf_ms_label(self.batch_flush_ms),
            self.drawn_cells
                .map(|n| n.to_string())
                .unwrap_or_else(|| "—".to_string()),
        )
    }
}

fn perf_emit(metrics: &PerfMetrics) {
    set_text("view-perf", &metrics.view_line());
    web_sys::console::log_1(&format!("[perf] {}", metrics.console_line()).into());
}

fn redraw_and_sample(state: &Rc<RefCell<AppState>>) {
    let t0 = perf_now();
    let drawn = redraw(&state.borrow());
    let ms = perf_now() - t0;
    let mut s = state.borrow_mut();
    s.perf.redraw_ms = Some(ms);
    s.perf.drawn_cells = Some(drawn);
    s.last_draw_snapshot = Some(draw_snapshot(&s));
    set_text("view-perf", &s.perf.view_line());
}

// perf-100k--raf-redraw-coalesce: at most one full redraw per animation frame.
#[derive(Clone, Copy, PartialEq)]
struct DrawSnapshot {
    zoom: f64,
    pan_x: f64,
    pan_y: f64,
    selected: Option<(i32, i32)>,
    show_grid: bool,
    hover_cell: Option<(i32, i32)>,
    color_mode: ColorMode,
    show_elevation_labels: bool,
    show_peaks: bool,
    content_rev: u64,
}

fn draw_snapshot(s: &AppState) -> DrawSnapshot {
    DrawSnapshot {
        zoom: s.zoom,
        pan_x: s.pan_x,
        pan_y: s.pan_y,
        selected: s.selected,
        show_grid: s.show_grid,
        hover_cell: s.hover_cell,
        color_mode: s.color_mode,
        show_elevation_labels: s.show_elevation_labels,
        show_peaks: s.show_peaks,
        content_rev: s.content_rev,
    }
}

fn bump_content_rev(s: &mut AppState) {
    s.content_rev = s.content_rev.wrapping_add(1);
}

fn schedule_redraw(state: Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        s.redraw_dirty = true;
        if s.redraw_raf_pending {
            return;
        }
        s.redraw_raf_pending = true;
    }

    let state_cb = state.clone();
    let closure = Closure::once(move || {
        flush_scheduled_redraw(state_cb);
    });
    let _ = window()
        .request_animation_frame(closure.as_ref().unchecked_ref())
        .expect("request_animation_frame");
    closure.forget();
}

fn flush_scheduled_redraw(state: Rc<RefCell<AppState>>) {
    let should_draw = {
        let mut s = state.borrow_mut();
        s.redraw_raf_pending = false;
        if !s.redraw_dirty {
            false
        } else {
            let snap = draw_snapshot(&s);
            if s.last_draw_snapshot == Some(snap) {
                s.redraw_dirty = false;
                false
            } else {
                s.redraw_dirty = false;
                true
            }
        }
    };
    if should_draw {
        redraw_and_sample(&state);
    }
    if state.borrow().redraw_dirty {
        schedule_redraw(state);
    }
}

#[derive(Deserialize)]
struct MapBoundsResponse {
    width: i32,
    height: i32,
    cell_count: u32,
}

#[derive(Deserialize)]
struct MapResponse {
    #[allow(dead_code)]
    world_id: String,
    bounds: MapBoundsResponse,
    legacy_map: bool,
    cells: Vec<CellSummary>,
}

#[derive(Deserialize)]
struct CellSummary {
    q: i32,
    r: i32,
    display_name: String,
}

#[derive(Serialize)]
struct ProfileInput {
    display_name: String,
    notes: String,
}

#[derive(Deserialize)]
struct ProjectEntry {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    path: String,
}

#[derive(Deserialize)]
struct ProjectStatus {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    path: String,
    valid: bool,
    legacy_map: bool,
    build_draft: bool,
    build_step: Option<u32>,
}

#[derive(Deserialize)]
struct ProjectsResponse {
    active: Option<ProjectEntry>,
    projects: Vec<ProjectStatus>,
    default_worlds_root: String,
}

#[derive(Serialize)]
struct BuildStateInput {
    status: &'static str,
    step: u32,
}

#[derive(Serialize)]
struct WizardLandMaskGenerateInput<'a> {
    recipe_id: &'a str,
    character: &'a str,
    variant: &'a str,
    regenerate_nonce: u32,
}

#[derive(Serialize)]
struct WizardGeologyGenerateInput<'a> {
    style: &'a str,
    regenerate_nonce: u32,
}

#[derive(Serialize)]
struct WizardElevationGenerateInput {
    style: &'static str,
}

#[derive(Serialize)]
struct WizardLandMaskCellInput<'a> {
    q: i32,
    r: i32,
    kind: &'a str,
}

#[derive(Serialize)]
struct CreateProjectInput<'a> {
    id: &'a str,
    path: &'a str,
    map_preset: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_wizard: Option<bool>,
}

#[derive(Serialize)]
struct OpenProjectInput<'a> {
    path: &'a str,
}

#[derive(Serialize)]
struct ForgetProjectInput<'a> {
    path: &'a str,
}

#[derive(Serialize)]
struct DeleteProjectInput<'a> {
    path: &'a str,
}

/// One cell in a generic layer batch (`PUT /api/layers/:id/batch`), matching
/// `core::layer::LayerCellWrite` + `WireCellState::Value`. Elevation paints are
/// always concrete integer values (scale-layers, D-46).
#[derive(Serialize)]
struct LayerCellWrite {
    q: i32,
    r: i32,
    state: &'static str,
    value: i32,
}

/// Active editing tool. `Inspect` keeps the old click→profile behavior; the
/// hydro brushes paint elevation-driven hydro (`land`/`water`) instead.
#[derive(Clone)]
enum Brush {
    Inspect,
    SetLand,
    SetWater,
    Raise,
    Lower,
    /// river-overlay-layer-v1: chain-click polyline brush.
    River,
    RiverErase,
}

struct AppState {
    /// Cells that have an author profile (used for the profile-presence marker).
    cells: HashMap<(i32, i32), String>,
    /// Index-addressed elevation layer — primary render cache (perf-100k--web-dense-client).
    elevation: DenseLayer,
    brush: Brush,
    /// Last terrain brush — restored when reopening Terrain tab.
    last_paint_brush: Brush,
    /// Last river brush — restored when reopening Rivers tab.
    last_river_brush: Brush,
    /// Active river chain id (`None` = next click starts a new river).
    active_river_id: Option<u32>,
    /// River catalog mirror (river-overlay-layer-v1).
    rivers: RiverCatalog,
    selected: Option<(i32, i32)>,
    /// Hex bounds from `map/manifest.json` (via `/api/map`).
    map_bounds: MapBounds,
    /// Camera zoom multiplier over fit-to-window base size.
    zoom: f64,
    /// Camera pan offset in screen pixels.
    pan_x: f64,
    pan_y: f64,
    /// Drag-pan interaction state.
    drag_active: bool,
    drag_moved: bool,
    drag_last_x: f64,
    drag_last_y: f64,
    /// Drag-paint interaction state (Land/Water brush).
    paint_active: bool,
    paint_moved: bool,
    paint_last_cell: Option<(i32, i32)>,
    /// Brush radius in hex cells (0 = single cell).
    brush_radius: i32,
    /// Raise/Lower step magnitude (1, 5, or 10).
    brush_step: i32,
    /// Even falloff vs hill gradient for Raise/Lower.
    falloff_even: bool,
    color_mode: ColorMode,
    show_elevation_labels: bool,
    show_peaks: bool,
    /// Local paint writes not yet persisted to server.
    pending_paints: HashMap<(i32, i32), i32>,
    paint_flush_scheduled: bool,
    paint_flush_in_flight: bool,
    hover_cell: Option<(i32, i32)>,
    /// Draw hex-cell borders over fills.
    show_grid: bool,
    suppress_next_click: bool,
    legacy_map: bool,
    default_worlds_root: Option<String>,
    path_touched: bool,
    build_path_touched: bool,
    /// Step 0 perf samples (perf-100k--measurement-hooks).
    perf: PerfMetrics,
    perf_timers: PerfTimers,
    /// Visual revision — bumps when elevation/cells change (perf-100k--raf-redraw-coalesce).
    content_rev: u64,
    last_draw_snapshot: Option<DrawSnapshot>,
    redraw_dirty: bool,
    redraw_raf_pending: bool,
    wizard_character: String,
    /// Selected layout class id (D-65: six cards always visible).
    wizard_layout_class: String,
    wizard_regenerate_nonce: u32,
    /// Active recipe within the selected class.
    wizard_recipe_id: String,
    wizard_accepted: bool,
    wizard_edit_mode: bool,
    wizard_edit_brush: String,
    /// Build wizard step: 3 silhouette · 4 tectonics · 5 elevation.
    wizard_step: u32,
    wizard_geo_style: String,
    wizard_geo_nonce: u32,
    wizard_geo_accepted: bool,
    /// Dense geology cache for tint overlay (index → palette string).
    geology: Option<DenseLayer>,
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();

    let initial_bounds = MapPreset::Small.bounds();
    let state = Rc::new(RefCell::new(AppState {
        cells: HashMap::new(),
        elevation: fresh_elevation_layer(initial_bounds),
        brush: Brush::Inspect,
        last_paint_brush: Brush::SetLand,
        last_river_brush: Brush::River,
        active_river_id: None,
        rivers: RiverCatalog::default(),
        selected: None,
        map_bounds: initial_bounds,
        zoom: 1.0,
        pan_x: 0.0,
        pan_y: 0.0,
        drag_active: false,
        drag_moved: false,
        drag_last_x: 0.0,
        drag_last_y: 0.0,
        paint_active: false,
        paint_moved: false,
        paint_last_cell: None,
        brush_radius: 0,
        brush_step: 1,
        falloff_even: true,
        color_mode: ColorMode::Hydro,
        show_elevation_labels: false,
        show_peaks: false,
        pending_paints: HashMap::new(),
        paint_flush_scheduled: false,
        paint_flush_in_flight: false,
        hover_cell: None,
        show_grid: false,
        suppress_next_click: false,
        legacy_map: false,
        default_worlds_root: None,
        path_touched: false,
        build_path_touched: false,
        perf: PerfMetrics::default(),
        perf_timers: PerfTimers::default(),
        content_rev: 0,
        last_draw_snapshot: None,
        redraw_dirty: false,
        redraw_raf_pending: false,
        wizard_character: "smooth".to_string(),
        wizard_layout_class: "pangea".to_string(),
        wizard_regenerate_nonce: 0,
        wizard_recipe_id: String::new(),
        wizard_accepted: false,
        wizard_edit_mode: false,
        wizard_edit_brush: "land".to_string(),
        wizard_step: 3,
        wizard_geo_style: "belts".to_string(),
        wizard_geo_nonce: 0,
        wizard_geo_accepted: false,
        geology: None,
    }));

    redraw(&state.borrow());
    attach_canvas_click(state.clone());
    attach_save_click(state.clone());
    attach_close_click(state.clone());
    attach_switch_world_click(state.clone());
    attach_create_click(state.clone());
    attach_build_start_click(state.clone());
    attach_wizard_handlers(state.clone());
    attach_generate_rivers_click(state.clone());
    attach_preset_warn_handlers();
    attach_project_list_click(state.clone());
    attach_dock_click(state.clone());
    attach_escape_key();
    attach_pan_drag(state.clone());
    attach_paint_drag(state.clone());
    attach_brush_hover_preview(state.clone());
    attach_wheel_zoom(state.clone());
    attach_window_resize(state.clone());
    attach_browse_folder_click(state.clone());
    attach_new_id_input(state.clone());
    attach_new_path_input(state.clone());
    attach_generate_id_input(state.clone());
    attach_generate_path_input(state.clone());

    wasm_bindgen_futures::spawn_local(refresh_projects(state));
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

fn document() -> web_sys::Document {
    window().document().expect("no document")
}

/// Click target may be a text node inside a button — walk up to the element.
fn click_target_element(event: &web_sys::MouseEvent) -> Option<web_sys::Element> {
    let target = event.target()?;
    if let Ok(el) = target.clone().dyn_into::<web_sys::Element>() {
        return Some(el);
    }
    target.dyn_into::<web_sys::Node>().ok()?.parent_element()
}

fn canvas() -> HtmlCanvasElement {
    document()
        .get_element_by_id("map")
        .expect("#map canvas missing")
        .dyn_into::<HtmlCanvasElement>()
        .expect("#map is not a canvas")
}

fn context() -> CanvasRenderingContext2d {
    canvas()
        .get_context("2d")
        .ok()
        .flatten()
        .expect("no 2d context")
        .dyn_into::<CanvasRenderingContext2d>()
        .expect("context is not 2d")
}

fn input(id: &str) -> HtmlInputElement {
    document()
        .get_element_by_id(id)
        .expect("missing input")
        .dyn_into()
        .expect("not an input")
}

fn textarea(id: &str) -> HtmlTextAreaElement {
    document()
        .get_element_by_id(id)
        .expect("missing textarea")
        .dyn_into()
        .expect("not a textarea")
}

fn set_text(id: &str, text: &str) {
    if let Some(el) = document().get_element_by_id(id) {
        el.set_text_content(Some(text));
    }
}

fn set_drawer_open(open: bool) {
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if open {
            let _ = drawer.class_list().remove_1("collapsed");
        } else {
            let _ = drawer.class_list().add_1("collapsed");
        }
    }
}

fn drawer_is_open() -> bool {
    document()
        .get_element_by_id("dock-drawer")
        .is_some_and(|drawer| !drawer.class_list().contains("collapsed"))
}

fn set_dock_tab(tab: &str) {
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(panes) = drawer.query_selector_all("[data-drawer]") {
            for i in 0..panes.length() {
                if let Some(node) = panes.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let active = el.get_attribute("data-drawer").as_deref() == Some(tab);
                        if active {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().remove_1("active");
                        }
                    }
                }
            }
        }
    }
    if let Some(rail) = document().get_element_by_id("dock-rail") {
        if let Ok(items) = rail.query_selector_all("[data-dock]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let active = el.get_attribute("data-dock").as_deref() == Some(tab);
                        if active {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().remove_1("active");
                        }
                    }
                }
            }
        }
    }
}

fn clear_pointer_interaction(s: &mut AppState) {
    s.drag_active = false;
    s.drag_moved = false;
    s.paint_active = false;
    s.paint_moved = false;
    s.paint_last_cell = None;
    s.suppress_next_click = false;
}

fn apply_paint_brush(s: &mut AppState, brush: Brush) {
    if terrain_brush(&brush) {
        s.last_paint_brush = brush.clone();
    }
    if river_brush(&brush) {
        s.last_river_brush = brush.clone();
    }
    s.brush = brush;
    s.hover_cell = None;
    clear_pointer_interaction(s);
}

fn terrain_brush(brush: &Brush) -> bool {
    matches!(
        brush,
        Brush::SetLand | Brush::SetWater | Brush::Raise | Brush::Lower
    )
}

fn river_brush(brush: &Brush) -> bool {
    matches!(brush, Brush::River | Brush::RiverErase)
}

fn active_dock_tab() -> Option<String> {
    document().get_element_by_id("dock-rail").and_then(|rail| {
        rail.query_selector("[data-dock].active")
            .ok()
            .flatten()
            .and_then(|el| el.get_attribute("data-dock"))
    })
}

/// tool-dock-brush-deselect-v1: leave paint mode — pan / inspect on canvas.
fn deactivate_paint_brush(s: &mut AppState) {
    s.brush = Brush::Inspect;
    s.hover_cell = None;
    clear_pointer_interaction(s);
}

fn sync_paint_tool_ui(brush: &Brush) {
    sync_dock_rail_for_brush(brush);
    sync_brush_swatch_active(brush);
}

fn sync_dock_rail_for_brush(brush: &Brush) {
    let terrain_active = terrain_brush(brush);
    let rivers_active = river_brush(brush);
    if let Some(rail) = document().get_element_by_id("dock-rail") {
        if let Ok(items) = rail.query_selector_all("[data-dock]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let dock = el.get_attribute("data-dock").unwrap_or_default();
                        let tool_active = (dock == "inspect" && !terrain_active && !rivers_active)
                            || (dock == "terrain" && terrain_active)
                            || (dock == "rivers" && rivers_active);
                        if tool_active {
                            let _ = el.class_list().add_1("tool-active");
                        } else {
                            let _ = el.class_list().remove_1("tool-active");
                        }
                    }
                }
            }
        }
    }
}

fn sync_brush_swatch_active(brush: &Brush) {
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(items) = drawer.query_selector_all("[data-brush]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let _ = el.class_list().remove_1("active");
                    }
                }
            }
        }
    }
    let kind = match brush {
        Brush::Inspect => return,
        Brush::SetLand => "land".to_string(),
        Brush::SetWater => "water".to_string(),
        Brush::Raise => "raise".to_string(),
        Brush::Lower => "lower".to_string(),
        Brush::River => "river".to_string(),
        Brush::RiverErase => "river-erase".to_string(),
    };
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(Some(button)) = drawer.query_selector(&format!("[data-brush=\"{kind}\"]")) {
            let _ = button.class_list().add_1("active");
        }
    }
}

fn sync_brush_radius_active(radius: i32) {
    let radius = radius.clamp(MIN_BRUSH_RADIUS, MAX_BRUSH_RADIUS);
    set_text("brush-size-value", &(radius + 1).to_string());
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(items) = drawer.query_selector_all("[data-brush-size]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let is_active = el
                            .get_attribute("data-brush-size")
                            .and_then(|v| v.parse::<i32>().ok())
                            .map(|v| v == radius)
                            .unwrap_or(false);
                        if is_active {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().remove_1("active");
                        }
                    }
                }
            }
        }
    }
}

fn draw_preview_boundary(
    state: &AppState,
    ctx: &CanvasRenderingContext2d,
    size: f64,
    ox: f64,
    oy: f64,
) {
    if matches!(state.brush, Brush::Inspect) || river_brush(&state.brush) {
        return;
    }
    let Some(center) = state.hover_cell else {
        return;
    };
    let cells = paint_stamp_cells(center, state.brush_radius, state.map_bounds);
    if cells.is_empty() {
        return;
    }
    let cell_set: HashSet<(i32, i32)> = cells.iter().copied().collect();
    ctx.set_line_width(2.0);
    ctx.set_stroke_style_str("#9fe3c4");
    for (q, r) in cells {
        let axial = Axial::new(q, r);
        let is_boundary = axial
            .neighbors()
            .iter()
            .any(|n| !cell_set.contains(&(n.q, n.r)));
        if !is_boundary {
            continue;
        }
        let (x, y) = axial.to_pixel(size);
        let corners = hex_corners(ox + x, oy + y, size * BRUSH_PREVIEW_GAP * 0.98);
        ctx.begin_path();
        ctx.move_to(corners[0].0, corners[0].1);
        for corner in &corners[1..] {
            ctx.line_to(corner.0, corner.1);
        }
        ctx.close_path();
        ctx.stroke();
    }
}

/// river-overlay-layer-v1: straight center-to-center polyline strokes.
fn draw_rivers(state: &AppState, ctx: &CanvasRenderingContext2d, size: f64, ox: f64, oy: f64) {
    if state.rivers.rivers.is_empty() {
        return;
    }
    ctx.set_stroke_style_str("#4da6ff");
    ctx.set_fill_style_str("#4da6ff");
    ctx.set_line_width(2.0);
    ctx.set_line_cap("round");
    ctx.set_line_join("round");
    let bounds = state.map_bounds;
    let dot_r = (size * 0.12).clamp(2.0, 6.0);
    for river in &state.rivers.rivers {
        if river.cells.is_empty() {
            continue;
        }
        if river.cells.len() == 1 {
            let Some(cell) = bounds.from_index(river.cells[0]) else {
                continue;
            };
            let (x, y) = cell.to_pixel(size);
            ctx.begin_path();
            let _ = ctx.arc(ox + x, oy + y, dot_r, 0.0, std::f64::consts::TAU);
            ctx.fill();
            continue;
        }
        ctx.begin_path();
        let mut started = false;
        for &idx in &river.cells {
            let Some(cell) = bounds.from_index(idx) else {
                continue;
            };
            let (x, y) = cell.to_pixel(size);
            let (cx, cy) = (ox + x, oy + y);
            if started {
                ctx.line_to(cx, cy);
            } else {
                ctx.move_to(cx, cy);
                started = true;
            }
        }
        if started {
            ctx.stroke();
        }
    }
}

fn sync_river_status(state: &AppState) {
    let active = state
        .active_river_id
        .map(|id| format!("River #{id}"))
        .unwrap_or_else(|| "New river on next click".to_string());
    let count = state.rivers.rivers.len();
    set_text(
        "river-status",
        &format!("{active} · {count} river(s) on map"),
    );
}

fn open_dock_tab(tab: &str) {
    set_dock_tab(tab);
    set_drawer_open(true);
}

fn toggle_dock_tab(tab: &str) {
    if drawer_is_open() {
        let current = document().get_element_by_id("dock-rail").and_then(|rail| {
            rail.query_selector("[data-dock].active")
                .ok()
                .flatten()
                .and_then(|el| el.get_attribute("data-dock"))
        });
        if current.as_deref() == Some(tab) {
            set_drawer_open(false);
            return;
        }
    }
    open_dock_tab(tab);
}

fn clear_inspect_selection() {
    set_text("panel-cell", "—");
    input("title").set_value("");
    textarea("notes").set_value("");
    input("title").set_disabled(true);
    textarea("notes").set_disabled(true);
    set_text("status", "");
}

fn set_world_label(world_id: &str) {
    set_text("world-name", world_id);
}

/// Toggle between the Home (project picker) and Editor (hex map) screens.
fn show_view(name: &str) {
    for id in ["home", "editor"] {
        if let Some(el) = document().get_element_by_id(id) {
            if id == name {
                let _ = el.class_list().add_1("active");
            } else {
                let _ = el.class_list().remove_1("active");
            }
        }
    }
}

fn build_step_label(step: u32) -> &'static str {
    match step {
        4 => "Step 4 · Tectonics",
        5 => "Step 5 · Elevation",
        _ => "Step 3 · Land silhouette",
    }
}

// home-build-draft-v1: persist wizard draft on active world.
async fn persist_build_draft(step: u32) -> bool {
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
fn set_wizard_active(active: bool) {
    let Some(editor) = document().get_element_by_id("editor") else {
        return;
    };
    if active {
        let _ = editor.class_list().add_1("wizard-active");
        set_drawer_open(false);
    } else {
        let _ = editor.class_list().remove_1("wizard-active");
        set_wizard_status("");
    }
}

fn open_build_wizard() {
    set_wizard_active(true);
}

fn close_build_wizard() {
    set_wizard_active(false);
}

fn set_wizard_status(msg: &str) {
    set_text("wizard-status", msg);
}

fn wizard_is_active() -> bool {
    document()
        .get_element_by_id("editor")
        .is_some_and(|el| el.class_list().contains("wizard-active"))
}

fn set_panel_hidden(id: &str, hidden: bool) {
    let Some(el) = document().get_element_by_id(id) else {
        return;
    };
    if hidden {
        let _ = el.class_list().add_1("hidden");
    } else {
        let _ = el.class_list().remove_1("hidden");
    }
}

fn sync_wizard_nav(step: u32) {
    if let Some(crumb) = document().query_selector(".wiz-crumb").ok().flatten() {
        let text = match step {
            4 => "Geo › Tectonics",
            5 => "Geo › Elevation",
            _ => "Geo › Land silhouette",
        };
        crumb.set_text_content(Some(text));
    }
    if let Ok(Some(list)) = document().query_selector(".wiz-steps") {
        if let Ok(items) = list.query_selector_all(".wiz-step") {
            for i in 0..items.length() {
                let Some(node) = items.item(i) else {
                    continue;
                };
                let Ok(el) = node.dyn_into::<web_sys::Element>() else {
                    continue;
                };
                let _ = el.class_list().remove_1("active");
                let _ = el.class_list().remove_1("done");
                let _ = el.class_list().remove_1("locked");
                // items: 0=size,1=grid,2=land,3=tectonics,4=elev,5=coasts
                let step_num = i + 1;
                if step_num < step {
                    let _ = el.class_list().add_1("done");
                } else if step_num == step {
                    let _ = el.class_list().add_1("active");
                } else {
                    let _ = el.class_list().add_1("locked");
                }
            }
        }
    }
}

fn show_wizard_step(state: &AppState) {
    let step = state.wizard_step;
    set_panel_hidden("wiz-panel-step3", step != 3);
    set_panel_hidden("wiz-panel-step4", step != 4);
    set_panel_hidden("wiz-panel-step5", step != 5);
    sync_wizard_nav(step);
    match step {
        4 => set_wizard_status("Step 4: generate geology, accept, continue."),
        5 => set_wizard_status("Step 5: generate elevation from land + geology, then Finish."),
        _ => set_wizard_status("Step 3 flow: 1) parameters, 2) generate, 3) accept/edit, 4) continue."),
    }
}

fn set_button_disabled(id: &str, disabled: bool) {
    let Some(el) = document().get_element_by_id(id) else {
        return;
    };
    if let Ok(btn) = el.dyn_into::<HtmlElement>() {
        if disabled {
            let _ = btn.set_attribute("disabled", "");
            let _ = btn.class_list().add_1("wiz-disabled");
        } else {
            let _ = btn.remove_attribute("disabled");
            let _ = btn.class_list().remove_1("wiz-disabled");
        }
    }
}

fn sync_wizard_layout_buttons(active: &str) {
    let Ok(Some(root)) = document().query_selector("#wiz-layout-classes") else {
        return;
    };
    let Ok(nodes) = root.query_selector_all("[data-wiz-layout]") else {
        return;
    };
    for i in 0..nodes.length() {
        let Some(node) = nodes.item(i) else {
            continue;
        };
        let Ok(el) = node.dyn_into::<Element>() else {
            continue;
        };
        let is_active = el.get_attribute("data-wiz-layout").as_deref() == Some(active);
        if is_active {
            let _ = el.class_list().add_1("active");
        } else {
            let _ = el.class_list().remove_1("active");
        }
    }
}

/// Ensure recipe_id matches selected layout class (D-65).
fn ensure_wizard_recipe(state: &mut AppState) {
    if !state.wizard_recipe_id.is_empty() {
        if let Some(recipe) = find_recipe(&state.wizard_recipe_id) {
            if recipe.layout_class.id() == state.wizard_layout_class {
                return;
            }
        }
    }
    let class = LayoutClass::parse(&state.wizard_layout_class);
    let seed = (state.wizard_regenerate_nonce as u64).wrapping_mul(0x9E37_79B9) ^ 0xC0FF_EE;
    let recipe = pick_recipe(class, seed);
    state.wizard_layout_class = class.id().to_string();
    state.wizard_recipe_id = recipe.id.to_string();
}

fn pick_wizard_recipe_for_class(state: &mut AppState, class_id: &str) {
    let class = LayoutClass::parse(class_id);
    let seed = (state.wizard_regenerate_nonce as u64).wrapping_mul(0x9E37_79B9) ^ 0xC0FF_EE;
    let recipe = pick_recipe(class, seed);
    state.wizard_layout_class = class.id().to_string();
    state.wizard_recipe_id = recipe.id.to_string();
}

fn rotate_wizard_recipe(state: &mut AppState) {
    let class = LayoutClass::parse(&state.wizard_layout_class);
    let seed = (state.wizard_regenerate_nonce as u64).wrapping_mul(0x9E37_79B9) ^ 0xBEEF;
    let recipe = next_recipe(class, &state.wizard_recipe_id, seed);
    state.wizard_layout_class = class.id().to_string();
    state.wizard_recipe_id = recipe.id.to_string();
}

fn sync_wizard_edit_mode_ui(edit_mode: bool, brush: &str) {
    if let Some(row) = document().get_element_by_id("wiz-edit-brushes") {
        if edit_mode {
            let _ = row.class_list().remove_1("hidden");
        } else {
            let _ = row.class_list().add_1("hidden");
        }
    }
    for (id, kind) in [("wiz-edit-land", "land"), ("wiz-edit-ocean", "ocean")] {
        let Some(el) = document().get_element_by_id(id) else {
            continue;
        };
        if kind == brush {
            let _ = el.class_list().add_1("active");
        } else {
            let _ = el.class_list().remove_1("active");
        }
    }
}

fn sync_wizard_actions(state: &AppState) {
    set_button_disabled("wiz-regenerate", false);
    set_button_disabled("wiz-accept", false);
    set_button_disabled("wiz-edit", !state.wizard_accepted);
    set_button_disabled("wiz-continue", !state.wizard_accepted);
    set_button_disabled("wiz-geo-continue", !state.wizard_geo_accepted);
    sync_wizard_layout_buttons(&state.wizard_layout_class);
    sync_wizard_edit_mode_ui(state.wizard_edit_mode, &state.wizard_edit_brush);
    show_wizard_step(state);
}

async fn generate_wizard_land_mask(state: Rc<RefCell<AppState>>) {
    set_wizard_generating(true);
    set_wizard_status("Generating silhouette… (can take a moment on large maps)");
    let (recipe_id, character, layout_class, nonce) = {
        let mut s = state.borrow_mut();
        ensure_wizard_recipe(&mut s);
        (
            s.wizard_recipe_id.clone(),
            s.wizard_character.clone(),
            s.wizard_layout_class.clone(),
            s.wizard_regenerate_nonce,
        )
    };
    let body = WizardLandMaskGenerateInput {
        recipe_id: &recipe_id,
        character: &character,
        variant: &layout_class,
        regenerate_nonce: nonce,
    };
    let Ok(resp) = gloo_net::http::Request::post("/api/build/land-mask/generate")
        .json(&body)
        .expect("serialize wizard generate")
        .send()
        .await
    else {
        set_wizard_generating(false);
        {
            let s = state.borrow();
            sync_wizard_actions(&s);
        }
        set_wizard_status("Generation failed (network).");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Generation rejected".to_string());
        set_wizard_generating(false);
        {
            let s = state.borrow();
            sync_wizard_actions(&s);
        }
        set_wizard_status(&msg);
        return;
    }
    load_elevation(&state).await;
    schedule_redraw(state.clone());
    set_wizard_generating(false);
    {
        let s = state.borrow();
        sync_wizard_actions(&s);
    }
    set_wizard_status("Shape generated.");
}

fn set_wizard_generating(busy: bool) {
    set_button_disabled("wiz-regenerate", busy);
    set_button_disabled("wiz-accept", busy);
    if let Some(el) = document().get_element_by_id("wiz-layout-classes") {
        if busy {
            let _ = el.class_list().add_1("wiz-busy");
            let _ = el.set_attribute("aria-busy", "true");
        } else {
            let _ = el.class_list().remove_1("wiz-busy");
            let _ = el.remove_attribute("aria-busy");
        }
    }
    if let Some(el) = document().get_element_by_id("wizard-status") {
        if busy {
            let _ = el.class_list().add_1("busy");
        } else {
            let _ = el.class_list().remove_1("busy");
        }
    }
}

async fn generate_wizard_geology(state: Rc<RefCell<AppState>>) {
    let (style, nonce) = {
        let s = state.borrow();
        (s.wizard_geo_style.clone(), s.wizard_geo_nonce)
    };
    let body = WizardGeologyGenerateInput {
        style: &style,
        regenerate_nonce: nonce,
    };
    let Ok(resp) = gloo_net::http::Request::post("/api/build/geology/generate")
        .json(&body)
        .expect("serialize geology generate")
        .send()
        .await
    else {
        set_wizard_status("Geology generation failed (network).");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Geology generation rejected".to_string());
        set_wizard_status(&msg);
        return;
    }
    load_geology(&state).await;
    {
        let mut s = state.borrow_mut();
        s.wizard_geo_accepted = false;
        sync_wizard_actions(&s);
    }
    schedule_redraw(state.clone());
    set_wizard_status("Geology generated — tint shows belts on land.");
}

async fn load_geology(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/layers/geology").send().await else {
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

async fn generate_wizard_elevation(state: Rc<RefCell<AppState>>) {
    let body = WizardElevationGenerateInput { style: "default" };
    let Ok(resp) = gloo_net::http::Request::post("/api/build/elevation/generate")
        .json(&body)
        .expect("serialize elevation generate")
        .send()
        .await
    else {
        set_wizard_status("Elevation generation failed (network).");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Elevation generation rejected".to_string());
        set_wizard_status(&msg);
        return;
    }
    load_elevation(&state).await;
    schedule_redraw(state.clone());
    set_wizard_status("Elevation generated from land + geology.");
}

async fn wizard_set_land_mask_cell(state: Rc<RefCell<AppState>>, q: i32, r: i32, kind: String) {
    let payload = vec![WizardLandMaskCellInput { q, r, kind: &kind }];
    let Ok(resp) = gloo_net::http::Request::put("/api/build/land-mask/cells")
        .json(&payload)
        .expect("serialize wizard land_mask cell")
        .send()
        .await
    else {
        set_wizard_status("Edit save failed.");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Edit rejected".to_string());
        set_wizard_status(&msg);
        return;
    }
    // local preview + sync with server write model
    {
        let mut s = state.borrow_mut();
        let bounds = s.map_bounds;
        let value = if kind == "land" { 1 } else { 0 };
        set_elevation_cell(&mut s.elevation, bounds, q, r, value);
        bump_content_rev(&mut s);
    }
    schedule_redraw(state);
}

fn wiz_toggle_style_group(container_id: &str, attr: &str, active: &web_sys::Element) {
    let Ok(Some(root)) = document().query_selector(&format!("#{container_id}")) else {
        return;
    };
    if let Ok(list) = root.query_selector_all(&format!("[{attr}]")) {
        for i in 0..list.length() {
            if let Some(node) = list.item(i) {
                if let Ok(el) = node.dyn_into::<web_sys::Element>() {
                    let _ = el.class_list().remove_1("active");
                }
            }
        }
    }
    let _ = active.class_list().add_1("active");
}

fn attach_wizard_handlers(state: Rc<RefCell<AppState>>) {
    // Save Draft — flush pending paints; world already on disk from create.
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            set_wizard_status("Saving…");
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                flush_pending_paints(state.clone()).await;
                if persist_build_draft(state.borrow().wizard_step.max(3)).await {
                    set_wizard_status("Draft saved.");
                } else {
                    set_wizard_status("Could not save draft.");
                }
            });
        });
        document()
            .get_element_by_id("wiz-save-draft")
            .expect("missing #wiz-save-draft")
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .expect("wiz-save-draft");
        closure.forget();
    }

    // ← Worlds — close wizard and return Home (same as tool-dock switch-world).
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            close_build_wizard();
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(wizard_return_home(state));
        });
        document()
            .get_element_by_id("wiz-worlds")
            .expect("missing #wiz-worlds")
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .expect("wiz-worlds");
        closure.forget();
    }

    // Shore character (block 1).
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            if !el.class_list().contains("wiz-style-btn") {
                return;
            }
            if let Some(character) = el.get_attribute("data-wiz-char") {
                wiz_toggle_style_group("wiz-chars", "data-wiz-char", &el);
                state.borrow_mut().wizard_character = character;
                set_wizard_status("Shore updated. Regenerate or pick a layout class.");
            }
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-chars") {
            let _ = root
                .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }

    // Layout class cards (D-65): pick class → recipe for that class → generate.
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            if !el.class_list().contains("wiz-style-btn") {
                return;
            }
            let Some(class_id) = el.get_attribute("data-wiz-layout") else {
                return;
            };
            {
                let mut s = state.borrow_mut();
                pick_wizard_recipe_for_class(&mut s, &class_id);
                s.wizard_accepted = false;
                s.wizard_edit_mode = false;
                sync_wizard_actions(&s);
            }
            set_wizard_status("Generating selected class…");
            wasm_bindgen_futures::spawn_local(generate_wizard_land_mask(state.clone()));
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-layout-classes") {
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }

    // Regenerate: rotate recipe within selected class only (D-65).
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            {
                let mut s = state.borrow_mut();
                s.wizard_regenerate_nonce = s.wizard_regenerate_nonce.saturating_add(1);
                rotate_wizard_recipe(&mut s);
                s.wizard_accepted = false;
                s.wizard_edit_mode = false;
                sync_wizard_actions(&s);
            }
            set_wizard_status("New shape for selected class…");
            wasm_bindgen_futures::spawn_local(generate_wizard_land_mask(state.clone()));
        });
        document()
            .get_element_by_id("wiz-regenerate")
            .expect("missing #wiz-regenerate")
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .expect("wiz-regenerate");
        closure.forget();
    }

    // Accept currently visible variant.
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            {
                let mut s = state.borrow_mut();
                s.wizard_accepted = true;
                s.wizard_edit_mode = false;
                sync_wizard_actions(&s);
            }
            set_wizard_status("Silhouette accepted. You can Edit or Continue.");
        });
        document()
            .get_element_by_id("wiz-accept")
            .expect("missing #wiz-accept")
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .expect("wiz-accept");
        closure.forget();
    }

    // Enter/exit manual edit mode.
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let mut s = state.borrow_mut();
            if !s.wizard_accepted {
                set_wizard_status("Accept a silhouette first.");
                return;
            }
            s.wizard_edit_mode = !s.wizard_edit_mode;
            let edit_now = s.wizard_edit_mode;
            sync_wizard_actions(&s);
            if edit_now {
                set_wizard_status("Edit mode: click map cells to paint land/ocean.");
            } else {
                set_wizard_status("Edit mode off.");
            }
        });
        document()
            .get_element_by_id("wiz-edit")
            .expect("missing #wiz-edit")
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .expect("wiz-edit");
        closure.forget();
    }

    // Land/ocean brush toggle in edit mode.
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            if !el.class_list().contains("wiz-style-btn") {
                return;
            }
            let Some(kind) = el.get_attribute("data-wiz-edit-brush") else {
                return;
            };
            let mut s = state.borrow_mut();
            s.wizard_edit_brush = kind;
            sync_wizard_actions(&s);
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-edit-brushes") {
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }

    // Continue: step 3 → draft step 4 (tectonics).
    for id in ["wiz-continue"] {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let accepted = state.borrow().wizard_accepted;
            if !accepted {
                set_wizard_status("Accept a silhouette first.");
                return;
            }
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if !persist_build_draft(4).await {
                    set_wizard_status("Could not advance to tectonics.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 4;
                    s.wizard_edit_mode = false;
                    s.wizard_geo_accepted = false;
                    s.wizard_geo_nonce = 0;
                    sync_wizard_actions(&s);
                }
                set_wizard_status("Step 4 · Tectonics — generate geology.");
                wasm_bindgen_futures::spawn_local(generate_wizard_geology(state.clone()));
            });
        });
        document()
            .get_element_by_id(id)
            .expect("missing wizard finalize button")
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .expect("attach wizard finalize");
        closure.forget();
    }

    // Step 4 geology style / generate / accept / continue.
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            if let Some(style) = el.get_attribute("data-wiz-geo-style") {
                wiz_toggle_style_group("wiz-geo-styles", "data-wiz-geo-style", &el);
                state.borrow_mut().wizard_geo_style = style;
                set_wizard_status("Geology style updated — Generate to apply.");
            }
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-geo-styles") {
            let _ = root
                .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            {
                let mut s = state.borrow_mut();
                s.wizard_geo_nonce = s.wizard_geo_nonce.saturating_add(1);
                s.wizard_geo_accepted = false;
                sync_wizard_actions(&s);
            }
            set_wizard_status("Generating geology…");
            wasm_bindgen_futures::spawn_local(generate_wizard_geology(state.clone()));
        });
        if let Some(btn) = document().get_element_by_id("wiz-geo-generate") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            if state.borrow().geology.is_none() {
                set_wizard_status("Generate geology first.");
                return;
            }
            let mut s = state.borrow_mut();
            s.wizard_geo_accepted = true;
            sync_wizard_actions(&s);
            set_wizard_status("Geology accepted. Continue to elevation.");
        });
        if let Some(btn) = document().get_element_by_id("wiz-geo-accept") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            if !state.borrow().wizard_geo_accepted {
                set_wizard_status("Accept geology first.");
                return;
            }
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if !persist_build_draft(5).await {
                    set_wizard_status("Could not advance to elevation.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 5;
                    sync_wizard_actions(&s);
                }
                wasm_bindgen_futures::spawn_local(generate_wizard_elevation(state.clone()));
            });
        });
        if let Some(btn) = document().get_element_by_id("wiz-geo-continue") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            set_wizard_status("Generating elevation…");
            wasm_bindgen_futures::spawn_local(generate_wizard_elevation(state.clone()));
        });
        if let Some(btn) = document().get_element_by_id("wiz-elev-generate") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let body = BuildStateInput {
                    status: "complete",
                    step: 5,
                };
                let Ok(resp) = gloo_net::http::Request::put("/api/build")
                    .json(&body)
                    .expect("serialize build complete")
                    .send()
                    .await
                else {
                    set_wizard_status("Could not finish build.");
                    return;
                };
                if !resp.ok() {
                    set_wizard_status("Could not finish build.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 3;
                    s.wizard_accepted = false;
                    s.wizard_geo_accepted = false;
                    s.wizard_edit_mode = false;
                    sync_wizard_actions(&s);
                }
                close_build_wizard();
                set_wizard_status("Build finished — edit elevation in the editor.");
            });
        });
        if let Some(btn) = document().get_element_by_id("wiz-finish") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }

    // Geo group collapse toggle (shell: only group with steps).
    {
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(head) = click_target_element(&mouse) else {
                return;
            };
            if head.get_attribute("data-wiz-group").as_deref() != Some("geo") {
                return;
            }
            let Some(group) = head.closest(".wiz-group").ok().flatten() else {
                return;
            };
            let expanded = group.class_list().contains("expanded");
            if expanded {
                let _ = group.class_list().remove_1("expanded");
                head.set_text_content(Some("▶ Geo"));
            } else {
                let _ = group.class_list().add_1("expanded");
                head.set_text_content(Some("▼ Geo"));
            }
        });
        if let Ok(Some(left)) = document().query_selector(".wiz-left") {
            let _ =
                left.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }

    {
        let mut s = state.borrow_mut();
        ensure_wizard_recipe(&mut s);
        sync_wizard_actions(&s);
    }
}

async fn wizard_return_home(state: Rc<RefCell<AppState>>) {
    if document()
        .get_element_by_id("editor")
        .is_some_and(|el| el.class_list().contains("wizard-active"))
    {
        let _ = persist_build_draft(state.borrow().wizard_step.max(3)).await;
    }
    close_build_wizard();
    let _ = gloo_net::http::Request::post("/api/projects/close")
        .send()
        .await;
    let mut state_mut = state.borrow_mut();
    state_mut.cells.clear();
    state_mut.elevation = fresh_elevation_layer(state_mut.map_bounds);
    state_mut.selected = None;
    state_mut.map_bounds = MapPreset::Small.bounds();
    state_mut.zoom = 1.0;
    state_mut.pan_x = 0.0;
    state_mut.pan_y = 0.0;
    state_mut.paint_active = false;
    state_mut.paint_moved = false;
    state_mut.paint_last_cell = None;
    state_mut.show_grid = false;
    reset_view_on_world_open(&mut state_mut);
    state_mut.legacy_map = false;
    state_mut.wizard_accepted = false;
    state_mut.wizard_edit_mode = false;
    state_mut.wizard_layout_class = "pangea".to_string();
    state_mut.wizard_regenerate_nonce = 0;
    state_mut.wizard_recipe_id.clear();
    state_mut.wizard_step = 3;
    state_mut.wizard_geo_style = "belts".to_string();
    state_mut.wizard_geo_nonce = 0;
    state_mut.wizard_geo_accepted = false;
    state_mut.geology = None;
    set_drawer_open(false);
    clear_inspect_selection();
    set_world_label("—");
    set_text("legacy-map-note", "");
    sync_wizard_actions(&state_mut);
    drop(state_mut);
    wasm_bindgen_futures::spawn_local(refresh_projects(state));
}

/// Cell-id label shown to the author — placeholder UI, real schema is 3.2.
fn cell_label(q: i32, r: i32) -> String {
    format!("hex q{q} r{r}")
}

fn hex_corners(cx: f64, cy: f64, size: f64) -> [(f64, f64); 6] {
    std::array::from_fn(|i| {
        let angle = (60.0 * i as f64 - 30.0).to_radians();
        (cx + size * angle.cos(), cy + size * angle.sin())
    })
}

/// Half-extent (in unit-size pixels) of the whole hex map, including the
/// outer cells' corner reach. Pointy-top corners stick out `√3/2` sideways
/// and `1.0` vertically. Used to fit the map into the current canvas.
fn select_value(id: &str) -> String {
    document()
        .get_element_by_id(id)
        .expect("missing select")
        .dyn_into::<HtmlSelectElement>()
        .expect("not a select")
        .value()
}

fn map_half_extent(bounds: MapBounds) -> (f64, f64) {
    let mut mx = 0.0_f64;
    let mut my = 0.0_f64;
    for cell in bounds.cells() {
        let (x, y) = cell.to_pixel(1.0);
        mx = mx.max(x.abs());
        my = my.max(y.abs());
    }
    (mx + 3f64.sqrt() / 2.0, my + 1.0)
}

/// Fit-to-window layout: hex `size` and origin so the rectangle map fills the
/// canvas with padding. Centered on the axial origin.
fn hex_layout(width: f64, height: f64, bounds: MapBounds) -> (f64, f64, f64) {
    let (hx, hy) = map_half_extent(bounds);
    let avail_w = (width - 2.0 * CANVAS_PAD).max(1.0);
    let avail_h = (height - 2.0 * CANVAS_PAD).max(1.0);
    let size = (avail_w / (2.0 * hx)).min(avail_h / (2.0 * hy)).max(1.0);
    (size, width / 2.0, height / 2.0)
}

/// Full layout with camera applied on top of the fit-to-window base.
fn map_layout(state: &AppState, width: f64, height: f64) -> (f64, f64, f64) {
    let (base_size, base_ox, base_oy) = hex_layout(width, height, state.map_bounds);
    let size = base_size * state.zoom;
    let ox = base_ox + state.pan_x;
    let oy = base_oy + state.pan_y;
    (size, ox, oy)
}

fn clamp_zoom(value: f64) -> f64 {
    value.clamp(MIN_ZOOM, MAX_ZOOM)
}

fn visible_scan_bounds(
    width: f64,
    height: f64,
    size: f64,
    ox: f64,
    oy: f64,
    bounds: MapBounds,
) -> (i32, i32, i32, i32) {
    let (bmin_q, bmax_q, bmin_r, bmax_r) = bounds.axial_limits();
    let mut min_q = i32::MAX;
    let mut max_q = i32::MIN;
    let mut min_r = i32::MAX;
    let mut max_r = i32::MIN;
    for (sx, sy) in [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)] {
        let cell = Axial::from_pixel(sx - ox, sy - oy, size);
        min_q = min_q.min(cell.q);
        max_q = max_q.max(cell.q);
        min_r = min_r.min(cell.r);
        max_r = max_r.max(cell.r);
    }
    let pad = ((2.0 / size).ceil() as i32).max(2);
    (
        min_q.saturating_sub(pad).max(bmin_q),
        max_q.saturating_add(pad).min(bmax_q),
        min_r.saturating_sub(pad).max(bmin_r),
        max_r.saturating_add(pad).min(bmax_r),
    )
}

/// Match the canvas backing store to its CSS box so the map scales with the
/// window (no browser upscaling blur). Returns the current pixel dimensions.
fn sync_canvas_size() -> (f64, f64) {
    let canvas = canvas();
    let rect = canvas.get_bounding_client_rect();
    let w = rect.width().max(1.0);
    let h = rect.height().max(1.0);
    if (canvas.width() as f64 - w).abs() >= 1.0 {
        canvas.set_width(w as u32);
    }
    if (canvas.height() as f64 - h).abs() >= 1.0 {
        canvas.set_height(h as u32);
    }
    (canvas.width() as f64, canvas.height() as f64)
}

fn redraw(state: &AppState) -> usize {
    let (width, height) = sync_canvas_size();
    let ctx = context();
    let bounds = state.map_bounds;
    let (size, ox, oy) = map_layout(state, width, height);

    ctx.clear_rect(0.0, 0.0, width, height);
    ctx.set_fill_style_str("#0e1113");
    ctx.fill_rect(0.0, 0.0, width, height);

    let (q_min, q_max, r_min, r_max) = visible_scan_bounds(width, height, size, ox, oy, bounds);
    let visible_cells = count_visible_in_bounds(q_min, q_max, r_min, r_max, bounds);
    let stroke_grid = stroke_grid_enabled(state.show_grid, visible_cells);
    let fill_scale = if state.show_grid {
        FILL_SCALE_GRID_ON
    } else {
        FILL_SCALE_GRID_OFF
    };
    let draw_profile_dots = show_profile_markers(state.zoom);
    let overlay_lod = overlays_lod_ok(visible_cells, state.zoom);
    let draw_labels = state.show_elevation_labels && overlay_lod;
    let draw_peaks = state.show_peaks && state.color_mode == ColorMode::Elevation && overlay_lod;
    let mut color_buf = String::with_capacity(20);
    let mut drawn_cells = 0usize;
    for q in q_min..=q_max {
        for r in r_min..=r_max {
            let cell = Axial::new(q, r);
            if !bounds.contains(cell) {
                continue;
            }
            let (x, y) = cell.to_pixel(size);
            let (cx, cy) = (ox + x, oy + y);
            let corners = hex_corners(cx, cy, size * fill_scale);

            ctx.begin_path();
            ctx.move_to(corners[0].0, corners[0].1);
            for corner in &corners[1..] {
                ctx.line_to(corner.0, corner.1);
            }
            ctx.close_path();

            let selected = state.selected == Some((q, r));
            let elevation = elevation_at(&state.elevation, bounds, q, r);
            match state.color_mode {
                ColorMode::Hydro => {
                    ctx.set_fill_style_str(hydro_fill(elevation));
                }
                ColorMode::Elevation => {
                    set_fill_rgb(
                        &ctx,
                        elevation_view::elevation_fill_rgb(elevation),
                        &mut color_buf,
                    );
                }
            }
            ctx.fill();
            // world-pipeline--tectonics-v1: subtle geology tint on step 4
            if wizard_is_active() && state.wizard_step == 4 {
                if let Some(geo) = state.geology.as_ref() {
                    if let Some(tint) = geology_tint(geo, bounds, q, r) {
                        ctx.set_fill_style_str(tint);
                        ctx.fill();
                    }
                }
            }
            if selected {
                ctx.set_line_width(3.0);
                ctx.set_stroke_style_str("#9fe3c4");
                ctx.stroke();
            } else if stroke_grid {
                ctx.set_line_width(GRID_LINE_WIDTH);
                ctx.set_stroke_style_str("#3a424b");
                ctx.stroke();
            }

            if draw_peaks && elevation > MOUNTAIN_THRESHOLD {
                draw_mountain_glyph(&ctx, cx, cy, size);
            }
            if draw_labels {
                let label_below = draw_peaks && elevation > MOUNTAIN_THRESHOLD;
                draw_elevation_label(&ctx, cx, cy, size, elevation, label_below);
            }

            // Profile-presence marker — a separate layer from terrain, so both
            // are visible at once (a cell can have terrain, a profile, or both).
            if draw_profile_dots && state.cells.contains_key(&(q, r)) {
                ctx.begin_path();
                let dot = (size * 0.13).clamp(2.5, 5.0);
                let _ = ctx.arc(cx, cy, dot, 0.0, std::f64::consts::PI * 2.0);
                ctx.set_fill_style_str("#e8d27a");
                ctx.fill();
            }
            drawn_cells += 1;
        }
    }
    let hover_elev = state
        .hover_cell
        .map(|(q, r)| elevation_at(&state.elevation, bounds, q, r));
    let hover_note = hover_elev
        .map(|e| format!(" · Hover elev {e}"))
        .unwrap_or_default();
    set_text(
        "view-stats",
        &format!(
            "Zoom {:.2}x · Draw {} / {} cells · Grid {} · {} · {}{}",
            state.zoom,
            drawn_cells,
            bounds.len(),
            grid_lines_stats_label(state.show_grid, visible_cells),
            labels_status_label(state.show_elevation_labels, visible_cells, state.zoom),
            peaks_status_label(
                state.show_peaks,
                state.color_mode,
                visible_cells,
                state.zoom,
            ),
            hover_note,
        ),
    );
    draw_preview_boundary(state, &ctx, size, ox, oy);
    draw_rivers(state, &ctx, size, ox, oy);
    set_text("toggle-grid", grid_lines_toggle_label(state.show_grid));
    set_text(
        "toggle-color-mode",
        if state.color_mode == ColorMode::Elevation {
            "Color: Elevation"
        } else {
            "Color: Hydro"
        },
    );
    set_text(
        "toggle-elevation-labels",
        if state.show_elevation_labels {
            "Show elevation: On"
        } else {
            "Show elevation: Off"
        },
    );
    set_text(
        "toggle-peaks",
        if state.show_peaks {
            "Peaks: On"
        } else {
            "Peaks: Off"
        },
    );
    sync_brush_radius_active(state.brush_radius);
    sync_falloff_active(state.falloff_even, state.brush_radius);
    sync_brush_step_active(state.brush_step);
    drawn_cells
}

fn hydro_fill(elevation: i32) -> &'static str {
    match hydro_from_elevation(elevation) {
        HydroKind::Land => "#6a7b43",
        HydroKind::Water => "#2e5f8a",
    }
}

/// Fetch `/api/projects`; show the Home list or jump straight into the
/// editor if a world is already active (e.g. server started with `--world`).
async fn refresh_projects(state: Rc<RefCell<AppState>>) {
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
    render_project_list(&data.projects);

    if let Some(active) = data.active {
        let _ = active;
        show_view("editor");
        wasm_bindgen_futures::spawn_local(load_map(state));
    } else {
        show_view("home");
    }
}

fn render_project_list(projects: &[ProjectStatus]) {
    let document = document();
    let Some(list) = document.get_element_by_id("project-list") else {
        return;
    };
    let empty = document.get_element_by_id("project-empty");

    if projects.is_empty() {
        list.set_inner_html("");
        if let Some(empty) = empty {
            let _ = empty.class_list().add_1("visible");
        }
        return;
    }
    if let Some(empty) = empty {
        let _ = empty.class_list().remove_1("visible");
    }

    let mut html = String::new();
    for p in projects {
        let missing = if !p.valid {
            "<div class=\"missing\">folder not found</div>"
        } else if p.legacy_map {
            "<div class=\"missing\">legacy map — no map/manifest.json</div>"
        } else {
            ""
        };
        let actions = if p.valid {
            let draft_attr = if p.build_draft { "1" } else { "0" };
            format!(
                "<button class=\"open-btn\" data-path=\"{path}\" data-build-draft=\"{draft_attr}\" data-build-step=\"{step}\" type=\"button\">Open</button><button class=\"manage-btn\" type=\"button\">Manage</button>",
                path = html_escape(&p.path),
                draft_attr = draft_attr,
                step = p.build_step.unwrap_or(3)
            )
        } else {
            format!(
                "<button class=\"remove-btn\" data-path=\"{path}\" type=\"button\">Remove</button>",
                path = html_escape(&p.path)
            )
        };
        let manage_row = if p.valid {
            format!(
                "<div class=\"manage-row\"><button class=\"remove-btn\" data-path=\"{path}\" type=\"button\">Remove</button><button class=\"delete-btn\" data-path=\"{path}\" type=\"button\">Delete…</button></div>",
                path = html_escape(&p.path)
            )
        } else {
            String::new()
        };
        let draft_badge = if p.build_draft {
            "<span class=\"badge draft-badge\">Draft</span>"
        } else {
            ""
        };
        let build_hint = if p.build_draft {
            format!(
                "<div class=\"build-hint\">{}</div>",
                build_step_label(p.build_step.unwrap_or(3))
            )
        } else {
            String::new()
        };
        html.push_str(&format!(
            "<li data-path=\"{path}\"><div class=\"main-row\"><div class=\"info\"><span class=\"id\">{id}</span>{draft_badge}{build_hint}<span class=\"path\">{path}</span>{missing}</div><div class=\"actions\">{actions}</div></div>{manage_row}</li>",
            id = html_escape(&p.id),
            path = html_escape(&p.path),
            draft_badge = draft_badge,
            build_hint = build_hint,
            missing = missing,
            actions = actions,
            manage_row = manage_row,
        ));
    }
    list.set_inner_html(&html);
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn suggested_world_path(state: &AppState, world_id: &str) -> Option<String> {
    let root = state.default_worlds_root.as_ref()?;
    let tail = if world_id.trim().is_empty() {
        "my-world"
    } else {
        world_id.trim()
    };
    let sep = if root.ends_with('\\') || root.ends_with('/') {
        ""
    } else {
        "\\"
    };
    Some(format!("{root}{sep}{tail}"))
}

fn refresh_suggested_path(state: &Rc<RefCell<AppState>>) {
    let state_ref = state.borrow();
    if !state_ref.path_touched {
        let id = input("new-id").value();
        if let Some(path) = suggested_world_path(&state_ref, &id) {
            input("new-path").set_value(&path);
        }
    }
    if !state_ref.build_path_touched {
        let id = input("generate-id").value();
        if let Some(path) = suggested_world_path(&state_ref, &id) {
            input("generate-path").set_value(&path);
        }
    }
}

async fn load_map(state: Rc<RefCell<AppState>>) {
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
    load_rivers(&state).await;
    let redraw_start = perf_now();
    let drawn = redraw(&state.borrow());
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
    perf_emit(&state.borrow().perf);
}

/// Fetch the dense elevation layer (scale-layers, D-46) into index buffers.
async fn load_elevation(state: &Rc<RefCell<AppState>>) {
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
async fn load_rivers(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/rivers").send().await else {
        return;
    };
    let Ok(catalog) = resp.json::<RiverCatalog>().await else {
        return;
    };
    let mut s = state.borrow_mut();
    s.rivers = catalog;
    s.active_river_id = None;
    sync_river_status(&s);
}

#[derive(Serialize)]
struct RiverAppendBody {
    river_id: Option<u32>,
    q: i32,
    r: i32,
}

async fn post_river_append(state: Rc<RefCell<AppState>>, q: i32, r: i32) {
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
    }
    schedule_redraw(state);
}

async fn delete_river_at_cell(state: Rc<RefCell<AppState>>, q: i32, r: i32) {
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
    schedule_redraw(state);
}

async fn post_river_pop(state: Rc<RefCell<AppState>>) {
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
    schedule_redraw(state);
}

async fn post_river_generate(state: Rc<RefCell<AppState>>) {
    set_text("river-status", "Generating rivers…");
    let Ok(resp) = gloo_net::http::Request::post("/api/rivers/generate")
        .send()
        .await
    else {
        set_text("river-status", "Generate failed (network)");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Generate rejected".into());
        set_text("river-status", &msg);
        return;
    }
    let Ok(catalog) = resp.json::<RiverCatalog>().await else {
        set_text("river-status", "Generate failed (parse)");
        return;
    };
    {
        let mut s = state.borrow_mut();
        s.rivers = catalog;
        s.active_river_id = None;
        bump_content_rev(&mut s);
        sync_river_status(&s);
    }
    set_text(
        "river-status",
        &format!("Generated {} river(s)", state.borrow().rivers.rivers.len()),
    );
    schedule_redraw(state);
}

fn cell_from_mouse_event(
    state: &Rc<RefCell<AppState>>,
    event: &web_sys::MouseEvent,
) -> Option<(i32, i32)> {
    let canvas = canvas();
    let rect = canvas.get_bounding_client_rect();
    let bounds = state.borrow().map_bounds;
    let (size, ox, oy) = map_layout(&state.borrow(), rect.width(), rect.height());
    let mx = event.client_x() as f64 - rect.left() - ox;
    let my = event.client_y() as f64 - rect.top() - oy;
    let cell = Axial::from_pixel(mx, my, size);
    if bounds.contains(cell) {
        Some((cell.q, cell.r))
    } else {
        None
    }
}

fn paint_stamp_cells(
    center: (i32, i32),
    brush_radius: i32,
    map_bounds: MapBounds,
) -> Vec<(i32, i32)> {
    let brush = brush_radius.clamp(MIN_BRUSH_RADIUS, MAX_BRUSH_RADIUS);
    Axial::new(center.0, center.1)
        .range(brush)
        .into_iter()
        .filter(|cell| map_bounds.contains(*cell))
        .map(|cell| (cell.q, cell.r))
        .collect()
}

fn queue_paint_stamp(state: Rc<RefCell<AppState>>, center: (i32, i32), new_elevation: i32) {
    let painted_cells = {
        let mut s = state.borrow_mut();
        let map_bounds = s.map_bounds;
        let painted = paint_stamp_cells(center, s.brush_radius, map_bounds);
        for (q, r) in &painted {
            set_elevation_cell(&mut s.elevation, map_bounds, *q, *r, new_elevation);
            s.pending_paints.insert((*q, *r), new_elevation);
        }
        bump_content_rev(&mut s);
        painted
    };
    schedule_redraw(state.clone());
    if !painted_cells.is_empty() {
        set_text("status", "Autosave pending…");
        schedule_paint_flush(state);
    }
}

/// elevation-authoring-v2: raise/lower with optional hill falloff.
fn queue_paint_delta_stamp(state: Rc<RefCell<AppState>>, center: (i32, i32), step_sign: i32) {
    let painted_cells = {
        let mut s = state.borrow_mut();
        let map_bounds = s.map_bounds;
        let brush_radius = s.brush_radius;
        let step = s.brush_step;
        let even = s.falloff_even || brush_radius == 0;
        let center_axial = Axial::new(center.0, center.1);
        let painted = paint_stamp_cells(center, brush_radius, map_bounds);
        let mut changed = 0usize;
        for (q, r) in &painted {
            let distance = center_axial.distance(Axial::new(*q, *r));
            let delta = stamp_delta(step * step_sign, distance, brush_radius, even);
            if delta == 0 {
                continue;
            }
            let current = elevation_at(&s.elevation, map_bounds, *q, *r);
            let new_elevation = current + delta;
            set_elevation_cell(&mut s.elevation, map_bounds, *q, *r, new_elevation);
            s.pending_paints.insert((*q, *r), new_elevation);
            changed += 1;
        }
        if changed > 0 {
            bump_content_rev(&mut s);
        }
        changed
    };
    schedule_redraw(state.clone());
    if painted_cells > 0 {
        set_text("status", "Autosave pending…");
        schedule_paint_flush(state);
    }
}

fn schedule_paint_flush(state: Rc<RefCell<AppState>>) {
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

async fn flush_pending_paints(state: Rc<RefCell<AppState>>) {
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
        perf_emit(&state.borrow().perf);
    }

    if !state.borrow().pending_paints.is_empty() {
        set_text("status", "Autosave retry…");
        schedule_paint_flush(state);
    } else {
        set_text("status", "");
    }
}

fn attach_canvas_click(state: Rc<RefCell<AppState>>) {
    let closure =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if state.borrow().suppress_next_click {
                state.borrow_mut().suppress_next_click = false;
                return;
            }
            let Some((q, r)) = cell_from_mouse_event(&state, &event) else {
                return;
            };

            if wizard_is_active() {
                let (edit_mode, kind) = {
                    let s = state.borrow();
                    (s.wizard_edit_mode, s.wizard_edit_brush.clone())
                };
                if edit_mode {
                    wasm_bindgen_futures::spawn_local(wizard_set_land_mask_cell(
                        state.clone(),
                        q,
                        r,
                        kind,
                    ));
                }
                return;
            }

            // Hydro brush paints elevation-driven hydro; Inspect opens the
            // author profile panel (unchanged behavior).
            let brush = state.borrow().brush.clone();
            if matches!(brush, Brush::River) {
                wasm_bindgen_futures::spawn_local(post_river_append(state.clone(), q, r));
                return;
            }
            if matches!(brush, Brush::RiverErase) {
                wasm_bindgen_futures::spawn_local(delete_river_at_cell(state.clone(), q, r));
                return;
            }
            if let Some(new_elevation) = brush_absolute_elevation(&brush) {
                queue_paint_stamp(state.clone(), (q, r), new_elevation);
                return;
            }
            if let Some(step_sign) = brush_delta_sign(&brush) {
                queue_paint_delta_stamp(state.clone(), (q, r), step_sign);
                return;
            }

            state.borrow_mut().selected = Some((q, r));
            open_dock_tab("inspect");
            set_text("panel-cell", &cell_label(q, r));
            input("title").set_value("");
            textarea("notes").set_value("");
            // Disabled while loading — otherwise a fast typist can fill the
            // fields before the fetch below resolves, and the (still pending)
            // response then silently overwrites what they just typed.
            input("title").set_disabled(true);
            textarea("notes").set_disabled(true);
            set_text("status", "Loading…");

            wasm_bindgen_futures::spawn_local(load_profile_into_panel(state.clone(), q, r));
        });
    canvas().set_onclick(Some(closure.as_ref().unchecked_ref()));
    closure.forget();
}

/// Left-drag pan (author-selected): keeps click semantics for short taps,
/// turns into viewport pan once movement exceeds a small threshold.
fn attach_pan_drag(state: Rc<RefCell<AppState>>) {
    let down_state = state.clone();
    let on_down =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if event.button() != 0 {
                return;
            }
            // Keep painting clicks reliable: pan starts only in Inspect mode.
            if !matches!(down_state.borrow().brush, Brush::Inspect) {
                return;
            }
            let mut s = down_state.borrow_mut();
            s.drag_active = true;
            s.drag_moved = false;
            s.drag_last_x = event.client_x() as f64;
            s.drag_last_y = event.client_y() as f64;
        });
    let _ =
        canvas().add_event_listener_with_callback("mousedown", on_down.as_ref().unchecked_ref());
    on_down.forget();

    let move_state = state.clone();
    let on_move =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if !move_state.borrow().drag_active {
                return;
            }
            let x = event.client_x() as f64;
            let y = event.client_y() as f64;
            let mut redraw_now = false;
            {
                let mut s = move_state.borrow_mut();
                let dx = x - s.drag_last_x;
                let dy = y - s.drag_last_y;
                s.drag_last_x = x;
                s.drag_last_y = y;
                if !s.drag_moved && (dx * dx + dy * dy).sqrt() >= PAN_DRAG_THRESHOLD {
                    s.drag_moved = true;
                }
                if s.drag_moved {
                    s.pan_x += dx;
                    s.pan_y += dy;
                    redraw_now = true;
                }
            }
            if redraw_now {
                schedule_redraw(move_state.clone());
            }
        });
    let _ =
        window().add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref());
    on_move.forget();

    let up_state = state.clone();
    let on_up =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if event.button() != 0 {
                return;
            }
            let moved = up_state.borrow().drag_moved;
            let was_active = up_state.borrow().drag_active;
            if !was_active {
                return;
            }
            {
                let mut s = up_state.borrow_mut();
                s.drag_active = false;
                s.drag_moved = false;
                if moved {
                    s.suppress_next_click = true;
                }
            }
        });
    let _ = window().add_event_listener_with_callback("mouseup", on_up.as_ref().unchecked_ref());
    on_up.forget();
}

/// Drag painting for Land/Water brushes: hold LMB and move across cells.
fn attach_paint_drag(state: Rc<RefCell<AppState>>) {
    let down_state = state.clone();
    let on_down =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if event.button() != 0 {
                return;
            }
            let brush = down_state.borrow().brush.clone();
            if let Some(elevation) = brush_absolute_elevation(&brush) {
                let Some((q, r)) = cell_from_mouse_event(&down_state, &event) else {
                    return;
                };
                {
                    let mut s = down_state.borrow_mut();
                    s.paint_active = true;
                    s.paint_moved = false;
                    s.paint_last_cell = Some((q, r));
                }
                queue_paint_stamp(down_state.clone(), (q, r), elevation);
                return;
            }
            if let Some(step_sign) = brush_delta_sign(&brush) {
                let Some((q, r)) = cell_from_mouse_event(&down_state, &event) else {
                    return;
                };
                {
                    let mut s = down_state.borrow_mut();
                    s.paint_active = true;
                    s.paint_moved = false;
                    s.paint_last_cell = Some((q, r));
                }
                queue_paint_delta_stamp(down_state.clone(), (q, r), step_sign);
            }
        });
    let _ =
        canvas().add_event_listener_with_callback("mousedown", on_down.as_ref().unchecked_ref());
    on_down.forget();

    let move_state = state.clone();
    let on_move =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if !move_state.borrow().paint_active {
                return;
            }
            let brush = move_state.borrow().brush.clone();
            let Some((q, r)) = cell_from_mouse_event(&move_state, &event) else {
                return;
            };
            let should_paint = {
                let mut s = move_state.borrow_mut();
                if s.paint_last_cell == Some((q, r)) {
                    false
                } else {
                    s.paint_moved = true;
                    true
                }
            };
            if !should_paint {
                return;
            }
            if let Some(elevation) = brush_absolute_elevation(&brush) {
                queue_paint_stamp(move_state.clone(), (q, r), elevation);
            } else if let Some(step_sign) = brush_delta_sign(&brush) {
                queue_paint_delta_stamp(move_state.clone(), (q, r), step_sign);
            } else {
                return;
            }
            move_state.borrow_mut().paint_last_cell = Some((q, r));
        });
    let _ =
        window().add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref());
    on_move.forget();

    let up_state = state.clone();
    let on_up =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if event.button() != 0 {
                return;
            }
            let (was_active, moved) = {
                let s = up_state.borrow();
                (s.paint_active, s.paint_moved)
            };
            if !was_active {
                return;
            }
            {
                let mut s = up_state.borrow_mut();
                s.paint_active = false;
                s.paint_moved = false;
                s.paint_last_cell = None;
                if moved {
                    s.suppress_next_click = true;
                }
            }
            wasm_bindgen_futures::spawn_local(flush_pending_paints(up_state.clone()));
        });
    let _ = window().add_event_listener_with_callback("mouseup", on_up.as_ref().unchecked_ref());
    on_up.forget();
}

fn attach_brush_hover_preview(state: Rc<RefCell<AppState>>) {
    let move_state = state.clone();
    let on_move =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            let next_hover = if brush_paints(&move_state.borrow().brush) {
                cell_from_mouse_event(&move_state, &event)
            } else {
                None
            };
            let changed = {
                let mut s = move_state.borrow_mut();
                if s.hover_cell == next_hover {
                    false
                } else {
                    s.hover_cell = next_hover;
                    true
                }
            };
            if changed {
                schedule_redraw(move_state.clone());
            }
        });
    let _ =
        canvas().add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref());
    on_move.forget();

    let leave_state = state.clone();
    let on_leave = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |_| {
        let changed = {
            let mut s = leave_state.borrow_mut();
            if s.hover_cell.is_none() {
                false
            } else {
                s.hover_cell = None;
                true
            }
        };
        if changed {
            schedule_redraw(leave_state.clone());
        }
    });
    let _ =
        canvas().add_event_listener_with_callback("mouseleave", on_leave.as_ref().unchecked_ref());
    on_leave.forget();
}

/// Wheel zoom with cursor anchor, clamped to the chosen 0.6x–2.5x range.
fn attach_wheel_zoom(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        event.prevent_default();
        let client_x = js_sys::Reflect::get(event.as_ref(), &JsValue::from_str("clientX"))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let client_y = js_sys::Reflect::get(event.as_ref(), &JsValue::from_str("clientY"))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let delta_y = js_sys::Reflect::get(event.as_ref(), &JsValue::from_str("deltaY"))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let canvas = canvas();
        let rect = canvas.get_bounding_client_rect();
        let mx = client_x - rect.left();
        let my = client_y - rect.top();
        let mut s = state.borrow_mut();
        let bounds = s.map_bounds;
        let (base_size, base_ox, base_oy) = hex_layout(rect.width(), rect.height(), bounds);
        let old_size = base_size * s.zoom;
        let old_ox = base_ox + s.pan_x;
        let old_oy = base_oy + s.pan_y;
        let world_x = (mx - old_ox) / old_size;
        let world_y = (my - old_oy) / old_size;
        let factor = if delta_y < 0.0 { 1.1 } else { 0.9 };
        let new_zoom = clamp_zoom(s.zoom * factor);
        s.zoom = new_zoom;
        let new_size = base_size * s.zoom;
        let new_ox = mx - world_x * new_size;
        let new_oy = my - world_y * new_size;
        s.pan_x = new_ox - base_ox;
        s.pan_y = new_oy - base_oy;
        drop(s);
        schedule_redraw(state.clone());
    });
    let _ = canvas().add_event_listener_with_callback("wheel", closure.as_ref().unchecked_ref());
    closure.forget();
}

async fn load_profile_into_panel(state: Rc<RefCell<AppState>>, q: i32, r: i32) {
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

fn attach_save_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        let Some((q, r)) = state.borrow().selected else {
            return;
        };
        let display_name = input("title").value();
        let notes = textarea("notes").value();
        let has_payload = !display_name.trim().is_empty() || !notes.trim().is_empty();
        let had_existing_profile = state.borrow().cells.contains_key(&(q, r));
        if !has_payload && !had_existing_profile {
            set_text("status", "Nothing to save.");
            return;
        }
        let state = state.clone();
        set_text("status", "Saving…");
        wasm_bindgen_futures::spawn_local(async move {
            let body = ProfileInput {
                display_name: display_name.clone(),
                notes,
            };
            let sent = gloo_net::http::Request::put(&format!("/api/cells/{q}/{r}/profile"))
                .json(&body)
                .expect("serializing profile body")
                .send()
                .await;
            match sent {
                Ok(resp) if resp.ok() => {
                    {
                        let mut s = state.borrow_mut();
                        s.cells.insert((q, r), display_name);
                        bump_content_rev(&mut s);
                    }
                    schedule_redraw(state.clone());
                    set_text("status", "Saved.");
                }
                _ => set_text("status", "Save failed."),
            }
        });
    });
    document()
        .get_element_by_id("save")
        .expect("missing #save")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching save handler");
    closure.forget();
}

fn attach_close_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        state.borrow_mut().selected = None;
        set_drawer_open(false);
        clear_inspect_selection();
        schedule_redraw(state.clone());
    });
    document()
        .get_element_by_id("close")
        .expect("missing #close")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching close handler");
    closure.forget();
}

/// "&larr; Worlds" button in the editor: clears the server's active world
/// and local UI state, then goes back to the Home screen.
fn attach_switch_world_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        close_build_wizard();
        let state = state.clone();
        wasm_bindgen_futures::spawn_local(wizard_return_home(state));
    });
    document()
        .get_element_by_id("switch-world")
        .expect("missing #switch-world")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching switch-world handler");
    closure.forget();
}

/// map-bounds--hex-rectangle-16x9: Grand/World preset warnings on Home (D-49).
fn sync_preset_size_warning(select_id: &str, warn_id: &str) {
    let preset = select_value(select_id);
    let Some(el) = document().get_element_by_id(warn_id) else {
        return;
    };
    let (text, class) = match preset.as_str() {
        "grand" => (
            "Large map — performance may vary on your machine.",
            "preset-warn-yellow",
        ),
        "world" => (
            "Experimental map size (not stable) — expect slowdowns.",
            "preset-warn-red",
        ),
        _ => ("", ""),
    };
    el.set_text_content(if text.is_empty() { None } else { Some(text) });
    let _ = el.class_list().remove_1("preset-warn-yellow");
    let _ = el.class_list().remove_1("preset-warn-red");
    if !class.is_empty() {
        let _ = el.class_list().add_1(class);
    }
}

fn attach_preset_warn_handlers() {
    for (select_id, warn_id) in [
        ("new-preset", "new-preset-warn"),
        ("generate-preset", "generate-preset-warn"),
    ] {
        sync_preset_size_warning(select_id, warn_id);
        let select_id = select_id.to_string();
        let warn_id = warn_id.to_string();
        let select_for_change = select_id.clone();
        let warn_for_change = warn_id.clone();
        let on_change = Closure::<dyn FnMut()>::new(move || {
            sync_preset_size_warning(&select_for_change, &warn_for_change);
        });
        if let Ok(Some(select)) = document().query_selector(&format!("#{select_id}")) {
            let _ = select
                .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
        }
        on_change.forget();
    }
}

/// "Create" button on the Home screen: scaffolds a new world via the server
/// and opens it directly (roadmap 5.7 minimal wizard).
fn attach_create_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        let id = input("new-id").value();
        let path = input("new-path").value();
        if id.trim().is_empty() || path.trim().is_empty() {
            set_text("home-status", "World name and folder are both required.");
            return;
        }
        let preset = select_value("new-preset");
        let state = state.clone();
        set_text("home-status", "Creating…");
        wasm_bindgen_futures::spawn_local(async move {
            let body = CreateProjectInput {
                id: &id,
                path: &path,
                map_preset: &preset,
                build_wizard: None,
            };
            let sent = gloo_net::http::Request::post("/api/projects").json(&body);
            let sent = match sent {
                Ok(req) => req.send().await,
                Err(err) => {
                    set_text("home-status", &format!("Error: {err}"));
                    return;
                }
            };
            match sent {
                Ok(resp) if resp.ok() => {
                    set_text("home-status", "");
                    input("new-id").set_value("");
                    state.borrow_mut().path_touched = false;
                    refresh_suggested_path(&state);
                    show_view("editor");
                    wasm_bindgen_futures::spawn_local(load_map(state));
                }
                Ok(resp) => {
                    let msg = resp
                        .text()
                        .await
                        .unwrap_or_else(|_| "create failed".to_string());
                    set_text("home-status", &msg);
                }
                Err(err) => set_text("home-status", &format!("Error: {err}")),
            }
        });
    });
    document()
        .get_element_by_id("create")
        .expect("missing #create")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching create handler");
    closure.forget();
}

/// "Generate" on Home — same scaffold API as Create; blank hex at chosen preset (D-40).
fn attach_generate_rivers_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        set_text("river-status", "Generating rivers…");
        wasm_bindgen_futures::spawn_local(post_river_generate(state.clone()));
    });
    document()
        .get_element_by_id("generate-rivers")
        .expect("missing #generate-rivers")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching generate-rivers handler");
    closure.forget();
}

/// **Start build** on Home — create world and open build wizard shell (D-57).
fn attach_build_start_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        let id = input("generate-id").value();
        let path = input("generate-path").value();
        if id.trim().is_empty() || path.trim().is_empty() {
            set_text(
                "generate-status",
                "World name and folder are both required.",
            );
            return;
        }
        let preset = select_value("generate-preset");
        let state = state.clone();
        set_text("generate-status", "Creating…");
        wasm_bindgen_futures::spawn_local(async move {
            let body = CreateProjectInput {
                id: &id,
                path: &path,
                map_preset: &preset,
                build_wizard: Some(true),
            };
            let sent = gloo_net::http::Request::post("/api/projects").json(&body);
            let sent = match sent {
                Ok(req) => req.send().await,
                Err(err) => {
                    set_text("generate-status", &format!("Error: {err}"));
                    return;
                }
            };
            match sent {
                Ok(resp) if resp.ok() => {
                    set_text("generate-status", "");
                    input("generate-id").set_value("");
                    state.borrow_mut().build_path_touched = false;
                    refresh_suggested_path(&state);
                    show_view("editor");
                    load_map(state.clone()).await;
                    open_build_wizard();
                    {
                        let mut s = state.borrow_mut();
                        s.wizard_layout_class = "pangea".to_string();
                        s.wizard_regenerate_nonce = 0;
                        s.wizard_recipe_id.clear();
                        s.wizard_accepted = false;
                        s.wizard_edit_mode = false;
                        ensure_wizard_recipe(&mut s);
                        sync_wizard_actions(&s);
                    }
                    set_wizard_status("Generating Pangea…");
                    wasm_bindgen_futures::spawn_local(generate_wizard_land_mask(state.clone()));
                }
                Ok(resp) => {
                    let msg = resp
                        .text()
                        .await
                        .unwrap_or_else(|_| "create failed".to_string());
                    set_text("generate-status", &msg);
                }
                Err(err) => set_text("generate-status", &format!("Error: {err}")),
            }
        });
    });
    document()
        .get_element_by_id("build-start")
        .expect("missing #build-start")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching build-start handler");
    closure.forget();
}

/// Redraw on window resize so the map keeps filling the viewport (4.2
/// fit-to-window). Cheap — one redraw per resize event.
fn attach_window_resize(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        schedule_redraw(state.clone());
    });
    let _ = window().add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn attach_new_id_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        refresh_suggested_path(&state);
    });
    let _ =
        input("new-id").add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn attach_new_path_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        state.borrow_mut().path_touched = true;
    });
    let _ = input("new-path")
        .add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn attach_generate_id_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        refresh_suggested_path(&state);
    });
    let _ = input("generate-id")
        .add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn attach_generate_path_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        state.borrow_mut().build_path_touched = true;
    });
    let _ = input("generate-path")
        .add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn sync_brush_step_active(step: i32) {
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(items) = drawer.query_selector_all("[data-brush-step]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let is_active = el
                            .get_attribute("data-brush-step")
                            .and_then(|v| v.parse::<i32>().ok())
                            .map(|v| v == step)
                            .unwrap_or(false);
                        if is_active {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().remove_1("active");
                        }
                    }
                }
            }
        }
    }
}

fn sync_falloff_active(falloff_even: bool, brush_radius: i32) {
    let hill_enabled = brush_radius > 0;
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(items) = drawer.query_selector_all("[data-falloff]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let mode = el.get_attribute("data-falloff").unwrap_or_default();
                        let is_hill = mode == "hill";
                        if is_hill && !hill_enabled {
                            let _ = el.class_list().add_1("disabled");
                            let _ = el.set_attribute("disabled", "");
                        } else {
                            let _ = el.class_list().remove_1("disabled");
                            let _ = el.remove_attribute("disabled");
                        }
                        let is_active = (mode == "even" && falloff_even)
                            || (mode == "hill" && !falloff_even && hill_enabled);
                        if is_active {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().remove_1("active");
                        }
                    }
                }
            }
        }
    }
}

fn apply_elevation_brush_intent(s: &mut AppState) {
    s.color_mode = ColorMode::Elevation;
    s.show_elevation_labels = true;
    // Peaks stay author-controlled — Raise/Lower does not force them on.
}

/// View defaults on world open (D-53) — elevation-first; grid on small maps only.
fn reset_view_on_world_open(s: &mut AppState) {
    s.color_mode = ColorMode::Elevation;
    s.show_elevation_labels = true;
    s.show_peaks = false;
    s.show_grid = s.map_bounds.len() <= elevation_view::OVERLAY_LOD_MAX_VISIBLE;
    s.active_river_id = None;
    s.rivers = RiverCatalog::default();
    s.wizard_accepted = false;
    s.wizard_edit_mode = false;
    deactivate_paint_brush(s);
}

/// Map a brush to absolute target elevation. `Inspect` / Raise / Lower write nothing here.
fn brush_absolute_elevation(brush: &Brush) -> Option<i32> {
    match brush {
        Brush::Inspect | Brush::Raise | Brush::Lower | Brush::River | Brush::RiverErase => None,
        Brush::SetLand => Some(1),
        Brush::SetWater => Some(0),
    }
}

fn brush_delta_sign(brush: &Brush) -> Option<i32> {
    match brush {
        Brush::Raise => Some(1),
        Brush::Lower => Some(-1),
        _ => None,
    }
}

fn brush_paints(brush: &Brush) -> bool {
    !matches!(brush, Brush::Inspect)
}

/// Tool dock: rail tabs toggle drawers; hydro swatches set the active brush.
fn attach_dock_click(state: Rc<RefCell<AppState>>) {
    let closure =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            let Some(target) = click_target_element(&event) else {
                return;
            };

            if let Ok(Some(button)) = target.closest("[data-view-toggle]") {
                let Some(kind) = button.get_attribute("data-view-toggle") else {
                    return;
                };
                match kind.as_str() {
                    "grid" => {
                        let show_grid = {
                            let mut s = state.borrow_mut();
                            s.show_grid = !s.show_grid;
                            s.show_grid
                        };
                        button.set_text_content(Some(grid_lines_toggle_label(show_grid)));
                        schedule_redraw(state.clone());
                    }
                    "color-mode" => {
                        let mut s = state.borrow_mut();
                        s.color_mode = if s.color_mode == ColorMode::Hydro {
                            ColorMode::Elevation
                        } else {
                            ColorMode::Hydro
                        };
                        drop(s);
                        schedule_redraw(state.clone());
                    }
                    "elevation-labels" => {
                        let show = {
                            let s = state.borrow();
                            !s.show_elevation_labels
                        };
                        state.borrow_mut().show_elevation_labels = show;
                        schedule_redraw(state.clone());
                    }
                    "peaks" => {
                        let show = {
                            let s = state.borrow();
                            !s.show_peaks
                        };
                        state.borrow_mut().show_peaks = show;
                        schedule_redraw(state.clone());
                    }
                    _ => {}
                }
                return;
            }

            if let Ok(Some(button)) = target.closest("[data-brush-step]") {
                let Some(raw) = button.get_attribute("data-brush-step") else {
                    return;
                };
                let Ok(step) = raw.parse::<i32>() else { return };
                if [1, 5, 10].contains(&step) {
                    state.borrow_mut().brush_step = step;
                    sync_brush_step_active(step);
                }
                return;
            }

            if let Ok(Some(button)) = target.closest("[data-falloff]") {
                if button.has_attribute("disabled") {
                    return;
                }
                let Some(mode) = button.get_attribute("data-falloff") else {
                    return;
                };
                let even = mode != "hill";
                if state.borrow().brush_radius == 0 && !even {
                    return;
                }
                state.borrow_mut().falloff_even = even;
                sync_falloff_active(even, state.borrow().brush_radius);
                return;
            }

            if let Ok(Some(button)) = target.closest("[data-brush-size]") {
                let Some(raw_size) = button.get_attribute("data-brush-size") else {
                    return;
                };
                let Ok(radius) = raw_size.parse::<i32>() else {
                    return;
                };
                {
                    let mut s = state.borrow_mut();
                    s.brush_radius = radius.clamp(MIN_BRUSH_RADIUS, MAX_BRUSH_RADIUS);
                    if s.brush_radius == 0 {
                        s.falloff_even = true;
                    }
                }
                sync_brush_radius_active(state.borrow().brush_radius);
                sync_falloff_active(state.borrow().falloff_even, state.borrow().brush_radius);
                return;
            }

            if let Ok(Some(button)) = target.closest("[data-dock]") {
                let Some(tab) = button.get_attribute("data-dock") else {
                    return;
                };
                let current = active_dock_tab();
                let drawer_open = drawer_is_open();
                // tool-dock-brush-deselect-v1 (variant A): repeat Terrain deselects brush, drawer stays.
                let terrain_deselect = tab == "terrain"
                    && current.as_deref() == Some("terrain")
                    && drawer_open
                    && terrain_brush(&state.borrow().brush);
                if terrain_deselect {
                    {
                        let mut s = state.borrow_mut();
                        deactivate_paint_brush(&mut s);
                    }
                    sync_paint_tool_ui(&Brush::Inspect);
                    schedule_redraw(state.clone());
                    return;
                }

                toggle_dock_tab(&tab);

                match tab.as_str() {
                    "inspect" | "view" | "world" => {
                        {
                            let mut s = state.borrow_mut();
                            deactivate_paint_brush(&mut s);
                        }
                        sync_paint_tool_ui(&Brush::Inspect);
                        schedule_redraw(state.clone());
                    }
                    "terrain" => {
                        let brush = {
                            let mut s = state.borrow_mut();
                            clear_pointer_interaction(&mut s);
                            if !terrain_brush(&s.brush) {
                                s.brush = s.last_paint_brush.clone();
                            }
                            s.hover_cell = None;
                            s.brush.clone()
                        };
                        sync_paint_tool_ui(&brush);
                        if brush_paints(&brush) {
                            schedule_redraw(state.clone());
                        }
                    }
                    "rivers" => {
                        let brush = {
                            let mut s = state.borrow_mut();
                            clear_pointer_interaction(&mut s);
                            if !river_brush(&s.brush) {
                                s.brush = s.last_river_brush.clone();
                            }
                            s.hover_cell = None;
                            s.brush.clone()
                        };
                        sync_paint_tool_ui(&brush);
                        sync_river_status(&state.borrow());
                        if brush_paints(&brush) {
                            schedule_redraw(state.clone());
                        }
                    }
                    _ => {}
                }
                return;
            }

            if let Ok(Some(button)) = target.closest("[data-river-action]") {
                let Some(action) = button.get_attribute("data-river-action") else {
                    return;
                };
                match action.as_str() {
                    "new" => {
                        state.borrow_mut().active_river_id = None;
                        sync_river_status(&state.borrow());
                    }
                    "undo" => {
                        wasm_bindgen_futures::spawn_local(post_river_pop(state.clone()));
                    }
                    _ => {}
                }
                return;
            }

            let Ok(Some(button)) = target.closest("[data-brush]") else {
                return;
            };
            let Some(kind) = button.get_attribute("data-brush") else {
                return;
            };

            let brush = match kind.as_str() {
                "land" => Brush::SetLand,
                "water" => Brush::SetWater,
                "raise" => Brush::Raise,
                "lower" => Brush::Lower,
                "river" => Brush::River,
                "river-erase" => Brush::RiverErase,
                _ => return,
            };
            {
                let mut s = state.borrow_mut();
                apply_paint_brush(&mut s, brush.clone());
                if matches!(brush, Brush::Raise | Brush::Lower) {
                    apply_elevation_brush_intent(&mut s);
                }
            }
            if river_brush(&brush) {
                open_dock_tab("rivers");
                sync_river_status(&state.borrow());
            } else {
                open_dock_tab("terrain");
            }
            sync_paint_tool_ui(&brush);
            if matches!(brush, Brush::Raise | Brush::Lower) {
                sync_falloff_active(state.borrow().falloff_even, state.borrow().brush_radius);
                sync_brush_step_active(state.borrow().brush_step);
                schedule_redraw(state.clone());
            } else if river_brush(&brush) {
                schedule_redraw(state.clone());
            }
        });
    document()
        .get_element_by_id("tool-dock")
        .expect("missing #tool-dock")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching dock handler");
    closure.forget();
}

fn attach_escape_key() {
    let closure =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
            if event.key() == "Escape" {
                set_drawer_open(false);
            }
        });
    let _ =
        document().add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());
    closure.forget();
}

/// "Browse…" button, desktop shell only (roadmap 5.9, D-29) — the button is
/// `display:none` in a plain browser tab (see `index.html`), so attaching a
/// listener here is harmless either way; it just never fires without Tauri.
fn attach_browse_folder_click(state: Rc<RefCell<AppState>>) {
    if let Some(button) = document().get_element_by_id("browse-folder") {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(path) = pick_folder_via_tauri().await {
                    input("new-path").set_value(&path);
                    state.borrow_mut().path_touched = true;
                }
            });
        });
        let _ = button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
    if let Some(button) = document().get_element_by_id("browse-generate-folder") {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(path) = pick_folder_via_tauri().await {
                    input("generate-path").set_value(&path);
                    state.borrow_mut().build_path_touched = true;
                }
            });
        });
        let _ = button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

/// Calls the `window.mapkeeperPickFolder()` bridge defined in `index.html`
/// (only present inside the Tauri shell) via `js_sys`, so this crate has no
/// direct Tauri dependency — it stays a plain WASM/web-sys build either way.
async fn pick_folder_via_tauri() -> Option<String> {
    let bridge = js_sys::Reflect::get(&window(), &JsValue::from_str("mapkeeperPickFolder")).ok()?;
    let bridge: js_sys::Function = bridge.dyn_into().ok()?;
    let promise: js_sys::Promise = bridge.call0(&window()).ok()?.dyn_into().ok()?;
    let result = wasm_bindgen_futures::JsFuture::from(promise).await.ok()?;
    result.as_string()
}

/// Delegated click on the project list: any `.open-btn` opens that world.
fn attach_project_list_click(state: Rc<RefCell<AppState>>) {
    let closure =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                return;
            };
            if let Ok(Some(button)) = target.closest(".manage-btn") {
                let Some(row) = button.closest("li").ok().flatten() else {
                    return;
                };
                if row.class_list().contains("manage-open") {
                    let _ = row.class_list().remove_1("manage-open");
                } else {
                    let _ = row.class_list().add_1("manage-open");
                }
                return;
            }
            if let Ok(Some(button)) = target.closest(".delete-btn") {
                let Some(path) = button.get_attribute("data-path") else {
                    return;
                };
                if !button.class_list().contains("armed") {
                    let _ = button.class_list().add_1("armed");
                    button.set_text_content(Some("Delete now"));
                    set_text(
                        "home-status",
                        "Click \"Delete now\" again to permanently remove this world from disk.",
                    );
                    return;
                }
                let state = state.clone();
                set_text("home-status", "Deleting…");
                wasm_bindgen_futures::spawn_local(async move {
                    let body = DeleteProjectInput { path: &path };
                    let sent = gloo_net::http::Request::post("/api/projects/delete").json(&body);
                    let sent = match sent {
                        Ok(req) => req.send().await,
                        Err(err) => {
                            set_text("home-status", &format!("Error: {err}"));
                            return;
                        }
                    };
                    match sent {
                        Ok(resp) if resp.ok() => {
                            set_text("home-status", "");
                            wasm_bindgen_futures::spawn_local(refresh_projects(state));
                        }
                        Ok(resp) => {
                            let msg = resp
                                .text()
                                .await
                                .unwrap_or_else(|_| "delete failed".to_string());
                            set_text("home-status", &msg);
                        }
                        Err(err) => set_text("home-status", &format!("Error: {err}")),
                    }
                });
                return;
            }
            if let Ok(Some(button)) = target.closest(".remove-btn") {
                let Some(path) = button.get_attribute("data-path") else {
                    return;
                };
                let state = state.clone();
                set_text("home-status", "Removing from launcher…");
                wasm_bindgen_futures::spawn_local(async move {
                    let body = ForgetProjectInput { path: &path };
                    let sent = gloo_net::http::Request::post("/api/projects/forget").json(&body);
                    let sent = match sent {
                        Ok(req) => req.send().await,
                        Err(err) => {
                            set_text("home-status", &format!("Error: {err}"));
                            return;
                        }
                    };
                    match sent {
                        Ok(resp) if resp.ok() => {
                            set_text("home-status", "");
                            wasm_bindgen_futures::spawn_local(refresh_projects(state));
                        }
                        Ok(resp) => {
                            let msg = resp
                                .text()
                                .await
                                .unwrap_or_else(|_| "remove failed".to_string());
                            set_text("home-status", &msg);
                        }
                        Err(err) => set_text("home-status", &format!("Error: {err}")),
                    }
                });
                return;
            }

            let Ok(Some(button)) = target.closest(".open-btn") else {
                return;
            };
            let Some(path) = button.get_attribute("data-path") else {
                return;
            };
            let resume_wizard = button.get_attribute("data-build-draft").as_deref() == Some("1");
            let resume_step = button
                .get_attribute("data-build-step")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(3)
                .clamp(3, 5);

            let state = state.clone();
            set_text("home-status", "Opening…");
            wasm_bindgen_futures::spawn_local(async move {
                let body = OpenProjectInput { path: &path };
                let sent = gloo_net::http::Request::post("/api/projects/open").json(&body);
                let sent = match sent {
                    Ok(req) => req.send().await,
                    Err(err) => {
                        set_text("home-status", &format!("Error: {err}"));
                        return;
                    }
                };
                match sent {
                    Ok(resp) if resp.ok() => {
                        set_text("home-status", "");
                        show_view("editor");
                        load_map(state.clone()).await;
                        if resume_wizard {
                            open_build_wizard();
                            {
                                let mut s = state.borrow_mut();
                                s.wizard_step = resume_step;
                                s.wizard_accepted = resume_step > 3;
                                s.wizard_edit_mode = false;
                                s.wizard_geo_accepted = resume_step > 4;
                                ensure_wizard_recipe(&mut s);
                                sync_wizard_actions(&s);
                            }
                            match resume_step {
                                4 => {
                                    set_wizard_status("Resumed at tectonics — generate or accept geology.");
                                    wasm_bindgen_futures::spawn_local(async move {
                                        load_geology(&state).await;
                                        schedule_redraw(state.clone());
                                    });
                                }
                                5 => {
                                    set_wizard_status("Resumed at elevation — generate or Finish.");
                                    wasm_bindgen_futures::spawn_local(async move {
                                        load_geology(&state).await;
                                        load_elevation(&state).await;
                                        schedule_redraw(state.clone());
                                    });
                                }
                                _ => set_wizard_status("Pick a class, regenerate until you like the shape, then accept."),
                            }
                        } else {
                            close_build_wizard();
                        }
                    }
                    Ok(resp) => {
                        let msg = resp
                            .text()
                            .await
                            .unwrap_or_else(|_| "open failed".to_string());
                        set_text("home-status", &msg);
                    }
                    Err(err) => set_text("home-status", &format!("Error: {err}")),
                }
            });
        });
    document()
        .get_element_by_id("project-list")
        .expect("missing #project-list")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching project-list handler");
    closure.forget();
}
