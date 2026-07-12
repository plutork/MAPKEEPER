//! Organic blob growth, prune, and shore fringe (D-66).

use crate::hex::{Axial, MapBounds};
use crate::layer::{DenseLayer, DenseState, LayerValue};

use super::types::{LAND_MASK_LAND, LAND_MASK_OCEAN, ShoreCharacter};
use super::util::{
    component_centroid, elongation_axis, flood_ocean, is_land_cell, land_components, mix64,
    opposite_hex_pair, set_land, shuffle_six, unit01,
};

/// Azgaar-style blob growth: parent height × decay × sharpness RNG; max-blend layers.
/// Neighbor order is shuffled each step so hex axes do not form persistent strips (D-66).
#[allow(clippy::too_many_arguments)]
pub(crate) fn grow_blob(
    bounds: &MapBounds,
    heights: &mut [f64],
    start: usize,
    start_h: f64,
    decay: f64,
    sharpness: f64,
    elongation: f64,
    axis_x: f64,
    axis_y: f64,
    seed: u64,
) {
    let n = heights.len();
    let mut used = vec![false; n];
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    heights[start] = heights[start].max(start_h);
    used[start] = true;
    queue.push_back(start);
    let mut step = 0u64;
    // Mild elongation only — strong axis stretch made comb-like coasts.
    let elong = (elongation * 0.12).clamp(0.0, 0.12);
    while let Some(index) = queue.pop_front() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let parent_h = heights[index];
        if parent_h < 0.02 {
            continue;
        }
        let (px, py) = cell.to_pixel(1.0);
        let mut neigh = cell.neighbors();
        shuffle_six(
            &mut neigh,
            seed ^ step.wrapping_mul(0xC2B2) ^ (index as u64),
        );
        for ncell in neigh {
            let Some(ni) = bounds.index_of(ncell) else {
                continue;
            };
            if used[ni] {
                continue;
            }
            used[ni] = true;
            step = step.wrapping_add(1);
            // Sharpness band kept narrow so ridges cannot runaway along one hex ray.
            let mut mod_v = if sharpness <= 0.05 {
                1.0
            } else {
                let band = sharpness.min(0.55);
                unit01(seed ^ step.wrapping_mul(0x9E37) ^ (ni as u64)) * band + (1.0 - band * 0.5)
            };
            mod_v = mod_v.clamp(0.72, 1.12);
            if elong > 0.01 {
                let (nx, ny) = ncell.to_pixel(1.0);
                let dx = nx - px;
                let dy = ny - py;
                let along = (dx * axis_x + dy * axis_y).abs();
                let across = (dx * -axis_y + dy * axis_x).abs();
                let stretch = 1.0 + elong * (along - across).tanh();
                mod_v *= stretch;
                mod_v = mod_v.clamp(0.70, 1.15);
            }
            let h = parent_h * decay * mod_v;
            if h > heights[ni] {
                heights[ni] = h;
            }
            if h > 0.02 {
                queue.push_back(ni);
            }
        }
    }
}

/// Remove 1-cell-wide corridors / tips that read as hex-axis "strips".
pub(crate) fn prune_thin_corridors(bounds: &MapBounds, layer: &mut DenseLayer, passes: usize) {
    for _ in 0..passes {
        let mut drop: Vec<usize> = Vec::new();
        for index in 0..bounds.len() {
            if !is_land_cell(layer, index) {
                continue;
            }
            let Some(cell) = bounds.from_index(index) else {
                continue;
            };
            let mut land_ns: Vec<Axial> = Vec::new();
            for nb in cell.neighbors() {
                let Some(ni) = bounds.index_of(nb) else {
                    continue;
                };
                if is_land_cell(layer, ni) {
                    land_ns.push(nb);
                }
            }
            let kill = match land_ns.len() {
                0 | 1 => true,
                2 => opposite_hex_pair(cell, land_ns[0], land_ns[1]),
                _ => false,
            };
            if kill {
                drop.push(index);
            }
        }
        if drop.is_empty() {
            break;
        }
        for index in drop {
            layer.set(
                index,
                DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
            );
        }
    }
}

