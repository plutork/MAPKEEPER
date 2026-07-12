//! Layout-class enforcement after growth (D-66 dogfood).

use crate::hex::MapBounds;
use crate::layer::DenseLayer;

use super::growth::{erode_land_mass, grow_organic_mass};
use super::types::LayoutClass;
use super::util::{
    component_centroid, flood_ocean, half_extent, is_land_cell, land_components, mix64,
    nearest_index, unit01,
};

/// Third pass after growth: enforce class identity (D-66 dogfood).
/// Pangea/Island → one mass; Continents → two roughly equal masses; CAI → main + small sats.
pub(crate) fn enforce_layout_class(bounds: &MapBounds, layer: &mut DenseLayer, class: LayoutClass, seed: u64) {
    let components = land_components(bounds, layer);
    if components.is_empty() {
        return;
    }
    match class {
        LayoutClass::Pangea | LayoutClass::Island | LayoutClass::Mediterranean => {
            for component in components.into_iter().skip(1) {
                flood_ocean(layer, &component);
            }
        }
        LayoutClass::Continents => {
            balance_continents(bounds, layer, seed);
        }
        LayoutClass::ContinentAndIslands => {
            let main_len = components[0].len().max(1);
            let sat_cap = (main_len / 4).max(8);
            for (i, component) in components.into_iter().enumerate() {
                if i == 0 {
                    continue;
                }
                if component.len() > sat_cap {
                    flood_ocean(layer, &component);
                }
            }
        }
        LayoutClass::Archipelago => {
            balance_archipelago(bounds, layer, seed);
        }
    }
}

/// Archipelago: many mid/small islands — not two continent-sized blobs (D-66 dogfood).
fn balance_archipelago(bounds: &MapBounds, layer: &mut DenseLayer, seed: u64) {
    let min_islands = 6;
    let max_island = ((bounds.len() as f64) * 0.07).round() as usize;
    let max_island = max_island.max(24);

    let mut comps = land_components(bounds, layer);
    for component in comps.iter() {
        if component.len() > max_island {
            erode_land_mass(bounds, layer, component, max_island);
        }
    }

    comps = land_components(bounds, layer);
    // Drop dust after erode.
    for component in comps.iter() {
        if component.len() < 3 {
            flood_ocean(layer, component);
        }
    }

    comps = land_components(bounds, layer);
    let mut rng = seed ^ 0xA2C4;
    let mut guard = 0u32;
    while comps.len() < min_islands && guard < 24 {
        guard += 1;
        rng = mix64(rng ^ (guard as u64) ^ 0x151);
        let Some(start) = pick_ocean_seed_away(bounds, layer, rng) else {
            break;
        };
        let target = (max_island / 3).max(10).min(max_island / 2);
        // Avoid all existing land so new islands stay separate.
        let avoid: Vec<usize> = comps.iter().flatten().copied().collect();
        grow_organic_mass(bounds, layer, &[start], &avoid, target, rng ^ 0x151A);
        comps = land_components(bounds, layer);
    }

    // Final dust cleanup after seeding.
    comps = land_components(bounds, layer);
    for component in comps.iter() {
        if component.len() < 3 {
            flood_ocean(layer, component);
        }
    }
}

fn pick_ocean_seed_away(bounds: &MapBounds, layer: &DenseLayer, seed: u64) -> Option<usize> {
    let n = bounds.len();
    // Distance to existing land via component centroids (cheap).
    let comps = land_components(bounds, layer);
    let cents: Vec<(f64, f64)> = comps
        .iter()
        .map(|c| component_centroid(bounds, c))
        .collect();
    let mut best = None;
    let mut best_score = -1.0f64;
    for k in 0..64u64 {
        let idx = (mix64(seed ^ k.wrapping_mul(0x9E37)) as usize) % n.max(1);
        if is_land_cell(layer, idx) {
            continue;
        }
        let Some(cell) = bounds.from_index(idx) else {
            continue;
        };
        let land_n = cell
            .neighbors()
            .iter()
            .filter(|nb| {
                bounds
                    .index_of(**nb)
                    .is_some_and(|ni| is_land_cell(layer, ni))
            })
            .count();
        if land_n > 0 {
            continue;
        }
        let (x, y) = cell.to_pixel(1.0);
        let min_d = cents
            .iter()
            .map(|(cx, cy)| (x - cx).hypot(y - cy))
            .fold(f64::MAX, f64::min);
        if min_d.is_finite() && min_d > best_score {
            best_score = min_d;
            best = Some(idx);
        }
    }
    if best.is_some() {
        return best;
    }
    // Fallback: full scan when random probes miss a valid ocean seed.
    for idx in 0..n {
        if is_land_cell(layer, idx) {
            continue;
        }
        let Some(cell) = bounds.from_index(idx) else {
            continue;
        };
        let land_n = cell
            .neighbors()
            .iter()
            .filter(|nb| {
                bounds
                    .index_of(**nb)
                    .is_some_and(|ni| is_land_cell(layer, ni))
            })
            .count();
        if land_n > 0 {
            continue;
        }
        let (x, y) = cell.to_pixel(1.0);
        let min_d = cents
            .iter()
            .map(|(cx, cy)| (x - cx).hypot(y - cy))
            .fold(f64::MAX, f64::min);
        if min_d.is_finite() && min_d > best_score {
            best_score = min_d;
            best = Some(idx);
        }
    }
    best
}

