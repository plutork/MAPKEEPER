//! Land temperature heuristic.

use crate::hex::Axial;

pub(crate) fn land_temperature(cell: Axial, elevation: i32, coast_dist: u32, height: f64) -> i32 {
    let r_norm = cell.r as f64 / (height - 1.0).max(1.0);
    let lat = 30.0 - r_norm * 52.0;
    let lapse = -(elevation.max(1) as f64) * 0.38;
    let coast_mod = (8.0 - coast_dist.min(8) as f64) * 1.8;
    (lat + lapse + coast_mod).round() as i32
}
