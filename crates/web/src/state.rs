//! Web app state, wire DTOs, and shared constants (D-94 B1).

use std::collections::HashMap;

use crate::elevation_view::ColorMode;
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue};
use mapkeeper_core::lakes::LakeCatalog;
use mapkeeper_core::rivers::RiverCatalog;
use serde::{Deserialize, Serialize};

pub(crate) const MIN_ZOOM: f64 = 0.6;
// zoom-cap--target-hex-px (D-85): max zoom so on-screen hex ≈ this many px (amends D-41 flat 2.5x).
pub(crate) const ZOOM_CLOSEUP_HEX_PX: f64 = 40.0;
pub(crate) const ZOOM_MAX_HARD: f64 = 32.0;
pub(crate) const PAN_DRAG_THRESHOLD: f64 = 3.0;
// save-batch--http-endpoint-v1: tuneable write buffering.
pub(crate) const PAINT_SAVE_COOLDOWN_MS: u32 = 300;
pub(crate) const PAINT_BATCH_MAX_CELLS: usize = 512;
// brush-size--zoom-adaptive (D-70): screen-space tier → cell radius from zoom.
pub(crate) const MIN_BRUSH_TIER: i32 = 0;
pub(crate) const MAX_BRUSH_TIER: i32 = 3;
/// Soft ceiling on effective hex radius (World overview perf guard; tunable).
pub(crate) const MAX_EFFECTIVE_BRUSH_RADIUS: i32 = 24;
/// Screen-space brush diameters (px) for tiers S/M/L/XL.
pub(crate) const BRUSH_SCREEN_DIAMETERS_PX: [f64; 4] = [28.0, 56.0, 96.0, 144.0];
/// Hex-boundary preview above this radius freezes the UI — use a circle instead.
pub(crate) const BRUSH_PREVIEW_HEX_DETAIL_MAX: i32 = 2;
// Legacy aliases — tier index still stored as brush_radius field.
pub(crate) const MIN_BRUSH_RADIUS: i32 = MIN_BRUSH_TIER;
pub(crate) const MAX_BRUSH_RADIUS: i32 = MAX_BRUSH_TIER;
// perf-100k--canvas-lod-grid-markers: skip per-hex stroke when many cells visible.
pub(crate) const GRID_STROKE_CELL_THRESHOLD: usize = 10_000;
// perf-100k--canvas-lod-grid-markers: hide profile dots when zoomed out.
pub(crate) const PROFILE_MARKER_MIN_ZOOM: f64 = 1.0;
// view-cells-seamless-v1: fill scales — On = edge-to-edge; Off = overlap seamless map.
pub(crate) const FILL_SCALE_GRID_ON: f64 = 1.0;
pub(crate) const FILL_SCALE_GRID_OFF: f64 = 1.04;
pub(crate) const GRID_LINE_WIDTH: f64 = 1.0;
/// Brush hover preview inset (unchanged from legacy HEX_GAP).
pub(crate) const BRUSH_PREVIEW_GAP: f64 = 0.92;
/// Breathing room (px) between the map and the canvas edge.
pub(crate) const CANVAS_PAD: f64 = 20.0;
/// Default land elevation when a cell is unknown/none (hydro projection).
pub(crate) const DEFAULT_LAND_ELEVATION: i32 = 1;

/// Last water generate call — dogfood diagnostics (not persisted).
#[derive(Default, Clone)]
pub(crate) struct WaterGenTrace {
    pub action: String,
    pub request: String,
    pub result: String,
    pub error: String,
}
// Home version label (D-80: in-app Check-for-updates CTA removed; alpha updates via update.ps1).
pub(crate) const APP_VERSION: &str = "0.2.1";

// perf-100k--web-dense-client: index-addressed elevation buffer (no sparse mirror).
pub(crate) fn fresh_elevation_layer(bounds: MapBounds) -> DenseLayer {
    DenseLayer::new_integer("elevation", bounds.len())
}