/// Continents: two compact masses of comparable size (not half-map flood).
pub(crate) fn balance_continents(bounds: &MapBounds, layer: &mut DenseLayer, seed: u64) {
    // Drop 3rd+ fragments first.
    let comps = land_components(bounds, layer);
    for component in comps.into_iter().skip(2) {
        flood_ocean(layer, &component);
    }

    let mut comps = land_components(bounds, layer);
    if comps.is_empty() {
        return;
    }

    // Tiny second mass is noise — remove and regrow compactly.
    if comps.len() >= 2 && comps[1].len() * 100 / comps[0].len().max(1) < 40 {
        flood_ocean(layer, &comps[1]);
        comps = land_components(bounds, layer);
    }

    if comps.len() == 1 {
        grow_second_continent(bounds, layer, &comps[0], seed);
        comps = land_components(bounds, layer);
    }

    if comps.len() < 2 {
        return;
    }

    // Grow the thin second first — never crush the main mass to match a speck
    // (Large dogfood: erode-before-grow left ~1% land / empty Continents).
    if comps.len() >= 2 {
        let big_n = comps[0].len();
        let small_n = comps[1].len();
        if small_n * 100 / big_n.max(1) < 45 {
            let target = (big_n as f64 * 0.65).round() as usize;
            grow_organic_mass(
                bounds,
                layer,
                &comps[1],
                &comps[0],
                target.max(small_n + 4),
                seed ^ 0xC071,
            );
            comps = land_components(bounds, layer);
        }
    }

    // Only then shrink an oversized first — floor keeps map-scale land (D-66).
    if comps.len() >= 2 {
        let big_n = comps[0].len();
        let small_n = comps[1].len();
        if small_n * 100 / big_n.max(1) < 45 {
            let target_big = ((small_n as f64) / 0.55).round() as usize;
            let floor = (small_n + small_n / 2)
                .max(small_n + 8)
                .max((bounds.len() as f64 * 0.12).round() as usize);
            erode_land_mass(bounds, layer, &comps[0], target_big.max(floor));
            comps = land_components(bounds, layer);
        }
    }

    // If total land collapsed below ~28% of map, top up both masses (Large dogfood).
    if comps.len() >= 2 {
        let total = comps[0].len() + comps[1].len();
        let want = ((bounds.len() as f64) * 0.34).round() as usize;
        if total < want {
            let extra = want - total;
            let add_big = (extra as f64 * 0.55).round() as usize;
            let add_small = extra.saturating_sub(add_big);
            grow_organic_mass(
                bounds,
                layer,
                &comps[0],
                &comps[1],
                comps[0].len() + add_big,
                seed ^ 0x70F1,
            );
            comps = land_components(bounds, layer);
            if comps.len() >= 2 {
                grow_organic_mass(
                    bounds,
                    layer,
                    &comps[1],
                    &comps[0],
                    comps[1].len() + add_small,
                    seed ^ 0x70F2,
                );
            }
        }
    }

    // Final: never more than two.
    let comps = land_components(bounds, layer);
    for component in comps.into_iter().skip(2) {
        flood_ocean(layer, &component);
    }
}

fn grow_second_continent(bounds: &MapBounds, layer: &mut DenseLayer, main: &[usize], seed: u64) {
    let (mx, my) = component_centroid(bounds, main);
    let (max_x, max_y) = half_extent(bounds);
    // Seed in the opposite half of the map from the main centroid.
    let target_nx = if mx >= 0.0 { -0.45 } else { 0.45 };
    let target_ny = (unit01(seed ^ 0x71) * 0.6 - 0.3) + if my >= 0.0 { -0.1 } else { 0.1 };
    let mut start = nearest_index(bounds, target_nx, target_ny, max_x, max_y);

    let start_ok = |index: usize| -> bool {
        if is_land_cell(layer, index) {
            return false;
        }
        let Some(cell) = bounds.from_index(index) else {
            return false;
        };
        !cell.neighbors().iter().any(|nb| {
            bounds
                .index_of(*nb)
                .is_some_and(|ni| is_land_cell(layer, ni))
        })
    };

    if !start_ok(start) {
        let mut best = None;
        let mut best_d = -1.0f64;
        for index in 0..bounds.len() {
            if !start_ok(index) {
                continue;
            }
            let Some(cell) = bounds.from_index(index) else {
                continue;
            };
            let (x, y) = cell.to_pixel(1.0);
            let d = (x - mx).hypot(y - my);
            if d > best_d {
                best_d = d;
                best = Some(index);
            }
        }
        let Some(s) = best else {
            return;
        };
        start = s;
    }

    let target = (main.len() as f64 * 0.70).round() as usize;
    // Cap: second continent must not exceed ~35% of the map (prevents half-fill).
    let map_cap = (bounds.len() as f64 * 0.35).round() as usize;
    let target = target.min(map_cap).max(12);
    grow_organic_mass(bounds, layer, &[start], main, target, seed ^ 0x5EC);
}
