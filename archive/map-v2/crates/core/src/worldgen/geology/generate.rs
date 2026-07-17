//! Geology layer generation entry point.

use crate::hex::MapBounds;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::worldgen::plates::{
    build_boundary_distances, build_hidden_plates, classify_plate_boundary_at,
};

use super::despeckle::despeckle_isolated_minors;
use super::land_helpers::is_land_cell;
use super::mapping::{coast_proximity, half_extent, map_hidden_tectonics_to_geology_style};
use super::types::{GeologyStyle, GEOLOGY_LAYER_ID, GEOLOGY_NONE};

/// Generate dense categorical `geology` from accepted `land_mask`.
/// Non-land cells are always `none`. Does not write elevation.
pub fn generate_geology(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    style: GeologyStyle,
    seed: u64,
) -> DenseLayer {
    let plates = build_hidden_plates(bounds, seed);
    let boundary_dist = build_boundary_distances(bounds, &plates);
    let mut layer = DenseLayer::new_categorical(GEOLOGY_LAYER_ID, bounds.len());
    let (max_x, max_y) = half_extent(bounds);
    for (index, _) in boundary_dist.iter().enumerate().take(bounds.len()) {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let kind = if !is_land_cell(land_mask, index) {
            GEOLOGY_NONE
        } else {
            let (x, y) = cell.to_pixel(1.0);
            let nx = if max_x > 0.0 { x / max_x } else { 0.0 };
            let ny = if max_y > 0.0 { y / max_y } else { 0.0 };
            let coast = coast_proximity(bounds, land_mask, cell);
            let (boundary, influence) = classify_plate_boundary_at(bounds, &plates, cell, index);
            map_hidden_tectonics_to_geology_style(
                style,
                boundary,
                influence,
                boundary_dist[index],
                nx,
                ny,
                coast,
                cell,
                seed,
            )
        };
        layer.set(index, DenseState::Value(LayerValue::Text(kind.to_string())));
    }
    despeckle_isolated_minors(bounds, &mut layer);
    layer
}
