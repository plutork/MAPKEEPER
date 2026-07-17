//! Shared hex brush footprint helpers (N-021 / N-022). Tool ops stay outside.

use super::convert::Axial;
use super::grid::HexGrid;

/// Hard performance ceiling on brush radius (cells), after alpha spike.
pub const BRUSH_RADIUS_PERF_CAP: u32 = 24;

/// Airbrush Rate ladder (elevation steps per second). N-023 amends N-022.
pub const AIRBRUSH_RATES_STEPS_PER_SEC: &[u32] = &[1, 5, 10, 20];

/// Default Airbrush Rate (steps/s).
pub const AIRBRUSH_DEFAULT_RATE: u32 = 5;

/// Soft flush chunk size for long airbrush gestures (historical ~512).
pub const FIELD_FLUSH_BATCH_MAX: usize = 512;

/// Cube-distance on axial hex lattice (pointy-top / Red Blob).
pub fn hex_distance(a: Axial, b: Axial) -> u32 {
    let dq = a.q - b.q;
    let dr = a.r - b.r;
    let ds = (-a.q - a.r) - (-b.q - b.r);
    ((dq.abs() + dr.abs() + ds.abs()) / 2) as u32
}

/// Max integer radius for the current grid: ~⅓ short side, then perf cap.
pub fn max_brush_radius(grid: &HexGrid) -> u32 {
    let short = grid.width.min(grid.height).max(1);
    let third = (short / 3).max(1);
    third.min(BRUSH_RADIUS_PERF_CAP)
}

/// Hard disk: all in-bounds cells with `hex_distance(center) <= radius`.
pub fn disk_footprint(grid: &HexGrid, center: Axial, radius: u32) -> Vec<Axial> {
    if !grid.contains_axial(center.q, center.r) {
        return Vec::new();
    }
    let r = radius as i32;
    let mut out = Vec::new();
    for dq in -r..=r {
        let r1 = i32::max(-r, -dq - r);
        let r2 = i32::min(r, -dq + r);
        for dr in r1..=r2 {
            let q = center.q + dq;
            let rr = center.r + dr;
            if grid.contains_axial(q, rr) {
                out.push(Axial { q, r: rr });
            }
        }
    }
    out
}

/// Approximate cell count for a disk of radius `r` (unclipped): 3r(r+1)+1.
pub fn disk_cell_count_estimate(radius: u32) -> u32 {
    let r = radius as u64;
    (3 * r * (r + 1) + 1) as u32
}

/// Relative axial offsets for a hard disk of `radius` (origin at 0,0).
/// Cache per radius and translate to each stamp center (N-022 perf).
pub fn disk_offsets(radius: u32) -> Vec<(i32, i32)> {
    let r = radius as i32;
    let mut out = Vec::new();
    for dq in -r..=r {
        let r1 = i32::max(-r, -dq - r);
        let r2 = i32::min(r, -dq + r);
        for dr in r1..=r2 {
            out.push((dq, dr));
        }
    }
    out
}

/// Apply cached offsets at `center`, clipped to the grid.
pub fn disk_from_offsets(grid: &HexGrid, center: Axial, offsets: &[(i32, i32)]) -> Vec<Axial> {
    if !grid.contains_axial(center.q, center.r) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(offsets.len());
    for &(dq, dr) in offsets {
        let q = center.q + dq;
        let rr = center.r + dr;
        if grid.contains_axial(q, rr) {
            out.push(Axial { q, r: rr });
        }
    }
    out
}

/// Wall-clock ms between Airbrush epoch resets for a Rate (steps/s).
pub fn pulse_interval_ms(rate_steps_per_sec: u32) -> Option<u32> {
    if rate_steps_per_sec == 0 || !AIRBRUSH_RATES_STEPS_PER_SEC.contains(&rate_steps_per_sec) {
        return None;
    }
    Some(1000 / rate_steps_per_sec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spatial::grid::HexGrid;

    fn grid(w: u32, h: u32) -> HexGrid {
        HexGrid {
            id: "t".into(),
            neighbor_center_distance_m: 1000.0,
            width: w,
            height: h,
        }
    }

    #[test]
    fn distance_neighbors_and_self() {
        let a = Axial { q: 0, r: 0 };
        assert_eq!(hex_distance(a, a), 0);
        assert_eq!(hex_distance(a, Axial { q: 1, r: 0 }), 1);
        assert_eq!(hex_distance(a, Axial { q: 1, r: -1 }), 1);
        assert_eq!(hex_distance(a, Axial { q: 2, r: -1 }), 2);
    }

    #[test]
    fn disk_radius_zero_is_center() {
        let g = grid(12, 8);
        let cells = disk_footprint(&g, Axial { q: 0, r: 0 }, 0);
        assert_eq!(cells, vec![Axial { q: 0, r: 0 }]);
    }

    #[test]
    fn disk_radius_one_has_seven_when_interior() {
        let g = grid(40, 26);
        // Interior-ish axial for odd-r map: q=10,r=10
        let cells = disk_footprint(&g, Axial { q: 10, r: 10 }, 1);
        assert_eq!(cells.len(), 7);
        assert!(cells.contains(&Axial { q: 10, r: 10 }));
    }

    #[test]
    fn max_radius_uses_third_then_cap() {
        let small = grid(12, 8);
        assert_eq!(max_brush_radius(&small), 2); // 8/3 = 2
        let huge = grid(277, 180);
        assert_eq!(max_brush_radius(&huge), BRUSH_RADIUS_PERF_CAP);
    }

    #[test]
    fn estimate_matches_unclipped_formula() {
        assert_eq!(disk_cell_count_estimate(0), 1);
        assert_eq!(disk_cell_count_estimate(1), 7);
        assert_eq!(disk_cell_count_estimate(2), 19);
    }

    #[test]
    fn offsets_match_disk_footprint_interior() {
        let g = grid(40, 26);
        let center = Axial { q: 10, r: 10 };
        let offsets = disk_offsets(2);
        let via_cache = disk_from_offsets(&g, center, &offsets);
        let direct = disk_footprint(&g, center, 2);
        assert_eq!(via_cache.len(), direct.len());
        for a in &direct {
            assert!(via_cache.contains(a));
        }
    }

    #[test]
    fn pulse_interval_for_ladder() {
        assert_eq!(pulse_interval_ms(1), Some(1000));
        assert_eq!(pulse_interval_ms(5), Some(200));
        assert_eq!(pulse_interval_ms(10), Some(100));
        assert_eq!(pulse_interval_ms(20), Some(50));
        assert_eq!(pulse_interval_ms(2), None);
        assert_eq!(pulse_interval_ms(8), None);
        assert_eq!(pulse_interval_ms(0), None);
    }
}
