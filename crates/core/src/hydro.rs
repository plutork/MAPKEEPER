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

use serde::{Deserialize, Serialize};

pub const ELEVATION_LAYER_ID: &str = "elevation";
pub const SEA_LEVEL: i16 = 0;
pub const DEFAULT_LAND_ELEVATION: i16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HydroKind {
    Land,
    Water,
}

pub fn hydro_from_elevation(elevation: i16) -> HydroKind {
    if elevation <= SEA_LEVEL {
        HydroKind::Water
    } else {
        HydroKind::Land
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sea_level_or_below_is_water() {
        assert_eq!(hydro_from_elevation(0), HydroKind::Water);
        assert_eq!(hydro_from_elevation(-3), HydroKind::Water);
    }

    #[test]
    fn above_sea_level_is_land() {
        assert_eq!(hydro_from_elevation(DEFAULT_LAND_ELEVATION), HydroKind::Land);
        assert_eq!(hydro_from_elevation(9), HydroKind::Land);
    }
}
