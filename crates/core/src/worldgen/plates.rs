//! hidden-plates-geology-foundation (D-87): ephemeral plate substrate for step-4 geology.
//! Not persisted; no time simulation.

use crate::hex::{Axial, MapBounds};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryKind {
    Interior,
    Divergent,
    Convergent,
    Transform,
}

/// In-memory plate field for one generate pass (all cells assigned).
#[derive(Debug, Clone)]
pub struct HiddenPlates {
    pub plate_ids: Vec<u16>,
    pub velocities: Vec<(f64, f64)>,
    pub n_plates: u16,
}

pub fn plate_count_for_map(cell_count: usize) -> usize {
    if cell_count < 2_000 {
        4
    } else if cell_count < 8_000 {
        6
    } else if cell_count < 25_000 {
        8
    } else if cell_count < 60_000 {
        12
    } else {
        16
    }
}

/// Build hidden plate ids + velocities for the full hex grid (incl. ocean).
pub fn build_hidden_plates(bounds: &MapBounds, seed: u64) -> HiddenPlates {
    let len = bounds.len();
    let n = plate_count_for_map(len);
    let seeds = pick_plate_seeds(bounds, seed, n);
    let plate_ids = assign_plate_ids(bounds, &seeds);
    let velocities = generate_plate_velocities(seed, n);
    HiddenPlates {
        plate_ids,
        velocities,
        n_plates: n as u16,
    }
}

pub fn generate_plate_velocities(seed: u64, n_plates: usize) -> Vec<(f64, f64)> {
    (0..n_plates)
        .map(|p| {
            let vx = hash01(seed ^ 0x000A_11CE, p as i32, 0) * 2.0 - 1.0;
            let vy = hash01(seed ^ 0x000A_11CE, p as i32, 1) * 2.0 - 1.0;
            (vx, vy)
        })
        .collect()
}

