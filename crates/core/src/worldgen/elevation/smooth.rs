//! One-pass hex elevation smoothing.

use crate::hex::MapBounds;
use crate::layer::DenseLayer;
use crate::layer::{DenseState, LayerValue};
use crate::worldgen::geology::geology_kind_at;
use crate::worldgen::land::LAND_MASK_LAND;

use super::bands::{clamp_land, clamp_to_band};
use super::types::ElevationIntensity;

const SAME_CLASS_WEIGHT: f64 = 1.0;
const CROSS_CLASS_WEIGHT_STANDARD: f64 = 0.35;
const CROSS_CLASS_WEIGHT_BOLD: f64 = 0.55;
const SELF_WEIGHT: f64 = 2.0;

fn is_land_cell(land_mask: &DenseLayer, index: usize) -> bool {
    matches!(
        land_mask.state(index),
        DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
    )
}

/// One light hex smooth over land; same-geology neighbors weigh more.
pub fn smooth_elevation_once(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    geology: &DenseLayer,
    heights: &mut [i32],
    intensity: ElevationIntensity,
) {
    let cross_weight = match intensity {
        ElevationIntensity::Bold => CROSS_CLASS_WEIGHT_BOLD,
        _ => CROSS_CLASS_WEIGHT_STANDARD,
    };
    let len = heights.len();
    let mut next = heights.to_vec();
    for index in 0..len {
        if !is_land_cell(land_mask, index) {
            next[index] = 0;
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let my_kind = geology_kind_at(geology, index);
        let mut sum = heights[index] as f64 * SELF_WEIGHT;
        let mut weight = SELF_WEIGHT;
        for nb in cell.neighbors() {
            let Some(ni) = bounds.index_of(nb) else {
                continue;
            };
            if !is_land_cell(land_mask, ni) {
                continue;
            }
            let w = if geology_kind_at(geology, ni) == my_kind {
                SAME_CLASS_WEIGHT
            } else {
                cross_weight
            };
            sum += heights[ni] as f64 * w;
            weight += w;
        }
        let smoothed = (sum / weight).round() as i32;
        next[index] = match intensity {
            ElevationIntensity::Chaos => clamp_land(smoothed),
            _ => clamp_to_band(smoothed, my_kind, intensity),
        };
    }
    heights.copy_from_slice(&next);
}
