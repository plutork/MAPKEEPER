//! Hydrology analysis types (H0 depression analysis, later lake/river density).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::climate::PRECIPITATION_LAYER_ID;
use crate::hydro::{DEFAULT_LAND_ELEVATION, SEA_LEVEL};
use crate::layer::{DenseLayer, DenseState, LayerValue};

/// Uniform land runoff when precipitation input is missing or unusable.
pub const FALLBACK_LAND_RUNOFF: u64 = 90;

/// Classified precipitation layer usability for hydrology runoff (hydrology-precip-input-semantics-v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrecipInputState {
    Missing,
    InvalidOrEmpty,
    Valid,
}

impl PrecipInputState {
    pub fn id(self) -> &'static str {
        match self {
            PrecipInputState::Missing => "missing",
            PrecipInputState::InvalidOrEmpty => "invalid_or_empty",
            PrecipInputState::Valid => "valid",
        }
    }

    /// Legacy API alias kept for one release (`precip_source`).
    pub fn legacy_precip_source(self) -> &'static str {
        match self {
            PrecipInputState::Valid => "climate",
            PrecipInputState::Missing | PrecipInputState::InvalidOrEmpty => "uniform_fallback",
        }
    }

    pub fn uses_climate_runoff(self) -> bool {
        matches!(self, PrecipInputState::Valid)
    }
}

/// Classify whether the precipitation layer can drive per-cell runoff.
pub fn classify_precip_input(
    elevation: &DenseLayer,
    precipitation: Option<&DenseLayer>,
) -> PrecipInputState {
    let Some(precip) = precipitation else {
        return PrecipInputState::Missing;
    };
    if precip.layer_id != PRECIPITATION_LAYER_ID {
        return PrecipInputState::InvalidOrEmpty;
    }
    let n = elevation.len().min(precip.len());
    for index in 0..n {
        if elevation.int_or(index, DEFAULT_LAND_ELEVATION) <= SEA_LEVEL {
            continue;
        }
        if matches!(
            precip.state(index),
            DenseState::Value(LayerValue::Int(v)) if v > 0
        ) {
            return PrecipInputState::Valid;
        }
    }
    PrecipInputState::InvalidOrEmpty
}

/// Per-terrain-cell local runoff from classified precipitation input.
pub fn terrain_runoff(
    cell: usize,
    precipitation: Option<&DenseLayer>,
    state: PrecipInputState,
) -> u64 {
    match state {
        PrecipInputState::Valid => precipitation
            .filter(|layer| layer.layer_id == PRECIPITATION_LAYER_ID)
            .map(|layer| layer.int_or(cell, 0).max(0) as u64)
            .unwrap_or(FALLBACK_LAND_RUNOFF),
        PrecipInputState::Missing | PrecipInputState::InvalidOrEmpty => FALLBACK_LAND_RUNOFF,
    }
}

/// Wizard / generate lake density presets (hydrology-lake-generation-v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LakeDensity {
    Sparse,
    Balanced,
    LakeRich,
}

impl LakeDensity {
    pub fn parse(raw: &str) -> LakeDensity {
        match raw.trim().to_ascii_lowercase().as_str() {
            "sparse" | "few" => LakeDensity::Sparse,
            "rich" | "lake_rich" | "lakerich" | "lake-rich" => LakeDensity::LakeRich,
            _ => LakeDensity::Balanced,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            LakeDensity::Sparse => "sparse",
            LakeDensity::Balanced => "balanced",
            LakeDensity::LakeRich => "lake_rich",
        }
    }
}

/// Wizard / generate river density presets (hydrology-river-lake-integration-v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RiverDensity {
    Few,
    #[default]
    Balanced,
    Many,
}

impl RiverDensity {
    pub fn parse(raw: &str) -> RiverDensity {
        match raw.trim().to_ascii_lowercase().as_str() {
            "few" | "sparse" => RiverDensity::Few,
            "many" | "rich" => RiverDensity::Many,
            _ => RiverDensity::Balanced,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            RiverDensity::Few => "few",
            RiverDensity::Balanced => "balanced",
            RiverDensity::Many => "many",
        }
    }
}

