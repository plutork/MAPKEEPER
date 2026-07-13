//! World Build Wizard UI and handlers (D-94 B3).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::api::{
    ensure_pending_saved_or_discard, flush_pending_paints, handle_mutate_response,
    load_elevation, load_geology, load_map, persist_build_draft_for, post_lake_generate,
    post_river_generate, refresh_projects, scoped_mutate_from_state, scoped_request,
};
use crate::mutate_retry::{classify_http_status, paint_stop_message, wizard_stamp_flush_action, MutateErrorKind, PaintFlushAction};
use crate::brush::{effective_brush_radius_from_hex_size, paint_stamp_cells, reset_view_on_world_open};
use crate::canvas::{current_hex_size_px, schedule_redraw};
use crate::dom::{
    active_attr_in_group, clear_inspect_panel, click_target_element, document,
    hide_post_finish_note, hide_wiz_confirm, select_value, set_drawer_open, set_select_value,
    set_text, set_world_label, show_post_finish_note, show_wiz_confirm, sync_preset_size_warning,
    toggle_active_in_group,
};
use crate::state::{
    bump_content_rev, fresh_elevation_layer, set_elevation_cell, AppState, BuildBoundsInput,
    BuildBoundsResponse, BuildStateInput, WizConfirmKind, WizardClimateGenerateInput,
    WizardElevationGenerateInput, WizardGeologyGenerateInput, WizardLandMaskCellInput,
    WizardLandMaskGenerateInput, WizardLandMaskGenerateResponse, MAX_BRUSH_RADIUS,
    MIN_BRUSH_RADIUS, PAINT_BATCH_MAX_CELLS, PAINT_SAVE_COOLDOWN_MS,
};
use crate::water_diag::sync_water_diagnostics;
use gloo_timers::future::TimeoutFuture;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::land_mask::{find_recipe, next_recipe, pick_recipe, LayoutClass};
use mapkeeper_core::map_preset::MapPreset;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement};

pub(crate) fn build_step_label(step: u32) -> &'static str {
    match step {
        1 => "Step 1 · Size & grid",
        3 => "Step 3 · Tectonics",
        4 => "Step 4 · Elevation",
        5 => "Step 5 · Climate",
        6 => "Step 6 · Water",
        _ => "Step 2 · Land silhouette",
    }
}

// home-build-draft-v1: persist wizard draft on active world.

fn wizard_back_message(from_step: u32) -> &'static str {
    match from_step {
        6 => "Go back to climate? You can regenerate rivers later.",
        5 => "Go back to elevation? You can regenerate climate later.",
        4 => "Go back to tectonics? You can regenerate elevation later.",
        3 => "Go back to land silhouette? Geology accept state stays until you regenerate.",
        2 => "Go back to map size? Changing the preset will reset Geo if land already exists.",
        _ => "Go back to the previous wizard step?",
    }
}

/// Apply wizard preset via PUT /api/build/bounds when different from current bounds (D-69).
/// If reset confirm needed, queues in-app dialog and returns false (caller waits for OK).
async fn apply_wizard_preset_if_needed(
    state: Rc<RefCell<AppState>>,
    preset: &str,
    pending_confirm: &Rc<RefCell<Option<WizConfirmKind>>>,
) -> bool {
    let (same, has_down) = {
        let s = state.borrow();
        let same = preset_id_for_bounds(&s.map_bounds) == Some(preset);
        (same, wizard_has_downstream(&s))
    };
    if same {
        return true;
    }
    if has_down {
        *pending_confirm.borrow_mut() = Some(WizConfirmKind::BoundsPreset(preset.to_string()));
        show_wiz_confirm(
            "Changing map size will reset land silhouette, geology, and elevation. Continue?",
        );
        return false;
    }
    apply_wizard_preset_now(state, preset).await
}

async fn apply_wizard_preset_now(state: Rc<RefCell<AppState>>, preset: &str) -> bool {
    let body = BuildBoundsInput { map_preset: preset };
    let Ok(resp) = scoped_mutate_from_state(&state, gloo_net::http::Request::put("/api/build/bounds"))
        .json(&body)
        .expect("serialize bounds")
        .send()
        .await
    else {
        set_wizard_status("Could not change map size (network).");
        return false;
    };
    handle_mutate_response(&state, &resp);
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Size change rejected".to_string());
        set_wizard_status(&msg);
        return false;
    }
    let _ = resp.json::<BuildBoundsResponse>().await;
    {
        let mut s = state.borrow_mut();
        s.wizard_accepted = false;
        s.wizard_edit_mode = false;
        clear_wizard_stamp_pending(&mut s);
        s.wizard_geo_accepted = false;
        s.geology = None;
        s.wizard_recipe_id.clear();
        s.wizard_gen_seed = None;
        s.wizard_regenerate_nonce = 0;
        s.wizard_step = 1;
    }
    load_map(state.clone()).await;
    {
        let mut s = state.borrow_mut();
        s.show_grid = true;
        sync_wizard_actions(&s);
    }
    schedule_redraw(state);
    set_wizard_status("Map size updated. Blank grid ready.");
    true
}

