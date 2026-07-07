//! WASM UI — calls mapkeeper-core for hex geometry + profile rules;
//! filesystem goes through the `mapkeeper-server` HTTP API, never direct
//! FS access. WASM framework choice: none — plain `wasm-bindgen` + `web-sys`
//! canvas, deliberately minimal for the flow-first pass (roadmap D-21).
//!
//! Flow: Home screen lists/creates worlds (roadmap D-12/5.7 launcher) ->
//! open a world -> render a blank hex grid -> click a cell -> edit a
//! placeholder profile (title + notes) -> save -> cell is "painted". No
//! real cell schema (roadmap 3.2) yet — that is the point of this pass.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::hydro::{hydro_from_elevation, HydroKind};
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue};
use mapkeeper_core::profile::CellProfile;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Element, HtmlCanvasElement, HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};

const DEFAULT_MAP_RADIUS: i32 = 6;
const MIN_ZOOM: f64 = 0.6;
const MAX_ZOOM: f64 = 2.5;
const PAN_DRAG_THRESHOLD: f64 = 3.0;
// save-batch--http-endpoint-v1: tuneable write buffering.
const PAINT_SAVE_COOLDOWN_MS: u32 = 300;
const PAINT_BATCH_MAX_CELLS: usize = 512;
const MIN_BRUSH_RADIUS: i32 = 0;
const MAX_BRUSH_RADIUS: i32 = 3;
/// Fill inset so adjacent hex fills leave a thin grid gap.
const HEX_GAP: f64 = 0.92;
/// Breathing room (px) between the map and the canvas edge.
const CANVAS_PAD: f64 = 20.0;

#[derive(Deserialize)]
struct MapBoundsResponse {
    radius: i32,
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
}

#[derive(Deserialize)]
struct ProjectsResponse {
    active: Option<ProjectEntry>,
    projects: Vec<ProjectStatus>,
    default_worlds_root: String,
}

#[derive(Serialize)]
struct CreateProjectInput<'a> {
    id: &'a str,
    path: &'a str,
    map_preset: &'a str,
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
    value: i16,
}

/// Active editing tool. `Inspect` keeps the old click→profile behavior; the
/// hydro brushes paint elevation-driven hydro (`land`/`water`) instead.
#[derive(Clone)]
enum Brush {
    Inspect,
    SetLand,
    SetWater,
}

struct AppState {
    /// Cells that have an author profile (used for the profile-presence marker).
    cells: HashMap<(i32, i32), String>,
    /// Sparse elevation overrides (missing => default land elevation).
    elevation: HashMap<(i32, i32), i16>,
    brush: Brush,
    selected: Option<(i32, i32)>,
    /// Hex bounds radius from `map/manifest.json` (via `/api/map`).
    map_radius: i32,
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
    /// Local paint writes not yet persisted to server.
    pending_paints: HashMap<(i32, i32), i16>,
    paint_flush_scheduled: bool,
    paint_flush_in_flight: bool,
    hover_cell: Option<(i32, i32)>,
    /// Draw hex-cell borders over fills.
    show_grid: bool,
    suppress_next_click: bool,
    legacy_map: bool,
    default_worlds_root: Option<String>,
    path_touched: bool,
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();

    let state = Rc::new(RefCell::new(AppState {
        cells: HashMap::new(),
        elevation: HashMap::new(),
        brush: Brush::Inspect,
        selected: None,
        map_radius: DEFAULT_MAP_RADIUS,
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
        pending_paints: HashMap::new(),
        paint_flush_scheduled: false,
        paint_flush_in_flight: false,
        hover_cell: None,
        show_grid: true,
        suppress_next_click: false,
        legacy_map: false,
        default_worlds_root: None,
        path_touched: false,
    }));