/// Only 1-wide opposite corridors (hex-axis fingers) — keeps coastal tips intact.
pub(crate) fn prune_axis_corridors(bounds: &MapBounds, layer: &mut DenseLayer, passes: usize) {
    for _ in 0..passes {
        let mut drop: Vec<usize> = Vec::new();
        for index in 0..bounds.len() {
            if !is_land_cell(layer, index) {
                continue;
            }
            let Some(cell) = bounds.from_index(index) else {
                continue;
            };
            let mut land_ns: Vec<Axial> = Vec::new();
            for nb in cell.neighbors() {
                let Some(ni) = bounds.index_of(nb) else {
                    continue;
                };
                if is_land_cell(layer, ni) {
                    land_ns.push(nb);
                }
            }
            if land_ns.len() == 2 && opposite_hex_pair(cell, land_ns[0], land_ns[1]) {
                drop.push(index);
            }
        }
        if drop.is_empty() {
            break;
        }
        for index in drop {
            layer.set(
                index,
                DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
            );
        }
    }
}

pub(crate) fn carve_pit(
    bounds: &MapBounds,
    heights: &mut [f64],
    start: usize,
    strength: f64,
    decay: f64,
    seed: u64,
) {
    let n = heights.len();
    let mut used = vec![false; n];
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let mut power = strength;
    heights[start] = (heights[start] - power).max(0.0);
    used[start] = true;
    queue.push_back(start);
    let mut step = 0u64;
    while let Some(index) = queue.pop_front() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        power *= decay;
        if power < 0.03 {
            continue;
        }
        for ncell in cell.neighbors() {
            let Some(ni) = bounds.index_of(ncell) else {
                continue;
            };
            if used[ni] {
                continue;
            }
            used[ni] = true;
            step = step.wrapping_add(1);
            let mod_v = 0.85 + unit01(seed ^ step) * 0.3;
            let cut = power * mod_v;
            heights[ni] = (heights[ni] - cut).max(0.0);
            if cut > 0.03 {
                queue.push_back(ni);
            }
        }
    }
}

pub(crate) fn threshold_for_fraction(heights: &[f64], target: f64) -> f64 {
    if heights.is_empty() {
        return 0.2;
    }
    let mut sorted: Vec<f64> = heights.iter().copied().filter(|h| *h > 0.0).collect();
    if sorted.is_empty() {
        return 1.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let want = (target.clamp(0.05, 0.85) * heights.len() as f64) as usize;
    let land_from_positive = want.min(sorted.len());
    if land_from_positive == 0 {
        return sorted[sorted.len() - 1] + 0.01;
    }
    let idx = sorted.len() - land_from_positive;
    sorted[idx] * 0.999
}

pub(crate) fn apply_shore_fringe(
    bounds: &MapBounds,
    layer: &mut DenseLayer,
    character: ShoreCharacter,
    seed: u64,
) {
    let chance = match character {
        ShoreCharacter::Smooth => 0.04,
        ShoreCharacter::Jagged => 0.11,
    };
    let n = bounds.len();
    let mut flips: Vec<(usize, bool)> = Vec::new();
    for index in 0..n {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let is_land = matches!(
            layer.state(index),
            DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
        );
        let mut land_n = 0;
        let mut ocean_n = 0;
        for nb in cell.neighbors() {
            let Some(ni) = bounds.index_of(nb) else {
                continue;
            };
            if matches!(
                layer.state(ni),
                DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
            ) {
                land_n += 1;
            } else {
                ocean_n += 1;
            }
        }
        let on_shore = land_n > 0 && ocean_n > 0;
        if !on_shore {
            continue;
        }
        let u = unit01(seed ^ (index as u64).wrapping_mul(0x85EB));
        if u > chance {
            continue;
        }
        if is_land && land_n <= 3 {
            flips.push((index, false));
        } else if !is_land && land_n >= 2 {
            flips.push((index, true));
        }
    }
    for (index, to_land) in flips {
        let value = if to_land {
            LAND_MASK_LAND
        } else {
            LAND_MASK_OCEAN
        };
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(value.to_string())),
        );
    }
}

pub(crate) fn remove_tiny_islands(bounds: &MapBounds, layer: &mut DenseLayer, min_cells: usize) {
    let components = land_components(bounds, layer);
    for component in components {
        if component.len() < min_cells {
            flood_ocean(layer, &component);
        }
    }
}

