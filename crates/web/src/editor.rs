//! Editor: tool dock, canvas input, paint/river handlers (D-94 B4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::api::{
    delete_river_at_cell, flush_pending_paints, load_profile_into_panel, post_lake_generate,
    post_river_append, post_river_detach, post_river_generate, post_river_pin, post_river_pop, schedule_paint_flush,
};
use crate::brush::{
    active_dock_tab, apply_elevation_brush_intent, apply_paint_brush, brush_absolute_elevation,
    brush_delta_sign, brush_paints, clear_pointer_interaction, deactivate_paint_brush,
    effective_brush_radius_from_hex_size, effective_paint_radius, river_brush,
    sync_brush_effective_label, sync_brush_radius_active, sync_brush_step_active,
    sync_falloff_active, sync_manual_river_authoring_ui, sync_paint_tool_ui, sync_river_status,
    sync_detach_tributary_ui,
    terrain_brush, RIVERS_READ_ONLY_MSG,
};
use crate::canvas::{clamp_zoom, current_hex_size_px, hex_layout, map_layout, schedule_redraw};
use crate::dom::{
    active_attr_in_group, canvas, click_target_element, document, drawer_is_open, input,
    set_dock_tab, set_drawer_open, set_text, textarea, toggle_active_in_group, window,
};
use crate::elevation_view::ColorMode;
use crate::state::{
    bump_content_rev, elevation_at, grid_lines_toggle_label, set_elevation_cell, AppState, Brush,
    ProfileInput, MAX_BRUSH_TIER, MAX_EFFECTIVE_BRUSH_RADIUS, MIN_BRUSH_TIER, PAN_DRAG_THRESHOLD,
};
use crate::wizard::{
    close_build_wizard, queue_wizard_land_mask_stamp, schedule_wizard_stamp_flush,
    wizard_is_active, wizard_return_home,
};
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::hydro::stamp_delta;
use mapkeeper_core::river_detach::tributary_at_cell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

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
/// Cell-id label shown to the author — placeholder UI, real schema is 3.2.
fn cell_label(q: i32, r: i32) -> String {
    format!("hex q{q} r{r}")
}
pub(crate) fn cell_from_mouse_event(
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

pub(crate) fn paint_stamp_cells(
    center: (i32, i32),
    brush_radius: i32,
    map_bounds: MapBounds,
) -> Vec<(i32, i32)> {
    let brush = brush_radius.clamp(0, MAX_EFFECTIVE_BRUSH_RADIUS);
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
        let radius = effective_brush_radius_from_hex_size(s.brush_radius, current_hex_size_px(&s));
        let painted = paint_stamp_cells(center, radius, map_bounds);
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
        let brush_radius =
            effective_brush_radius_from_hex_size(s.brush_radius, current_hex_size_px(&s));
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

pub fn attach_canvas_click(state: Rc<RefCell<AppState>>) {
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
                let edit_mode = state.borrow().wizard_edit_mode;
                if edit_mode {
                    // Drag already painted; click is for single stamp when not dragged.
                    queue_wizard_land_mask_stamp(state.clone(), (q, r));
                }
                return;
            }

            // Hydro brush paints elevation-driven hydro; Inspect opens the
            // author profile panel (unchanged behavior).
            let brush = state.borrow().brush.clone();
            if state.borrow().rivers_read_only && river_brush(&brush) {
                set_text("river-status", RIVERS_READ_ONLY_MSG);
                return;
            }
            if matches!(brush, Brush::RiverPin) {
                let mut s = state.borrow_mut();
                if let Some(source) = s.river_pin_source {
                    let mouth = (q, r);
                    s.river_pin_source = None;
                    drop(s);
                    wasm_bindgen_futures::spawn_local(post_river_pin(state.clone(), source, mouth));
                } else if let Some(index) = s.map_bounds.index_of(Axial::new(q, r)) {
                    if let Some(id) = tributary_at_cell(&s.rivers, index) {
                        s.active_river_id = Some(id);
                        s.river_pin_source = None;
                        drop(s);
                        sync_river_status(&state.borrow());
                        sync_detach_tributary_ui(&state.borrow());
                    } else {
                        s.river_pin_source = Some((q, r));
                        set_text("river-status", "Pin: now click mouth cell");
                    }
                } else {
                    s.river_pin_source = Some((q, r));
                    set_text("river-status", "Pin: now click mouth cell");
                }
                return;
            }
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
pub fn attach_pan_drag(state: Rc<RefCell<AppState>>) {
    let down_state = state.clone();
    let on_down =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if event.button() != 0 {
                return;
            }
            // Wizard silhouette Edit owns LMB drag; do not start viewport pan.
            if wizard_is_active() && down_state.borrow().wizard_edit_mode {
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
pub fn attach_paint_drag(state: Rc<RefCell<AppState>>) {
    let down_state = state.clone();
    let on_down =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            if event.button() != 0 {
                return;
            }
            // Wizard land edit: same stamp+drag as editor (D-43 sizes).
            if wizard_is_active() && down_state.borrow().wizard_edit_mode {
                let Some((q, r)) = cell_from_mouse_event(&down_state, &event) else {
                    return;
                };
                {
                    let mut s = down_state.borrow_mut();
                    s.paint_active = true;
                    s.paint_moved = false;
                    s.paint_last_cell = Some((q, r));
                    s.wizard_stamp_last_center = None;
                    // mousedown already stamps; skip the following click.
                    s.suppress_next_click = true;
                }
                queue_wizard_land_mask_stamp(down_state.clone(), (q, r));
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
            if wizard_is_active() && move_state.borrow().wizard_edit_mode {
                queue_wizard_land_mask_stamp(move_state.clone(), (q, r));
                move_state.borrow_mut().paint_last_cell = Some((q, r));
                return;
            }
            let brush = move_state.borrow().brush.clone();
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
                s.wizard_stamp_last_center = None;
                if moved {
                    s.suppress_next_click = true;
                }
            }
            if !wizard_is_active() {
                wasm_bindgen_futures::spawn_local(flush_pending_paints(up_state.clone()));
            } else if up_state.borrow().wizard_edit_mode
                || !up_state.borrow().pending_wizard_stamps.is_empty()
            {
                schedule_wizard_stamp_flush(up_state.clone());
            }
        });
    let _ = window().add_event_listener_with_callback("mouseup", on_up.as_ref().unchecked_ref());
    on_up.forget();
}

pub fn attach_brush_hover_preview(state: Rc<RefCell<AppState>>) {
    let move_state = state.clone();
    let on_move =
        Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |event: web_sys::MouseEvent| {
            let next_hover = {
                let s = move_state.borrow();
                let show = brush_paints(&s.brush) || (wizard_is_active() && s.wizard_edit_mode);
                if !show {
                    None
                } else if pointer_over_wizard_chrome(&event) {
                    // Right/left panels overlay hit-testing; clear preview there.
                    None
                } else {
                    drop(s);
                    cell_from_mouse_event(&move_state, &event)
                }
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
    // Window-level so moving onto wiz-right clears hover before size clicks.
    let _ =
        window().add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref());
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

fn pointer_over_wizard_chrome(event: &web_sys::MouseEvent) -> bool {
    let Some(el) = click_target_element(event) else {
        return false;
    };
    el.closest(".wiz-left, .wiz-right, .wiz-top, #wiz-confirm-overlay")
        .ok()
        .flatten()
        .is_some()
}

/// Wheel zoom with cursor anchor; max from target on-screen hex px (D-85).
pub fn attach_wheel_zoom(state: Rc<RefCell<AppState>>) {
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
        let new_zoom = clamp_zoom(base_size, s.zoom * factor);
        s.zoom = new_zoom;
        let new_size = base_size * s.zoom;
        let new_ox = mx - world_x * new_size;
        let new_oy = my - world_y * new_size;
        s.pan_x = new_ox - base_ox;
        s.pan_y = new_oy - base_oy;
        drop(s);
        sync_brush_effective_label(&state.borrow());
        schedule_redraw(state.clone());
    });
    let _ = canvas().add_event_listener_with_callback("wheel", closure.as_ref().unchecked_ref());
    closure.forget();
}

pub fn attach_save_click(state: Rc<RefCell<AppState>>) {
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

pub fn attach_close_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        state.borrow_mut().selected = None;
        set_drawer_open(false);
        crate::dom::clear_inspect_panel();
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
pub fn attach_switch_world_click(state: Rc<RefCell<AppState>>) {
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
pub fn attach_generate_lakes_click(state: Rc<RefCell<AppState>>) {
    {
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            if el.get_attribute("data-editor-lake-density").is_some() {
                toggle_active_in_group("editor-lake-densities", "data-editor-lake-density", &el);
            } else if el.get_attribute("data-editor-river-density").is_some() {
                toggle_active_in_group("editor-river-densities", "data-editor-river-density", &el);
            }
        });
        if let Ok(Some(root)) = document().query_selector("[data-drawer=\"rivers\"]") {
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let density = active_attr_in_group(
                "editor-lake-densities",
                "data-editor-lake-density",
                "balanced",
            );
            wasm_bindgen_futures::spawn_local(post_lake_generate(
                state.clone(),
                "water-gen-status",
                density,
            ));
        });
        document()
            .get_element_by_id("generate-lakes")
            .expect("missing #generate-lakes")
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .expect("attaching generate-lakes handler");
        closure.forget();
    }
}

pub fn attach_generate_rivers_click(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut()>::new(move || {
        let density = active_attr_in_group(
            "editor-river-densities",
            "data-editor-river-density",
            "balanced",
        );
        wasm_bindgen_futures::spawn_local(post_river_generate(
            state.clone(),
            "water-gen-status",
            density,
        ));
    });
    document()
        .get_element_by_id("generate-rivers")
        .expect("missing #generate-rivers")
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("attaching generate-rivers handler");
    closure.forget();
}

pub fn attach_window_resize(state: Rc<RefCell<AppState>>) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        {
            let canvas = canvas();
            let rect = canvas.get_bounding_client_rect();
            let mut s = state.borrow_mut();
            let (base_size, _, _) = hex_layout(rect.width(), rect.height(), s.map_bounds);
            s.zoom = clamp_zoom(base_size, s.zoom);
        }
        schedule_redraw(state.clone());
    });
    let _ = window().add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref());
    closure.forget();
}
pub fn attach_dock_click(state: Rc<RefCell<AppState>>) {
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
                let eff = effective_paint_radius(&state.borrow());
                if eff == 0 && !even {
                    return;
                }
                state.borrow_mut().falloff_even = even;
                sync_falloff_active(even, eff);
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
                    s.brush_radius = radius.clamp(MIN_BRUSH_TIER, MAX_BRUSH_TIER);
                    if s.brush_radius == 0 {
                        s.falloff_even = true;
                    }
                }
                let s = state.borrow();
                sync_brush_radius_active(s.brush_radius);
                sync_brush_effective_label(&s);
                let eff = effective_paint_radius(&s);
                sync_falloff_active(s.falloff_even, eff);
                drop(s);
                schedule_redraw(state.clone());
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
                            if s.rivers_read_only {
                                s.brush = Brush::Inspect;
                            } else if !river_brush(&s.brush) {
                                s.brush = s.last_river_brush.clone();
                            }
                            s.hover_cell = None;
                            s.brush.clone()
                        };
                        sync_paint_tool_ui(&brush);
                        sync_river_status(&state.borrow());
                        sync_manual_river_authoring_ui(&state.borrow());
                        if brush_paints(&brush) {
                            schedule_redraw(state.clone());
                        }
                    }
                    _ => {}
                }
                return;
            }

            if let Ok(Some(button)) = target.closest("[data-river-action]") {
                if state.borrow().rivers_read_only {
                    set_text("river-status", RIVERS_READ_ONLY_MSG);
                    return;
                }
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
                    "detach" => {
                        wasm_bindgen_futures::spawn_local(post_river_detach(state.clone()));
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
                "river-pin" => {
                    if state.borrow().rivers_read_only {
                        set_text("river-status", RIVERS_READ_ONLY_MSG);
                        return;
                    }
                    Brush::RiverPin
                }
                "river" => {
                    if state.borrow().rivers_read_only {
                        set_text("river-status", RIVERS_READ_ONLY_MSG);
                        return;
                    }
                    Brush::River
                }
                "river-erase" => {
                    if state.borrow().rivers_read_only {
                        set_text("river-status", RIVERS_READ_ONLY_MSG);
                        return;
                    }
                    Brush::RiverErase
                }
                _ => return,
            };
            {
                let mut s = state.borrow_mut();
                apply_paint_brush(&mut s, brush.clone());
                if matches!(brush, Brush::RiverPin) {
                    s.river_pin_source = None;
                }
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

pub fn attach_escape_key() {
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
