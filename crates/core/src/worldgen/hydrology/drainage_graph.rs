//! Final lake-aware drainage topology (hydrology-v2--final-drainage-graph).

use std::collections::{HashMap, HashSet};

use crate::hex::MapBounds;
use crate::hydro::SEA_LEVEL;
use crate::lakes::LakeCatalog;

use super::types::DepressionAnalysis;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DrainageNodeId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrainageNode {
    TerrainCell(usize),
    Lake(u32),
    OceanSink,
    EndorheicSink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainageGraph {
    pub nodes: Vec<DrainageNode>,
    pub receiver: Vec<Option<DrainageNodeId>>,
    pub rank: Vec<u32>,
    /// Dense per-cell projection; lake and ocean cells have no terrain node.
    pub terrain_receiver: Vec<Option<DrainageNodeId>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrainageGraphError {
    InvalidLakeCell { lake_id: u32, cell: usize },
    OverlappingLakeCell { cell: usize },
    MissingLakeOutlet { lake_id: u32 },
    IllegalLakeOutlet { lake_id: u32, cell: usize },
    ReceiverCycle,
}

/// Materialize final terrain, lake, and terminal-sink topology after lake selection.
pub fn build_drainage_graph(
    analysis: &DepressionAnalysis,
    lakes: &LakeCatalog,
    bounds: &MapBounds,
) -> Result<DrainageGraph, DrainageGraphError> {
    let lake_at_cell = lake_cells(lakes, bounds)?;
    let mut nodes = Vec::new();
    let mut terrain_node = vec![None; bounds.len()];
    for cell in 0..bounds.len() {
        if analysis.original_heights[cell] > SEA_LEVEL && !lake_at_cell.contains_key(&cell) {
            terrain_node[cell] = Some(DrainageNodeId(nodes.len()));
            nodes.push(DrainageNode::TerrainCell(cell));
        }
    }

    let mut lake_node = HashMap::new();
    let mut sorted_lakes: Vec<_> = lakes.lakes.iter().collect();
    sorted_lakes.sort_by_key(|lake| lake.id);
    for lake in &sorted_lakes {
        lake_node.insert(lake.id, DrainageNodeId(nodes.len()));
        nodes.push(DrainageNode::Lake(lake.id));
    }
    let ocean_sink = DrainageNodeId(nodes.len());
    nodes.push(DrainageNode::OceanSink);
    let endorheic_sink = DrainageNodeId(nodes.len());
    nodes.push(DrainageNode::EndorheicSink);

    let mut receiver = vec![None; nodes.len()];
    for (cell, node) in terrain_node
        .iter()
        .enumerate()
        .filter_map(|(cell, node)| node.map(|node| (cell, node)))
    {
        receiver[node.0] = Some(
            analysis.provisional_receiver[cell]
                .map(|next| {
                    resolve_cell_target(
                        next,
                        None,
                        analysis,
                        &lake_at_cell,
                        &terrain_node,
                        &lake_node,
                        ocean_sink,
                        endorheic_sink,
                    )
                })
                .unwrap_or(endorheic_sink),
        );
    }

    for lake in sorted_lakes {
        let node = lake_node[&lake.id];
        receiver[node.0] = Some(if lake.endorheic {
            endorheic_sink
        } else {
            let outlet = lake
                .outlet_cell
                .ok_or(DrainageGraphError::MissingLakeOutlet { lake_id: lake.id })?;
            if !lake_at_cell
                .get(&outlet)
                .is_some_and(|&owner| owner == lake.id)
                && !touches_lake(outlet, lake.id, &lake_at_cell, bounds)
            {
                return Err(DrainageGraphError::IllegalLakeOutlet {
                    lake_id: lake.id,
                    cell: outlet,
                });
            }
            resolve_cell_target(
                outlet,
                Some(lake.id),
                analysis,
                &lake_at_cell,
                &terrain_node,
                &lake_node,
                ocean_sink,
                endorheic_sink,
            )
        });
    }

    let rank = ranks(&nodes, &receiver)?;
    let terrain_receiver = terrain_node
        .iter()
        .map(|node| node.and_then(|node| receiver[node.0]))
        .collect();
    Ok(DrainageGraph {
        nodes,
        receiver,
        rank,
        terrain_receiver,
    })
}

fn lake_cells(
    lakes: &LakeCatalog,
    bounds: &MapBounds,
) -> Result<HashMap<usize, u32>, DrainageGraphError> {
    let mut cells = HashMap::new();
    for lake in &lakes.lakes {
        for &cell in &lake.cells {
            if cell >= bounds.len() {
                return Err(DrainageGraphError::InvalidLakeCell {
                    lake_id: lake.id,
                    cell,
                });
            }
            if cells.insert(cell, lake.id).is_some() {
                return Err(DrainageGraphError::OverlappingLakeCell { cell });
            }
        }
    }
    Ok(cells)
}

#[allow(clippy::too_many_arguments)]
fn resolve_cell_target(
    start: usize,
    source_lake: Option<u32>,
    analysis: &DepressionAnalysis,
    lake_at_cell: &HashMap<usize, u32>,
    terrain_node: &[Option<DrainageNodeId>],
    lake_node: &HashMap<u32, DrainageNodeId>,
    ocean_sink: DrainageNodeId,
    endorheic_sink: DrainageNodeId,
) -> DrainageNodeId {
    let mut current = start;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(current) {
            return endorheic_sink;
        }
        if analysis.original_heights[current] <= SEA_LEVEL {
            return ocean_sink;
        }
        if let Some(&lake_id) = lake_at_cell.get(&current) {
            if Some(lake_id) != source_lake {
                return lake_node[&lake_id];
            }
            let Some(next) = analysis.provisional_receiver[current] else {
                return endorheic_sink;
            };
            current = next;
            continue;
        }
        return terrain_node[current].unwrap_or(endorheic_sink);
    }
}

fn touches_lake(
    cell: usize,
    lake_id: u32,
    lake_at_cell: &HashMap<usize, u32>,
    bounds: &MapBounds,
) -> bool {
    bounds
        .from_index(cell)
        .into_iter()
        .flat_map(|axial| axial.neighbors())
        .filter_map(|neighbor| bounds.index_of(neighbor))
        .any(|neighbor| lake_at_cell.get(&neighbor) == Some(&lake_id))
}

fn ranks(
    nodes: &[DrainageNode],
    receiver: &[Option<DrainageNodeId>],
) -> Result<Vec<u32>, DrainageGraphError> {
    let mut rank = vec![0u32; nodes.len()];
    let mut visiting = vec![false; nodes.len()];
    let mut resolved = vec![false; nodes.len()];
    for node in 0..nodes.len() {
        assign_rank(node, receiver, &mut rank, &mut visiting, &mut resolved)?;
    }
    Ok(rank)
}

fn assign_rank(
    node: usize,
    receiver: &[Option<DrainageNodeId>],
    rank: &mut [u32],
    visiting: &mut [bool],
    resolved: &mut [bool],
) -> Result<u32, DrainageGraphError> {
    if resolved[node] {
        return Ok(rank[node]);
    }
    if visiting[node] {
        return Err(DrainageGraphError::ReceiverCycle);
    }
    visiting[node] = true;
    rank[node] = receiver[node]
        .map(|downstream| assign_rank(downstream.0, receiver, rank, visiting, resolved))
        .transpose()?
        .unwrap_or(0)
        .saturating_add(u32::from(receiver[node].is_some()));
    visiting[node] = false;
    resolved[node] = true;
    Ok(rank[node])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lakes::Lake;
    use crate::layer::{DenseLayer, DenseState, LayerValue};
    use crate::worldgen::hydrology::analyze_depressions;

    fn slope_analysis() -> (MapBounds, DepressionAnalysis) {
        let bounds = MapBounds::new(7, 7);
        let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
        for index in 0..bounds.len() {
            let height = if index < bounds.width as usize { 0 } else { 20 };
            elevation.set(index, DenseState::Value(LayerValue::Int(height)));
        }
        (bounds.clone(), analyze_depressions(&elevation, &bounds))
    }

    #[test]
    fn lake_chain_is_ranked_and_acyclic() {
        let (bounds, analysis) = slope_analysis();
        let first = (0..bounds.len())
            .find(|&cell| {
                analysis.original_heights[cell] > SEA_LEVEL
                    && analysis.provisional_receiver[cell]
                        .is_some_and(|next| analysis.original_heights[next] > SEA_LEVEL)
            })
            .unwrap();
        let second = analysis.provisional_receiver[first].unwrap();
        let catalog = LakeCatalog {
            schema_version: 1,
            next_id: 3,
            lakes: vec![
                Lake {
                    id: 1,
                    cells: vec![first],
                    outlet_cell: Some(first),
                    endorheic: false,
                    name: None,
                },
                Lake {
                    id: 2,
                    cells: vec![second],
                    outlet_cell: Some(second),
                    endorheic: false,
                    name: None,
                },
            ],
        };

        let graph = build_drainage_graph(&analysis, &catalog, &bounds).unwrap();
        let first_node = graph
            .nodes
            .iter()
            .position(|node| *node == DrainageNode::Lake(1))
            .unwrap();
        let second_node = graph
            .nodes
            .iter()
            .position(|node| *node == DrainageNode::Lake(2))
            .unwrap();
        assert_eq!(
            graph.receiver[first_node],
            Some(DrainageNodeId(second_node))
        );
        assert!(graph.rank[first_node] > graph.rank[second_node]);
    }

    #[test]
    fn endorheic_lake_routes_to_terminal_sink() {
        let (bounds, analysis) = slope_analysis();
        let cell = (0..bounds.len())
            .find(|&cell| analysis.original_heights[cell] > SEA_LEVEL)
            .unwrap();
        let catalog = LakeCatalog {
            schema_version: 1,
            next_id: 2,
            lakes: vec![Lake {
                id: 9,
                cells: vec![cell],
                outlet_cell: None,
                endorheic: true,
                name: None,
            }],
        };

        let graph = build_drainage_graph(&analysis, &catalog, &bounds).unwrap();
        let lake = graph
            .nodes
            .iter()
            .position(|node| *node == DrainageNode::Lake(9))
            .unwrap();
        let terminal = graph.receiver[lake].unwrap();
        assert_eq!(graph.nodes[terminal.0], DrainageNode::EndorheicSink);
        assert!(graph.rank[lake] > graph.rank[terminal.0]);
    }
}