/// D-71: navigate back one Geo step (4→3→2→1).
async fn wizard_go_back_one_step(state: Rc<RefCell<AppState>>) {
    let from = state.borrow().wizard_step;
    let to = match from {
        6 => 5,
        5 => 4,
        4 => 3,
        3 => 2,
        2 => 1,
        _ => 1,
    };
    if !persist_build_draft_for(&state, to).await {
        set_wizard_status("Could not go back.");
        return;
    }
    {
        let mut s = state.borrow_mut();
        s.wizard_step = to;
        s.wizard_edit_mode = false;
        if to <= 1 {
            s.show_grid = true;
            if let Some(id) = preset_id_for_bounds(&s.map_bounds) {
                set_select_value("wiz-preset", id);
            }
            sync_preset_size_warning("wiz-preset", "wiz-preset-warn");
        }
        sync_wizard_actions(&s);
    }
    schedule_redraw(state.clone());
    match to {
        1 => set_wizard_status(
            "Map size — change preset to resize (resets Geo if you already generated land).",
        ),
        2 => set_wizard_status("Land silhouette — Back to size if the map is too large/small."),
        3 => set_wizard_status("Tectonics — Back returns to silhouette."),
        4 => set_wizard_status("Elevation — Back returns to tectonics."),
        5 => set_wizard_status("Climate — Back returns to elevation."),
        _ => {}
    }
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

pub fn open_build_wizard() {
    hide_post_finish_note();
    set_wizard_active(true);
}

pub(crate) fn close_build_wizard() {
    set_wizard_active(false);
}

pub(crate) fn set_wizard_status(msg: &str) {
    set_text("wizard-status", msg);
}

pub(crate) fn wizard_is_active() -> bool {
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
            1 => "Geo › Size & grid",
            3 => "Geo › Tectonics",
            4 => "Geo › Elevation",
            5 => "Climate › Precipitation",
            6 => "Water › Lakes & rivers",
            _ => "Geo › Land silhouette",
        };
        crumb.set_text_content(Some(text));
    }
    if let Ok(Some(list)) = document().query_selector(".wiz-group.expanded .wiz-steps") {
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
                let step_num = i + 1;
                if step_num < step.min(5) {
                    let _ = el.class_list().add_1("done");
                } else if step_num == step && step < 5 {
                    let _ = el.class_list().add_1("active");
                } else if step >= 5 {
                    let _ = el.class_list().add_1("done");
                } else {
                    let _ = el.class_list().add_1("locked");
                }
            }
        }
    }
    if let Some(climate_group) = document().get_element_by_id("wiz-group-climate") {
        if step >= 5 {
            let _ = climate_group.class_list().remove_1("locked");
            let _ = climate_group.class_list().add_1("expanded");
            if let Ok(Some(head)) = climate_group.query_selector(".wiz-group-head") {
                let _ = head.remove_attribute("disabled");
                head.set_text_content(Some("▼ Climate"));
            }
            if let Ok(Some(list)) = climate_group.query_selector(".wiz-steps") {
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
                        if step == 5 {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().add_1("done");
                        }
                    }
                }
            }
        } else {
            let _ = climate_group.class_list().add_1("locked");
            let _ = climate_group.class_list().remove_1("expanded");
            if let Ok(Some(head)) = climate_group.query_selector(".wiz-group-head") {
                let _ = head.set_attribute("disabled", "");
                head.set_text_content(Some("▶ Climate"));
            }
        }
    }
    if let Some(water_group) = document().get_element_by_id("wiz-group-water") {
        if step >= 6 {
            let _ = water_group.class_list().remove_1("locked");
            let _ = water_group.class_list().add_1("expanded");
            if let Ok(Some(head)) = water_group.query_selector(".wiz-group-head") {
                let _ = head.remove_attribute("disabled");
                head.set_text_content(Some("▼ Water"));
            }
            if let Ok(Some(list)) = water_group.query_selector(".wiz-steps") {
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
                        if step == 6 {
                            let _ = el.class_list().add_1("active");
                        } else {
                            let _ = el.class_list().add_1("done");
                        }
                    }
                }
            }
        } else {
            let _ = water_group.class_list().add_1("locked");
            let _ = water_group.class_list().remove_1("expanded");
            if let Ok(Some(head)) = water_group.query_selector(".wiz-group-head") {
                let _ = head.set_attribute("disabled", "");
                head.set_text_content(Some("▶ Water"));
            }
        }
    }
}