    redraw(&state.borrow());
    attach_canvas_click(state.clone());
    attach_save_click(state.clone());
    attach_close_click(state.clone());
    attach_switch_world_click(state.clone());
    attach_create_click(state.clone());
    attach_generate_click(state.clone());
    attach_project_list_click(state.clone());
    attach_dock_click(state.clone());
    attach_escape_key();
    attach_pan_drag(state.clone());
    attach_paint_drag(state.clone());
    attach_brush_hover_preview(state.clone());
    attach_wheel_zoom(state.clone());
    attach_window_resize(state.clone());
    attach_browse_folder_click();
    attach_new_id_input(state.clone());
    attach_new_path_input(state.clone());

    wasm_bindgen_futures::spawn_local(refresh_projects(state));
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

fn document() -> web_sys::Document {
    window().document().expect("no document")
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
    document().get_element_by_id(id).expect("missing input").dyn_into().expect("not an input")
}

fn textarea(id: &str) -> HtmlTextAreaElement {
    document().get_element_by_id(id).expect("missing textarea").dyn_into().expect("not a textarea")
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

fn sync_dock_rail_for_brush(brush: &Brush) {
    let terrain_active = !matches!(brush, Brush::Inspect);
    if let Some(rail) = document().get_element_by_id("dock-rail") {
        if let Ok(items) = rail.query_selector_all("[data-dock]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let dock = el.get_attribute("data-dock").unwrap_or_default();
                        let tool_active = (dock == "inspect" && !terrain_active)
                            || (dock == "terrain" && terrain_active);
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

fn draw_preview_boundary(state: &AppState, ctx: &CanvasRenderingContext2d, size: f64, ox: f64, oy: f64) {
    if matches!(state.brush, Brush::Inspect) {
        return;
    }
    let Some(center) = state.hover_cell else { return };
    let cells = paint_stamp_cells(center, state.brush_radius, state.map_radius.max(0));
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
        let corners = hex_corners(ox + x, oy + y, size * HEX_GAP * 0.98);
        ctx.begin_path();
        ctx.move_to(corners[0].0, corners[0].1);
        for corner in &corners[1..] {
            ctx.line_to(corner.0, corner.1);
        }
        ctx.close_path();
        ctx.stroke();
    }
}

fn open_dock_tab(tab: &str) {
    set_dock_tab(tab);
    set_drawer_open(true);
}

fn toggle_dock_tab(tab: &str) {
    if drawer_is_open() {
        let current = document()
            .get_element_by_id("dock-rail")
            .and_then(|rail| {
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

fn map_half_extent(radius: i32) -> (f64, f64) {
    let mut mx = 0.0_f64;
    let mut my = 0.0_f64;
    for q in -radius..=radius {
        for r in -radius..=radius {
            if (q.abs() + r.abs() + (q + r).abs()) / 2 > radius {
                continue;
            }
            let (x, y) = Axial::new(q, r).to_pixel(1.0);
            mx = mx.max(x.abs());
            my = my.max(y.abs());
        }
    }
    (mx + 3f64.sqrt() / 2.0, my + 1.0)
}

/// Fit-to-window layout: hex `size` and origin so the radial map fills the
/// canvas with padding. The map is symmetric about the axial origin, so the
/// pixel origin is just the canvas center.
fn hex_layout(width: f64, height: f64, radius: i32) -> (f64, f64, f64) {
    let (hx, hy) = map_half_extent(radius);
    let avail_w = (width - 2.0 * CANVAS_PAD).max(1.0);
    let avail_h = (height - 2.0 * CANVAS_PAD).max(1.0);
    let size = (avail_w / (2.0 * hx)).min(avail_h / (2.0 * hy)).max(1.0);
    (size, width / 2.0, height / 2.0)
}

/// Full layout with camera applied on top of the fit-to-window base.
fn map_layout(state: &AppState, width: f64, height: f64) -> (f64, f64, f64) {
    let radius = state.map_radius.max(0);
    let (base_size, base_ox, base_oy) = hex_layout(width, height, radius);
    let size = base_size * state.zoom;
    let ox = base_ox + state.pan_x;
    let oy = base_oy + state.pan_y;
    (size, ox, oy)
}

fn clamp_zoom(value: f64) -> f64 {
    value.clamp(MIN_ZOOM, MAX_ZOOM)
}

fn total_cell_count(radius: i32) -> usize {
    let r = radius.max(0) as i64;
    (1 + 3 * r * (r + 1)) as usize
}

/// Approximate axial scan bounds for visible cells. Adds a small padding ring
/// to avoid clipping border cells due to hex corner geometry.
fn visible_scan_bounds(width: f64, height: f64, size: f64, ox: f64, oy: f64, radius: i32) -> (i32, i32, i32, i32) {
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
        min_q.saturating_sub(pad).max(-radius),
        max_q.saturating_add(pad).min(radius),
        min_r.saturating_sub(pad).max(-radius),
        max_r.saturating_add(pad).min(radius),
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

fn redraw(state: &AppState) {
    let (width, height) = sync_canvas_size();
    let ctx = context();
    let radius = state.map_radius.max(0);
    let (size, ox, oy) = map_layout(state, width, height);

    ctx.clear_rect(0.0, 0.0, width, height);
    ctx.set_fill_style_str("#0e1113");
    ctx.fill_rect(0.0, 0.0, width, height);

    let (q_min, q_max, r_min, r_max) = visible_scan_bounds(width, height, size, ox, oy, radius);
    let mut drawn_cells = 0usize;
    for q in q_min..=q_max {
        for r in r_min..=r_max {
            if (q.abs() + r.abs() + (q + r).abs()) / 2 > radius {
                continue;
            }
            let cell = Axial::new(q, r);
            let (x, y) = cell.to_pixel(size);
            let (cx, cy) = (ox + x, oy + y);
            let corners = hex_corners(cx, cy, size * HEX_GAP);

            ctx.begin_path();
            ctx.move_to(corners[0].0, corners[0].1);
            for corner in &corners[1..] {
                ctx.line_to(corner.0, corner.1);
            }
            ctx.close_path();

            let selected = state.selected == Some((q, r));
            let elevation = state.elevation.get(&(q, r)).copied().unwrap_or(1);
            // Fill = hydro projection derived from elevation threshold.
            ctx.set_fill_style_str(hydro_fill(elevation));
            ctx.fill();
            if selected || state.show_grid {
                ctx.set_line_width(if selected { 3.0 } else { 1.0 });
                ctx.set_stroke_style_str(if selected { "#9fe3c4" } else { "#3a424b" });
                ctx.stroke();
            }

            // Profile-presence marker — a separate layer from terrain, so both
            // are visible at once (a cell can have terrain, a profile, or both).
            if state.cells.contains_key(&(q, r)) {
                ctx.begin_path();
                let dot = (size * 0.13).clamp(2.5, 5.0);
                let _ = ctx.arc(cx, cy, dot, 0.0, std::f64::consts::PI * 2.0);
                ctx.set_fill_style_str("#e8d27a");
                ctx.fill();
            }
            drawn_cells += 1;
        }
    }
    set_text(
        "view-stats",
        &format!(
            "Zoom {:.2}x · Draw {} / {} cells · Grid {}",
            state.zoom,
            drawn_cells,
            total_cell_count(radius),
            if state.show_grid { "On" } else { "Off" }
        ),
    );
    draw_preview_boundary(state, &ctx, size, ox, oy);
    set_text("toggle-grid", if state.show_grid { "Cells: On" } else { "Cells: Off" });
    sync_brush_radius_active(state.brush_radius);
}

fn hydro_fill(elevation: i16) -> &'static str {
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
    let Ok(data) = resp.json::<ProjectsResponse>().await else { return };

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
    let Some(list) = document.get_element_by_id("project-list") else { return };
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
            format!(
                "<button class=\"open-btn\" data-path=\"{path}\" type=\"button\">Open</button><button class=\"manage-btn\" type=\"button\">Manage</button>",
                path = html_escape(&p.path)
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
        html.push_str(&format!(
            "<li data-path=\"{path}\"><div class=\"main-row\"><div class=\"info\"><span class=\"id\">{id}</span><span class=\"path\">{path}</span>{missing}</div><div class=\"actions\">{actions}</div></div>{manage_row}</li>",
            id = html_escape(&p.id),
            path = html_escape(&p.path),
            missing = missing,
            actions = actions,
            manage_row = manage_row,
        ));
    }
    list.set_inner_html(&html);
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn suggested_world_path(state: &AppState, world_id: &str) -> Option<String> {
    let root = state.default_worlds_root.as_ref()?;
    let tail = if world_id.trim().is_empty() { "my-world" } else { world_id.trim() };
    let sep = if root.ends_with('\\') || root.ends_with('/') { "" } else { "\\" };
    Some(format!("{root}{sep}{tail}"))
}

fn refresh_suggested_path(state: &Rc<RefCell<AppState>>) {
    let state_ref = state.borrow();
    if state_ref.path_touched {
        return;
    }
    let id = input("new-id").value();
    if let Some(path) = suggested_world_path(&state_ref, &id) {
        input("new-path").set_value(&path);
    }
}

async fn load_map(state: Rc<RefCell<AppState>>) {
    if let Ok(resp) = gloo_net::http::Request::get("/api/map").send().await {
        if let Ok(map) = resp.json::<MapResponse>().await {
            let mut state_mut = state.borrow_mut();
            state_mut.cells = map.cells.into_iter().map(|c| ((c.q, c.r), c.display_name)).collect();
            state_mut.map_radius = map.bounds.radius.max(0);
            state_mut.zoom = 1.0;
            state_mut.pan_x = 0.0;
            state_mut.pan_y = 0.0;
            state_mut.pending_paints.clear();
            state_mut.paint_flush_scheduled = false;
            state_mut.paint_flush_in_flight = false;
            state_mut.legacy_map = map.legacy_map;
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
    redraw(&state.borrow());
}

/// Fetch the dense elevation layer (scale-layers, D-46) and index its concrete
/// integer values by `(q,r)`; unpainted cells stay default land at draw time.
async fn load_elevation(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/layers/elevation").send().await else { return };
    let Ok(layer) = resp.json::<DenseLayer>().await else { return };
    let mut state_mut = state.borrow_mut();
    let bounds = MapBounds::new(state_mut.map_radius.max(0));
    state_mut.elevation.clear();
    for index in 0..layer.len() {
        if let DenseState::Value(LayerValue::Int(v)) = layer.state(index) {
            if let Some(cell) = bounds.from_index(index) {
                state_mut.elevation.insert((cell.q, cell.r), v as i16);
            }
        }
    }
}

fn cell_from_mouse_event(state: &Rc<RefCell<AppState>>, event: &web_sys::MouseEvent) -> Option<(i32, i32)> {
    let canvas = canvas();
    let rect = canvas.get_bounding_client_rect();
    let radius = state.borrow().map_radius.max(0);
    let (size, ox, oy) = map_layout(&state.borrow(), rect.width(), rect.height());
    let mx = event.client_x() as f64 - rect.left() - ox;
    let my = event.client_y() as f64 - rect.top() - oy;
    let cell = Axial::from_pixel(mx, my, size);
    if (cell.q.abs() + cell.r.abs() + (cell.q + cell.r).abs()) / 2 > radius {
        return None;
    }
    Some((cell.q, cell.r))
}

fn in_map_radius(q: i32, r: i32, radius: i32) -> bool {
    (q.abs() + r.abs() + (q + r).abs()) / 2 <= radius
}

fn paint_stamp_cells(center: (i32, i32), radius: i32, map_radius: i32) -> Vec<(i32, i32)> {
    let radius = radius.clamp(MIN_BRUSH_RADIUS, MAX_BRUSH_RADIUS);
    Axial::new(center.0, center.1)
        .range(radius)
        .into_iter()
        .filter(|cell| in_map_radius(cell.q, cell.r, map_radius))
        .map(|cell| (cell.q, cell.r))
        .collect()
}

fn queue_paint_stamp(state: Rc<RefCell<AppState>>, center: (i32, i32), new_elevation: i16) {
    let painted_cells = {
        let mut s = state.borrow_mut();
        let painted = paint_stamp_cells(center, s.brush_radius, s.map_radius.max(0));
        for (q, r) in &painted {
            s.elevation.insert((*q, *r), new_elevation);
            s.pending_paints.insert((*q, *r), new_elevation);
        }
        redraw(&s);
        painted
    };
    if !painted_cells.is_empty() {
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

    let mut failed_cells: Vec<((i32, i32), i16)> = Vec::new();
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
        let sent = gloo_net::http::Request::put("/api/layers/elevation/batch")
            .json(&payload)
            .expect("serializing elevation batch body")
            .send()
            .await;
        if !matches!(sent, Ok(resp) if resp.ok()) {
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

    if !state.borrow().pending_paints.is_empty() {
        set_text("status", "Autosave retry…");
        schedule_paint_flush(state);
    } else {
        set_text("status", "");
    }
}

fn attach_canvas_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        if state.borrow().suppress_next_click {
            state.borrow_mut().suppress_next_click = false;
            return;
        }
        let Some((q, r)) = cell_from_mouse_event(&state, &event) else { return };

        // Hydro brush paints elevation-driven hydro; Inspect opens the
        // author profile panel (unchanged behavior).
        let brush = state.borrow().brush.clone();
        if let Some(new_elevation) = brush_elevation(&brush) {
            queue_paint_stamp(state.clone(), (q, r), new_elevation);
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
    let on_down = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
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
    let _ = canvas().add_event_listener_with_callback("mousedown", on_down.as_ref().unchecked_ref());
    on_down.forget();

    let move_state = state.clone();
    let on_move = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
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
            redraw(&move_state.borrow());
        }
    });
    let _ = window().add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref());
    on_move.forget();

    let up_state = state.clone();
    let on_up = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
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
    let on_down = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        if event.button() != 0 {
            return;
        }
        let brush = down_state.borrow().brush.clone();
        let Some(elevation) = brush_elevation(&brush) else { return };
        let Some((q, r)) = cell_from_mouse_event(&down_state, &event) else { return };
        {
            let mut s = down_state.borrow_mut();
            s.paint_active = true;
            s.paint_moved = false;
            s.paint_last_cell = Some((q, r));
        }
        queue_paint_stamp(down_state.clone(), (q, r), elevation);
    });
    let _ = canvas().add_event_listener_with_callback("mousedown", on_down.as_ref().unchecked_ref());
    on_down.forget();

    let move_state = state.clone();
    let on_move = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        if !move_state.borrow().paint_active {
            return;
        }
        let brush = move_state.borrow().brush.clone();
        let Some(elevation) = brush_elevation(&brush) else { return };
        let Some((q, r)) = cell_from_mouse_event(&move_state, &event) else { return };
        let should_paint = {
            let mut s = move_state.borrow_mut();
            if s.paint_last_cell == Some((q, r)) {
                false
            } else {
                s.paint_moved = true;
                true
            }
        };
        if should_paint {
            // Keep path faithful to real cursor movement: no straight-line
            // interpolation between sparse samples.
            queue_paint_stamp(move_state.clone(), (q, r), elevation);
            move_state.borrow_mut().paint_last_cell = Some((q, r));
        }
    });
    let _ = window().add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref());
    on_move.forget();

    let up_state = state.clone();
    let on_up = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
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
    let on_move = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        let next_hover = if matches!(move_state.borrow().brush, Brush::Inspect) {
            None
        } else {
            cell_from_mouse_event(&move_state, &event)
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
            redraw(&move_state.borrow());
        }
    });
    let _ = canvas().add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref());
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
            redraw(&leave_state.borrow());
        }
    });
    let _ = canvas().add_event_listener_with_callback("mouseleave", on_leave.as_ref().unchecked_ref());
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
        let radius = s.map_radius.max(0);
        let (base_size, base_ox, base_oy) = hex_layout(rect.width(), rect.height(), radius);
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
        redraw(&state.borrow());
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
        let Some((q, r)) = state.borrow().selected else { return };
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
            let body = ProfileInput { display_name: display_name.clone(), notes };
            let sent = gloo_net::http::Request::put(&format!("/api/cells/{q}/{r}/profile"))
                .json(&body)
                .expect("serializing profile body")
                .send()
                .await;
            match sent {
                Ok(resp) if resp.ok() => {
                    state.borrow_mut().cells.insert((q, r), display_name);
                    redraw(&state.borrow());
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
        redraw(&state.borrow());
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
        let state = state.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = gloo_net::http::Request::post("/api/projects/close").send().await;
            let mut state_mut = state.borrow_mut();
            state_mut.cells.clear();
            state_mut.elevation.clear();
            state_mut.selected = None;
            state_mut.map_radius = DEFAULT_MAP_RADIUS;
            state_mut.zoom = 1.0;
            state_mut.pan_x = 0.0;
            state_mut.pan_y = 0.0;
            state_mut.paint_active = false;
            state_mut.paint_moved = false;
            state_mut.paint_last_cell = None;
            state_mut.show_grid = true;
            state_mut.legacy_map = false;
            set_drawer_open(false);
            clear_inspect_selection();
            set_world_label("—");
            set_text("legacy-map-note", "");
            drop(state_mut);
            wasm_bindgen_futures::spawn_local(refresh_projects(state));
        });
    });
    document()
        .get_element_by_id("switch-world")
        .expect("missing #switch-world")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching switch-world handler");
    closure.forget();
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
            let body = CreateProjectInput { id: &id, path: &path, map_preset: &preset };
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
                    let msg = resp.text().await.unwrap_or_else(|_| "create failed".to_string());
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
fn attach_generate_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        let id = input("generate-id").value();
        let path = input("generate-path").value();
        if id.trim().is_empty() || path.trim().is_empty() {
            set_text("generate-status", "World name and folder are both required.");
            return;
        }
        let preset = select_value("generate-preset");
        let state = state.clone();
        set_text("generate-status", "Generating…");
        wasm_bindgen_futures::spawn_local(async move {
            let body = CreateProjectInput { id: &id, path: &path, map_preset: &preset };
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
                    show_view("editor");
                    wasm_bindgen_futures::spawn_local(load_map(state));
                }
                Ok(resp) => {
                    let msg = resp.text().await.unwrap_or_else(|_| "generate failed".to_string());
                    set_text("generate-status", &msg);
                }
                Err(err) => set_text("generate-status", &format!("Error: {err}")),
            }
        });
    });
    document()
        .get_element_by_id("generate")
        .expect("missing #generate")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching generate handler");
    closure.forget();
}

