//! Product/catalog projections derived from the physical RiverGraph.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::layer::{DenseLayer, DenseState, LayerValue, RIVER_ID_LAYER_ID};
use crate::rivers::{River, RiverCatalog, RIVER_CATALOG_SCHEMA_VERSION};

use super::channel_graph::{PhysicalSegment, RiverGraph, RiverGraphNodeKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalRiverSegment {
    pub id: u32,
    pub from_node: u32,
    pub to_node: u32,
    pub cells: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedRiverBinding {
    pub name: String,
    pub segment_ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameMigrationReport {
    pub name: String,
    pub segment_id: Option<u32>,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HydrologyCatalog {
    pub physical_segments: Vec<PhysicalRiverSegment>,
    pub named_rivers: Vec<NamedRiverBinding>,
    pub migration: Vec<NameMigrationReport>,
}

impl HydrologyCatalog {
    pub fn from_river_graph(graph: &RiverGraph, legacy: &RiverCatalog) -> Self {
        let terrain_nodes: HashMap<u32, usize> = graph
            .nodes
            .iter()
            .filter_map(|node| node.terrain_cell.map(|cell| (node.id, cell)))
            .collect();
        let physical_segments: Vec<_> = graph
            .segments
            .iter()
            .map(|segment| physical_segment(segment, &terrain_nodes))
            .collect();
        let mut named_rivers = Vec::new();
        let mut migration = Vec::new();
        for legacy_river in legacy.rivers.iter().filter(|river| river.name.is_some()) {
            let name = legacy_river.name.clone().unwrap_or_default();
            let candidates: Vec<_> = physical_segments
                .iter()
                .map(|segment| {
                    (
                        segment.id,
                        segment
                            .cells
                            .iter()
                            .filter(|cell| legacy_river.cells.contains(cell))
                            .count(),
                    )
                })
                .filter(|(_, overlap)| *overlap > 0)
                .collect();
            let max_overlap = candidates
                .iter()
                .map(|(_, overlap)| *overlap)
                .max()
                .unwrap_or(0);
            let best: Vec<_> = candidates
                .iter()
                .filter_map(|(id, overlap)| (*overlap == max_overlap).then_some(*id))
                .collect();
            let segment_id = (best.len() == 1).then(|| best[0]);
            if let Some(segment_id) = segment_id {
                named_rivers.push(NamedRiverBinding {
                    name: name.clone(),
                    segment_ids: vec![segment_id],
                });
            }
            migration.push(NameMigrationReport {
                name,
                segment_id,
                ambiguous: best.len() > 1,
            });
        }
        Self {
            physical_segments,
            named_rivers,
            migration,
        }
    }

    /// Lossy legacy compatibility only; it never reconstructs topology.
    pub fn compatibility_river_catalog(&self) -> RiverCatalog {
        let names: HashMap<u32, String> = self
            .named_rivers
            .iter()
            .filter_map(|binding| {
                (binding.segment_ids.len() == 1)
                    .then(|| (binding.segment_ids[0], binding.name.clone()))
            })
            .collect();
        let rivers = self
            .physical_segments
            .iter()
            .filter(|segment| !segment.cells.is_empty())
            .map(|segment| River {
                id: segment.id,
                source: segment.cells[0],
                mouth: *segment.cells.last().unwrap_or(&segment.cells[0]),
                cells: segment.cells.clone(),
                parent: segment.id,
                basin: segment.id,
                name: names.get(&segment.id).cloned(),
            })
            .collect::<Vec<_>>();
        let next_id = rivers
            .iter()
            .map(|river| river.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        RiverCatalog {
            schema_version: RIVER_CATALOG_SCHEMA_VERSION,
            rivers,
            next_id,
        }
    }
}

fn physical_segment(
    segment: &PhysicalSegment,
    terrain_nodes: &HashMap<u32, usize>,
) -> PhysicalRiverSegment {
    let mut cells = Vec::new();
    if let Some(&cell) = terrain_nodes.get(&segment.from_node) {
        cells.push(cell);
    }
    cells.extend(segment.cells.iter().copied());
    if let Some(&cell) = terrain_nodes.get(&segment.to_node) {
        if cells.last().copied() != Some(cell) {
            cells.push(cell);
        }
    }
    PhysicalRiverSegment {
        id: segment.id,
        from_node: segment.from_node,
        to_node: segment.to_node,
        cells,
    }
}

/// Dense compatibility projection. Topology remains in `RiverGraph`.
pub fn compatibility_river_id_layer(graph: &RiverGraph, cell_count: usize) -> DenseLayer {
    let mut layer = DenseLayer::new_integer(RIVER_ID_LAYER_ID, cell_count);
    for cell in 0..cell_count {
        layer.set(cell, DenseState::Value(LayerValue::Int(0)));
    }
    for segment in &graph.segments {
        for &cell in &segment.cells {
            if cell < cell_count {
                layer.set(cell, DenseState::Value(LayerValue::Int(segment.id as i32)));
            }
        }
    }
    for node in &graph.nodes {
        let Some(cell) = node.terrain_cell else {
            continue;
        };
        let segment = match node.kind {
            RiverGraphNodeKind::Source
            | RiverGraphNodeKind::Confluence
            | RiverGraphNodeKind::LakeOutlet => graph
                .segments
                .iter()
                .find(|segment| segment.from_node == node.id),
            RiverGraphNodeKind::Mouth | RiverGraphNodeKind::LakeInlet => graph
                .segments
                .iter()
                .find(|segment| segment.to_node == node.id),
        };
        if let Some(segment) = segment {
            if cell < cell_count {
                layer.set(cell, DenseState::Value(LayerValue::Int(segment.id as i32)));
            }
        }
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::hydrology::{RiverGraphNode, RiverGraphNodeKind};

    #[test]
    fn confluence_projection_belongs_to_the_downstream_segment() {
        let graph = RiverGraph {
            nodes: vec![
                RiverGraphNode {
                    id: 1,
                    kind: RiverGraphNodeKind::Source,
                    drainage_node: super::super::drainage_graph::DrainageNodeId(0),
                    terrain_cell: Some(1),
                },
                RiverGraphNode {
                    id: 2,
                    kind: RiverGraphNodeKind::Confluence,
                    drainage_node: super::super::drainage_graph::DrainageNodeId(1),
                    terrain_cell: Some(3),
                },
                RiverGraphNode {
                    id: 3,
                    kind: RiverGraphNodeKind::Mouth,
                    drainage_node: super::super::drainage_graph::DrainageNodeId(2),
                    terrain_cell: Some(5),
                },
            ],
            segments: vec![
                PhysicalSegment {
                    id: 10,
                    from_node: 1,
                    to_node: 2,
                    cells: vec![2],
                },
                PhysicalSegment {
                    id: 11,
                    from_node: 2,
                    to_node: 3,
                    cells: vec![4],
                },
            ],
            channel_mask: vec![false; 6],
            channel_segment_id: vec![0; 6],
            channel_node_id: vec![0; 6],
        };
        let layer = compatibility_river_id_layer(&graph, 6);

        assert_eq!(layer.int_or(1, -1), 10);
        assert_eq!(layer.int_or(3, -1), 11);
        assert_eq!(layer.int_or(5, -1), 11);
    }

    #[test]
    fn name_migration_binds_only_an_unambiguous_overlapping_segment() {
        let graph = RiverGraph {
            nodes: vec![
                RiverGraphNode {
                    id: 1,
                    kind: RiverGraphNodeKind::Source,
                    drainage_node: super::super::drainage_graph::DrainageNodeId(0),
                    terrain_cell: Some(1),
                },
                RiverGraphNode {
                    id: 2,
                    kind: RiverGraphNodeKind::Mouth,
                    drainage_node: super::super::drainage_graph::DrainageNodeId(1),
                    terrain_cell: Some(3),
                },
            ],
            segments: vec![PhysicalSegment {
                id: 7,
                from_node: 1,
                to_node: 2,
                cells: vec![2],
            }],
            channel_mask: vec![false; 4],
            channel_segment_id: vec![0; 4],
            channel_node_id: vec![0; 4],
        };
        let legacy = RiverCatalog {
            schema_version: RIVER_CATALOG_SCHEMA_VERSION,
            next_id: 2,
            rivers: vec![River {
                id: 1,
                cells: vec![1, 2, 3],
                source: 1,
                mouth: 3,
                parent: 1,
                basin: 1,
                name: Some("Silver".to_string()),
            }],
        };
        let catalog = HydrologyCatalog::from_river_graph(&graph, &legacy);

        assert_eq!(catalog.named_rivers[0].segment_ids, vec![7]);
        assert_eq!(catalog.migration[0].segment_id, Some(7));
        assert!(!catalog.migration[0].ambiguous);
    }
}
