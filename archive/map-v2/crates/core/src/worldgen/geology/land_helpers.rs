//! Shared land-mask helpers for geology.

use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::worldgen::land::LAND_MASK_LAND;

pub(crate) fn is_land_cell(land_mask: &DenseLayer, index: usize) -> bool {
    matches!(
        land_mask.state(index),
        DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
    )
}