/// Redraw on window resize so the map keeps filling the viewport (4.2
/// fit-to-window). Cheap — one redraw per resize event.
fn attach_window_resize(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        redraw(&state.borrow());
    });
    let _ = window().add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn attach_new_id_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        refresh_suggested_path(&state);
    });
    let _ = input("new-id").add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn attach_new_path_input(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        state.borrow_mut().path_touched = true;
    });
    let _ = input("new-path").add_event_listener_with_callback("input", closure.as_ref().unchecked_ref());
    closure.forget();
}

/// Map a brush to target elevation. `Inspect` writes nothing.
fn brush_elevation(brush: &Brush) -> Option<i16> {
    match brush {
        Brush::Inspect => None,
        Brush::SetLand => Some(1),
        Brush::SetWater => Some(0),
    }
}

/// Tool dock: rail tabs toggle drawers; hydro swatches set the active brush.
fn attach_dock_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };

        if let Ok(Some(button)) = target.closest("[data-view-toggle]") {
            let Some(kind) = button.get_attribute("data-view-toggle") else { return };
            if kind == "grid" {
                let show_grid = {
                    let mut s = state.borrow_mut();
                    s.show_grid = !s.show_grid;
                    s.show_grid
                };
                button.set_text_content(Some(if show_grid { "Cells: On" } else { "Cells: Off" }));
                redraw(&state.borrow());
            }
            return;
        }

        if let Ok(Some(button)) = target.closest("[data-brush-size]") {
            let Some(raw_size) = button.get_attribute("data-brush-size") else { return };
            let Ok(radius) = raw_size.parse::<i32>() else { return };
            {
                let mut s = state.borrow_mut();
                s.brush_radius = radius.clamp(MIN_BRUSH_RADIUS, MAX_BRUSH_RADIUS);
            }
            sync_brush_radius_active(state.borrow().brush_radius);
            return;
        }

        if let Ok(Some(button)) = target.closest("[data-dock]") {
            let Some(tab) = button.get_attribute("data-dock") else { return };
            toggle_dock_tab(&tab);
            if tab == "inspect" {
                let brush = Brush::Inspect;
                {
                    let mut s = state.borrow_mut();
                    s.brush = brush.clone();
                    s.hover_cell = None;
                    s.paint_active = false;
                    s.paint_moved = false;
                    s.paint_last_cell = None;
                    s.suppress_next_click = false;
                }
                sync_dock_rail_for_brush(&brush);
                sync_brush_swatch_active(&brush);
            }
            return;
        }

        let Ok(Some(button)) = target.closest("[data-brush]") else { return };
        let Some(kind) = button.get_attribute("data-brush") else { return };

        let brush = match kind.as_str() {
            "land" => Brush::SetLand,
            "water" => Brush::SetWater,
            _ => return,
        };
        {
            let mut s = state.borrow_mut();
            s.brush = brush.clone();
            s.hover_cell = None;
            s.paint_active = false;
            s.paint_moved = false;
            s.paint_last_cell = None;
            s.suppress_next_click = false;
        }
        open_dock_tab("terrain");
        sync_dock_rail_for_brush(&brush);
        sync_brush_swatch_active(&brush);
    });
    document()
        .get_element_by_id("tool-dock")
        .expect("missing #tool-dock")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching dock handler");
    closure.forget();
}