// geology-readable--preview-contrast (D-72): opaque class fills for wizard Tectonics
pub(crate) fn geology_tint(
    geo: &DenseLayer,
    bounds: MapBounds,
    q: i32,
    r: i32,
) -> Option<&'static str> {
    let index = bounds.index_of(Axial::new(q, r))?;
    match geo.state(index) {
        DenseState::Value(LayerValue::Text(ref t)) => match t.as_str() {
            "ridge" => Some("rgba(196, 92, 42, 0.78)"),
            "rift" => Some("rgba(140, 70, 180, 0.72)"),
            "basin" => Some("rgba(45, 110, 190, 0.72)"),
            "volcanic_arc" => Some("rgba(210, 55, 45, 0.82)"),
            "stable" => Some("rgba(70, 150, 85, 0.62)"),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn elevation_at(layer: &DenseLayer, bounds: MapBounds, q: i32, r: i32) -> i32 {
    let index = bounds.index_of(Axial::new(q, r)).unwrap_or(0);
    layer.int_or(index, DEFAULT_LAND_ELEVATION)
}

pub(crate) fn set_elevation_cell(
    layer: &mut DenseLayer,
    bounds: MapBounds,
    q: i32,
    r: i32,
    value: i32,
) {
    if let Some(index) = bounds.index_of(Axial::new(q, r)) {
        layer.set(index, DenseState::Value(LayerValue::Int(value)));
    }
}

pub(crate) fn count_visible_in_bounds(
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

pub(crate) fn stroke_grid_enabled(show_grid: bool, visible_cells: usize) -> bool {
    show_grid && visible_cells <= GRID_STROKE_CELL_THRESHOLD
}

pub(crate) fn show_profile_markers(zoom: f64) -> bool {
    zoom >= PROFILE_MARKER_MIN_ZOOM
}

pub(crate) fn grid_lines_stats_label(show_grid: bool, visible_cells: usize) -> &'static str {
    if !show_grid {
        "lines Off"
    } else if visible_cells > GRID_STROKE_CELL_THRESHOLD {
        "lines Auto-off"
    } else {
        "lines On"
    }
}

pub(crate) fn grid_lines_toggle_label(show_grid: bool) -> &'static str {
    if show_grid {
        "Grid lines: On"
    } else {
        "Grid lines: Off"
    }
}
// perf-100k--measurement-hooks: lightweight Step 0 timing (console + view pane).
#[derive(Default)]
pub(crate) struct PerfMetrics {
    pub(crate) open_ms: Option<f64>,
    pub(crate) layer_fetch_ms: Option<f64>,
    pub(crate) layer_parse_or_decode_ms: Option<f64>,
    pub(crate) client_mirror_ms: Option<f64>,
    pub(crate) first_redraw_ms: Option<f64>,
    pub(crate) redraw_ms: Option<f64>,
    pub(crate) drawn_cells: Option<usize>,
    pub(crate) batch_flush_ms: Option<f64>,
}

#[derive(Default)]
pub(crate) struct PerfTimers {
    pub(crate) open_start: Option<f64>,
}

pub(crate) fn perf_ms_label(v: Option<f64>) -> String {
    match v {
        Some(ms) => format!("{ms:.0}ms"),
        None => "—".to_string(),
    }
}

impl PerfMetrics {
    pub(crate) fn console_line(&self) -> String {
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

    pub(crate) fn view_line(&self) -> String {
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
// perf-100k--raf-redraw-coalesce: at most one full redraw per animation frame.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct DrawSnapshot {
    pub(crate) zoom: f64,
    pub(crate) pan_x: f64,
    pub(crate) pan_y: f64,
    pub(crate) selected: Option<(i32, i32)>,
    pub(crate) show_grid: bool,
    pub(crate) hover_cell: Option<(i32, i32)>,
    pub(crate) color_mode: ColorMode,
    pub(crate) show_elevation_labels: bool,
    pub(crate) show_peaks: bool,
    pub(crate) content_rev: u64,
}

pub(crate) fn draw_snapshot(s: &AppState) -> DrawSnapshot {
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

pub(crate) fn bump_content_rev(s: &mut AppState) {
    s.content_rev = s.content_rev.wrapping_add(1);
}
#[derive(Deserialize)]
pub(crate) struct MapBoundsResponse {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) cell_count: u32,
}

#[derive(Deserialize)]
pub(crate) struct MapResponse {
    #[allow(dead_code)]
    pub(crate) world_id: String,
    pub(crate) bounds: MapBoundsResponse,
    pub(crate) legacy_map: bool,
    pub(crate) cells: Vec<CellSummary>,
}

#[derive(Deserialize)]
pub(crate) struct CellSummary {
    pub(crate) q: i32,
    pub(crate) r: i32,
    pub(crate) display_name: String,
}

#[derive(Serialize)]
pub(crate) struct ProfileInput {
    pub(crate) display_name: String,
    pub(crate) notes: String,
}

#[derive(Deserialize)]
pub(crate) struct ProjectEntry {
    #[allow(dead_code)]
    pub(crate) id: String,
    #[allow(dead_code)]
    pub(crate) path: String,
}

#[derive(Deserialize)]
pub(crate) struct ProjectStatus {
    #[allow(dead_code)]
    pub(crate) id: String,
    #[allow(dead_code)]
    pub(crate) path: String,
    pub(crate) valid: bool,
    pub(crate) legacy_map: bool,
    pub(crate) build_draft: bool,
    pub(crate) build_step: Option<u32>,
}

#[derive(Deserialize)]
pub(crate) struct ProjectsResponse {
    pub(crate) active: Option<ProjectEntry>,
    pub(crate) projects: Vec<ProjectStatus>,
    pub(crate) default_worlds_root: String,
}

#[derive(Serialize)]
pub(crate) struct BuildStateInput {
    pub(crate) status: &'static str,
    pub(crate) step: u32,
}

#[derive(Serialize)]
pub(crate) struct BuildBoundsInput<'a> {
    pub(crate) map_preset: &'a str,
}

#[derive(Deserialize)]
pub(crate) struct BuildBoundsResponse {
    #[allow(dead_code)]
    pub(crate) bounds: MapBoundsResponse,
    #[allow(dead_code)]
    pub(crate) reset: bool,
}

#[derive(Serialize)]
pub(crate) struct WizardLandMaskGenerateInput<'a> {
    pub(crate) recipe_id: &'a str,
    pub(crate) character: &'a str,
    pub(crate) variant: &'a str,
    pub(crate) regenerate_nonce: u32,
}

/// D-68: identity echoed from land-mask generate.
#[derive(Deserialize)]
pub(crate) struct WizardLandMaskGenerateResponse {
    pub(crate) seed: u64,
    pub(crate) recipe_id: String,
    pub(crate) layout_class: String,
    pub(crate) character: String,
    pub(crate) regenerate_nonce: u64,
}

#[derive(Serialize)]
pub(crate) struct WizardGeologyGenerateInput<'a> {
    pub(crate) style: &'a str,
    pub(crate) regenerate_nonce: u32,
}

