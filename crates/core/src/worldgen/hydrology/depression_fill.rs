//! DEM depression analysis — conditioned routing surface + basin metadata (H0).

use std::collections::HashMap;

use crate::hex::MapBounds;
use crate::hydro::{DEFAULT_LAND_ELEVATION, SEA_LEVEL};
use crate::layer::DenseLayer;

use super::types::DepressionAnalysis;

/// Build routing surface and geometric basin/spill metadata from elevation.
pub fn analyze_depressions(elevation: &DenseLayer, bounds: &MapBounds) -> DepressionAnalysis {
    let n = bounds.len();
    let mut conditioned = read_heights(elevation, n);
    condition_depressions(&mut conditioned, bounds);

    let fill_depth: Vec<i32> = (0..n)
        .map(|i| {
            if conditioned[i] <= SEA_LEVEL {
                0
            } else {
                let orig = elevation.int_or(i, DEFAULT_LAND_ELEVATION);
                (conditioned[i] - orig).max(0)
            }
        })
        .collect();

    let (basin_id, spill_cell, spill_elevation) =
        label_depression_basins(&conditioned, &fill_depth, bounds);

    DepressionAnalysis {
        conditioned_heights: conditioned,
        fill_depth,
        basin_id,
        spill_cell,
        spill_elevation,
    }
}

fn read_heights(elevation: &DenseLayer, n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| elevation.int_or(i, DEFAULT_LAND_ELEVATION))
        .collect()
}

/// Raise sinks so each land cell can drain to a lower or equal neighbor.
fn condition_depressions(heights: &mut [i32], bounds: &MapBounds) {
    let land: Vec<usize> = (0..heights.len())
        .filter(|&i| heights[i] > SEA_LEVEL)
        .collect();
    let max_iters = heights.len().max(1);
    for _ in 0..max_iters {
        let mut changed = false;
        let mut sorted = land.clone();
        sorted.sort_by_key(|&i| heights[i]);
        for &i in &sorted {
            let Some(min_n) = lowest_neighbor_elevation(i, heights, bounds) else {
                continue;
            };
            if heights[i] > SEA_LEVEL && min_n > heights[i] {
                heights[i] = min_n;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn lowest_neighbor_elevation(index: usize, heights: &[i32], bounds: &MapBounds) -> Option<i32> {
    neighbor_indices(index, bounds)
        .into_iter()
        .map(|n| heights[n])
        .min()
}

pub(crate) fn lowest_neighbor(index: usize, heights: &[i32], bounds: &MapBounds) -> Option<usize> {
    neighbor_indices(index, bounds)
        .into_iter()
        .min_by_key(|&n| heights[n])
}

fn neighbor_indices(index: usize, bounds: &MapBounds) -> Vec<usize> {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
        .collect()
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
            neighbor_indices(i, bounds).iter().any(|&n| {
                heights[n] <= SEA_LEVEL || (fill_depth[n] == 0 && !in_basin(n))
            })
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
                let edge =
                    row == 0 || col == 0 || row == bounds.height - 1 || col == w - 1;
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
        assert!(has_spill, "coastal depression should expose spill toward sea");
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
}
