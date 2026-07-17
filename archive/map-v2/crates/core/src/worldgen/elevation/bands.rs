//! Geology band anchors and clamp helpers.

use crate::worldgen::geology::{
    GEOLOGY_BASIN, GEOLOGY_NONE, GEOLOGY_RIDGE, GEOLOGY_RIFT, GEOLOGY_STABLE, GEOLOGY_VOLCANIC_ARC,
};

use super::types::ElevationIntensity;

/// Center/base Z per geology class (D-72 readability anchors).
pub fn base_elevation_for_geology(kind: &str) -> i32 {
    match kind {
        GEOLOGY_BASIN => 11,
        GEOLOGY_RIFT => 19,
        GEOLOGY_STABLE | GEOLOGY_NONE => 30,
        GEOLOGY_RIDGE => 56,
        GEOLOGY_VOLCANIC_ARC => 72,
        _ => 30,
    }
}

/// Inclusive jitter band per geology class (D-88 Standard).
pub fn elevation_band_for_geology(kind: &str) -> (i32, i32) {
    match kind {
        GEOLOGY_BASIN => (8, 14),
        GEOLOGY_RIFT => (14, 24),
        GEOLOGY_STABLE | GEOLOGY_NONE => (26, 34),
        GEOLOGY_RIDGE => (48, 64),
        GEOLOGY_VOLCANIC_ARC => (62, 82),
        _ => (26, 34),
    }
}

pub(crate) fn elevation_band_for_intensity(
    kind: &str,
    intensity: ElevationIntensity,
) -> (i32, i32) {
    let (lo, hi) = elevation_band_for_geology(kind);
    match intensity {
        ElevationIntensity::Standard => (lo, hi),
        ElevationIntensity::Bold => {
            let base = base_elevation_for_geology(kind);
            let half = ((hi - lo).max(1) as f64 * 0.75).round() as i32;
            let new_lo = (base - half).clamp(1, 100);
            let new_hi = (base + half).clamp(1, 100);
            (new_lo.min(new_hi), new_lo.max(new_hi))
        }
        ElevationIntensity::Chaos => (1, 100),
    }
}

pub(crate) fn clamp_to_band(z: i32, kind: &str, intensity: ElevationIntensity) -> i32 {
    let (lo, hi) = elevation_band_for_intensity(kind, intensity);
    z.clamp(lo, hi).clamp(1, 100)
}

pub(crate) fn clamp_land(z: i32) -> i32 {
    z.clamp(1, 100)
}

/// Clamp land elevation to geology band; water forced to 0.
pub fn clamp_elevation_by_geology(
    z: i32,
    kind: &str,
    land: bool,
    intensity: ElevationIntensity,
) -> i32 {
    if !land {
        return 0;
    }
    match intensity {
        ElevationIntensity::Chaos => clamp_land(z),
        _ => clamp_to_band(z, kind, intensity),
    }
}