fn pick_plate_seeds(bounds: &MapBounds, seed: u64, n_plates: usize) -> Vec<usize> {
    let len = bounds.len();
    if len == 0 || n_plates == 0 {
        return Vec::new();
    }

    let mut order: Vec<usize> = (0..len).collect();
    order.sort_by(|&a, &b| {
        let ha = hash01(seed, a as i32, -7);
        let hb = hash01(seed, b as i32, -7);
        ha.partial_cmp(&hb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut seeds = vec![order[0]];
    while seeds.len() < n_plates {
        let mut best_idx = None;
        let mut best_score = -1i32;
        for &candidate in &order {
            if seeds.contains(&candidate) {
                continue;
            }
            let Some(cell) = bounds.from_index(candidate) else {
                continue;
            };
            let mut min_d = i32::MAX;
            for &s in &seeds {
                let Some(sc) = bounds.from_index(s) else {
                    continue;
                };
                min_d = min_d.min(cell.distance(sc));
            }
            if min_d > best_score {
                best_score = min_d;
                best_idx = Some(candidate);
            }
        }
        seeds.push(best_idx.unwrap_or_else(|| order[seeds.len() % len]));
    }
    seeds
}

/// Multi-source BFS Voronoi: each cell → nearest seed plate (hex distance).
pub fn assign_plate_ids(bounds: &MapBounds, seeds: &[usize]) -> Vec<u16> {
    let len = bounds.len();
    let mut dist = vec![i32::MAX; len];
    let mut plate_ids = vec![u16::MAX; len];
    let mut queue = std::collections::VecDeque::new();

    for (pid, &seed_idx) in seeds.iter().enumerate() {
        if seed_idx >= len {
            continue;
        }
        dist[seed_idx] = 0;
        plate_ids[seed_idx] = pid as u16;
        queue.push_back(seed_idx);
    }

    while let Some(idx) = queue.pop_front() {
        let Some(cell) = bounds.from_index(idx) else {
            continue;
        };
        let d = dist[idx];
        for nb in cell.neighbors() {
            let Some(ni) = bounds.index_of(nb) else {
                continue;
            };
            let nd = d + 1;
            if nd < dist[ni] {
                dist[ni] = nd;
                plate_ids[ni] = plate_ids[idx];
                queue.push_back(ni);
            } else if nd == dist[ni] && plate_ids[idx] < plate_ids[ni] {
                // deterministic tie-break
                plate_ids[ni] = plate_ids[idx];
            }
        }
    }

    for pid in plate_ids.iter_mut().take(len) {
        if *pid == u16::MAX {
            *pid = 0;
        }
    }
    plate_ids
}

/// Strongest cross-plate boundary touching this cell (full grid neighbors).
pub fn classify_plate_boundary_at(
    bounds: &MapBounds,
    plates: &HiddenPlates,
    cell: Axial,
    index: usize,
) -> (BoundaryKind, f64) {
    let my_plate = plates.plate_ids[index] as usize;
    let (vx0, vy0) = plates.velocities[my_plate.min(plates.velocities.len().saturating_sub(1))];

    let mut best_kind = BoundaryKind::Interior;
    let mut best_strength = 0.0f64;

    for nb in cell.neighbors() {
        let Some(ni) = bounds.index_of(nb) else {
            continue;
        };
        let other = plates.plate_ids[ni] as usize;
        if other == my_plate {
            continue;
        }
        let (vx1, vy1) = plates.velocities[other.min(plates.velocities.len().saturating_sub(1))];
        let (dx, dy) = neighbor_direction(cell, nb);
        let (rvx, rvy) = (vx1 - vx0, vy1 - vy0);
        let dot = rvx * dx + rvy * dy;
        let kind = if dot > 0.15 {
            BoundaryKind::Divergent
        } else if dot < -0.15 {
            BoundaryKind::Convergent
        } else {
            BoundaryKind::Transform
        };
        let strength = dot.abs().clamp(0.0, 1.0);
        if strength > best_strength {
            best_strength = strength;
            best_kind = kind;
        }
    }

    let influence = if best_kind == BoundaryKind::Interior {
        0.0
    } else {
        best_strength.max(0.35)
    };
    (best_kind, influence)
}

/// Hex steps from nearest cross-plate edge (`0` = on edge, `255` = far interior).
pub fn build_boundary_distances(bounds: &MapBounds, plates: &HiddenPlates) -> Vec<u8> {
    let len = bounds.len();
    let mut dist = vec![u8::MAX; len];
    let mut queue = std::collections::VecDeque::new();

    for (index, d) in dist.iter_mut().enumerate().take(len) {
        if is_cross_plate_cell(bounds, plates, index) {
            *d = 0;
            queue.push_back(index);
        }
    }

    while let Some(index) = queue.pop_front() {
        let d = dist[index];
        if d >= 4 {
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        for nb in cell.neighbors() {
            let Some(ni) = bounds.index_of(nb) else {
                continue;
            };
            let nd = d.saturating_add(1);
            if dist[ni] > nd {
                dist[ni] = nd;
                queue.push_back(ni);
            }
        }
    }
    dist
}

fn is_cross_plate_cell(bounds: &MapBounds, plates: &HiddenPlates, index: usize) -> bool {
    let my_plate = plates.plate_ids[index];
    let Some(cell) = bounds.from_index(index) else {
        return false;
    };
    cell.neighbors().into_iter().any(|nb| {
        bounds
            .index_of(nb)
            .is_some_and(|ni| plates.plate_ids[ni] != my_plate)
    })
}

fn neighbor_direction(from: Axial, to: Axial) -> (f64, f64) {
    let (x0, y0) = from.to_pixel(1.0);
    let (x1, y1) = to.to_pixel(1.0);
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    (dx / len, dy / len)
}

pub fn hash01(seed: u64, q: i32, r: i32) -> f64 {
    let mut x = seed
        ^ ((q as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        ^ ((r as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    (x as f64) / (u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::MapBounds;

    #[test]
    fn all_cells_assigned_to_plates() {
        let bounds = MapBounds::new(40, 24);
        let plates = build_hidden_plates(&bounds, 99);
        assert_eq!(plates.plate_ids.len(), bounds.len());
        for &pid in &plates.plate_ids {
            assert!(pid < plates.n_plates);
        }
    }

    #[test]
    fn plate_assignment_is_deterministic() {
        let bounds = MapBounds::new(30, 18);
        let a = build_hidden_plates(&bounds, 7);
        let b = build_hidden_plates(&bounds, 7);
        assert_eq!(a.plate_ids, b.plate_ids);
        assert_eq!(a.velocities, b.velocities);
    }

    #[test]
    fn plate_count_scales_with_map_size() {
        assert!(plate_count_for_map(1_500) < plate_count_for_map(30_000));
        assert!(plate_count_for_map(30_000) <= 16);
    }
}
