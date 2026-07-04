//! WASM UI — calls mapkeeper-core for hex geometry + profile rules;
//! filesystem goes through the `mapkeeper-server` HTTP API, never direct
//! FS access. WASM framework choice: none — plain `wasm-bindgen` + `web-sys`
//! canvas, deliberately minimal for the flow-first pass (roadmap D-21).
//!
//! Flow: render a blank hex grid -> click a cell -> edit a placeholder
//! profile (title + notes) -> save -> cell is "painted". No real cell
//! schema (roadmap 3.2) yet — that is the point of this pass.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mapkeeper_core::hex::Axial;
use mapkeeper_core::profile::CellProfile;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement, HtmlTextAreaElement};

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

struct AppState {
    cells: HashMap<(i32, i32), String>,
    selected: Option<(i32, i32)>,
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();

    let state = Rc::new(RefCell::new(AppState { cells: HashMap::new(), selected: None }));

    redraw(&state.borrow());
    attach_canvas_click(state.clone());
    attach_save_click(state.clone());
    attach_close_click(state.clone());

    wasm_bindgen_futures::spawn_local(load_map(state));
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
        set_text("status", "Loading…");

        wasm_bindgen_futures::spawn_local(load_profile_into_panel(cell.q, cell.r));
    });
    canvas().set_onclick(Some(closure.as_ref().unchecked_ref()));
    closure.forget();
}

async fn load_profile_into_panel(q: i32, r: i32) {
    let url = format!("/api/cells/{q}/{r}/profile");
    let profile = gloo_net::http::Request::get(&url)
        .send()
        .await
        .ok()
        .and_then(|resp| resp.ok().then_some(resp));
    let Some(resp) = profile else {
        set_text("status", "Could not load profile");
        return;
    };
    if let Ok(profile) = resp.json::<CellProfile>().await {
        input("title").set_value(&profile.display_name);
        textarea("notes").set_value(&profile.notes);
    }
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
