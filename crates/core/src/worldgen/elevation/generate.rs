//! Step 5 elevation bridge entry point.

use crate::hex::MapBounds;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::worldgen::geology::{geology_kind_at, GEOLOGY_NONE};
use crate::worldgen::land::LAND_MASK_LAND;

use super::bands::{clamp_elevation_by_geology, clamp_land, clamp_to_band};
use super::jitter::{chaos_cell_height, deterministic_cell_jitter};
use super::smooth::smooth_elevation_once;
use super::types::ElevationIntensity;

fn is_land_cell(land_mask: &DenseLayer, index: usize) -> bool {
    matches!(
        land_mask.state(index),
        DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
    )
}

/// Step 5 bridge: land_mask + geology + seed + intensity → dense integer elevation.
pub fn elevation_from_land_mask_and_geology(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    geology: &DenseLayer,
    seed: u64,
    intensity: ElevationIntensity,
) -> DenseLayer {
    let len = bounds.len();
    let mut heights = vec![0i32; len];

    for (index, h) in heights.iter_mut().enumerate().take(len) {
        if !is_land_cell(land_mask, index) {
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let kind = geology_kind_at(geology, index);
        let z = match intensity {
            ElevationIntensity::Chaos => chaos_cell_height(kind, cell.q, cell.r, seed),
            _ => deterministic_cell_jitter(kind, cell.q, cell.r, seed, intensity),
        };
        *h = match intensity {
            ElevationIntensity::Chaos => clamp_land(z),
            _ => clamp_to_band(z, kind, intensity),
        };
    }

    smooth_elevation_once(bounds, land_mask, geology, &mut heights, intensity);

    let mut elevation = DenseLayer::new_integer("elevation", len);
    for (index, &h) in heights.iter().enumerate().take(len) {
        let land = is_land_cell(land_mask, index);
        let kind = if land {
            geology_kind_at(geology, index)
        } else {
            GEOLOGY_NONE
        };
        let z = clamp_elevation_by_geology(h, kind, land, intensity);
        elevation.set(index, DenseState::Value(LayerValue::Int(z)));
    }
    elevation
}
