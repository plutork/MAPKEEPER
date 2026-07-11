//! DEM depression analysis — Priority-Flood surface + provisional routing.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::climate::PRECIPITATION_LAYER_ID;
use crate::hex::MapBounds;
use crate::hydro::{DEFAULT_LAND_ELEVATION, SEA_LEVEL};
use crate::layer::DenseLayer;

use super::types::{DepressionAnalysis, ProvisionalDrainage};

const FALLBACK_LAND_RUNOFF: u64 = 90;

/// Build routing surface and geometric basin/spill metadata from elevation.
pub fn analyze_depressions(elevation: &DenseLayer, bounds: &MapBounds) -> DepressionAnalysis {
    let n = bounds.len();
    let original_heights = read_heights(elevation, n);
    let (conditioned_heights, flood_rank, provisional_receiver) =
        priority_flood(&original_heights, bounds);

    let fill_depth: Vec<i32> = (0..n)
        .map(|i| {
            if conditioned_heights[i] <= SEA_LEVEL {
                0
            } else {
                (conditioned_heights[i] - original_heights[i]).max(0)
            }
        })
        .collect();

    let (basin_id, spill_cell, spill_elevation) =
        label_depression_basins(&conditioned_heights, &fill_depth, bounds);
    let basin_parent = basin_hierarchy(
        &basin_id,
        &spill_cell,
        &provisional_receiver,
        &conditioned_heights,
    );

    DepressionAnalysis {
        original_heights,
        conditioned_heights,
        flood_rank,
        provisional_receiver,
        fill_depth,
        basin_id,
        spill_cell,
        spill_elevation,
        basin_parent,
    }
}

fn read_heights(elevation: &DenseLayer, n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| elevation.int_or(i, DEFAULT_LAND_ELEVATION))
        .collect()
}

/// Priority-Flood conditioned surface plus a strict, terrain-only receiver order.
fn priority_flood(
    original: &[i32],
    bounds: &MapBounds,
) -> (Vec<i32>, Vec<u32>, Vec<Option<usize>>) {
    let n = original.len();
    let mut conditioned = original.to_vec();
    let mut rank = vec![u32::MAX; n];
    let mut receiver = vec![None; n];
    let mut visited = vec![false; n];
    let mut queue = BinaryHeap::new();
    let mut next_rank = 0u32;

    for index in 0..n {
        if original[index] <= SEA_LEVEL {
            visited[index] = true;
            queue.push(Reverse((original[index], index)));
        }
    }

    loop {
        while let Some(Reverse((height, index))) = queue.pop() {
            rank[index] = next_rank;
            next_rank = next_rank.saturating_add(1);
            for neighbor in neighbor_indices(index, bounds) {
                if visited[neighbor] || original[neighbor] <= SEA_LEVEL {
                    continue;
                }
                visited[neighbor] = true;
                conditioned[neighbor] = original[neighbor].max(height);
                receiver[neighbor] = Some(index);
                queue.push(Reverse((conditioned[neighbor], neighbor)));
            }
        }

        let Some(seed) = (0..n)
            .filter(|&index| !visited[index] && original[index] > SEA_LEVEL)
            .min_by_key(|&index| (original[index], index))
        else {
            break;
        };
        visited[seed] = true;
        queue.push(Reverse((original[seed], seed)));
    }

    (conditioned, rank, receiver)
}

/// Build the ephemeral terrain-only receiver graph and runoff accumulation.
pub fn provisional_drainage(
    analysis: &DepressionAnalysis,
    precipitation: Option<&DenseLayer>,
    bounds: &MapBounds,
    use_climate: bool,
) -> ProvisionalDrainage {
    let n = bounds.len();
    let mut accumulated_runoff = vec![0u64; n];
    for index in 0..n {
        if analysis.original_heights[index] > SEA_LEVEL {
            accumulated_runoff[index] = if use_climate {
                precipitation
                    .filter(|layer| layer.layer_id == PRECIPITATION_LAYER_ID)
                    .map(|layer| layer.int_or(index, 0).max(0) as u64)
                    .unwrap_or(0)
            } else {
                FALLBACK_LAND_RUNOFF
            };
        }
    }

    let mut cells: Vec<usize> = (0..n)
        .filter(|&index| analysis.original_heights[index] > SEA_LEVEL)
        .collect();
    cells.sort_by_key(|&index| Reverse(analysis.flood_rank[index]));
    for index in cells {
        let Some(receiver) = analysis.provisional_receiver[index] else {
            continue;
        };
        if analysis.original_heights[receiver] > SEA_LEVEL {
            accumulated_runoff[receiver] =
                accumulated_runoff[receiver].saturating_add(accumulated_runoff[index]);
        }
    }

    let mut basin_supply = HashMap::new();
    for (&basin, _) in &analysis.spill_cell {
        let lowest = analysis
            .basin_id
            .iter()
            .enumerate()
            .filter(|(_, &id)| id == basin)
            .min_by_key(|(index, _)| analysis.flood_rank[*index])
            .map(|(index, _)| index);
        if let Some(index) = lowest {
            basin_supply.insert(basin, accumulated_runoff[index]);
        }
    }

    ProvisionalDrainage {
        receiver: analysis.provisional_receiver.clone(),
        accumulated_runoff,
        basin_supply,
    }
}