fn show_wizard_step(state: &AppState) {
    let step = state.wizard_step;
    set_panel_hidden("wiz-panel-step1", step != 1);
    set_panel_hidden("wiz-panel-step2", step != 2);
    set_panel_hidden("wiz-panel-step3", step != 3);
    set_panel_hidden("wiz-panel-step4", step != 4);
    set_panel_hidden("wiz-panel-step5", step != 5);
    set_panel_hidden("wiz-panel-step6", step != 6);
    sync_wizard_nav(step);
    if step <= 1 {
        sync_wizard_size_meta(state);
    }
    match step {
        1 => set_wizard_status("Confirm map size on the blank grid, then continue."),
        3 => set_wizard_status("Step 3: generate geology, accept, continue."),
        4 => set_wizard_status("Step 4: pick relief style, generate elevation, then continue."),
        5 => {
            set_wizard_status("Step 5: pick precipitation style, generate climate, then continue.")
        }
        6 => {
            set_wizard_status("Step 6: generate lakes, then rivers from climate rainfall.");
            sync_water_diagnostics(state);
        }
        _ => set_wizard_status(
            "Step 2 flow: 1) parameters, 2) generate, 3) accept/edit, 4) continue.",
        ),
    }
}

fn sync_wizard_size_meta(state: &AppState) {
    let (w, h) = (state.map_bounds.width, state.map_bounds.height);
    let n = state.map_bounds.len();
    let line = format!("{w}×{h} hex-rectangle · {n} cells");
    set_text("wiz-size-meta", &line);
}

pub(crate) fn preset_id_for_bounds(bounds: &MapBounds) -> Option<&'static str> {
    for p in MapPreset::wizard_presets() {
        let (w, h) = p.dimensions();
        if w == bounds.width && h == bounds.height {
            return Some(p.id());
        }
    }
    None
}

