use serde::{Deserialize, Serialize};

use super::frame::WorldFrame;
use super::grid::HexGrid;
use super::presets::red_blob_radius_m;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Axial {
    pub q: i32,
    pub r: i32,
}

/// Ephemeral camera — never part of persisted spatial state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub center_world_x: f64,
    pub center_world_y: f64,
    /// Screen pixels per world meter.
    pub zoom: f64,
    pub screen_width: f64,
    pub screen_height: f64,
}

fn lattice_size(grid: &HexGrid) -> f64 {
    red_blob_radius_m(grid.neighbor_center_distance_m)
}

/// World point → containing axial cell (pointy-top).
pub fn world_to_axial(frame: &WorldFrame, grid: &HexGrid, x: f64, y: f64) -> Axial {
    let size = lattice_size(grid);
    let lx = (x - frame.origin_x) / size;
    let ly = (y - frame.origin_y) / size;
    let q_f = (3.0_f64.sqrt() / 3.0) * lx - (1.0 / 3.0) * ly;
    let r_f = (2.0 / 3.0) * ly;
    axial_round(q_f, r_f)
}

/// Axial cell center in world coordinates (meters).
pub fn axial_to_world(frame: &WorldFrame, grid: &HexGrid, axial: Axial) -> (f64, f64) {
    let size = lattice_size(grid);
    let x = size * (3.0_f64.sqrt() * (axial.q as f64 + axial.r as f64 / 2.0));
    let y = size * (1.5 * axial.r as f64);
    (frame.origin_x + x, frame.origin_y + y)
}

pub fn world_to_screen(view: &Viewport, x: f64, y: f64) -> (f64, f64) {
    let sx = (x - view.center_world_x) * view.zoom + view.screen_width / 2.0;
    let sy = (y - view.center_world_y) * view.zoom + view.screen_height / 2.0;
    (sx, sy)
}

pub fn screen_to_world(view: &Viewport, sx: f64, sy: f64) -> (f64, f64) {
    let x = (sx - view.screen_width / 2.0) / view.zoom + view.center_world_x;
    let y = (sy - view.screen_height / 2.0) / view.zoom + view.center_world_y;
    (x, y)
}

fn axial_round(q_f: f64, r_f: f64) -> Axial {
    let s_f = -q_f - r_f;
    let mut q = q_f.round();
    let mut r = r_f.round();
    let s = s_f.round();
    let q_diff = (q - q_f).abs();
    let r_diff = (r - r_f).abs();
    let s_diff = (s - s_f).abs();
    if q_diff > r_diff && q_diff > s_diff {
        q = -r - s;
    } else if r_diff > s_diff {
        r = -q - s;
    }
    Axial {
        q: q as i32,
        r: r as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_axial_world_stays_near_center() {
        let frame = WorldFrame::default_probe();
        let grid = HexGrid::default_probe();
        for (q, r) in grid.iter_axial() {
            let axial = Axial { q, r };
            let (x, y) = axial_to_world(&frame, &grid, axial);
            let back = world_to_axial(&frame, &grid, x, y);
            assert_eq!(back, axial, "round-trip at {q},{r}");
        }
    }

    #[test]
    fn neighbor_centers_are_one_km_apart() {
        let frame = WorldFrame::default_probe();
        let grid = HexGrid::default_probe();
        let a = axial_to_world(&frame, &grid, Axial { q: 0, r: 0 });
        let b = axial_to_world(&frame, &grid, Axial { q: 1, r: 0 });
        let dist = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        assert!((dist - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn screen_round_trip_does_not_touch_persistence() {
        let view = Viewport {
            center_world_x: 2000.0,
            center_world_y: 1000.0,
            zoom: 0.04,
            screen_width: 800.0,
            screen_height: 600.0,
        };
        let (sx, sy) = world_to_screen(&view, 2000.0, 1000.0);
        let (x, y) = screen_to_world(&view, sx, sy);
        assert!((x - 2000.0).abs() < 1e-9);
        assert!((y - 1000.0).abs() < 1e-9);
    }
}
