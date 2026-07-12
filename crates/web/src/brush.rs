//! Brush tiers, paint stamps, and dock brush UI sync (D-94).

use crate::canvas::current_hex_size_px;
use crate::dom::{document, set_text};
use crate::elevation_view::{self, ColorMode};
use crate::state::{
    AppState, Brush, BRUSH_PREVIEW_HEX_DETAIL_MAX, BRUSH_SCREEN_DIAMETERS_PX, MAX_BRUSH_TIER,
    MAX_EFFECTIVE_BRUSH_RADIUS, MIN_BRUSH_TIER,
};
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::rivers::RiverCatalog;
use wasm_bindgen::JsCast;
use web_sys::Element;

fn wizard_overlay_active() -> bool {
    document()
        .get_element_by_id("editor")
        .is_some_and(|el| el.class_list().contains("wizard-active"))
}

pub(crate) fn clear_pointer_interaction(s: &mut AppState) {
    s.drag_active = false;
    s.drag_moved = false;
    s.paint_active = false;
    s.paint_moved = false;
    s.paint_last_cell = None;
    s.suppress_next_click = false;
}

pub(crate) fn apply_paint_brush(s: &mut AppState, brush: Brush) {
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

pub(crate) fn terrain_brush(brush: &Brush) -> bool {
    matches!(
        brush,
        Brush::SetLand | Brush::SetWater | Brush::Raise | Brush::Lower
    )
}

pub(crate) fn river_brush(brush: &Brush) -> bool {
    matches!(brush, Brush::River | Brush::RiverPin | Brush::RiverErase)
}

pub(crate) fn active_dock_tab() -> Option<String> {
    document().get_element_by_id("dock-rail").and_then(|rail| {
        rail.query_selector("[data-dock].active")
            .ok()
            .flatten()
            .and_then(|el| el.get_attribute("data-dock"))
    })
}

/// tool-dock-brush-deselect-v1: leave paint mode — pan / inspect on canvas.
pub(crate) fn deactivate_paint_brush(s: &mut AppState) {
    s.brush = Brush::Inspect;
    s.hover_cell = None;
    clear_pointer_interaction(s);
}

pub(crate) fn sync_paint_tool_ui(brush: &Brush) {
    sync_dock_rail_for_brush(brush);
    sync_brush_swatch_active(brush);
}

pub(crate) fn sync_dock_rail_for_brush(brush: &Brush) {
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

pub(crate) fn sync_brush_swatch_active(brush: &Brush) {
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
        Brush::RiverPin => "river-pin".to_string(),
        Brush::RiverErase => "river-erase".to_string(),
    };
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(Some(button)) = drawer.query_selector(&format!("[data-brush=\"{kind}\"]")) {
            let _ = button.class_list().add_1("active");
        }
    }
}

