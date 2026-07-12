//! Read-only river paths for map rendering.

use serde::{Deserialize, Serialize};

use super::channel_graph::{RiverGraph, RiverGraphNodeKind};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiverRenderPaths {
    pub paths: Vec<Vec<usize>>,
}

/// Project physical topology into drawable paths without changing hydrology truth.
pub fn river_render_paths(graph: &RiverGraph) -> RiverRenderPaths {
    let path_for = |segment: &super::channel_graph::PhysicalSegment| {
        let mut cells = Vec::with_capacity(segment.cells.len() + 2);
        if let Some(cell) = graph
            .nodes
            .iter()
            .find(|node| node.id == segment.from_node)
            .and_then(|node| node.terrain_cell)
        {
            cells.push(cell);
        }
        cells.extend(segment.cells.iter().copied());
        if let Some(cell) = graph
            .nodes
            .iter()
            .find(|node| node.id == segment.to_node)
            .and_then(|node| node.terrain_cell)
        {
            if cells.last().copied() != Some(cell) {
                cells.push(cell);
            }
        }
        cells
    };

    let segment_paths: Vec<_> = graph
        .segments
        .iter()
        .map(|segment| (segment, path_for(segment)))
        .collect();
    let mut paths = segment_paths
        .iter()
        .filter_map(|(_, cells)| (!cells.is_empty()).then(|| cells.clone()))
        .collect::<Vec<_>>();

    for inlet in graph
        .nodes
        .iter()
        .filter(|node| node.kind == RiverGraphNodeKind::LakeInlet)
    {
        let Some(outlet) = graph.nodes.iter().find(|node| {
            node.kind == RiverGraphNodeKind::LakeOutlet && node.drainage_node == inlet.drainage_node
        }) else {
            continue;
        };
        for (_, incoming) in segment_paths
            .iter()
            .filter(|(segment, _)| segment.to_node == inlet.id)
        {
            for (_, outgoing) in segment_paths
                .iter()
                .filter(|(segment, _)| segment.from_node == outlet.id)
            {
                let (Some(&from), Some(&to)) = (incoming.last(), outgoing.first()) else {
                    continue;
                };
                if from != to {
                    paths.push(vec![from, to]);
                }
            }
        }
    }

    RiverRenderPaths { paths }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::hydrology::{DrainageNodeId, PhysicalSegment, RiverGraphNode};

    fn node(
        id: u32,
        kind: RiverGraphNodeKind,
        drainage: usize,
        terrain_cell: Option<usize>,
    ) -> RiverGraphNode {
        RiverGraphNode {
            id,
            kind,
            drainage_node: DrainageNodeId(drainage),
            terrain_cell,
        }
    }

    #[test]
    fn render_paths_include_confluence_cells() {
        let graph = RiverGraph {
            nodes: vec![
                node(1, RiverGraphNodeKind::Source, 0, Some(1)),
                node(2, RiverGraphNodeKind::Confluence, 1, Some(3)),
                node(3, RiverGraphNodeKind::Mouth, 2, Some(5)),
            ],
            segments: vec![
                PhysicalSegment {
                    id: 1,
                    from_node: 1,
                    to_node: 2,
                    cells: vec![2],
                },
                PhysicalSegment {
                    id: 2,
                    from_node: 2,
                    to_node: 3,
                    cells: vec![4],
                },
            ],
            channel_mask: vec![],
            channel_segment_id: vec![],
            channel_node_id: vec![],
        };

        assert_eq!(
            river_render_paths(&graph).paths,
            vec![vec![1, 2, 3], vec![3, 4, 5]]
        );
    }

    #[test]
    fn render_paths_connect_inlet_and_outlet_across_a_lake() {
        let graph = RiverGraph {
            nodes: vec![
                node(1, RiverGraphNodeKind::Source, 0, Some(1)),
                node(2, RiverGraphNodeKind::LakeInlet, 7, None),
                node(3, RiverGraphNodeKind::LakeOutlet, 7, None),
                node(4, RiverGraphNodeKind::Mouth, 8, Some(5)),
            ],
            segments: vec![
                PhysicalSegment {
                    id: 1,
                    from_node: 1,
                    to_node: 2,
                    cells: vec![2],
                },
                PhysicalSegment {
                    id: 2,
                    from_node: 3,
                    to_node: 4,
                    cells: vec![4],
                },
            ],
            channel_mask: vec![],
            channel_segment_id: vec![],
            channel_node_id: vec![],
        };

        assert_eq!(
            river_render_paths(&graph).paths,
            vec![vec![1, 2], vec![4, 5], vec![2, 4]]
        );
    }
}
