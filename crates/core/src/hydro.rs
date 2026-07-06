//! Elevation-driven hydro foundation (before rivers).
//!
//! `elevation` is the physical source of truth; hydro (`land`/`water`) is
//! derived by threshold:
//!
//! - `elevation <= SEA_LEVEL` => water
//! - `elevation > SEA_LEVEL` => land
//!
//! Storage is sparse: absent cells default to `DEFAULT_LAND_ELEVATION`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const ELEVATION_LAYER_ID: &str = "elevation";
pub const SEA_LEVEL: i16 = 0;
pub const DEFAULT_LAND_ELEVATION: i16 = 1;
pub const ELEVATION_SCHEMA_VERSION: u32 = 1;

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

/// `map/layers/elevation.json`
///
/// Sparse per-cell elevation. Missing key => `DEFAULT_LAND_ELEVATION`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElevationLayer {
    pub schema_version: u32,
    pub layer_id: String,
    pub value_type: String,
    #[serde(default)]
    pub cells: BTreeMap<String, i16>,
}

impl ElevationLayer {
    pub fn new() -> Self {
        Self {
            schema_version: ELEVATION_SCHEMA_VERSION,
            layer_id: ELEVATION_LAYER_ID.to_string(),
            value_type: "integer".to_string(),
            cells: BTreeMap::new(),
        }
    }

    /// Missing key means default land elevation.
    pub fn elevation(&self, cell_id: &str) -> i16 {
        self.cells.get(cell_id).copied().unwrap_or(DEFAULT_LAND_ELEVATION)
    }

    pub fn hydro(&self, cell_id: &str) -> HydroKind {
        hydro_from_elevation(self.elevation(cell_id))
    }

    /// Keep sparse semantics: writing default land removes the key.
    pub fn set(&mut self, cell_id: impl Into<String>, elevation: i16) {
        let cell_id = cell_id.into();
        if elevation == DEFAULT_LAND_ELEVATION {
            self.cells.remove(&cell_id);
            return;
        }
        self.cells.insert(cell_id, elevation);
    }

    pub fn from_json(raw: &str) -> serde_json::Result<ElevationLayer> {
        serde_json::from_str(raw)
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

impl Default for ElevationLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_cells_default_to_land() {
        let layer = ElevationLayer::new();
        assert_eq!(layer.elevation("w.hex.q0.r0"), DEFAULT_LAND_ELEVATION);
        assert_eq!(layer.hydro("w.hex.q0.r0"), HydroKind::Land);
    }

    #[test]
    fn sea_level_or_below_is_water() {
        let mut layer = ElevationLayer::new();
        layer.set("w.hex.q0.r0", 0);
        layer.set("w.hex.q1.r0", -3);
        assert_eq!(layer.hydro("w.hex.q0.r0"), HydroKind::Water);
        assert_eq!(layer.hydro("w.hex.q1.r0"), HydroKind::Water);
    }

    #[test]
    fn default_land_is_sparse() {
        let mut layer = ElevationLayer::new();
        layer.set("w.hex.q0.r0", 5);
        assert!(layer.cells.contains_key("w.hex.q0.r0"));
        layer.set("w.hex.q0.r0", DEFAULT_LAND_ELEVATION);
        assert!(!layer.cells.contains_key("w.hex.q0.r0"));
    }
}
