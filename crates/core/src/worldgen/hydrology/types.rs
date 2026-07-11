//! Hydrology analysis types (H0 depression analysis, later lake/river density).

use std::collections::HashMap;

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
