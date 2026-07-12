//! Provisional spill → terminal classification (hydrology-lake-terminal-routing-v1).

use std::collections::HashSet;

use crate::hydro::SEA_LEVEL;

use super::types::DepressionAnalysis;

/// Downstream terminal reached from a depression basin spill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpillTerminal {
    Ocean,
    Endorheic,
    Cycle,
    Unresolved,
}

impl SpillTerminal {
    pub fn is_draining(self) -> bool {
        matches!(self, SpillTerminal::Ocean)
    }
}

/// Classify whether a depression basin ultimately drains to ocean or a closed terminal.
pub fn classify_basin_terminal(bid: u32, analysis: &DepressionAnalysis) -> SpillTerminal {
    let Some(mut spill) = analysis.spill_cell.get(&bid).copied() else {
        return SpillTerminal::Unresolved;
    };
    let mut visited_basins = HashSet::from([bid]);
    let mut current_basin = bid;

    loop {
        match trace_from_spill(spill, current_basin, analysis) {
            TerrainTrace::Ocean => return SpillTerminal::Ocean,
            TerrainTrace::Endorheic => return SpillTerminal::Endorheic,
            TerrainTrace::Cycle => return SpillTerminal::Cycle,
            TerrainTrace::DownstreamBasin(downstream) => {
                if !visited_basins.insert(downstream) {
                    return SpillTerminal::Cycle;
                }
                current_basin = downstream;
                spill = match analysis.spill_cell.get(&downstream).copied() {
                    Some(s) => s,
                    None => return SpillTerminal::Unresolved,
                };
            }
        }
    }
}

enum TerrainTrace {
    Ocean,
    Endorheic,
    Cycle,
    DownstreamBasin(u32),
}

fn trace_from_spill(
    start: usize,
    source_basin: u32,
    analysis: &DepressionAnalysis,
) -> TerrainTrace {
    let mut current = start;
    let mut visited = HashSet::new();
    for _ in 0..analysis.original_heights.len().saturating_add(1) {
        if analysis.original_heights[current] <= SEA_LEVEL {
            return TerrainTrace::Ocean;
        }
        let cell_basin = analysis.basin_id[current];
        if cell_basin != 0 && cell_basin != source_basin {
            return TerrainTrace::DownstreamBasin(cell_basin);
        }
        if !visited.insert(current) {
            return TerrainTrace::Cycle;
        }
        match analysis.provisional_receiver[current] {
            None => return TerrainTrace::Endorheic,
            Some(next) => current = next,
        }
    }
    TerrainTrace::Cycle
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mock_analysis(
        original_heights: Vec<i32>,
        provisional_receiver: Vec<Option<usize>>,
        fill_depth: Vec<i32>,
        basin_id: Vec<u32>,
        spill_cell: HashMap<u32, usize>,
        basin_parent: HashMap<u32, Option<u32>>,
    ) -> DepressionAnalysis {
        DepressionAnalysis {
            original_heights: original_heights.clone(),
            conditioned_heights: original_heights,
            flood_rank: vec![0; provisional_receiver.len()],
            provisional_receiver,
            fill_depth,
            basin_id,
            spill_cell,
            spill_elevation: HashMap::new(),
            basin_parent,
        }
    }

    #[test]
    fn traces_receiver_chain_to_ocean_without_neighbor_touch() {
        // Spill cell 1 is not ocean-adjacent; receiver chain reaches cell 4 (ocean).
        let analysis = mock_analysis(
            vec![0, 25, 22, 18, 0],
            vec![None, Some(2), Some(3), Some(4), None],
            vec![0, 3, 2, 0, 0],
            vec![0, 1, 1, 1, 0],
            HashMap::from([(1, 1)]),
            HashMap::from([(1, None)]),
        );
        assert_eq!(classify_basin_terminal(1, &analysis), SpillTerminal::Ocean);
    }

    #[test]
    fn follows_downstream_basin_chain_to_ocean() {
        let analysis = mock_analysis(
            vec![0, 20, 20, 18, 16, 0],
            vec![None, Some(2), Some(3), Some(4), Some(5), None],
            vec![0, 2, 2, 2, 2, 0],
            vec![0, 1, 1, 2, 2, 0],
            HashMap::from([(1, 2), (2, 4)]),
            HashMap::from([(1, Some(2)), (2, None)]),
        );
        assert_eq!(classify_basin_terminal(1, &analysis), SpillTerminal::Ocean);
    }

    #[test]
    fn closed_receiver_chain_is_endorheic() {
        let analysis = mock_analysis(
            vec![10, 8, 8],
            vec![None, None, Some(1)],
            vec![0, 2, 2],
            vec![0, 1, 1],
            HashMap::from([(1, 1)]),
            HashMap::from([(1, None)]),
        );
        assert_eq!(classify_basin_terminal(1, &analysis), SpillTerminal::Endorheic);
    }

    #[test]
    fn basin_cycle_is_detected() {
        let analysis = mock_analysis(
            vec![10, 10, 10],
            vec![None, Some(2), Some(1)],
            vec![0, 2, 2],
            vec![0, 1, 1],
            HashMap::from([(1, 1)]),
            HashMap::from([(1, None)]),
        );
        assert_eq!(classify_basin_terminal(1, &analysis), SpillTerminal::Cycle);
    }
}
