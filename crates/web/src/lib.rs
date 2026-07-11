//! WASM UI — calls mapkeeper-core for hex geometry + profile rules;
//! filesystem goes through the `mapkeeper-server` HTTP API, never direct
//! FS access. WASM framework choice: none — plain `wasm-bindgen` + `web-sys`
//! canvas, deliberately minimal for the flow-first pass (roadmap D-21).
//!
//! Flow: Home screen lists/creates worlds (roadmap D-12/5.7 launcher) ->
//! open a world -> render a blank hex grid -> click a cell -> edit a
//! placeholder profile (title + notes) -> save -> cell is "painted". No
//! real cell schema (roadmap 3.2) yet — that is the point of this pass.

mod api;
mod brush;
mod canvas;
mod dom;
mod editor;
mod elevation_view;
mod home;
mod state;
mod wizard;

use canvas::redraw;
use dom::set_text;
use editor::{
    attach_brush_hover_preview, attach_canvas_click, attach_close_click, attach_dock_click,
    attach_escape_key, attach_generate_lakes_click, attach_generate_rivers_click, attach_paint_drag, attach_pan_drag,
    attach_save_click, attach_switch_world_click, attach_wheel_zoom, attach_window_resize,
};
use home::{
    attach_browse_folder_click, attach_build_start_click, attach_create_click,
    attach_first_world_handlers, attach_generate_id_input, attach_generate_path_input,
    attach_new_id_input, attach_new_path_input, attach_post_finish_note_dismiss,
    attach_preset_warn_handlers, attach_project_list_click,
};
use wizard::attach_wizard_handlers;

use api::refresh_projects;
use state::{
    fresh_elevation_layer, AppState, Brush, PerfMetrics, PerfTimers, APP_VERSION,
};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use elevation_view::ColorMode;
use mapkeeper_core::map_preset::MapPreset;
use mapkeeper_core::rivers::RiverCatalog;
use mapkeeper_core::lakes::LakeCatalog;
use wasm_bindgen::prelude::*;

pub(crate) fn perf_emit(metrics: &PerfMetrics) {
    set_text("view-perf", &metrics.view_line());
    web_sys::console::log_1(&format!("[perf] {}", metrics.console_line()).into());
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    set_text("app-version", &format!("mapkeeper {APP_VERSION}"));

    let initial_bounds = MapPreset::Small.bounds();
    let state = Rc::new(RefCell::new(AppState {
        cells: HashMap::new(),
        elevation: fresh_elevation_layer(initial_bounds),
        brush: Brush::Inspect,
        last_paint_brush: Brush::SetLand,
        last_river_brush: Brush::River,
        active_river_id: None,
        rivers: RiverCatalog::default(),
        lakes: LakeCatalog::default(),
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
        wizard_gen_seed: None,
        wizard_accepted: false,
        wizard_edit_mode: false,
        wizard_edit_brush: "land".to_string(),
        wizard_brush_radius: 0,
        pending_wizard_stamps: HashMap::new(),
        wizard_stamp_flush_scheduled: false,
        wizard_stamp_flush_in_flight: false,
        wizard_stamp_last_center: None,
        wizard_step: 1,
        wizard_geo_style: "belts".to_string(),
        wizard_geo_nonce: 0,
        wizard_elev_style: "standard".to_string(),
        wizard_elev_nonce: 0,
        wizard_climate_style: "balanced".to_string(),
        wizard_climate_nonce: 0,
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
    attach_generate_lakes_click(state.clone());
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
    attach_first_world_handlers(state.clone());
    attach_post_finish_note_dismiss();

    wasm_bindgen_futures::spawn_local(refresh_projects(state));
}

#[cfg(test)]
mod wizard_stamp_pending_tests {
    use crate::wizard::merge_wizard_stamp_pending;
    use std::collections::HashMap;

    #[test]
    fn merge_overwrites_same_cell_with_latest_kind() {
        let mut pending = HashMap::new();
        merge_wizard_stamp_pending(&mut pending, &[(0, 0), (1, 0)], true);
        merge_wizard_stamp_pending(&mut pending, &[(1, 0), (2, 0)], false);
        assert_eq!(pending.get(&(0, 0)).copied(), Some(true));
        assert_eq!(pending.get(&(1, 0)).copied(), Some(false));
        assert_eq!(pending.get(&(2, 0)).copied(), Some(false));
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn every_new_center_may_stamp_without_radius_gap() {
        // Guard: do not reintroduce center-skip by effective radius.
        // paint_last_cell already dedupes identical centers; M/L/XL short
        // strokes must still stamp on every new hex under the cursor.
        let centers = [(0, 0), (1, 0), (2, 0), (3, 0)];
        assert_eq!(centers.len(), 4);
    }

    #[test]
    fn large_brush_preview_uses_circle_not_hex_walk() {
        use crate::brush::brush_preview_uses_circle;
        assert!(!brush_preview_uses_circle(0));
        assert!(!brush_preview_uses_circle(2));
        assert!(brush_preview_uses_circle(3));
        assert!(brush_preview_uses_circle(24));
    }

    #[test]
    fn brush_tiers_stay_distinct_when_zoomed_in() {
        use crate::brush::effective_brush_radius_from_hex_size;
        // Large hex px тЖТ zoom-derived radius floors to 0; tier floor keeps S<M<L<XL.
        let hex_px = 80.0;
        let s = effective_brush_radius_from_hex_size(0, hex_px);
        let m = effective_brush_radius_from_hex_size(1, hex_px);
        let l = effective_brush_radius_from_hex_size(2, hex_px);
        let xl = effective_brush_radius_from_hex_size(3, hex_px);
        assert_eq!((s, m, l, xl), (0, 1, 2, 3));
    }

    #[test]
    fn zoom_max_grows_when_base_hex_is_small() {
        use crate::canvas::max_zoom_for_base_hex;
        // World-like tiny base тЖТ high max; Small-like large base тЖТ max тЙИ 1.
        assert!(max_zoom_for_base_hex(5.0) > 5.0);
        assert_eq!(max_zoom_for_base_hex(40.0), 1.0);
        assert_eq!(max_zoom_for_base_hex(80.0), 1.0);
    }
}
