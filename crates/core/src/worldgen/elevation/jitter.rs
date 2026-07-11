//! Per-cell deterministic elevation jitter.

use crate::worldgen::plates::hash01;

use super::bands::{base_elevation_for_geology, elevation_band_for_intensity};
use super::types::ElevationIntensity;

const CHAOS_BIAS_STRENGTH: f64 = 0.35;

/// Deterministic jitter Z inside the class band.
pub fn deterministic_cell_jitter(
    kind: &str,
    cell_q: i32,
    cell_r: i32,
    seed: u64,
    intensity: ElevationIntensity,
) -> i32 {
    let (lo, hi) = elevation_band_for_intensity(kind, intensity);
    if lo >= hi {
        return lo;
    }
    let t = hash01(seed ^ 0x00E1_E801, cell_q, cell_r);
    let span = (hi - lo) as f64;
    lo + (t * span).round() as i32
}

pub(crate) fn chaos_cell_height(kind: &str, cell_q: i32, cell_r: i32, seed: u64) -> i32 {
    let t = hash01(seed ^ 0x0C4A_0512, cell_q, cell_r);
    let uniform = 1 + (t * 98.0).round() as i32;
    let anchor = base_elevation_for_geology(kind);
    let bias = ((anchor - 50) as f64 * CHAOS_BIAS_STRENGTH).round() as i32;
    (uniform + bias).clamp(1, 100)
}
