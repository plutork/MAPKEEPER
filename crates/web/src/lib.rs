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
use std::rc::Rc;

use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::hex::Axial;
use mapkeeper_core::hydro::{hydro_from_elevation, ElevationLayer, HydroKind};
use mapkeeper_core::profile::CellProfile;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Element, HtmlCanvasElement, HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};

const DEFAULT_MAP_RADIUS: i32 = 6;
const MIN_ZOOM: f64 = 0.6;
const MAX_ZOOM: f64 = 2.5;
const PAN_DRAG_THRESHOLD: f64 = 3.0;
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
    value.max(MIN_ZOOM).min(MAX_ZOOM)
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
            ctx.set_line_width(if selected { 3.0 } else { 1.0 });
            ctx.set_stroke_style_str(if selected { "#9fe3c4" } else { "#3a424b" });
            ctx.stroke();

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
            "Zoom {:.2}x · Draw {} / {} cells",
            state.zoom,
            drawn_cells,
            total_cell_count(radius)
        ),
    );
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

/// Fetch sparse elevation overrides; missing keys remain default land.
async fn load_elevation(state: &Rc<RefCell<AppState>>) {
    let Ok(resp) = gloo_net::http::Request::get("/api/layers/elevation").send().await else { return };
    let Ok(layer) = resp.json::<ElevationLayer>().await else { return };
    let mut state_mut = state.borrow_mut();
    state_mut.elevation.clear();
    for (cell_id, value) in layer.cells {
        let Some(id) = CellId::parse(&cell_id) else { continue };
        state_mut.elevation.insert((id.q, id.r), value);
    }
}

fn attach_canvas_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        let canvas = canvas();
        let rect = canvas.get_bounding_client_rect();
        let radius = state.borrow().map_radius.max(0);
        if state.borrow().suppress_next_click {
            state.borrow_mut().suppress_next_click = false;
            return;
        }
        // Hit-testing uses the same camera-aware layout as redraw.
        let (size, ox, oy) = map_layout(&state.borrow(), rect.width(), rect.height());
        let mx = event.client_x() as f64 - rect.left() - ox;
        let my = event.client_y() as f64 - rect.top() - oy;
        let cell = Axial::from_pixel(mx, my, size);
        if (cell.q.abs() + cell.r.abs() + (cell.q + cell.r).abs()) / 2 > radius {
            return;
        }

        // Hydro brush paints elevation-driven hydro; Inspect opens the
        // author profile panel (unchanged behavior).
        let brush = state.borrow().brush.clone();
        if let Some(new_elevation) = brush_elevation(&brush) {
            wasm_bindgen_futures::spawn_local(paint_elevation(
                state.clone(),
                cell.q,
                cell.r,
                new_elevation,
            ));
            return;
        }

        state.borrow_mut().selected = Some((cell.q, cell.r));
        open_dock_tab("inspect");
        set_text("panel-cell", &cell_label(cell.q, cell.r));
        input("title").set_value("");
        textarea("notes").set_value("");
        // Disabled while loading — otherwise a fast typist can fill the
        // fields before the fetch below resolves, and the (still pending)
        // response then silently overwrites what they just typed.
        input("title").set_disabled(true);
        textarea("notes").set_disabled(true);
        set_text("status", "Loading…");

        wasm_bindgen_futures::spawn_local(load_profile_into_panel(state.clone(), cell.q, cell.r));
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

/// PUT elevation for a cell, then mirror locally and redraw. The
/// filesystem write happens server-side (D-20); the WASM UI never touches FS.
async fn paint_elevation(state: Rc<RefCell<AppState>>, q: i32, r: i32, new_elevation: i16) {
    set_text("status", "Painting…");
    let sent = gloo_net::http::Request::put(&format!("/api/cells/{q}/{r}/elevation"))
        .json(&new_elevation)
        .expect("serializing elevation body")
        .send()
        .await;
    match sent {
        Ok(resp) if resp.ok() => {
            let mut state_mut = state.borrow_mut();
            state_mut.elevation.insert((q, r), new_elevation);
            redraw(&state_mut);
            drop(state_mut);
            set_text("status", "");
        }
        _ => set_text("status", "Paint failed."),
    }
}

/// Tool dock: rail tabs toggle drawers; hydro swatches set the active brush.
fn attach_dock_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };

        if let Ok(Some(button)) = target.closest("[data-dock]") {
            let Some(tab) = button.get_attribute("data-dock") else { return };
            toggle_dock_tab(&tab);
            if tab == "inspect" {
                let brush = Brush::Inspect;
                state.borrow_mut().brush = brush.clone();
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
        state.borrow_mut().brush = brush.clone();
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
