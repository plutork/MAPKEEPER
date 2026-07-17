//! Precipitation heuristic with orographic rain shadow.

use crate::hex::{Axial, MapBounds};
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::worldgen::land::LAND_MASK_LAND;
use crate::worldgen::plates::hash01;

use super::types::PrecipitationStyle;

pub(crate) fn is_land(land_mask: &DenseLayer, index: usize) -> bool {
    matches!(
        land_mask.state(index),
        DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
    )
}

pub(crate) fn upwind_elevation_west(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    elevation: &DenseLayer,
    cell: Axial,
) -> i32 {
    let up = Axial::new(cell.q - 1, cell.r);
    let Some(ui) = bounds.index_of(up) else {
        return elevation.int_or(0, 0);
    };
    if is_land(land_mask, ui) {
        elevation.int_or(ui, 0)
    } else {
        0
    }
}

pub(crate) fn land_precipitation(
    cell: Axial,
    elevation: i32,
    coast_dist: u32,
    upwind_elev: i32,
    style: PrecipitationStyle,
    seed: u64,
) -> i32 {
    let coast_base = 118.0 / (1.0 + coast_dist as f64 * 0.32);
    let orographic = if elevation > upwind_elev + 10 {
        22.0
    } else {
        0.0
    };
    let rain_shadow = if upwind_elev > elevation + 14 {
        -38.0
    } else {
        0.0
    };
    let mut value = coast_base + orographic + rain_shadow;

    let interior = coast_dist >= 5;
    value *= match style {
        PrecipitationStyle::Balanced => {
            if interior {
                0.92
            } else {
                1.0
            }
        }
        PrecipitationStyle::WetCoasts => {
            if interior {
                0.82
            } else {
                1.35
            }
        }
        PrecipitationStyle::DryInterior => {
            if interior {
                0.52
            } else {
                0.95
            }
        }
    };

    let jitter = (hash01(seed ^ 0x00C1_AA7E, cell.q, cell.r) - 0.5) * 14.0;
    value += jitter;
    value.round().clamp(1.0, 220.0) as i32
}
