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

use mapkeeper_core::hex::Axial;
use mapkeeper_core::profile::CellProfile;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Element, HtmlCanvasElement, HtmlInputElement, HtmlTextAreaElement};

const RADIUS: i32 = 6;
const HEX_SIZE: f64 = 34.0;

#[derive(Deserialize)]
struct MapResponse {
    #[allow(dead_code)]
    world_id: String,
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
    id: String,
    path: String,
    valid: bool,
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

struct AppState {
    cells: HashMap<(i32, i32), String>,
    selected: Option<(i32, i32)>,
    default_worlds_root: Option<String>,
    path_touched: bool,
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();

    let state = Rc::new(RefCell::new(AppState {
        cells: HashMap::new(),
        selected: None,
        default_worlds_root: None,
        path_touched: false,
    }));

    redraw(&state.borrow());
    attach_canvas_click(state.clone());
    attach_save_click(state.clone());
    attach_close_click(state.clone());
    attach_switch_world_click(state.clone());
    attach_create_click(state.clone());
    attach_project_list_click(state.clone());
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

fn set_panel_open(open: bool) {
    if let Some(panel) = document().get_element_by_id("panel") {
        if open {
            let _ = panel.class_list().add_1("open");
        } else {
            let _ = panel.class_list().remove_1("open");
        }
    }
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

fn redraw(state: &AppState) {
    let canvas = canvas();
    let ctx = context();
    let width = canvas.width() as f64;
    let height = canvas.height() as f64;
    let (ox, oy) = (width / 2.0, height / 2.0);

    ctx.clear_rect(0.0, 0.0, width, height);
    ctx.set_fill_style_str("#0e1113");
    ctx.fill_rect(0.0, 0.0, width, height);

    for q in -RADIUS..=RADIUS {
        for r in -RADIUS..=RADIUS {
            if (q.abs() + r.abs() + (q + r).abs()) / 2 > RADIUS {
                continue;
            }
            let cell = Axial::new(q, r);
            let (x, y) = cell.to_pixel(HEX_SIZE);
            let (cx, cy) = (ox + x, oy + y);
            let corners = hex_corners(cx, cy, HEX_SIZE * 0.92);

            ctx.begin_path();
            ctx.move_to(corners[0].0, corners[0].1);
            for corner in &corners[1..] {
                ctx.line_to(corner.0, corner.1);
            }
            ctx.close_path();

            let painted = state.cells.contains_key(&(q, r));
            let selected = state.selected == Some((q, r));
            ctx.set_fill_style_str(if painted { "#2f6b4f" } else { "#1c2126" });
            ctx.fill();
            ctx.set_line_width(if selected { 3.0 } else { 1.0 });
            ctx.set_stroke_style_str(if selected { "#9fe3c4" } else { "#333940" });
            ctx.stroke();
        }
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
        let missing = if p.valid { "" } else { "<div class=\"missing\">folder not found</div>" };
        let actions = if p.valid {
            format!(
                "<button class=\"open-btn\" data-path=\"{path}\">Open</button><button class=\"forget-btn\" data-path=\"{path}\">Forget</button><button class=\"delete-btn\" data-path=\"{path}\">Delete</button>",
                path = html_escape(&p.path)
            )
        } else {
            format!(
                "<button class=\"forget-btn\" data-path=\"{path}\">Forget</button>",
                path = html_escape(&p.path)
            )
        };
        html.push_str(&format!(
            "<li data-path=\"{path}\"><div class=\"info\"><span class=\"id\">{id}</span><span class=\"path\">{path}</span>{missing}</div><div class=\"actions\">{actions}</div></li>",
            id = html_escape(&p.id),
            path = html_escape(&p.path),
            missing = missing,
            actions = actions,
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
    let Ok(resp) = gloo_net::http::Request::get("/api/map").send().await else { return };
    let Ok(map) = resp.json::<MapResponse>().await else { return };
    let mut state_mut = state.borrow_mut();
    state_mut.cells = map.cells.into_iter().map(|c| ((c.q, c.r), c.display_name)).collect();
    redraw(&state_mut);
}

fn attach_canvas_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
        let canvas = canvas();
        let rect = canvas.get_bounding_client_rect();
        let mx = event.client_x() as f64 - rect.left() - canvas.width() as f64 / 2.0;
        let my = event.client_y() as f64 - rect.top() - canvas.height() as f64 / 2.0;
        let cell = Axial::from_pixel(mx, my, HEX_SIZE);
        if (cell.q.abs() + cell.r.abs() + (cell.q + cell.r).abs()) / 2 > RADIUS {
            return;
        }

        state.borrow_mut().selected = Some((cell.q, cell.r));
        redraw(&state.borrow());
        set_panel_open(true);
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
        redraw(&state.borrow());
        set_panel_open(false);
        set_text("status", "");
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
            state_mut.selected = None;
            set_panel_open(false);
            set_text("status", "");
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
            set_text("home-status", "World id and folder are both required.");
            return;
        }
        let state = state.clone();
        set_text("home-status", "Creating…");
        wasm_bindgen_futures::spawn_local(async move {
            let body = CreateProjectInput { id: &id, path: &path };
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
        if let Ok(Some(button)) = target.closest(".delete-btn") {
            let Some(path) = button.get_attribute("data-path") else { return };
            if !window()
                .confirm_with_message(&format!(
                    "Delete world folder from disk?\n\n{}\n\nThis cannot be undone.",
                    path
                ))
                .unwrap_or(false)
            {
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
        if let Ok(Some(button)) = target.closest(".forget-btn") {
            let Some(path) = button.get_attribute("data-path") else { return };
            let state = state.clone();
            set_text("home-status", "Forgetting…");
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
                        let msg = resp.text().await.unwrap_or_else(|_| "forget failed".to_string());
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