fn neighbor_indices(index: usize, bounds: &MapBounds) -> Vec<usize> {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
        .collect()
}

fn basin_hierarchy(
    basin_id: &[u32],
    spill_cell: &HashMap<u32, usize>,
    receiver: &[Option<usize>],
    heights: &[i32],
) -> HashMap<u32, Option<u32>> {
    let mut parents = HashMap::new();
    for (&basin, &spill) in spill_cell {
        let mut current = Some(spill);
        let mut parent = None;
        for _ in 0..heights.len() {
            let Some(index) = current else {
                break;
            };
            if heights[index] <= SEA_LEVEL {
                break;
            }
            if basin_id[index] != 0 && basin_id[index] != basin {
                parent = Some(basin_id[index]);
                break;
            }
            current = receiver[index];
        }
        parents.insert(basin, parent);
    }
    parents
}

/// Label geometric depression basins from filled cells (`fill_depth > 0`).
fn label_depression_basins(
    heights: &[i32],
    fill_depth: &[i32],
    bounds: &MapBounds,
) -> (Vec<u32>, HashMap<u32, usize>, HashMap<u32, i32>) {
    let n = heights.len();
    let mut basin_id = vec![0u32; n];
    let mut visited = vec![false; n];
    let mut next_basin = 1u32;
    let mut spill_cell = HashMap::new();
    let mut spill_elevation = HashMap::new();

    for i in 0..n {
        if visited[i] || fill_depth[i] <= 0 || heights[i] <= SEA_LEVEL {
            continue;
        }
        let bid = next_basin;
        next_basin += 1;
        let mut component = Vec::new();
        let mut stack = vec![i];
        visited[i] = true;
        while let Some(cur) = stack.pop() {
            basin_id[cur] = bid;
            component.push(cur);
            for nbr in neighbor_indices(cur, bounds) {
                if visited[nbr] || fill_depth[nbr] <= 0 || heights[nbr] <= SEA_LEVEL {
                    continue;
                }
                visited[nbr] = true;
                stack.push(nbr);
            }
        }
        if let Some(spill) =
            spill_for_component(&component, bid, heights, fill_depth, bounds, &basin_id)
        {
            spill_cell.insert(bid, spill);
            spill_elevation.insert(bid, heights[spill]);
        }
    }

    (basin_id, spill_cell, spill_elevation)
}