pub(crate) fn sync_brush_radius_active(radius: i32) {
    let tier = radius.clamp(MIN_BRUSH_TIER, MAX_BRUSH_TIER);
    set_text("brush-size-value", brush_tier_label(tier));
    if let Some(drawer) = document().get_element_by_id("dock-drawer") {
        if let Ok(items) = drawer.query_selector_all("[data-brush-size]") {
            for i in 0..items.length() {
                if let Some(node) = items.item(i) {
                    if let Ok(el) = node.dyn_into::<Element>() {
                        let is_active = el
                            .get_attribute("data-brush-size")
                            .and_then(|v| v.parse::<i32>().ok())
                            .map(|v| v == tier)
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

pub(crate) fn sync_brush_effective_label(state: &AppState) {
    let eff = effective_paint_radius(state).max(1);
    set_text("brush-effective-radius", &eff.to_string());
}

pub(crate) fn brush_preview_uses_circle(radius: i32) -> bool {
    radius > BRUSH_PREVIEW_HEX_DETAIL_MAX
}

pub(crate) const RIVERS_READ_ONLY_MSG: &str =
    "Generated rivers are read-only — use Rebuild rivers to rebuild.";

pub(crate) fn channel_topology_counts(state: &AppState) -> (usize, usize) {
    let (legacy_rivers, legacy_cells) = (
        state.rivers.rivers.len(),
        state.rivers.rivers.iter().map(|r| r.cells.len()).sum(),
    );
    let segments = state
        .channel_segment_count
        .unwrap_or(legacy_rivers);
    let cells = state.channel_cell_count.unwrap_or(legacy_cells);
    (segments, cells)
}

pub(crate) fn is_tributary(catalog: &RiverCatalog, river_id: u32) -> bool {
    catalog
        .rivers
        .iter()
        .find(|river| river.id == river_id)
        .is_some_and(|river| river.parent != river.id)
}

pub(crate) fn sync_detach_tributary_ui(state: &AppState) {
    let enabled = state
        .active_river_id
        .is_some_and(|id| is_tributary(&state.rivers, id));
    if let Some(button) = document().get_element_by_id("detach-tributary") {
        if enabled {
            let _ = button.remove_attribute("disabled");
        } else {
            let _ = button.set_attribute("disabled", "disabled");
        }
    }
}

pub(crate) fn sync_manual_river_authoring_ui(state: &AppState) {
    let read_only = state.rivers_read_only;
    if let Some(authoring) = document().get_element_by_id("manual-rivers-authoring") {
        if read_only {
            let _ = authoring.class_list().add_1("hidden");
        } else {
            let _ = authoring.class_list().remove_1("hidden");
        }
    }
    sync_detach_tributary_ui(state);
    if let Some(note) = document().get_element_by_id("manual-rivers-readonly-note") {
        if read_only {
            let _ = note.class_list().remove_1("hidden");
        } else {
            let _ = note.class_list().add_1("hidden");
        }
    }
}

pub(crate) fn sync_name_migration_warning(state: &AppState) {
    let ambiguous = state.name_migration.iter().filter(|r| r.ambiguous).count();
    if let Some(note) = document().get_element_by_id("river-migration-warning") {
        if ambiguous > 0 {
            set_text(
                "river-migration-warning",
                &format!("{ambiguous} river name(s) could not be rebound — see diagnostics."),
            );
            let _ = note.class_list().remove_1("hidden");
        } else {
            let _ = note.class_list().add_1("hidden");
        }
    }
}

pub(crate) fn sync_river_status(state: &AppState) {
    let (segments, cells) = channel_topology_counts(state);
    let named = state.named_rivers.len();
    if state.rivers_read_only {
        set_text(
            "river-status",
            &format!("{named} named river(s) · {segments} physical segments · {cells} channel cells (read-only)"),
        );
    } else {
        let active = state.active_river_id.map(|id| {
            if is_tributary(&state.rivers, id) {
                format!("River #{id} (tributary — Detach to split)")
            } else {
                format!("River #{id}")
            }
        }).unwrap_or_else(|| "New river on next click".to_string());
        set_text(
            "river-status",
            &format!("{active} · {segments} legacy river(s)"),
        );
    }
    sync_detach_tributary_ui(state);
}
pub(crate) fn brush_tier_screen_diameter(tier: i32) -> f64 {
    let i = tier.clamp(MIN_BRUSH_TIER, MAX_BRUSH_TIER) as usize;
    BRUSH_SCREEN_DIAMETERS_PX[i]
}

pub(crate) fn effective_brush_radius_from_hex_size(tier: i32, hex_size_px: f64) -> i32 {
    let tier = tier.clamp(MIN_BRUSH_TIER, MAX_BRUSH_TIER);
    let diameter = brush_tier_screen_diameter(tier);
    let hex_w = (3f64.sqrt() * hex_size_px).max(1.0);
    let from_zoom = ((diameter / hex_w) * 0.5).floor() as i32;
    // D-70: tiers must stay distinct at close zoom (floor alone collapsed S=M=L=0).
    from_zoom.max(tier).clamp(0, MAX_EFFECTIVE_BRUSH_RADIUS)
}

pub(crate) fn effective_paint_radius(state: &AppState) -> i32 {
    let tier = if wizard_overlay_active() && state.wizard_edit_mode {
        state.wizard_brush_radius
    } else {
        state.brush_radius
    };
    effective_brush_radius_from_hex_size(tier, current_hex_size_px(state))
}
pub(crate) fn brush_tier_label(tier: i32) -> &'static str {
    match tier.clamp(MIN_BRUSH_TIER, MAX_BRUSH_TIER) {
        0 => "S",
        1 => "M",
        2 => "L",
        _ => "XL",
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
pub(crate) fn sync_brush_step_active(step: i32) {
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

pub(crate) fn sync_falloff_active(falloff_even: bool, brush_radius: i32) {
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

pub(crate) fn apply_elevation_brush_intent(s: &mut AppState) {
    s.color_mode = ColorMode::Elevation;
    s.show_elevation_labels = true;
    // Peaks stay author-controlled — Raise/Lower does not force them on.
}

/// View defaults on world open (D-53) — elevation-first; grid on small maps only.
pub(crate) fn reset_view_on_world_open(s: &mut AppState) {
    s.color_mode = ColorMode::Elevation;
    s.show_elevation_labels = true;
    s.show_peaks = false;
    s.show_grid = s.map_bounds.len() <= elevation_view::OVERLAY_LOD_MAX_VISIBLE;
    s.active_river_id = None;
    s.river_pin_source = None;
    s.rivers = RiverCatalog::default();
    s.river_render_paths = Default::default();
    s.rivers_read_only = false;
    s.channel_segment_count = None;
    s.channel_cell_count = None;
    s.named_rivers.clear();
    s.name_migration.clear();
    s.rivers_compatibility_projection = false;
    s.precip_source = None;
    s.wizard_accepted = false;
    s.wizard_edit_mode = false;
    deactivate_paint_brush(s);
}
/// Map a brush to absolute target elevation. `Inspect` / Raise / Lower write nothing here.
pub(crate) fn brush_absolute_elevation(brush: &Brush) -> Option<i32> {
    match brush {
        Brush::Inspect | Brush::Raise | Brush::Lower | Brush::River | Brush::RiverPin | Brush::RiverErase => None,
        Brush::SetLand => Some(1),
        Brush::SetWater => Some(0),
    }
}

pub(crate) fn brush_delta_sign(brush: &Brush) -> Option<i32> {
    match brush {
        Brush::Raise => Some(1),
        Brush::Lower => Some(-1),
        _ => None,
    }
}

pub(crate) fn brush_paints(brush: &Brush) -> bool {
    !matches!(brush, Brush::Inspect)
}
