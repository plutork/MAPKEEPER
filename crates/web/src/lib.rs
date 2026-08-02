//! Minimal WASM bootstrap + shared spatial conversion helpers (N-008 / N-014).
//! Hard-disk brush + Airbrush rate helpers (N-021 / N-022).

use mapkeeper_core::spatial::{
    axial_to_world, disk_from_offsets, disk_offsets, hex_distance, max_brush_radius,
    next_relief_value, pulse_interval_ms, world_to_axial, Axial, HexGrid, WorldFrame, RELIEF_MAX,
    RELIEF_MIN,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    if let Some(element) = document.get_element_by_id("app-version") {
        element.set_text_content(Some(concat!("mapkeeper ", env!("CARGO_PKG_VERSION"))));
    }
}

#[wasm_bindgen]
pub struct ProbeAxial {
    pub q: i32,
    pub r: i32,
}

#[wasm_bindgen]
pub struct ProbePoint {
    pub x: f64,
    pub y: f64,
}

fn frame_from(origin_x: f64, origin_y: f64) -> WorldFrame {
    WorldFrame {
        id: "world".into(),
        origin_x,
        origin_y,
    }
}

fn grid_from(neighbor_center_distance_m: f64, width: u32, height: u32) -> HexGrid {
    HexGrid {
        id: "primary".into(),
        neighbor_center_distance_m,
        width,
        height,
    }
}

/// Shared world→grid pick (must match server/core persistence rules).
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn probe_world_to_axial(
    origin_x: f64,
    origin_y: f64,
    neighbor_center_distance_m: f64,
    width: u32,
    height: u32,
    x: f64,
    y: f64,
) -> ProbeAxial {
    let frame = frame_from(origin_x, origin_y);
    let grid = grid_from(neighbor_center_distance_m, width, height);
    let axial = world_to_axial(&frame, &grid, x, y);
    ProbeAxial {
        q: axial.q,
        r: axial.r,
    }
}

#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn probe_axial_to_world(
    origin_x: f64,
    origin_y: f64,
    neighbor_center_distance_m: f64,
    width: u32,
    height: u32,
    q: i32,
    r: i32,
) -> ProbePoint {
    let frame = frame_from(origin_x, origin_y);
    let grid = grid_from(neighbor_center_distance_m, width, height);
    let (x, y) = axial_to_world(&frame, &grid, Axial { q, r });
    ProbePoint { x, y }
}

/// Flat `[x,y, x,y, …]` row-major centers — one WASM call (N-026 CRS).
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn probe_grid_centers(
    origin_x: f64,
    origin_y: f64,
    neighbor_center_distance_m: f64,
    width: u32,
    height: u32,
) -> Vec<f64> {
    let frame = frame_from(origin_x, origin_y);
    let grid = grid_from(neighbor_center_distance_m, width, height);
    let mut out = Vec::with_capacity((width as usize) * (height as usize) * 2);
    for (q, r) in grid.iter_axial() {
        let (x, y) = axial_to_world(&frame, &grid, Axial { q, r });
        out.push(x);
        out.push(y);
    }
    out
}

/// Axis-aligned world bounds of cell centers `[min_x, min_y, max_x, max_y]`.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn probe_grid_center_bounds(
    origin_x: f64,
    origin_y: f64,
    neighbor_center_distance_m: f64,
    width: u32,
    height: u32,
) -> Vec<f64> {
    let frame = frame_from(origin_x, origin_y);
    let grid = grid_from(neighbor_center_distance_m, width, height);
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (q, r) in grid.iter_axial() {
        let (x, y) = axial_to_world(&frame, &grid, Axial { q, r });
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    vec![min_x, min_y, max_x, max_y]
}

#[wasm_bindgen]
pub fn probe_hex_distance(q0: i32, r0: i32, q1: i32, r1: i32) -> u32 {
    hex_distance(Axial { q: q0, r: r0 }, Axial { q: q1, r: r1 })
}

#[wasm_bindgen]
pub fn probe_max_brush_radius(width: u32, height: u32) -> u32 {
    max_brush_radius(&grid_from(1000.0, width, height))
}

/// Flat `[q,r, q,r, …]` for hard-disk footprint clipped to the grid.
#[wasm_bindgen]
pub fn probe_disk_cells(q: i32, r: i32, radius: u32, width: u32, height: u32) -> Vec<i32> {
    let grid = grid_from(1000.0, width, height);
    let offsets = disk_offsets(radius);
    disk_from_offsets(&grid, Axial { q, r }, &offsets)
        .into_iter()
        .flat_map(|a| [a.q, a.r])
        .collect()
}

/// Wall-clock ms between Airbrush epochs for Rate steps/s (0 = invalid).
#[wasm_bindgen]
pub fn probe_pulse_interval_ms(rate_steps_per_sec: u32) -> u32 {
    pulse_interval_ms(rate_steps_per_sec).unwrap_or(0)
}

/// One Raise/Lower step on a cell; `undefined` means no change (N-030).
/// The shell must not re-implement this rule.
#[wasm_bindgen]
pub fn probe_next_relief(current: i32, delta: i32, edit_ocean: bool) -> Option<i32> {
    next_relief_value(current, delta, edit_ocean)
}

/// Author elevation range `[min, max]` — mirrored thresholds check against it.
#[wasm_bindgen]
pub fn probe_relief_range() -> Vec<i32> {
    vec![RELIEF_MIN, RELIEF_MAX]
}
