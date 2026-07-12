//! Flow accumulation and physical channel extraction (hydrology-v2--accumulation-channel-graph).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::climate::PRECIPITATION_LAYER_ID;
use crate::hydro::SEA_LEVEL;
use crate::layer::DenseLayer;

use super::drainage_graph::{DrainageGraph, DrainageNode, DrainageNodeId};
use super::types::DepressionAnalysis;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelPolicy {
    pub min_flow: u64,
    pub min_contributing_area: u32,
}

impl Default for ChannelPolicy {
    fn default() -> Self {
        Self {
            min_flow: 180,
            min_contributing_area: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HydrologyFlow {
    pub local_runoff: Vec<u64>,
    pub accumulated_flow: Vec<u64>,
    pub contributing_area: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiverGraphNodeKind {
    Source,
    Confluence,
    LakeInlet,
    LakeOutlet,
    Mouth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiverGraphNode {
    pub id: u32,
    pub kind: RiverGraphNodeKind,
    pub drainage_node: DrainageNodeId,
    pub terrain_cell: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalSegment {
    pub id: u32,
    pub from_node: u32,
    pub to_node: u32,
    /// Terrain interiors only; endpoint terrain cells belong to neither segment.
    pub cells: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiverGraph {
    pub nodes: Vec<RiverGraphNode>,
    pub segments: Vec<PhysicalSegment>,
    pub channel_mask: Vec<bool>,
    pub channel_segment_id: Vec<u32>,
    pub channel_node_id: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelGraph {
    pub flow: HydrologyFlow,
    pub river_graph: RiverGraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelGraphError {
    RankViolation,
    ConservationViolation,
    DuplicateSegmentOwner { cell: usize },
    NonContinuousSegment { cell: usize },
    ChannelNodeOwnsSegment { cell: usize },
    OrphanChannelCell { cell: usize },
}

/// Derive runoff, channels, and physical segments from final drainage topology.
pub fn build_channel_graph(
    drainage: &DrainageGraph,
    analysis: &DepressionAnalysis,
    precipitation: Option<&DenseLayer>,
    policy: ChannelPolicy,
) -> Result<ChannelGraph, ChannelGraphError> {
    let flow = accumulate(drainage, analysis, precipitation);
    let river_graph = extract_river_graph(drainage, &flow, policy);
    let graph = ChannelGraph { flow, river_graph };
    validate_channel_graph(drainage, &graph)?;
    Ok(graph)
}

fn accumulate(
    drainage: &DrainageGraph,
    analysis: &DepressionAnalysis,
    precipitation: Option<&DenseLayer>,
) -> HydrologyFlow {
    let mut local_runoff = vec![0u64; drainage.nodes.len()];
    let mut contributing_area = vec![0u32; drainage.nodes.len()];
    for (node_id, node) in drainage.nodes.iter().enumerate() {
        let DrainageNode::TerrainCell(cell) = node else {
            continue;
        };
        if analysis.original_heights[*cell] <= SEA_LEVEL {
            continue;
        }
        local_runoff[node_id] = precipitation
            .filter(|layer| layer.layer_id == PRECIPITATION_LAYER_ID)
            .map(|layer| layer.int_or(*cell, 0).max(0) as u64)
            .unwrap_or(90);
        contributing_area[node_id] = 1;
    }

    let mut accumulated_flow = local_runoff.clone();
    let mut order: Vec<usize> = (0..drainage.nodes.len()).collect();
    order.sort_by_key(|&node| std::cmp::Reverse(drainage.rank[node]));
    for node in order {
        let Some(receiver) = drainage.receiver[node] else {
            continue;
        };
        accumulated_flow[receiver.0] =
            accumulated_flow[receiver.0].saturating_add(accumulated_flow[node]);
        contributing_area[receiver.0] =
            contributing_area[receiver.0].saturating_add(contributing_area[node]);
    }

    HydrologyFlow {
        local_runoff,
        accumulated_flow,
        contributing_area,
    }
}

fn extract_river_graph(
    drainage: &DrainageGraph,
    flow: &HydrologyFlow,
    policy: ChannelPolicy,
) -> RiverGraph {
    let mut channel_mask = vec![false; drainage.terrain_receiver.len()];
    let mut node_is_channel = vec![false; drainage.nodes.len()];
    for (node_id, node) in drainage.nodes.iter().enumerate() {
        if let DrainageNode::TerrainCell(cell) = node {
            let is_channel = flow.accumulated_flow[node_id] >= policy.min_flow
                && flow.contributing_area[node_id] >= policy.min_contributing_area;
            channel_mask[*cell] = is_channel;
            node_is_channel[node_id] = is_channel;
        }
    }

    let upstream = upstream_index(drainage);
    let mut nodes = Vec::new();
    let mut terrain_node = vec![None; drainage.terrain_receiver.len()];
    let mut lake_inlet = HashMap::new();
    let mut lake_outlet = HashMap::new();
    let mut lake_mouth = HashMap::new();
    let mut next_id = 1u32;

    for (node_id, node) in drainage.nodes.iter().enumerate() {
        let DrainageNode::TerrainCell(cell) = node else {
            continue;
        };
        if !node_is_channel[node_id] {
            continue;
        }
        let channel_upstream = upstream[node_id]
            .iter()
            .filter(|&&upstream| node_is_channel[upstream])
            .count();
        let receives_lake = upstream[node_id]
            .iter()
            .any(|&upstream| matches!(drainage.nodes[upstream], DrainageNode::Lake(_)));
        let drains_to_terminal = drainage.receiver[node_id]
            .is_some_and(|downstream| is_terminal(&drainage.nodes[downstream.0]));
        let kind = if drains_to_terminal {
            Some(RiverGraphNodeKind::Mouth)
        } else if receives_lake {
            Some(RiverGraphNodeKind::LakeOutlet)
        } else if channel_upstream == 0 {
            Some(RiverGraphNodeKind::Source)
        } else if channel_upstream >= 2 {
            Some(RiverGraphNodeKind::Confluence)
        } else {
            None
        };
        if let Some(kind) = kind {
            terrain_node[*cell] = Some(next_id);
            nodes.push(RiverGraphNode {
                id: next_id,
                kind,
                drainage_node: DrainageNodeId(node_id),
                terrain_cell: Some(*cell),
            });
            next_id += 1;
        }
    }

    for (node_id, node) in drainage.nodes.iter().enumerate() {
        let DrainageNode::Lake(_) = node else {
            continue;
        };
        let has_inlet = upstream[node_id]
            .iter()
            .any(|&upstream| node_is_channel[upstream]);
        let has_outlet = drainage.receiver[node_id].is_some_and(|downstream| {
            node_is_channel[downstream.0] || is_terminal(&drainage.nodes[downstream.0])
        });
        if has_inlet {
            lake_inlet.insert(DrainageNodeId(node_id), next_id);
            nodes.push(RiverGraphNode {
                id: next_id,
                kind: RiverGraphNodeKind::LakeInlet,
                drainage_node: DrainageNodeId(node_id),
                terrain_cell: None,
            });
            next_id += 1;
        }
        if has_outlet {
            lake_outlet.insert(DrainageNodeId(node_id), next_id);
            nodes.push(RiverGraphNode {
                id: next_id,
                kind: RiverGraphNodeKind::LakeOutlet,
                drainage_node: DrainageNodeId(node_id),
                terrain_cell: None,
            });
            next_id += 1;
            if drainage.receiver[node_id]
                .is_some_and(|downstream| is_terminal(&drainage.nodes[downstream.0]))
            {
                lake_mouth.insert(DrainageNodeId(node_id), next_id);
                nodes.push(RiverGraphNode {
                    id: next_id,
                    kind: RiverGraphNodeKind::Mouth,
                    drainage_node: drainage.receiver[node_id].unwrap(),
                    terrain_cell: None,
                });
                next_id += 1;
            }
        }
    }

    let mut channel_node_id = vec![0u32; channel_mask.len()];
    for node in &nodes {
        if let Some(cell) = node.terrain_cell {
            channel_node_id[cell] = node.id;
        }
    }

    let mut segments = Vec::new();
    let mut channel_segment_id = vec![0u32; channel_mask.len()];
    let mut next_segment = 1u32;
    for node in &nodes {
        let target = match node.kind {
            RiverGraphNodeKind::Mouth => None,
            RiverGraphNodeKind::LakeInlet => lake_outlet
                .get(&node.drainage_node)
                .copied()
                .map(SegmentTarget::RiverNode),
            RiverGraphNodeKind::LakeOutlet => lake_mouth
                .get(&node.drainage_node)
                .copied()
                .map(SegmentTarget::RiverNode)
                .or_else(|| {
                    drainage.receiver[node.drainage_node.0].map(SegmentTarget::DrainageNode)
                }),
            _ => drainage.receiver[node.drainage_node.0].map(SegmentTarget::DrainageNode),
        };
        let Some(target) = target else {
            continue;
        };
        let Some((to_node, cells)) = trace_segment(
            target,
            drainage,
            &node_is_channel,
            &terrain_node,
            &lake_inlet,
        ) else {
            continue;
        };
        for &cell in &cells {
            channel_segment_id[cell] = next_segment;
        }
        segments.push(PhysicalSegment {
            id: next_segment,
            from_node: node.id,
            to_node,
            cells,
        });
        next_segment += 1;
    }

    RiverGraph {
        nodes,
        segments,
        channel_mask,
        channel_segment_id,
        channel_node_id,
    }
}

#[derive(Clone, Copy)]
enum SegmentTarget {
    RiverNode(u32),
    DrainageNode(DrainageNodeId),
}

fn trace_segment(
    target: SegmentTarget,
    drainage: &DrainageGraph,
    node_is_channel: &[bool],
    terrain_node: &[Option<u32>],
    lake_inlet: &HashMap<DrainageNodeId, u32>,
) -> Option<(u32, Vec<usize>)> {
    if let SegmentTarget::RiverNode(node) = target {
        return Some((node, Vec::new()));
    }
    let SegmentTarget::DrainageNode(mut current) = target else {
        return None;
    };
    let mut cells = Vec::new();
    loop {
        match &drainage.nodes[current.0] {
            DrainageNode::TerrainCell(cell) => {
                let cell = *cell;
                if let Some(node) = terrain_node[cell] {
                    return Some((node, cells));
                }
                if !node_is_channel[current.0] {
                    return None;
                }
                cells.push(cell);
                current = drainage.receiver[current.0]?;
            }
            DrainageNode::Lake(_) => {
                return lake_inlet.get(&current).copied().map(|node| (node, cells));
            }
            DrainageNode::OceanSink | DrainageNode::EndorheicSink => return None,
        }
    }
}

fn upstream_index(drainage: &DrainageGraph) -> Vec<Vec<usize>> {
    let mut upstream = vec![Vec::new(); drainage.nodes.len()];
    for (node, receiver) in drainage.receiver.iter().enumerate() {
        if let Some(receiver) = receiver {
            upstream[receiver.0].push(node);
        }
    }
    upstream
}

fn is_terminal(node: &DrainageNode) -> bool {
    matches!(node, DrainageNode::OceanSink | DrainageNode::EndorheicSink)
}

pub fn validate_channel_graph(
    drainage: &DrainageGraph,
    graph: &ChannelGraph,
) -> Result<(), ChannelGraphError> {
    for (node, receiver) in drainage.receiver.iter().enumerate() {
        if let Some(receiver) = receiver {
            if drainage.rank[receiver.0] >= drainage.rank[node] {
                return Err(ChannelGraphError::RankViolation);
            }
            if graph.flow.accumulated_flow[receiver.0] < graph.flow.accumulated_flow[node] {
                return Err(ChannelGraphError::ConservationViolation);
            }
        }
    }
    for (cell, &node_id) in graph.river_graph.channel_node_id.iter().enumerate() {
        if node_id != 0 && graph.river_graph.channel_segment_id[cell] != 0 {
            return Err(ChannelGraphError::ChannelNodeOwnsSegment { cell });
        }
    }
    let mut owned_cells = HashSet::new();
    let cell_node: HashMap<usize, usize> = drainage
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(node, value)| match value {
            DrainageNode::TerrainCell(cell) => Some((*cell, node)),
            _ => None,
        })
        .collect();
    for segment in &graph.river_graph.segments {
        for &cell in &segment.cells {
            if !owned_cells.insert(cell) {
                return Err(ChannelGraphError::DuplicateSegmentOwner { cell });
            }
            if graph.river_graph.channel_segment_id[cell] != segment.id {
                return Err(ChannelGraphError::DuplicateSegmentOwner { cell });
            }
        }
        for cells in segment.cells.windows(2) {
            let from = cell_node[&cells[0]];
            let to = cell_node[&cells[1]];
            if drainage.receiver[from] != Some(DrainageNodeId(to)) {
                return Err(ChannelGraphError::NonContinuousSegment { cell: cells[0] });
            }
        }
    }
    for (cell, &channel) in graph.river_graph.channel_mask.iter().enumerate() {
        if channel
            && graph.river_graph.channel_node_id[cell] == 0
            && graph.river_graph.channel_segment_id[cell] == 0
        {
            return Err(ChannelGraphError::OrphanChannelCell { cell });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::MapBounds;
    use crate::lakes::{Lake, LakeCatalog};
    use crate::layer::{DenseLayer, DenseState, LayerValue};
    use crate::worldgen::hydrology::{analyze_depressions, build_drainage_graph};

    fn slope() -> (MapBounds, DepressionAnalysis) {
        let bounds = MapBounds::new(7, 7);
        let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
        for cell in 0..bounds.len() {
            let height = if cell < bounds.width as usize { 0 } else { 20 };
            elevation.set(cell, DenseState::Value(LayerValue::Int(height)));
        }
        (bounds.clone(), analyze_depressions(&elevation, &bounds))
    }

    fn permissive_policy() -> ChannelPolicy {
        ChannelPolicy {
            min_flow: 90,
            min_contributing_area: 1,
        }
    }

    #[test]
    fn channels_are_deterministic_closed_and_conservative() {
        let (bounds, analysis) = slope();
        let drainage = build_drainage_graph(&analysis, &LakeCatalog::default(), &bounds).unwrap();
        let first = build_channel_graph(&drainage, &analysis, None, permissive_policy()).unwrap();
        let second = build_channel_graph(&drainage, &analysis, None, permissive_policy()).unwrap();

        assert_eq!(first, second);
        assert!(first.river_graph.channel_mask.iter().any(|&cell| cell));
        for (node, receiver) in drainage.receiver.iter().enumerate() {
            if let Some(receiver) = receiver {
                assert!(
                    first.flow.accumulated_flow[receiver.0] >= first.flow.accumulated_flow[node]
                );
            }
        }
    }

    #[test]
    fn lake_outflow_uses_the_same_accumulation_field() {
        let (bounds, analysis) = slope();
        let lake_cell = (0..bounds.len())
            .find(|&cell| {
                analysis.original_heights[cell] > SEA_LEVEL
                    && analysis.provisional_receiver[cell]
                        .is_some_and(|next| analysis.original_heights[next] > SEA_LEVEL)
            })
            .unwrap();
        let lakes = LakeCatalog {
            schema_version: 1,
            next_id: 2,
            lakes: vec![Lake {
                id: 1,
                cells: vec![lake_cell],
                outlet_cell: Some(lake_cell),
                endorheic: false,
                name: None,
            }],
        };
        let drainage = build_drainage_graph(&analysis, &lakes, &bounds).unwrap();
        let channels =
            build_channel_graph(&drainage, &analysis, None, permissive_policy()).unwrap();
        let lake = drainage
            .nodes
            .iter()
            .position(|node| *node == DrainageNode::Lake(1))
            .unwrap();
        let receiver = drainage.receiver[lake].unwrap();

        assert!(channels.flow.accumulated_flow[lake] > 0);
        assert!(channels.flow.accumulated_flow[receiver.0] >= channels.flow.accumulated_flow[lake]);
        assert!(channels
            .river_graph
            .nodes
            .iter()
            .any(|node| node.kind == RiverGraphNodeKind::LakeInlet));
        assert!(channels
            .river_graph
            .nodes
            .iter()
            .any(|node| node.kind == RiverGraphNodeKind::LakeOutlet));
    }
}