fn wizard_has_downstream(state: &AppState) -> bool {
    if state.wizard_accepted || state.wizard_geo_accepted || state.wizard_step > 2 {
        return true;
    }
    if state.geology.is_some() {
        return true;
    }
    // Land painted into elevation (land_mask sync writes elev=1).
    for i in 0..state.elevation.len() {
        if state.elevation.int_or(i, 0) > 0 {
            return true;
        }
    }
    false
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
pub(crate) fn ensure_wizard_recipe(state: &mut AppState) {
    if !state.wizard_recipe_id.is_empty() {
        if let Some(recipe) = find_recipe(&state.wizard_recipe_id) {
            if recipe.layout_class.id() == state.wizard_layout_class {
                return;
            }
        }
    }
    let class = LayoutClass::parse(&state.wizard_layout_class);
    let seed = (state.wizard_regenerate_nonce as u64).wrapping_mul(0x9E37_79B9) ^ 0x00C0_FFEE;
    let recipe = pick_recipe(class, seed);
    state.wizard_layout_class = class.id().to_string();
    state.wizard_recipe_id = recipe.id.to_string();
}

fn pick_wizard_recipe_for_class(state: &mut AppState, class_id: &str) {
    let class = LayoutClass::parse(class_id);
    let seed = (state.wizard_regenerate_nonce as u64).wrapping_mul(0x9E37_79B9) ^ 0x00C0_FFEE;
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

/// D-68: quiet identity line under Regenerate (dogfood chrome).
fn sync_wizard_gen_identity(state: &AppState) {
    let recipe = if state.wizard_recipe_id.is_empty() {
        "—"
    } else {
        state.wizard_recipe_id.as_str()
    };
    let seed = state
        .wizard_gen_seed
        .map(|s| format!("0x{s:016x}"))
        .unwrap_or_else(|| "—".to_string());
    set_text(
        "wiz-gen-identity",
        &format!(
            "class: {} · recipe: {} · shore: {} · nonce: {} · seed: {}",
            state.wizard_layout_class,
            recipe,
            state.wizard_character,
            state.wizard_regenerate_nonce,
            seed
        ),
    );
}

fn sync_wizard_edit_mode_ui(edit_mode: bool, brush: &str, brush_radius: i32) {
    for id in [
        "wiz-edit-brushes",
        "wiz-edit-sizes",
        "wiz-edit-size-label",
        "wiz-edit-size-hint",
    ] {
        if let Some(el) = document().get_element_by_id(id) {
            if edit_mode {
                let _ = el.class_list().remove_1("hidden");
            } else {
                let _ = el.class_list().add_1("hidden");
            }
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
    if let Ok(Some(root)) = document().query_selector("#wiz-edit-sizes") {
        if let Ok(list) = root.query_selector_all("[data-wiz-edit-size]") {
            for i in 0..list.length() {
                let Some(node) = list.item(i) else {
                    continue;
                };
                let Ok(el) = node.dyn_into::<web_sys::Element>() else {
                    continue;
                };
                let radius = el
                    .get_attribute("data-wiz-edit-size")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(-1);
                if radius == brush_radius {
                    let _ = el.class_list().add_1("active");
                } else {
                    let _ = el.class_list().remove_1("active");
                }
            }
        }
    }
}

pub(crate) fn sync_wizard_actions(state: &AppState) {
    set_button_disabled("wiz-regenerate", false);
    set_button_disabled("wiz-accept", false);
    set_button_disabled("wiz-edit", !state.wizard_accepted);
    set_button_disabled("wiz-continue", !state.wizard_accepted);
    set_button_disabled("wiz-geo-continue", !state.wizard_geo_accepted);
    sync_wizard_layout_buttons(&state.wizard_layout_class);
    sync_wizard_edit_mode_ui(
        state.wizard_edit_mode,
        &state.wizard_edit_brush,
        state.wizard_brush_radius,
    );
    sync_wizard_gen_identity(state);
    show_wizard_step(state);
}

async fn generate_wizard_land_mask(state: Rc<RefCell<AppState>>) {
    set_wizard_generating(true);
    set_wizard_status("Generating silhouette… (can take a moment on large maps)");
    let (recipe_id, character, layout_class, nonce) = {
        let mut s = state.borrow_mut();
        clear_wizard_stamp_pending(&mut s);
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
    let world_id = state.borrow().scoped_world_id.clone();
    let Ok(resp) = scoped_request(
        gloo_net::http::Request::post("/api/build/land-mask/generate"),
        world_id.as_deref(),
    )
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
    if let Ok(identity) = resp.json::<WizardLandMaskGenerateResponse>().await {
        let mut s = state.borrow_mut();
        s.wizard_gen_seed = Some(identity.seed);
        if !identity.recipe_id.is_empty() {
            s.wizard_recipe_id = identity.recipe_id;
        }
        if !identity.layout_class.is_empty() {
            s.wizard_layout_class = identity.layout_class;
        }
        if !identity.character.is_empty() {
            s.wizard_character = identity.character;
        }
        s.wizard_regenerate_nonce = identity.regenerate_nonce as u32;
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
    let world_id = state.borrow().scoped_world_id.clone();
    let Ok(resp) = scoped_request(
        gloo_net::http::Request::post("/api/build/geology/generate"),
        world_id.as_deref(),
    )
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
    set_wizard_status("Geology generated — classes shown on the map (see legend).");
}

async fn generate_wizard_elevation(state: Rc<RefCell<AppState>>) {
    let (style, nonce) = {
        let s = state.borrow();
        (s.wizard_elev_style.clone(), s.wizard_elev_nonce)
    };
    let body = WizardElevationGenerateInput {
        style: &style,
        regenerate_nonce: nonce,
    };
    let world_id = state.borrow().scoped_world_id.clone();
    let Ok(resp) = scoped_request(
        gloo_net::http::Request::post("/api/build/elevation/generate"),
        world_id.as_deref(),
    )
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

async fn generate_wizard_climate(state: Rc<RefCell<AppState>>) {
    let (style, nonce) = {
        let s = state.borrow();
        (s.wizard_climate_style.clone(), s.wizard_climate_nonce)
    };
    let body = WizardClimateGenerateInput {
        style: &style,
        regenerate_nonce: nonce,
    };
    let world_id = state.borrow().scoped_world_id.clone();
    let Ok(resp) = scoped_request(
        gloo_net::http::Request::post("/api/build/climate/generate"),
        world_id.as_deref(),
    )
        .json(&body)
        .expect("serialize climate generate")
        .send()
        .await
    else {
        set_wizard_status("Climate generation failed (network).");
        return;
    };
    if !resp.ok() {
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "Climate generation rejected".to_string());
        set_wizard_status(&msg);
        return;
    }
    schedule_redraw(state.clone());
    set_wizard_status("Climate generated — temperature and precipitation layers saved.");
}

/// Merge stamp cells into pending wizard edits (no I/O — optimistic path guard).
pub(crate) fn merge_wizard_stamp_pending(
    pending: &mut HashMap<(i32, i32), bool>,
    cells: &[(i32, i32)],
    land: bool,
) {
    for &cell in cells {
        pending.insert(cell, land);
    }
}

fn clear_wizard_stamp_pending(s: &mut AppState) {
    s.pending_wizard_stamps.clear();
    s.wizard_stamp_flush_scheduled = false;
    s.wizard_stamp_flush_in_flight = false;
    s.wizard_stamp_last_center = None;
}

/// Optimistic local stamp; persist only on mouseup / leave-edit (no mid-drag HTTP).
/// One stamp per new center cell (`paint_last_cell` already dedupes) — do not
/// skip centers by brush radius (that broke M/L/XL short strokes).
pub(crate) fn queue_wizard_land_mask_stamp(state: Rc<RefCell<AppState>>, center: (i32, i32)) {
    let painted = {
        let mut s = state.borrow_mut();
        let land = s.wizard_edit_brush == "land";
        let radius =
            effective_brush_radius_from_hex_size(s.wizard_brush_radius, current_hex_size_px(&s));
        let cells = paint_stamp_cells(center, radius, s.map_bounds);
        if cells.is_empty() {
            0
        } else {
            let value = if land { 1 } else { 0 };
            let bounds = s.map_bounds;
            for &(q, r) in &cells {
                set_elevation_cell(&mut s.elevation, bounds, q, r, value);
            }
            merge_wizard_stamp_pending(&mut s.pending_wizard_stamps, &cells, land);
            s.wizard_stamp_last_center = Some(center);
            bump_content_rev(&mut s);
            cells.len()
        }
    };
    if painted == 0 {
        return;
    }
    schedule_redraw(state.clone());
    // Status once — avoid DOM thrash every mousemove.
    let pending_n = state.borrow().pending_wizard_stamps.len();
    if pending_n == painted {
        set_wizard_status("Edit pending — release mouse to save.");
    }
}

pub(crate) fn schedule_wizard_stamp_flush(state: Rc<RefCell<AppState>>) {
    let should_schedule = {
        let mut s = state.borrow_mut();
        if s.wizard_stamp_flush_scheduled || s.pending_wizard_stamps.is_empty() {
            false
        } else {
            s.wizard_stamp_flush_scheduled = true;
            true
        }
    };
    if !should_schedule {
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        // Short yield so mouseup UI stays responsive before heavy save.
        TimeoutFuture::new(0).await;
        flush_wizard_land_mask_stamps(state).await;
    });
}

async fn flush_wizard_land_mask_stamps(state: Rc<RefCell<AppState>>) {
    if state.borrow().wizard_stamp_autosave_blocked {
        set_wizard_status("Land edit blocked — reload world to continue");
        return;
    }

    let (batch, need_retry) = {
        let mut s = state.borrow_mut();
        s.wizard_stamp_flush_scheduled = false;
        if s.wizard_stamp_flush_in_flight {
            let retry = !s.pending_wizard_stamps.is_empty();
            if retry {
                s.wizard_stamp_flush_scheduled = true;
            }
            (None, retry)
        } else if s.pending_wizard_stamps.is_empty() {
            (None, false)
        } else {
            s.wizard_stamp_flush_in_flight = true;
            (
                Some(s.pending_wizard_stamps.drain().collect::<Vec<_>>()),
                false,
            )
        }
    };
    if need_retry {
        let retry = state.clone();
        wasm_bindgen_futures::spawn_local(async move {
            TimeoutFuture::new(PAINT_SAVE_COOLDOWN_MS).await;
            flush_wizard_land_mask_stamps(retry).await;
        });
        return;
    }
    let Some(batch) = batch else {
        return;
    };

    set_wizard_status("Saving edit…");
    let mut failed: Vec<((i32, i32), bool)> = Vec::new();
    let mut stop_scheduling = false;

    'chunks: for chunk in batch.chunks(PAINT_BATCH_MAX_CELLS.max(1)) {
        let kinds: Vec<&'static str> = chunk
            .iter()
            .map(|(_, land)| if *land { "land" } else { "ocean" })
            .collect();
        let payload: Vec<WizardLandMaskCellInput<'_>> = chunk
            .iter()
            .zip(kinds.iter())
            .map(|(((q, r), _), kind)| WizardLandMaskCellInput { q: *q, r: *r, kind })
            .collect();
        let mut attempt = state.borrow().wizard_stamp_retry_attempts;

        loop {
            let sent = scoped_mutate_from_state(
                &state,
                gloo_net::http::Request::put("/api/build/land-mask/cells"),
            )
            .json(&payload)
            .expect("serialize wizard land_mask batch")
            .send()
            .await;
            let kind = match &sent {
                Err(_) => Some(MutateErrorKind::Network),
                Ok(resp) if resp.ok() => {
                    handle_mutate_response(&state, resp);
                    None
                }
                Ok(resp) => Some(classify_http_status(resp.status())),
            };
            let action = wizard_stamp_flush_action(kind, attempt);
            match action {
                PaintFlushAction::Success => {
                    state.borrow_mut().wizard_stamp_retry_attempts = 0;
                    continue 'chunks;
                }
                PaintFlushAction::Retry {
                    next_attempt,
                    delay_ms,
                } => {
                    set_wizard_status(paint_stop_message(action));
                    state.borrow_mut().wizard_stamp_retry_attempts = next_attempt;
                    attempt = next_attempt;
                    TimeoutFuture::new(delay_ms).await;
                }
                PaintFlushAction::StopConflict | PaintFlushAction::StopPermanent => {
                    failed.extend(chunk.iter().copied());
                    stop_scheduling = true;
                    if matches!(action, PaintFlushAction::StopConflict) {
                        state.borrow_mut().wizard_stamp_autosave_blocked = true;
                    }
                    set_wizard_status(paint_stop_message(action));
                    break 'chunks;
                }
                PaintFlushAction::ReloadAndRebase => break 'chunks,
            }
        }
    }

    {
        let mut s = state.borrow_mut();
        for (cell, land) in failed {
            s.pending_wizard_stamps.insert(cell, land);
        }
        s.wizard_stamp_flush_in_flight = false;
    }
    if state.borrow().pending_wizard_stamps.is_empty() {
        set_wizard_status("Edit saved.");
    } else if !stop_scheduling {
        let attempts = state.borrow().wizard_stamp_retry_attempts;
        if attempts > 0 && attempts < crate::mutate_retry::MUTATE_MAX_RETRY_ATTEMPTS {
            schedule_wizard_stamp_flush(state);
        }
    }
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

pub fn attach_wizard_handlers(state: Rc<RefCell<AppState>>) {
    // Shared pending confirm for Back / bounds reset (in-app dialog, not window.confirm).
    let pending_confirm: Rc<RefCell<Option<WizConfirmKind>>> = Rc::new(RefCell::new(None));

    {
        let state = state.clone();
        let pending = pending_confirm.clone();
        let on_ok = Closure::<dyn FnMut()>::new(move || {
            let kind = pending.borrow_mut().take();
            hide_wiz_confirm();
            let Some(kind) = kind else {
                return;
            };
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match kind {
                    WizConfirmKind::Back => wizard_go_back_one_step(state).await,
                    WizConfirmKind::BoundsPreset(preset) => {
                        let _ = apply_wizard_preset_now(state, &preset).await;
                    }
                }
            });
        });
        if let Some(btn) = document().get_element_by_id("wiz-confirm-ok") {
            let _ = btn.add_event_listener_with_callback("click", on_ok.as_ref().unchecked_ref());
        }
        on_ok.forget();
    }
    {
        let pending = pending_confirm.clone();
        let on_cancel = Closure::<dyn FnMut()>::new(move || {
            *pending.borrow_mut() = None;
            hide_wiz_confirm();
            set_wizard_status("Cancelled.");
        });
        if let Some(btn) = document().get_element_by_id("wiz-confirm-cancel") {
            let _ =
                btn.add_event_listener_with_callback("click", on_cancel.as_ref().unchecked_ref());
        }
        on_cancel.forget();
    }

    // Save Draft — flush pending paints; world already on disk from create.
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            set_wizard_status("Saving…");
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                flush_pending_paints(state.clone()).await;
                let step = state.borrow().wizard_step.max(2);
                if persist_build_draft_for(&state, step).await {
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

    // wizard-merge-size-grid (D-71): size+blank grid → silhouette.
    {
        let state = state.clone();
        let pending = pending_confirm.clone();
        let on_change = Closure::<dyn FnMut()>::new(move || {
            sync_preset_size_warning("wiz-preset", "wiz-preset-warn");
            let preset = select_value("wiz-preset");
            let state = state.clone();
            let pending = pending.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let step = state.borrow().wizard_step;
                if step > 1 {
                    return;
                }
                let _ = apply_wizard_preset_if_needed(state, &preset, &pending).await;
            });
        });
        if let Ok(Some(select)) = document().query_selector("#wiz-preset") {
            let _ = select
                .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
        }
        on_change.forget();
    }
    {
        let state = state.clone();
        let pending = pending_confirm.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let preset = select_value("wiz-preset");
            let state = state.clone();
            let pending = pending.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if !apply_wizard_preset_if_needed(state.clone(), &preset, &pending).await {
                    return;
                }
                if !persist_build_draft_for(&state, 2).await {
                    set_wizard_status("Could not advance to silhouette.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 2;
                    s.show_grid = false;
                    ensure_wizard_recipe(&mut s);
                    sync_wizard_actions(&s);
                }
                set_wizard_status("Pick a layout class and generate a silhouette.");
                schedule_redraw(state.clone());
            });
        });
        if let Some(btn) = document().get_element_by_id("wiz-size-continue") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    for back_id in [
        "wiz-sil-back",
        "wiz-geo-back",
        "wiz-elev-back",
        "wiz-climate-back",
        "wiz-water-back",
    ] {
        let state = state.clone();
        let pending = pending_confirm.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let from = state.borrow().wizard_step;
            *pending.borrow_mut() = Some(WizConfirmKind::Back);
            show_wiz_confirm(wizard_back_message(from));
        });
        if let Some(btn) = document().get_element_by_id(back_id) {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
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
                let mut s = state.borrow_mut();
                s.wizard_character = character;
                s.wizard_gen_seed = None;
                sync_wizard_gen_identity(&s);
                set_wizard_status("Shore updated. Regenerate or pick a layout class.");
            }
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-chars") {
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
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
            let (edit_now, should_flush) = {
                let mut s = state.borrow_mut();
                if !s.wizard_accepted {
                    set_wizard_status("Accept a silhouette first.");
                    return;
                }
                s.wizard_edit_mode = !s.wizard_edit_mode;
                let edit_now = s.wizard_edit_mode;
                sync_wizard_actions(&s);
                (edit_now, !edit_now && !s.pending_wizard_stamps.is_empty())
            };
            if should_flush {
                schedule_wizard_stamp_flush(state.clone());
            }
            if edit_now {
                set_wizard_status(
                    "Edit mode: paint land/ocean — S–XL follows zoom (zoom out = larger stamp).",
                );
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

    // Brush size 1x–4x (D-43) for wizard land edit on large maps.
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            let Ok(Some(btn)) = el.closest("[data-wiz-edit-size]") else {
                return;
            };
            let Some(raw) = btn.get_attribute("data-wiz-edit-size") else {
                return;
            };
            let Ok(radius) = raw.parse::<i32>() else {
                return;
            };
            let mut s = state.borrow_mut();
            s.wizard_brush_radius = radius.clamp(MIN_BRUSH_RADIUS, MAX_BRUSH_RADIUS);
            // Panel sits over the map: keep last hover → huge preview redraw freeze.
            s.hover_cell = None;
            sync_wizard_actions(&s);
            // No map redraw required to flip S/M/L/XL active state.
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-edit-sizes") {
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }

    // Continue: step 2 → draft step 3 (tectonics).
    {
        let id = "wiz-continue";
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let accepted = state.borrow().wizard_accepted;
            if !accepted {
                set_wizard_status("Accept a silhouette first.");
                return;
            }
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if !persist_build_draft_for(&state, 3).await {
                    set_wizard_status("Could not advance to tectonics.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 3;
                    s.wizard_edit_mode = false;
                    s.wizard_geo_accepted = false;
                    s.wizard_geo_nonce = 0;
                    sync_wizard_actions(&s);
                }
                set_wizard_status("Step 3 · Tectonics — generate geology.");
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

    // Step 3 geology style / generate / accept / continue.
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
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
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
                if !persist_build_draft_for(&state, 4).await {
                    set_wizard_status("Could not advance to elevation.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 4;
                    s.wizard_elev_nonce = 0;
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
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            if let Some(style) = el.get_attribute("data-wiz-elev-style") {
                wiz_toggle_style_group("wiz-elev-styles", "data-wiz-elev-style", &el);
                state.borrow_mut().wizard_elev_style = style;
                set_wizard_status("Relief style updated — Generate to apply.");
            }
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-elev-styles") {
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            {
                let mut s = state.borrow_mut();
                s.wizard_elev_nonce = s.wizard_elev_nonce.saturating_add(1);
            }
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
                if !persist_build_draft_for(&state, 5).await {
                    set_wizard_status("Could not advance to climate.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 5;
                    s.wizard_climate_nonce = 0;
                    sync_wizard_actions(&s);
                }
                wasm_bindgen_futures::spawn_local(generate_wizard_climate(state.clone()));
            });
        });
        if let Some(btn) = document().get_element_by_id("wiz-elev-continue") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            if let Some(style) = el.get_attribute("data-wiz-climate-style") {
                wiz_toggle_style_group("wiz-climate-styles", "data-wiz-climate-style", &el);
                state.borrow_mut().wizard_climate_style = style;
                set_wizard_status("Precipitation style updated — Generate to apply.");
            }
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-climate-styles") {
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            {
                let mut s = state.borrow_mut();
                s.wizard_climate_nonce = s.wizard_climate_nonce.saturating_add(1);
            }
            set_wizard_status("Generating climate…");
            wasm_bindgen_futures::spawn_local(generate_wizard_climate(state.clone()));
        });
        if let Some(btn) = document().get_element_by_id("wiz-climate-generate") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let state = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if !persist_build_draft_for(&state, 6).await {
                    set_wizard_status("Could not advance to water.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 6;
                    sync_wizard_actions(&s);
                }
                set_wizard_status("Step 6: generate lakes, then rivers.");
            });
        });
        if let Some(btn) = document().get_element_by_id("wiz-climate-continue") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Ok(mouse) = event.dyn_into::<web_sys::MouseEvent>() else {
                return;
            };
            let Some(el) = click_target_element(&mouse) else {
                return;
            };
            if let Some(density) = el.get_attribute("data-wiz-lake-density") {
                toggle_active_in_group("wiz-lake-densities", "data-wiz-lake-density", &el);
                set_wizard_status("Lake density updated — Generate to apply.");
                let _ = density;
            } else if let Some(density) = el.get_attribute("data-wiz-river-density") {
                toggle_active_in_group("wiz-river-densities", "data-wiz-river-density", &el);
                set_wizard_status("River density updated — Generate to apply.");
                let _ = density;
            }
        });
        if let Ok(Some(root)) = document().query_selector("#wiz-panel-step6") {
            let _ =
                root.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let density =
                active_attr_in_group("wiz-lake-densities", "data-wiz-lake-density", "balanced");
            set_wizard_status("Generating lakes…");
            wasm_bindgen_futures::spawn_local(post_lake_generate(
                state.clone(),
                "wizard-status",
                density,
            ));
        });
        if let Some(btn) = document().get_element_by_id("wiz-lake-generate") {
            let _ = btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let density =
                active_attr_in_group("wiz-river-densities", "data-wiz-river-density", "balanced");
            set_wizard_status("Rebuilding rivers…");
            wasm_bindgen_futures::spawn_local(post_river_generate(
                state.clone(),
                "wizard-status",
                density,
            ));
        });
        if let Some(btn) = document().get_element_by_id("wiz-water-generate") {
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
                    step: 6,
                };
                let Ok(resp) = scoped_mutate_from_state(&state, gloo_net::http::Request::put("/api/build"))
                    .json(&body)
                    .expect("serialize build complete")
                    .send()
                    .await
                else {
                    set_wizard_status("Could not finish build.");
                    return;
                };
                handle_mutate_response(&state, &resp);
                if !resp.ok() {
                    set_wizard_status("Could not finish build.");
                    return;
                }
                {
                    let mut s = state.borrow_mut();
                    s.wizard_step = 2;
                    s.wizard_accepted = false;
                    s.wizard_geo_accepted = false;
                    s.wizard_edit_mode = false;
                    sync_wizard_actions(&s);
                }
                close_build_wizard();
                set_wizard_status("Build finished.");
                show_post_finish_note();
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