/// Decay so blob radius roughly covers `target` cells (D-66 Large Continents).
fn blob_decay_for_target(target: usize) -> f64 {
    let need_r = ((target as f64 / std::f64::consts::PI).sqrt()).max(4.0);
    // parent≈0.95, stop≈0.02 → decay^r ≈ 0.02/0.95
    (0.02f64 / 0.95).powf(1.0 / need_r).clamp(0.88, 0.975)
}

/// Grow / expand a land mass via organic blob heights (D-66) — not a radial disk.
pub(crate) fn grow_organic_mass(
    bounds: &MapBounds,
    layer: &mut DenseLayer,
    seed_cells: &[usize],
    avoid: &[usize],
    target: usize,
    seed: u64,
) {
    if seed_cells.is_empty() || target <= seed_cells.len() {
        return;
    }
    let n = bounds.len();
    let mut heights = vec![0.0f64; n];
    let avoid_set: std::collections::HashSet<usize> = avoid.iter().copied().collect();
    let (ax, ay) = component_centroid(bounds, avoid);
    let (sx, sy) = component_centroid(bounds, seed_cells);
    let start = seed_cells[seed_cells.len() / 2];

    // Elongation axis roughly away from the main mass (not a perfect circle).
    let mut ex = sx - ax;
    let mut ey = sy - ay;
    let elen = (ex * ex + ey * ey).sqrt().max(1e-6);
    ex /= elen;
    ey /= elen;
    // Twist axis a bit so forms vary.
    let twist = (unit01(seed ^ 0x71) - 0.5) * 0.8;
    let (ex, ey) = (
        ex * twist.cos() - ey * twist.sin(),
        ex * twist.sin() + ey * twist.cos(),
    );

    // Seed existing / starter cells so growth stays connected to them.
    for &i in seed_cells {
        if i < n {
            heights[i] = 1.0;
            if !is_land_cell(layer, i) && !avoid_set.contains(&i) {
                set_land(layer, i);
            }
        }
    }

    grow_blob(
        bounds,
        &mut heights,
        start,
        0.95,
        // Scale decay so height field covers ~target cells (fixed 0.89 stalled on Large).
        blob_decay_for_target(target),
        0.32,
        0.22,
        ex,
        ey,
        seed,
    );

    // Extra seeds along the existing mass edge — irregular coastline.
    let edge_seeds: Vec<usize> = seed_cells
        .iter()
        .copied()
        .filter(|&i| {
            let Some(cell) = bounds.from_index(i) else {
                return false;
            };
            cell.neighbors().iter().any(|nb| {
                bounds
                    .index_of(*nb)
                    .is_some_and(|ni| !is_land_cell(layer, ni) && !avoid_set.contains(&ni))
            })
        })
        .collect();
    let overlay_n = 3.min(edge_seeds.len().max(1));
    for i in 0..overlay_n as u64 {
        let s = mix64(seed ^ (i * 0x9E37) ^ 0x0B1);
        let cur = if edge_seeds.is_empty() {
            start
        } else {
            edge_seeds[(s as usize) % edge_seeds.len()]
        };
        if avoid_set.contains(&cur) {
            continue;
        }
        let (ox, oy) = elongation_axis(s ^ 0xE2, 0.3);
        grow_blob(
            bounds,
            &mut heights,
            cur,
            0.45 + unit01(s) * 0.2,
            0.87,
            0.35,
            0.2,
            ox,
            oy,
            s,
        );
    }

    // Ban cells that touch the avoided mass (keep a channel).
    for (index, h) in heights.iter_mut().enumerate().take(n) {
        if avoid_set.contains(&index) {
            *h = 0.0;
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let touches_avoid = cell.neighbors().iter().any(|nb| {
            bounds
                .index_of(*nb)
                .is_some_and(|ni| avoid_set.contains(&ni))
        });
        if touches_avoid {
            *h = 0.0;
        }
    }

    // Soft elliptical falloff + wobble — avoids flat circular / chord edges.
    // Radius scales with target so Large Continents can actually fill budget.
    let soft_r = ((target as f64 / std::f64::consts::PI).sqrt() * 3.2).max(8.0);
    for (index, h) in heights.iter_mut().enumerate().take(n) {
        if *h <= 0.0 {
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let (x, y) = cell.to_pixel(1.0);
        let dx = x - sx;
        let dy = y - sy;
        let along = dx * ex + dy * ey;
        let across = dx * -ey + dy * ex;
        let d = (along * along + (across * 1.2).powi(2)).sqrt();
        let wobble = 1.0 + 0.2 * (unit01(seed ^ (index as u64).wrapping_mul(0x45) ^ 0xF00D) - 0.5);
        let r = soft_r * wobble;
        if d > r {
            let t = ((d - r) / (r * 0.8)).clamp(0.0, 1.0);
            *h *= 1.0 - t;
        }
    }

    let seed_set: std::collections::HashSet<usize> = seed_cells.iter().copied().collect();
    let mut grown = seed_set.clone();
    let need = target.saturating_sub(seed_cells.len());
    // Prefer cells above a soft floor; do not use map-fraction threshold (stalls when
    // height field is smaller than the map — Large Continents dogfood).
    let h_floor = 0.04;
    let mut painted = 0usize;
    // Paint by descending height; prefer cells with ≥2 land neighbors (compact, fewer fingers).
    let mut order: Vec<(i32, usize)> = heights
        .iter()
        .enumerate()
        .filter(|(i, h)| **h > h_floor && !seed_set.contains(i))
        .map(|(i, h)| ((-*h * 1_000_000.0) as i32, i))
        .collect();
    order.sort_unstable();
    for (_, index) in order {
        if painted >= need {
            break;
        }
        if is_land_cell(layer, index) || avoid_set.contains(&index) {
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        // Keep channel vs avoided mass.
        if cell.neighbors().iter().any(|nb| {
            bounds
                .index_of(*nb)
                .is_some_and(|ni| avoid_set.contains(&ni))
        }) {
            continue;
        }
        let touch_n = cell
            .neighbors()
            .iter()
            .filter(|nb| bounds.index_of(**nb).is_some_and(|ni| grown.contains(&ni)))
            .count();
        // Compact fill: require 2 attachments after the first ring; allow some 1-touch jitter.
        if touch_n == 0 {
            continue;
        }
        if touch_n == 1 && painted > 6 && unit01(seed ^ (index as u64) ^ 0x71) > 0.28 {
            continue;
        }
        set_land(layer, index);
        grown.insert(index);
        painted += 1;
    }
}

/// Peel land from the coast inward until `target` cells remain.
/// Frontier-based — O(n log n), not O(n²) full rescans (Continents dogfood).
pub(crate) fn erode_land_mass(bounds: &MapBounds, layer: &mut DenseLayer, mass: &[usize], target: usize) {
    if mass.len() <= target {
        return;
    }
    let mut alive: std::collections::HashSet<usize> = mass.iter().copied().collect();
    let mut land_n: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::with_capacity(mass.len());
    for &i in mass {
        let Some(cell) = bounds.from_index(i) else {
            continue;
        };
        let mut n = 0usize;
        for nb in cell.neighbors() {
            if bounds.index_of(nb).is_some_and(|ni| alive.contains(&ni)) {
                n += 1;
            }
        }
        land_n.insert(i, n);
    }

    // Min-heap by land-neighbor count (coastal tips first).
    let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(usize, usize)>> =
        std::collections::BinaryHeap::new();
    for (&i, &n) in &land_n {
        if n < 6 {
            heap.push(std::cmp::Reverse((n, i)));
        }
    }

    while alive.len() > target {
        let Some(std::cmp::Reverse((n, drop_i))) = heap.pop() else {
            break;
        };
        if !alive.contains(&drop_i) {
            continue;
        }
        // Stale heap entry after neighbor updates.
        if land_n.get(&drop_i).copied() != Some(n) {
            continue;
        }
        alive.remove(&drop_i);
        land_n.remove(&drop_i);
        layer.set(
            drop_i,
            DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
        );
        let Some(cell) = bounds.from_index(drop_i) else {
            continue;
        };
        for nb in cell.neighbors() {
            let Some(ni) = bounds.index_of(nb) else {
                continue;
            };
            if !alive.contains(&ni) {
                continue;
            }
            let entry = land_n.entry(ni).or_insert(0);
            *entry = entry.saturating_sub(1);
            heap.push(std::cmp::Reverse((*entry, ni)));
        }
    }
}