fn attach_escape_key() {
    let closure = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
        if event.key() == "Escape" {
            set_drawer_open(false);
        }
    });
    let _ = document().add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());
    closure.forget();
}

/// "Browse…" button, desktop shell only (roadmap 5.9, D-29) — the button is
/// `display:none` in a plain browser tab (see `index.html`), so attaching a
/// listener here is harmless either way; it just never fires without Tauri.
fn attach_browse_folder_click() {
    let Some(button) = document().get_element_by_id("browse-folder") else { return };
    let closure = Closure::<dyn FnMut()>::new(move || {
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(path) = pick_folder_via_tauri().await {
                input("new-path").set_value(&path);
            }
        });
    });
    let _ = button.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
    closure.forget();
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
    let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
        if let Ok(Some(button)) = target.closest(".manage-btn") {
            let Some(row) = button.closest("li").ok().flatten() else { return };
            if row.class_list().contains("manage-open") {
                let _ = row.class_list().remove_1("manage-open");
            } else {
                let _ = row.class_list().add_1("manage-open");
            }
            return;
        }
        if let Ok(Some(button)) = target.closest(".delete-btn") {
            let Some(path) = button.get_attribute("data-path") else { return };
            if !button.class_list().contains("armed") {
                let _ = button.class_list().add_1("armed");
                button.set_text_content(Some("Delete now"));
                set_text("home-status", "Click \"Delete now\" again to permanently remove this world from disk.");
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
                        let msg = resp.text().await.unwrap_or_else(|_| "delete failed".to_string());
                        set_text("home-status", &msg);
                    }
                    Err(err) => set_text("home-status", &format!("Error: {err}")),
                }
            });
            return;
        }
        if let Ok(Some(button)) = target.closest(".remove-btn") {
            let Some(path) = button.get_attribute("data-path") else { return };
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
                        let msg = resp.text().await.unwrap_or_else(|_| "remove failed".to_string());
                        set_text("home-status", &msg);
                    }
                    Err(err) => set_text("home-status", &format!("Error: {err}")),
                }
            });
            return;
        }

        let Ok(Some(button)) = target.closest(".open-btn") else { return };
        let Some(path) = button.get_attribute("data-path") else { return };

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
                    wasm_bindgen_futures::spawn_local(load_map(state));
                }
                Ok(resp) => {
                    let msg = resp.text().await.unwrap_or_else(|_| "open failed".to_string());
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