pub(crate) async fn wizard_return_home(state: Rc<RefCell<AppState>>) {
    if !ensure_pending_saved_or_discard(state.clone()).await {
        return;
    }
    if document()
        .get_element_by_id("editor")
        .is_some_and(|el| el.class_list().contains("wizard-active"))
    {
        let step = state.borrow().wizard_step.max(2);
        let _ = persist_build_draft_for(&state, step).await;
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
    state_mut.pending_paints.clear();
    state_mut.paint_autosave_blocked = false;
    state_mut.paint_retry_attempts = 0;
    state_mut.paint_rebased_after_conflict = false;
    state_mut.pending_wizard_stamps.clear();
    state_mut.wizard_stamp_autosave_blocked = false;
    state_mut.wizard_stamp_retry_attempts = 0;
    state_mut.show_grid = false;
    state_mut.scoped_world_id = None;
    reset_view_on_world_open(&mut state_mut);
    state_mut.legacy_map = false;
    state_mut.wizard_accepted = false;
    state_mut.wizard_edit_mode = false;
    state_mut.wizard_layout_class = "pangea".to_string();
    state_mut.wizard_regenerate_nonce = 0;
    state_mut.wizard_recipe_id.clear();
    state_mut.wizard_gen_seed = None;
    state_mut.wizard_step = 1;
    state_mut.wizard_geo_style = "belts".to_string();
    state_mut.wizard_geo_nonce = 0;
    state_mut.wizard_elev_style = "standard".to_string();
    state_mut.wizard_elev_nonce = 0;
    state_mut.wizard_climate_style = "balanced".to_string();
    state_mut.wizard_climate_nonce = 0;
    state_mut.wizard_geo_accepted = false;
    state_mut.geology = None;
    set_drawer_open(false);
    clear_inspect_panel();
    set_world_label("—");
    set_text("legacy-map-note", "");
    sync_wizard_actions(&state_mut);
    drop(state_mut);
    wasm_bindgen_futures::spawn_local(refresh_projects(state));
}