fn spill_for_component(
    component: &[usize],
    bid: u32,
    heights: &[i32],
    fill_depth: &[i32],
    bounds: &MapBounds,
    basin_id: &[u32],
) -> Option<usize> {
    let in_basin = |i: usize| basin_id.get(i) == Some(&bid);
    let mut candidates: Vec<usize> = component.to_vec();
    for &i in component {
        for nbr in neighbor_indices(i, bounds) {
            if heights[nbr] > SEA_LEVEL && fill_depth[nbr] == 0 && !in_basin(nbr) {
                candidates.push(nbr);
            }
        }
    }
    candidates.sort_unstable();
    candidates.dedup();

    let ocean_touching: Vec<usize> = candidates
        .into_iter()
        .filter(|&i| {
            neighbor_indices(i, bounds)
                .iter()
                .any(|&n| heights[n] <= SEA_LEVEL)
        })
        .collect();
    if !ocean_touching.is_empty() {
        return ocean_touching.into_iter().min_by_key(|&i| heights[i]);
    }

    let mut enclosed_rim: Vec<usize> = component
        .iter()
        .copied()
        .filter(|&i| {
            neighbor_indices(i, bounds)
                .iter()
                .any(|&n| heights[n] <= SEA_LEVEL || (fill_depth[n] == 0 && !in_basin(n)))
        })
        .collect();
    if enclosed_rim.is_empty() {
        return component.first().copied();
    }
    enclosed_rim.sort_by_key(|&i| heights[i]);
    Some(enclosed_rim[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::Axial;
    use crate::layer::{DenseLayer, DenseState, LayerValue};

    fn set_elev(layer: &mut DenseLayer, bounds: &MapBounds, q: i32, r: i32, v: i32) {
        let i = bounds.index_of(Axial::new(q, r)).unwrap();
        layer.set(i, DenseState::Value(LayerValue::Int(v)));
    }

    #[test]
    fn pit_groups_single_basin() {
        let bounds = MapBounds::new(7, 7);
        let w = bounds.width;
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        for row in 0..bounds.height {
            for col in 0..w {
                let i = (row * w + col) as usize;
                let edge = row == 0 || col == 0 || row == bounds.height - 1 || col == w - 1;
                elev.set(
                    i,
                    DenseState::Value(LayerValue::Int(if edge { 0 } else { 20 })),
                );
            }
        }
        let center = (bounds.height / 2 * w + w / 2) as usize;
        elev.set(center, DenseState::Value(LayerValue::Int(5)));

        let analysis = analyze_depressions(&elev, &bounds);
        let depression_basins: Vec<u32> = analysis
            .basin_id
            .iter()
            .enumerate()
            .filter(|(i, _)| analysis.fill_depth[*i] > 0)
            .map(|(_, b)| *b)
            .collect();
        assert!(
            !depression_basins.is_empty(),
            "pit should produce filled depression cells"
        );
        let unique: std::collections::HashSet<_> = depression_basins.iter().copied().collect();
        assert_eq!(unique.len(), 1, "pit depression should be one basin");
    }

    #[test]
    fn ocean_adjacent_basin_has_spill_to_sea() {
        let bounds = MapBounds::new(7, 7);
        let w = bounds.width;
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        for row in 0..bounds.height {
            for col in 0..w {
                let i = (row * w + col) as usize;
                let ocean = col == 0;
                elev.set(
                    i,
                    DenseState::Value(LayerValue::Int(if ocean { 0 } else { 25 })),
                );
            }
        }
        // coastal bowl open to ocean on the west
        let pit = (3 * w + 2) as usize;
        elev.set(pit, DenseState::Value(LayerValue::Int(8)));
        elev.set((3 * w + 1) as usize, DenseState::Value(LayerValue::Int(12)));

        let analysis = analyze_depressions(&elev, &bounds);
        assert!(
            analysis.fill_depth.iter().any(|&d| d > 0),
            "coastal bowl should require fill"
        );
        let has_spill = analysis.spill_cell.values().any(|&cell| {
            neighbor_indices(cell, &bounds)
                .iter()
                .any(|&n| analysis.conditioned_heights[n] <= SEA_LEVEL)
        });
        assert!(
            has_spill,
            "coastal depression should expose spill toward sea"
        );
    }

    #[test]
    fn fill_depth_nonnegative_on_land() {
        let bounds = MapBounds::new(8, 6);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        for q in 0..8 {
            for r in 0..6 {
                if bounds.contains(Axial::new(q, r)) {
                    set_elev(&mut elev, &bounds, q, r, 10 + q);
                }
            }
        }
        let analysis = analyze_depressions(&elev, &bounds);
        for (i, &fd) in analysis.fill_depth.iter().enumerate() {
            if analysis.conditioned_heights[i] > SEA_LEVEL {
                assert!(fd >= 0);
            } else {
                assert_eq!(fd, 0);
            }
        }
    }

    #[test]
    fn priority_flood_ranks_are_deterministic_and_descend_to_terrain_receivers() {
        let bounds = MapBounds::new(7, 7);
        let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
        for index in 0..bounds.len() {
            elevation.set(index, DenseState::Value(LayerValue::Int(20)));
        }
        for index in 0..bounds.width as usize {
            elevation.set(index, DenseState::Value(LayerValue::Int(0)));
        }
        let center = (bounds.height / 2 * bounds.width + bounds.width / 2) as usize;
        elevation.set(center, DenseState::Value(LayerValue::Int(4)));

        let first = analyze_depressions(&elevation, &bounds);
        let second = analyze_depressions(&elevation, &bounds);

        assert_eq!(first, second);
        assert_eq!(first.original_heights[0], 0);
        for (index, receiver) in first.provisional_receiver.iter().enumerate() {
            let Some(receiver) = receiver else {
                continue;
            };
            if first.original_heights[*receiver] > SEA_LEVEL {
                assert!(
                    first.flood_rank[*receiver] < first.flood_rank[index],
                    "receiver rank must decrease at {index}"
                );
            }
        }
    }

    #[test]
    fn provisional_runoff_accumulates_at_a_depression_basin() {
        let bounds = MapBounds::new(7, 7);
        let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
        for index in 0..bounds.len() {
            elevation.set(index, DenseState::Value(LayerValue::Int(20)));
        }
        for index in 0..bounds.width as usize {
            elevation.set(index, DenseState::Value(LayerValue::Int(0)));
        }
        let center = (bounds.height / 2 * bounds.width + bounds.width / 2) as usize;
        elevation.set(center, DenseState::Value(LayerValue::Int(4)));
        let analysis = analyze_depressions(&elevation, &bounds);
        let drainage = provisional_drainage(&analysis, None, &bounds, false);

        assert!(
            drainage.basin_supply.values().any(|&supply| supply > 1),
            "a basin should receive runoff beyond its local cell"
        );
    }
}
