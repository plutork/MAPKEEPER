//! Read-only river paths for map rendering.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::hex::{Axial, MapBounds};
use crate::hydro::{hydro_from_elevation, HydroKind, DEFAULT_LAND_ELEVATION};
use crate::layer::DenseLayer;

use super::channel_graph::{RiverGraph, RiverGraphNodeKind};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiverRenderPaths {
    pub paths: Vec<Vec<usize>>,
    /// Land mouth cell → adjacent ocean cell (draw-only, hydrology-river-flow-arrows).
    #[serde(default)]
    pub mouth_extensions: Vec<[usize; 2]>,
}

/// Project physical topology into drawable paths without changing hydrology truth.
pub fn river_render_paths(
    graph: &RiverGraph,
    bounds: &MapBounds,
    elevation: &DenseLayer,
    lake_id: Option<&DenseLayer>,
) -> RiverRenderPaths {
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
                    let span = lake_id
                        .and_then(|layer| lake_span_path(from, to, bounds, layer))
                        .unwrap_or_else(|| vec![from, to]);
                    paths.push(span);
                }
            }
        }
    }

    let mouth_cells: BTreeSet<usize> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == RiverGraphNodeKind::Mouth)
        .filter_map(|node| node.terrain_cell)
        .collect();
    let mouth_extensions = mouth_extensions_for_cells(&mouth_cells, bounds, elevation);

    RiverRenderPaths {
        paths,
        mouth_extensions,
    }
}

/// Legacy catalog paths + optional mouth strokes from terminal cells.
pub fn legacy_river_render_paths(
    paths: Vec<Vec<usize>>,
    bounds: &MapBounds,
    elevation: &DenseLayer,
) -> RiverRenderPaths {
    let mouth_cells: BTreeSet<usize> = paths
        .iter()
        .filter_map(|path| path.last().copied())
        .collect();
    RiverRenderPaths {
        paths,
        mouth_extensions: mouth_extensions_for_cells(&mouth_cells, bounds, elevation),
    }
}

fn mouth_extensions_for_cells(
    mouth_cells: &BTreeSet<usize>,
    bounds: &MapBounds,
    elevation: &DenseLayer,
) -> Vec<[usize; 2]> {
    let mut out = Vec::new();
    for &mouth in mouth_cells {
        let Some(axial) = bounds.from_index(mouth) else {
            continue;
        };
        let Some(ocean) = axial
            .neighbors()
            .iter()
            .filter_map(|neighbor| bounds.index_of(*neighbor))
            .filter(|&idx| is_water(elevation, idx))
            .min()
        else {
            continue;
        };
        out.push([mouth, ocean]);
    }
    out.sort_unstable();
    out
}

fn is_water(elevation: &DenseLayer, index: usize) -> bool {
    matches!(
        hydro_from_elevation(elevation.int_or(index, DEFAULT_LAND_ELEVATION)),
        HydroKind::Water
    )
}

/// Land inlet → lake hex chain → land outlet (D-102 lake as intermediate cells).
fn lake_span_path(
    from: usize,
    to: usize,
    bounds: &MapBounds,
    lake_id_layer: &DenseLayer,
) -> Option<Vec<usize>> {
    let from_ax = bounds.from_index(from)?;
    let to_ax = bounds.from_index(to)?;
    let from_lake_cell = lake_neighbor(from_ax, bounds, lake_id_layer)?;
    let lake_id = lake_id_layer.int_or(from_lake_cell, 0);
    if lake_id <= 0 {
        return None;
    }
    let to_lake_neighbors: Vec<usize> = to_ax
        .neighbors()
        .iter()
        .filter_map(|neighbor| bounds.index_of(*neighbor))
        .filter(|&idx| lake_id_layer.int_or(idx, 0) == lake_id)
        .collect();
    if to_lake_neighbors.is_empty() {
        return None;
    }
    let through_lake = bfs_lake_path(
        from_lake_cell,
        &to_lake_neighbors,
        lake_id,
        bounds,
        lake_id_layer,
    )
    .unwrap_or_else(|| vec![from_lake_cell]);
    let mut path = vec![from];
    path.extend(through_lake);
    if path.last().copied() != Some(to) {
        path.push(to);
    }
    Some(path)
}

fn lake_neighbor(axial: Axial, bounds: &MapBounds, lake_id_layer: &DenseLayer) -> Option<usize> {
    axial
        .neighbors()
        .iter()
        .filter_map(|neighbor| bounds.index_of(*neighbor))
        .find(|&idx| lake_id_layer.int_or(idx, 0) > 0)
}