#[derive(Serialize)]
pub(crate) struct WizardElevationGenerateInput<'a> {
    pub(crate) style: &'a str,
    pub(crate) regenerate_nonce: u32,
}

#[derive(Serialize)]
pub(crate) struct WizardClimateGenerateInput<'a> {
    pub(crate) style: &'a str,
    pub(crate) regenerate_nonce: u32,
}

#[derive(Serialize)]
pub(crate) struct WizardLandMaskCellInput<'a> {
    pub(crate) q: i32,
    pub(crate) r: i32,
    pub(crate) kind: &'a str,
}

#[derive(Serialize)]
pub(crate) struct CreateProjectInput<'a> {
    pub(crate) id: &'a str,
    pub(crate) path: &'a str,
    pub(crate) map_preset: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) build_wizard: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct OpenProjectInput<'a> {
    pub(crate) path: &'a str,
}

#[derive(Serialize)]
pub(crate) struct ForgetProjectInput<'a> {
    pub(crate) path: &'a str,
}

#[derive(Serialize)]
pub(crate) struct DeleteProjectInput<'a> {
    pub(crate) path: &'a str,
}

/// One cell in a generic layer batch (`PUT /api/layers/:id/batch`), matching
/// `core::layer::LayerCellWrite` + `WireCellState::Value`. Elevation paints are
/// always concrete integer values (scale-layers, D-46).
#[derive(Serialize)]
pub(crate) struct LayerCellWrite {
    pub(crate) q: i32,
    pub(crate) r: i32,
    pub(crate) state: &'static str,
    pub(crate) value: i32,
}

/// Active editing tool. `Inspect` keeps the old click→profile behavior; the
/// hydro brushes paint elevation-driven hydro (`land`/`water`) instead.
#[derive(Clone)]
pub(crate) enum Brush {
    Inspect,
    SetLand,
    SetWater,
    Raise,
    Lower,
    /// river-overlay-layer-v1: chain-click polyline brush.
    River,
    RiverErase,
}