/// In-memory DEM conditioning + geometric depression metadata (H0).
/// Does not persist to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepressionAnalysis {
    /// Immutable terrain input used to classify filled depressions.
    pub original_heights: Vec<i32>,
    /// Routing surface after sink fill (same semantics as legacy `resolve_depressions`).
    pub conditioned_heights: Vec<i32>,
    /// Strict, deterministic Priority-Flood order for terrain-only routing.
    pub flood_rank: Vec<u32>,
    /// One terrain-only receiver per land cell. `None` is an ocean or endorheic terminal.
    pub provisional_receiver: Vec<Option<usize>>,
    /// Per-cell fill depth: `conditioned - elevation` on land; 0 on ocean.
    pub fill_depth: Vec<i32>,
    /// Geometric depression basin id per cell; 0 = ocean / unassigned.
    pub basin_id: Vec<u32>,
    /// Basin id → spill outlet cell index (when a geometric path to lower exit exists).
    pub spill_cell: HashMap<u32, usize>,
    /// Basin id → elevation at spill cell (conditioned surface).
    pub spill_elevation: HashMap<u32, i32>,
    /// Basin id → downstream depression basin, if the spill enters one.
    pub basin_parent: HashMap<u32, Option<u32>>,
}

/// Ephemeral terrain-only routing and runoff accumulation for lake selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionalDrainage {
    /// Copy of the deterministic terrain receiver graph from `DepressionAnalysis`.
    pub receiver: Vec<Option<usize>>,
    /// Local runoff plus all upstream terrain contributions.
    pub accumulated_runoff: Vec<u64>,
    /// Selected depression basin → contributing runoff at its lowest-rank cell.
    pub basin_supply: HashMap<u32, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{DenseLayer, DenseState, LayerValue};

    fn land_elevation(n: usize) -> DenseLayer {
        let mut elev = DenseLayer::new_integer("elevation", n);
        for i in 0..n {
            elev.set(i, DenseState::Value(LayerValue::Int(20)));
        }
        elev
    }

    #[test]
    fn missing_precip_is_missing() {
        let elev = land_elevation(4);
        assert_eq!(
            classify_precip_input(&elev, None),
            PrecipInputState::Missing
        );
    }

    #[test]
    fn empty_precip_layer_is_invalid() {
        let elev = land_elevation(4);
        let precip = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, 4);
        assert_eq!(
            classify_precip_input(&elev, Some(&precip)),
            PrecipInputState::InvalidOrEmpty
        );
    }

    #[test]
    fn valid_dry_climate_stays_valid() {
        let elev = land_elevation(4);
        let mut precip = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, 4);
        for i in 0..4 {
            precip.set(i, DenseState::Value(LayerValue::Int(3)));
        }
        let state = classify_precip_input(&elev, Some(&precip));
        assert_eq!(state, PrecipInputState::Valid);
        assert_eq!(terrain_runoff(0, Some(&precip), state), 3);
    }

    #[test]
    fn invalid_all_zero_land_values_use_fallback() {
        let elev = land_elevation(4);
        let mut precip = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, 4);
        for i in 0..4 {
            precip.set(i, DenseState::Value(LayerValue::Int(0)));
        }
        let state = classify_precip_input(&elev, Some(&precip));
        assert_eq!(state, PrecipInputState::InvalidOrEmpty);
        assert_eq!(terrain_runoff(0, Some(&precip), state), FALLBACK_LAND_RUNOFF);
    }

    #[test]
    fn lakes_and_channels_share_classification_policy() {
        let elev = land_elevation(6);
        let mut wet = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, 6);
        for i in 0..6 {
            wet.set(i, DenseState::Value(LayerValue::Int(120)));
        }
        let state = classify_precip_input(&elev, Some(&wet));
        assert_eq!(state, PrecipInputState::Valid);
        assert_eq!(terrain_runoff(2, Some(&wet), state), 120);
    }
}