fn bfs_lake_path(
    start: usize,
    goals: &[usize],
    lake_id: i32,
    bounds: &MapBounds,
    lake_id_layer: &DenseLayer,
) -> Option<Vec<usize>> {
    use std::collections::{HashSet, VecDeque};
    let goal_set: HashSet<usize> = goals.iter().copied().collect();
    if goal_set.contains(&start) {
        return Some(vec![start]);
    }
    let mut queue = VecDeque::from([(start, vec![start])]);
    let mut seen = HashSet::from([start]);
    while let Some((current, path)) = queue.pop_front() {
        let Some(axial) = bounds.from_index(current) else {
            continue;
        };
        for neighbor in axial.neighbors() {
            let Some(idx) = bounds.index_of(neighbor) else {
                continue;
            };
            if lake_id_layer.int_or(idx, 0) != lake_id || seen.contains(&idx) {
                continue;
            }
            let mut next_path = path.clone();
            next_path.push(idx);
            if goal_set.contains(&idx) {
                return Some(next_path);
            }
            seen.insert(idx);
            queue.push_back((idx, next_path));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::{Axial, MapBounds};
    use crate::hydro::OCEAN_ELEVATION;
    use crate::layer::{DenseLayer, DenseState, LayerValue};
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
        let bounds = MapBounds::new(3, 3);
        let elevation = DenseLayer::new_integer("elevation", bounds.len());

        assert_eq!(
            river_render_paths(&graph, &bounds, &elevation, None).paths,
            vec![vec![1, 2, 3], vec![3, 4, 5]]
        );
    }

    #[test]
    fn render_paths_connect_inlet_and_outlet_through_lake_cells() {
        let bounds = MapBounds::new(5, 5);
        let upstream = bounds.index_of(Axial::new(2, 0)).unwrap();
        let inlet = bounds.index_of(Axial::new(1, 0)).unwrap();
        let lake = bounds.index_of(Axial::new(0, 0)).unwrap();
        let outlet = bounds.index_of(Axial::new(-1, 0)).unwrap();
        let downstream = bounds.index_of(Axial::new(-2, 0)).unwrap();
        let mut lake_id = DenseLayer::new_integer("lake_id", bounds.len());
        lake_id.set(lake, DenseState::Value(LayerValue::Int(1)));

        let graph = RiverGraph {
            nodes: vec![
                node(1, RiverGraphNodeKind::Source, 0, Some(upstream)),
                node(2, RiverGraphNodeKind::LakeInlet, 7, None),
                node(3, RiverGraphNodeKind::LakeOutlet, 7, None),
                node(4, RiverGraphNodeKind::Mouth, 8, Some(downstream)),
            ],
            segments: vec![
                PhysicalSegment {
                    id: 1,
                    from_node: 1,
                    to_node: 2,
                    cells: vec![inlet],
                },
                PhysicalSegment {
                    id: 2,
                    from_node: 3,
                    to_node: 4,
                    cells: vec![outlet],
                },
            ],
            channel_mask: vec![],
            channel_segment_id: vec![],
            channel_node_id: vec![],
        };
        let elevation = DenseLayer::new_integer("elevation", bounds.len());

        assert_eq!(
            river_render_paths(&graph, &bounds, &elevation, Some(&lake_id)).paths,
            vec![
                vec![upstream, inlet],
                vec![outlet, downstream],
                vec![inlet, lake, outlet],
            ]
        );
    }

    #[test]
    fn lake_span_path_includes_lake_hex_between_inlet_and_outlet() {
        let bounds = MapBounds::new(5, 5);
        let inlet = bounds.index_of(Axial::new(1, 0)).unwrap();
        let lake = bounds.index_of(Axial::new(0, 0)).unwrap();
        let outlet = bounds.index_of(Axial::new(-1, 0)).unwrap();
        let mut lake_id = DenseLayer::new_integer("lake_id", bounds.len());
        lake_id.set(lake, DenseState::Value(LayerValue::Int(1)));
        assert_eq!(
            lake_span_path(inlet, outlet, &bounds, &lake_id),
            Some(vec![inlet, lake, outlet])
        );
    }

    #[test]
    fn mouth_extension_picks_lowest_ocean_neighbor_index() {
        let bounds = MapBounds::new(3, 3);
        let center = bounds.index_of(Axial::new(0, 0)).unwrap();
        let mut ocean_neighbors: Vec<usize> = Axial::new(0, 0)
            .neighbors()
            .iter()
            .filter_map(|neighbor| bounds.index_of(*neighbor))
            .collect();
        ocean_neighbors.sort_unstable();
        assert!(ocean_neighbors.len() >= 2);
        let pick = ocean_neighbors[0];

        let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
        for i in 0..bounds.len() {
            elevation.set(i, DenseState::Value(LayerValue::Int(10)));
        }
        elevation.set(center, DenseState::Value(LayerValue::Int(5)));
        for idx in &ocean_neighbors {
            elevation.set(*idx, DenseState::Value(LayerValue::Int(OCEAN_ELEVATION)));
        }

        let mut mouths = BTreeSet::new();
        mouths.insert(center);
        assert_eq!(
            mouth_extensions_for_cells(&mouths, &bounds, &elevation),
            vec![[center, pick]]
        );
    }
}
