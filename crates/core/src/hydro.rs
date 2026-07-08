//! Elevation-driven hydro foundation (before rivers).
//!
//! `elevation` is the physical source of truth; hydro (`land`/`water`) is
//! derived by threshold:
//!
//! - `elevation <= SEA_LEVEL` => water
//! - `elevation > SEA_LEVEL` => land
//!
//! scale-layers (D-46): elevation is stored as a dense integer layer
//! (`core::layer::DenseLayer`); this module only owns the id + the threshold
//! projection. Unpainted cells resolve to `DEFAULT_LAND_ELEVATION` via
//! `DenseLayer::int_or(index, DEFAULT_LAND_ELEVATION)`.

use crate::hex::MapBounds;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use serde::{Deserialize, Serialize};

pub const ELEVATION_LAYER_ID: &str = "elevation";
pub const SEA_LEVEL: i32 = 0;
pub const DEFAULT_LAND_ELEVATION: i32 = 1;
/// Programmatic ocean fill for new worlds (elevation-authoring-v2).
pub const OCEAN_ELEVATION: i32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HydroKind {
    Land,
    Water,
}

pub fn hydro_from_elevation(elevation: i32) -> HydroKind {
    if elevation <= SEA_LEVEL {
        HydroKind::Water
    } else {
        HydroKind::Land
    }
}

/// Dense elevation layer with every cell set to `value` (create-fill ocean).
pub fn filled_elevation_layer(bounds: &MapBounds, value: i32) -> DenseLayer {
    let mut layer = DenseLayer::new_integer(ELEVATION_LAYER_ID, bounds.len());
    for i in 0..bounds.len() {
        layer.set(i, DenseState::Value(LayerValue::Int(value)));
    }
    layer
}

/// Hill falloff weight: center 1.0, edge of brush disk > 0 (elevation-authoring-v2).
pub fn stamp_falloff_weight(distance: i32, brush_radius: i32) -> f64 {
    if brush_radius <= 0 {
        return 1.0;
    }
    let span = brush_radius + 1;
    ((span - distance).max(0) as f64) / (span as f64)
}

/// Signed stamp delta for raise/lower; `even` uses full step on every cell.
pub fn stamp_delta(step: i32, distance: i32, brush_radius: i32, even: bool) -> i32 {
    if step == 0 {
        return 0;
    }
    let weight = if even {
        1.0
    } else {
        stamp_falloff_weight(distance, brush_radius)
    };
    (step as f64 * weight).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::MapBounds;

    #[test]
    fn sea_level_or_below_is_water() {
        assert_eq!(hydro_from_elevation(0), HydroKind::Water);
        assert_eq!(hydro_from_elevation(-3), HydroKind::Water);
    }

    #[test]
    fn above_sea_level_is_land() {
        assert_eq!(
            hydro_from_elevation(DEFAULT_LAND_ELEVATION),
            HydroKind::Land
        );
        assert_eq!(hydro_from_elevation(9), HydroKind::Land);
    }

    #[test]
    fn hill_falloff_radius_one_edge_nonzero() {
        assert!((stamp_falloff_weight(0, 1) - 1.0).abs() < f64::EPSILON);
        assert!((stamp_falloff_weight(1, 1) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn stamp_delta_zero_when_weight_zero() {
        assert_eq!(stamp_delta(10, 99, 1, false), 0);
    }

    #[test]
    fn ocean_fill_layer_len() {
        let bounds = MapBounds::new(4, 4);
        let layer = filled_elevation_layer(&bounds, OCEAN_ELEVATION);
        assert_eq!(layer.cell_count, bounds.len());
        assert_eq!(layer.int_or(0, DEFAULT_LAND_ELEVATION), OCEAN_ELEVATION);
    }
}
