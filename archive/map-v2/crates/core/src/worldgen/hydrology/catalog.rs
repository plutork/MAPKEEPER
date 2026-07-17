//! Product/catalog projections derived from the physical RiverGraph.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::layer::{DenseLayer, DenseState, LayerValue, RIVER_ID_LAYER_ID};
use crate::rivers::{River, RiverCatalog, RIVER_CATALOG_SCHEMA_VERSION};

use super::channel_graph::{PhysicalSegment, RiverGraph, RiverGraphNodeKind};

pub const NAMED_RIVER_STORE_SCHEMA_VERSION: u32 = 1;
pub const NAMED_RIVERS_FILE: &str = "named-rivers.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalRiverSegment {
    pub id: u32,
    pub from_node: u32,
    pub to_node: u32,
    pub cells: Vec<usize>,
}

/// Author-facing river identity — stable across regen; not equal to `PhysicalSegment.id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedRiverBinding {
    pub id: u32,
    pub name: String,
    pub segment_ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NamedRiverStore {
    pub schema_version: u32,
    pub next_id: u32,
    pub rivers: Vec<NamedRiverBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameMigrationReport {
    pub name: String,
    pub named_river_id: Option<u32>,
    pub segment_ids: Vec<u32>,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HydrologyCatalog {
    pub physical_segments: Vec<PhysicalRiverSegment>,
    pub named_rivers: Vec<NamedRiverBinding>,
    pub migration: Vec<NameMigrationReport>,
}

impl NamedRiverStore {
    pub fn from_catalog(catalog: &HydrologyCatalog) -> Self {
        let next_id = catalog.next_named_id();
        Self {
            schema_version: NAMED_RIVER_STORE_SCHEMA_VERSION,
            next_id,
            rivers: catalog.named_rivers.clone(),
        }
    }

    pub fn from_json(raw: &str) -> serde_json::Result<Self> {
        serde_json::from_str(raw)
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

impl HydrologyCatalog {
    pub fn from_river_graph(
        graph: &RiverGraph,
        legacy: &RiverCatalog,
        prior_store: Option<&NamedRiverStore>,
        prior_catalog: Option<&HydrologyCatalog>,
    ) -> Self {
        let physical_segments: Vec<_> = graph
            .segments
            .iter()
            .map(|segment| physical_segment(segment, &terrain_nodes(graph)))
            .collect();
        let (named_rivers, migration) = if let Some(store) = prior_store.filter(|s| !s.rivers.is_empty())
        {
            rebind_named_rivers(graph, &physical_segments, store, prior_catalog)
        } else {
            import_legacy_names(graph, &physical_segments, legacy)
        };
        Self {
            physical_segments,
            named_rivers,
            migration,
        }
    }

    pub fn named_river_count(&self) -> usize {
        self.named_rivers.len()
    }

    fn next_named_id(&self) -> u32 {
        self.named_rivers
            .iter()
            .map(|river| river.id)
            .max()
            .unwrap_or_else(|| self.min_named_id().saturating_sub(1))
            .saturating_add(1)
    }

    fn min_named_id(&self) -> u32 {
        self.physical_segments
            .iter()
            .map(|segment| segment.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1)
    }

    /// Lossy legacy compatibility only; segment ids are not author river ids.
    pub fn compatibility_river_catalog(&self) -> RiverCatalog {
        let names: HashMap<u32, String> = self
            .named_rivers
            .iter()
            .flat_map(|binding| {
                binding
                    .segment_ids
                    .iter()
                    .map(|&segment_id| (segment_id, binding.name.clone()))
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

fn terrain_nodes(graph: &RiverGraph) -> HashMap<u32, usize> {
    graph
        .nodes
        .iter()
        .filter_map(|node| node.terrain_cell.map(|cell| (node.id, cell)))
        .collect()
}

fn import_legacy_names(
    graph: &RiverGraph,
    physical_segments: &[PhysicalRiverSegment],
    legacy: &RiverCatalog,
) -> (Vec<NamedRiverBinding>, Vec<NameMigrationReport>) {
    let mut named_rivers = Vec::new();
    let mut migration = Vec::new();
    let mut next_id = physical_segments
        .iter()
        .map(|segment| segment.id)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1);
    for legacy_river in legacy.rivers.iter().filter(|river| river.name.is_some()) {
        let name = legacy_river.name.clone().unwrap_or_default();
        let anchor: HashSet<usize> = legacy_river.cells.iter().copied().collect();
        let (segment_ids, ambiguous) =
            resolve_anchor_binding(graph, physical_segments, &anchor);
        let named_river_id = if !ambiguous && !segment_ids.is_empty() {
            let id = next_id;
            next_id = next_id.saturating_add(1);
            named_rivers.push(NamedRiverBinding {
                id,
                name: name.clone(),
                segment_ids: segment_ids.clone(),
            });
            Some(id)
        } else {
            None
        };
        migration.push(NameMigrationReport {
            name,
            named_river_id,
            segment_ids,
            ambiguous,
        });
    }
    (named_rivers, migration)
}

fn rebind_named_rivers(
    graph: &RiverGraph,
    physical_segments: &[PhysicalRiverSegment],
    prior_store: &NamedRiverStore,
    prior_catalog: Option<&HydrologyCatalog>,
) -> (Vec<NamedRiverBinding>, Vec<NameMigrationReport>) {
    let mut named_rivers = Vec::new();
    let mut migration = Vec::new();
    for prior in &prior_store.rivers {
        let anchor = prior_catalog
            .map(|catalog| collect_segment_cells(catalog, &prior.segment_ids))
            .unwrap_or_default();
        let (segment_ids, ambiguous) =
            resolve_anchor_binding(graph, physical_segments, &anchor);
        if !ambiguous && !segment_ids.is_empty() {
            named_rivers.push(NamedRiverBinding {
                id: prior.id,
                name: prior.name.clone(),
                segment_ids: segment_ids.clone(),
            });
        }
        migration.push(NameMigrationReport {
            name: prior.name.clone(),
            named_river_id: (!ambiguous && !segment_ids.is_empty()).then_some(prior.id),
            segment_ids,
            ambiguous,
        });
    }
    (named_rivers, migration)
}

fn collect_segment_cells(catalog: &HydrologyCatalog, segment_ids: &[u32]) -> HashSet<usize> {
    segment_ids
        .iter()
        .flat_map(|segment_id| {
            catalog
                .physical_segments
                .iter()
                .find(|segment| segment.id == *segment_id)
                .into_iter()
                .flat_map(|segment| segment.cells.iter().copied())
        })
        .collect()
}

fn resolve_anchor_binding(
    graph: &RiverGraph,
    physical_segments: &[PhysicalRiverSegment],
    anchor_cells: &HashSet<usize>,
) -> (Vec<u32>, bool) {
    if anchor_cells.is_empty() {
        return (vec![], false);
    }
    let candidates: Vec<(u32, usize)> = physical_segments
        .iter()
        .map(|segment| {
            let overlap = segment
                .cells
                .iter()
                .filter(|cell| anchor_cells.contains(cell))
                .count();
            (segment.id, overlap)
        })
        .filter(|(_, overlap)| *overlap > 0)
        .collect();
    if candidates.is_empty() {
        return (vec![], false);
    }
    let max_overlap = candidates.iter().map(|(_, overlap)| overlap).max().copied().unwrap_or(0);
    let top: Vec<u32> = candidates
        .iter()
        .filter(|(_, overlap)| *overlap == max_overlap)
        .map(|(id, _)| *id)
        .collect();
    if top.len() > 1 && !all_mutually_connected(graph, &top) {
        return (vec![], true);
    }
    let seed = top[0];
    let bound = expand_connected_overlapping(graph, physical_segments, seed, anchor_cells);
    if bound.is_empty() {
        (vec![], false)
    } else {
        (bound, false)
    }
}

fn all_mutually_connected(graph: &RiverGraph, segment_ids: &[u32]) -> bool {
    if segment_ids.len() <= 1 {
        return true;
    }
    let mut remaining: HashSet<u32> = segment_ids.iter().copied().collect();
    let mut queue = VecDeque::from([segment_ids[0]]);
    remaining.remove(&segment_ids[0]);
    while let Some(current) = queue.pop_front() {
        for other in segment_ids {
            if remaining.contains(other) && segments_share_node(graph, current, *other) {
                remaining.remove(other);
                queue.push_back(*other);
            }
        }
    }
    remaining.is_empty()
}

fn expand_connected_overlapping(
    graph: &RiverGraph,
    physical_segments: &[PhysicalRiverSegment],
    seed: u32,
    anchor_cells: &HashSet<usize>,
) -> Vec<u32> {
    let overlaps = |segment_id: u32| -> bool {
        physical_segments
            .iter()
            .find(|segment| segment.id == segment_id)
            .is_some_and(|segment| {
                segment
                    .cells
                    .iter()
                    .any(|cell| anchor_cells.contains(cell))
            })
    };
    if !overlaps(seed) {
        return vec![];
    }
    let mut bound = vec![seed];
    let mut queue = VecDeque::from([seed]);
    let mut seen = HashSet::from([seed]);
    while let Some(current) = queue.pop_front() {
        for segment in &graph.segments {
            if seen.contains(&segment.id) || !overlaps(segment.id) {
                continue;
            }
            if segments_share_node(graph, current, segment.id) {
                seen.insert(segment.id);
                bound.push(segment.id);
                queue.push_back(segment.id);
            }
        }
    }
    bound.sort_unstable();
    bound
}

fn segments_share_node(graph: &RiverGraph, a: u32, b: u32) -> bool {
    let (Some(seg_a), Some(seg_b)) = (
        graph.segments.iter().find(|segment| segment.id == a),
        graph.segments.iter().find(|segment| segment.id == b),
    ) else {
        return false;
    };
    [seg_a.from_node, seg_a.to_node]
        .iter()
        .any(|node| [seg_b.from_node, seg_b.to_node].contains(node))
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
            RiverGraphNodeKind::Mouth
            | RiverGraphNodeKind::EndorheicMouth
            | RiverGraphNodeKind::LakeInlet => graph
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

    fn two_segment_graph() -> RiverGraph {
        RiverGraph {
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
        }
    }

    #[test]
    fn confluence_projection_belongs_to_the_downstream_segment() {
        let graph = two_segment_graph();
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
        let catalog = HydrologyCatalog::from_river_graph(&graph, &legacy, None, None);

        assert_eq!(catalog.named_rivers[0].id, 8);
        assert_ne!(catalog.named_rivers[0].id, 7);
        assert_eq!(catalog.named_rivers[0].segment_ids, vec![7]);
        assert_eq!(catalog.migration[0].named_river_id, Some(8));
        assert_eq!(catalog.migration[0].segment_ids, vec![7]);
        assert!(!catalog.migration[0].ambiguous);
    }

    #[test]
    fn legacy_name_can_bind_multiple_connected_segments() {
        let graph = two_segment_graph();
        let legacy = RiverCatalog {
            schema_version: RIVER_CATALOG_SCHEMA_VERSION,
            next_id: 2,
            rivers: vec![River {
                id: 1,
                cells: vec![1, 2, 3, 4, 5],
                source: 1,
                mouth: 5,
                parent: 1,
                basin: 1,
                name: Some("Long River".to_string()),
            }],
        };
        let catalog = HydrologyCatalog::from_river_graph(&graph, &legacy, None, None);

        assert_eq!(catalog.named_rivers.len(), 1);
        assert_eq!(catalog.named_rivers[0].segment_ids, vec![10, 11]);
        assert!(!catalog.migration[0].ambiguous);
    }

    #[test]
    fn regen_rebind_preserves_named_river_id_when_anchor_unique() {
        let graph = two_segment_graph();
        let legacy = RiverCatalog::default();
        let first = HydrologyCatalog::from_river_graph(&graph, &legacy, None, None);
        let store = NamedRiverStore {
            schema_version: NAMED_RIVER_STORE_SCHEMA_VERSION,
            next_id: 2,
            rivers: vec![NamedRiverBinding {
                id: 42,
                name: "Main".to_string(),
                segment_ids: vec![10, 11],
            }],
        };
        let second = HydrologyCatalog::from_river_graph(&graph, &legacy, Some(&store), Some(&first));

        assert_eq!(second.named_rivers.len(), 1);
        assert_eq!(second.named_rivers[0].id, 42);
        assert_eq!(second.named_rivers[0].segment_ids, vec![10, 11]);
        assert!(!second.migration[0].ambiguous);
    }

    #[test]
    fn legacy_import_flags_ambiguous_parallel_segments() {
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
                RiverGraphNode {
                    id: 3,
                    kind: RiverGraphNodeKind::Source,
                    drainage_node: super::super::drainage_graph::DrainageNodeId(2),
                    terrain_cell: Some(5),
                },
                RiverGraphNode {
                    id: 4,
                    kind: RiverGraphNodeKind::Mouth,
                    drainage_node: super::super::drainage_graph::DrainageNodeId(3),
                    terrain_cell: Some(7),
                },
            ],
            segments: vec![
                PhysicalSegment {
                    id: 20,
                    from_node: 1,
                    to_node: 2,
                    cells: vec![2],
                },
                PhysicalSegment {
                    id: 21,
                    from_node: 3,
                    to_node: 4,
                    cells: vec![6],
                },
            ],
            channel_mask: vec![false; 8],
            channel_segment_id: vec![0; 8],
            channel_node_id: vec![0; 8],
        };
        let legacy = RiverCatalog {
            schema_version: RIVER_CATALOG_SCHEMA_VERSION,
            next_id: 2,
            rivers: vec![River {
                id: 1,
                cells: vec![2, 6],
                source: 2,
                mouth: 6,
                parent: 1,
                basin: 1,
                name: Some("Fork".to_string()),
            }],
        };
        let catalog = HydrologyCatalog::from_river_graph(&graph, &legacy, None, None);

        assert!(catalog.named_rivers.is_empty());
        assert!(catalog.migration[0].ambiguous);
        assert!(catalog.migration[0].segment_ids.is_empty());
    }
}