pub(crate) struct AppState {
    /// Cells that have an author profile (used for the profile-presence marker).
    pub(crate) cells: HashMap<(i32, i32), String>,
    /// Index-addressed elevation layer — primary render cache (perf-100k--web-dense-client).
    pub(crate) elevation: DenseLayer,
    pub(crate) brush: Brush,
    /// Last terrain brush — restored when reopening Terrain tab.
    pub(crate) last_paint_brush: Brush,
    /// Last river brush — restored when reopening Rivers tab.
    pub(crate) last_river_brush: Brush,
    /// Active river chain id (`None` = next click starts a new river).
    pub(crate) active_river_id: Option<u32>,
    /// River catalog mirror (river-overlay-layer-v1).
    pub(crate) rivers: RiverCatalog,
    /// Lake catalog mirror (hydrology-lake-domain-v1).
    pub(crate) lakes: LakeCatalog,
    /// Precipitation layer exists on disk (probe on world open).
    pub(crate) precip_layer_present: Option<bool>,
    /// Last lakes/rivers generate — copy-paste diagnostics for /fix.
    pub(crate) water_gen_trace: WaterGenTrace,
    pub(crate) selected: Option<(i32, i32)>,
    /// Hex bounds from `map/manifest.json` (via `/api/map`).
    pub(crate) map_bounds: MapBounds,
    /// Camera zoom multiplier over fit-to-window base size.
    pub(crate) zoom: f64,
    /// Camera pan offset in screen pixels.
    pub(crate) pan_x: f64,
    pub(crate) pan_y: f64,
    /// Drag-pan interaction state.
    pub(crate) drag_active: bool,
    pub(crate) drag_moved: bool,
    pub(crate) drag_last_x: f64,
    pub(crate) drag_last_y: f64,
    /// Drag-paint interaction state (Land/Water brush).
    pub(crate) paint_active: bool,
    pub(crate) paint_moved: bool,
    pub(crate) paint_last_cell: Option<(i32, i32)>,
    /// Brush size tier 0..=3 (S/M/L/XL). Effective hex radius from zoom (D-70).
    pub(crate) brush_radius: i32,
    /// Raise/Lower step magnitude (1, 5, or 10).
    pub(crate) brush_step: i32,
    /// Even falloff vs hill gradient for Raise/Lower.
    pub(crate) falloff_even: bool,
    pub(crate) color_mode: ColorMode,
    pub(crate) show_elevation_labels: bool,
    pub(crate) show_peaks: bool,
    /// Local paint writes not yet persisted to server.
    pub(crate) pending_paints: HashMap<(i32, i32), i32>,
    pub(crate) paint_flush_scheduled: bool,
    pub(crate) paint_flush_in_flight: bool,
    pub(crate) hover_cell: Option<(i32, i32)>,
    /// Draw hex-cell borders over fills.
    pub(crate) show_grid: bool,
    pub(crate) suppress_next_click: bool,
    pub(crate) legacy_map: bool,
    pub(crate) default_worlds_root: Option<String>,
    pub(crate) path_touched: bool,
    pub(crate) build_path_touched: bool,
    /// Step 0 perf samples (perf-100k--measurement-hooks).
    pub(crate) perf: PerfMetrics,
    pub(crate) perf_timers: PerfTimers,
    /// Visual revision — bumps when elevation/cells change (perf-100k--raf-redraw-coalesce).
    pub(crate) content_rev: u64,
    pub(crate) last_draw_snapshot: Option<DrawSnapshot>,
    pub(crate) redraw_dirty: bool,
    pub(crate) redraw_raf_pending: bool,
    pub(crate) wizard_character: String,
    /// Selected layout class id (D-65: six cards always visible).
    pub(crate) wizard_layout_class: String,
    pub(crate) wizard_regenerate_nonce: u32,
    /// Active recipe within the selected class.
    pub(crate) wizard_recipe_id: String,
    /// Effective silhouette seed from last generate (D-68).
    pub(crate) wizard_gen_seed: Option<u64>,
    pub(crate) wizard_accepted: bool,
    pub(crate) wizard_edit_mode: bool,
    pub(crate) wizard_edit_brush: String,
    /// D-70: screen tier 0..=3 (S–XL); effective radius from zoom.
    pub(crate) wizard_brush_radius: i32,
    /// Wizard land-edit: optimistic local stamps not yet flushed (true=land).
    pub(crate) pending_wizard_stamps: HashMap<(i32, i32), bool>,
    pub(crate) wizard_stamp_flush_scheduled: bool,
    pub(crate) wizard_stamp_flush_in_flight: bool,
    /// Hex distance throttle between drag stamp centers.
    pub(crate) wizard_stamp_last_center: Option<(i32, i32)>,
    /// Build wizard step: 1 size · 2 land · 3 tectonics · 4 elevation · 5 climate · 6 water (D-71/D-90/D-91).
    pub(crate) wizard_step: u32,
    pub(crate) wizard_geo_style: String,
    pub(crate) wizard_geo_nonce: u32,
    pub(crate) wizard_elev_style: String,
    pub(crate) wizard_elev_nonce: u32,
    pub(crate) wizard_climate_style: String,
    pub(crate) wizard_climate_nonce: u32,
    pub(crate) wizard_geo_accepted: bool,
    /// Dense geology cache for tint overlay (index → palette string).
    pub(crate) geology: Option<DenseLayer>,
}
/// D-69: in-app confirm — window.confirm is often silent/blocked in Tauri WebView2.
#[derive(Clone)]
pub(crate) enum WizConfirmKind {
    Back,
    BoundsPreset(String),
}
