//! Read-only diagnostics for the active Hydrology v2 snapshot.

use serde::Serialize;

use super::snapshot::HydrologySnapshot;
use super::types::DepressionAnalysis;
use crate::lakes::LakeCatalog;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HydrologyDiagnostics {
    pub snapshot_active: bool,
    pub depression_basin_count: usize,
    pub depression_cell_count: usize,
    pub unresolved_depression_count: usize,
    pub lake_count: usize,
    pub endorheic_lake_count: usize,
    pub lake_outlet_count: usize,
    pub drainage_node_count: usize,
    pub channel_node_count: usize,
    pub channel_segment_count: usize,
    pub channel_cell_count: usize,
    pub generated_river_count: usize,
}

/// Report persisted v2 topology only; legacy catalogs are migration input, not truth.
pub fn diagnose_hydrology(
    analysis: &DepressionAnalysis,
    lakes: &LakeCatalog,
    snapshot: Option<&HydrologySnapshot>,
) -> HydrologyDiagnostics {
    let basin_ids: std::collections::BTreeSet<u32> = analysis
        .basin_id
        .iter()
        .copied()
        .filter(|&id| id != 0)
        .collect();
    let (
        drainage_node_count,
        channel_node_count,
        channel_segment_count,
        channel_cell_count,
        generated_river_count,
    ) = snapshot.map_or((0, 0, 0, 0, 0), |snapshot| {
        let graph = &snapshot.channels.river_graph;
        (
            snapshot.drainage.nodes.len(),
            graph.nodes.len(),
            graph.segments.len(),
            graph
                .channel_mask
                .iter()
                .filter(|&&is_channel| is_channel)
                .count(),
            snapshot.catalog.physical_segments.len(),
        )
    });

    HydrologyDiagnostics {
        snapshot_active: snapshot.is_some(),
        depression_basin_count: basin_ids.len(),
        depression_cell_count: analysis
            .fill_depth
            .iter()
            .filter(|&&depth| depth > 0)
            .count(),
        unresolved_depression_count: basin_ids
            .iter()
            .filter(|id| !analysis.spill_cell.contains_key(id))
            .count(),
        lake_count: lakes.lakes.len(),
        endorheic_lake_count: lakes.lakes.iter().filter(|lake| lake.endorheic).count(),
        lake_outlet_count: lakes
            .lakes
            .iter()
            .filter(|lake| lake.outlet_cell.is_some() && !lake.endorheic)
            .count(),
        drainage_node_count,
        channel_node_count,
        channel_segment_count,
        channel_cell_count,
        generated_river_count,
    }
}
