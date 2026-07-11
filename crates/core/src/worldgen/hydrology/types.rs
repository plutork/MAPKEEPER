//! Hydrology analysis types (H0 depression analysis, later lake/river density).

use std::collections::HashMap;

/// In-memory DEM conditioning + geometric depression metadata (H0).
/// Does not copy elevation layer; does not persist to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepressionAnalysis {
    /// Routing surface after sink fill (same semantics as legacy `resolve_depressions`).
    pub conditioned_heights: Vec<i32>,
    /// Per-cell fill depth: `conditioned - elevation` on land; 0 on ocean.
    pub fill_depth: Vec<i32>,
    /// Geometric depression basin id per cell; 0 = ocean / unassigned.
    pub basin_id: Vec<u32>,
    /// Basin id → spill outlet cell index (when a geometric path to lower exit exists).
    pub spill_cell: HashMap<u32, usize>,
    /// Basin id → elevation at spill cell (conditioned surface).
    pub spill_elevation: HashMap<u32, i32>,
}
