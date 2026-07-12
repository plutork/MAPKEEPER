//! Land mask generation orchestration (D-66).

use crate::hex::MapBounds;
use crate::layer::{DenseLayer, DenseState, LayerValue};

use super::catalog::pick_recipe;
use super::enforce::{balance_continents, enforce_layout_class};
use super::growth::{
    apply_shore_fringe, carve_pit, grow_blob, prune_axis_corridors, prune_thin_corridors,
    remove_tiny_islands, threshold_for_fraction,
};
use super::types::{
    LAND_MASK_INLAND_SEA, LAND_MASK_LAND, LAND_MASK_LAYER_ID, LAND_MASK_OCEAN, LayoutClass,
    LayoutRecipe, ShoreCharacter,
};
use super::util::{
    elongation_axis, half_extent, is_boundary_cell, is_non_land, land_components, min_island_cells,
    mix64, pick_seed_cell, unit01,
};

pub fn generate_land_mask(
    bounds: &MapBounds,
    style: LayoutClass,
    character: ShoreCharacter,
    seed: u64,
) -> DenseLayer {
    let recipe = pick_recipe(style, seed);
    generate_land_mask_recipe(bounds, recipe, character, seed)
}

/// D-66 Track A: seeded layered land growth from growth-plan recipe.
pub fn generate_land_mask_recipe(
    bounds: &MapBounds,
    recipe: &LayoutRecipe,
    character: ShoreCharacter,
    seed: u64,
) -> DenseLayer {
    let n = bounds.len();
    let mut heights = vec![0.0f64; n];
    let (max_x, max_y) = half_extent(bounds);
    let mut rng = seed ^ 0xD660_06A1_1C00;

    let shore_scale = match character {
        ShoreCharacter::Smooth => 0.55,
        ShoreCharacter::Jagged => 1.15,
    };
    let sharpness = (recipe.irregularity * shore_scale).clamp(0.05, 1.2);
    let mut decay = match character {
        ShoreCharacter::Smooth => 0.90,
        ShoreCharacter::Jagged => 0.86,
    };
    // Archipelago: keep blobs small so seeds stay separate islands (D-66).
    let archipelago = recipe.layout_class == LayoutClass::Archipelago;
    if archipelago {
        decay *= 0.93;
    }

    let zones = recipe.seed_zones;
    let zone_n = zones.len().max(1);

    for i in 0..recipe.primary_count as usize {
        rng = mix64(rng ^ (i as u64 * 0x9E37) ^ 0x0041);
        let zone = &zones[i % zone_n];
        let start = pick_seed_cell(bounds, zone, rng, max_x, max_y, recipe.merge_bias);
        let h0 = if archipelago {
            0.72 + unit01(rng ^ 0xA1) * 0.12
        } else {
            0.92 + unit01(rng ^ 0xA1) * 0.08
        };
        let (ex, ey) = elongation_axis(rng ^ 0xE1, recipe.elongation);
        grow_blob(
            bounds,
            &mut heights,
            start,
            h0,
            decay,
            sharpness,
            recipe.elongation,
            ex,
            ey,
            rng,
        );
    }

    for i in 0..recipe.satellite_count as usize {
        rng = mix64(rng ^ (i as u64 * 0xC2B2) ^ 0x5A70);
        let zone = &zones[(i + recipe.primary_count as usize) % zone_n];
        let start = pick_seed_cell(bounds, zone, rng, max_x, max_y, recipe.merge_bias);
        let h0 = if archipelago {
            0.28 + unit01(rng ^ 0xB2) * 0.22
        } else {
            0.35 + unit01(rng ^ 0xB2) * 0.35
        };
        let sat_decay = decay * (0.86 + unit01(rng ^ 0xD2) * 0.06);
        let (ex, ey) = elongation_axis(rng ^ 0xE2, recipe.elongation * 0.7);
        grow_blob(
            bounds,
            &mut heights,
            start,
            h0,
            sat_decay,
            sharpness * 0.9,
            recipe.elongation * 0.7,
            ex,
            ey,
            rng,
        );
    }

    if let Some(hole) = recipe.hole {
        rng = mix64(rng ^ 0x401E);
        let start = pick_seed_cell(bounds, &hole, rng, max_x, max_y, 1.0);
        carve_pit(bounds, &mut heights, start, 0.55, 0.88, rng);
    }

    let threshold = threshold_for_fraction(&heights, recipe.land_fraction);
    let mut layer = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, n);
    for (index, &h) in heights.iter().enumerate().take(n) {
        let value = if h > threshold {
            LAND_MASK_LAND
        } else {
            LAND_MASK_OCEAN
        };
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(value.to_string())),
        );
    }

    apply_shore_fringe(bounds, &mut layer, character, seed);
    // D-66 dogfood: kill 1-hex-wide axis strips left by growth.
    prune_thin_corridors(bounds, &mut layer, 4);
    remove_tiny_islands(bounds, &mut layer, min_island_cells(recipe));
    // Third pass: remember layout_class identity (e.g. Pangea = one mass).
    enforce_layout_class(bounds, &mut layer, recipe.layout_class, seed);
    // Second mass grows after tip-prune — cut axis corridors only (tips would erase it).
    prune_axis_corridors(bounds, &mut layer, 8);
    if recipe.layout_class == LayoutClass::Continents {
        let comps = land_components(bounds, &layer);
        let thin = comps.len() < 2
            || (comps.len() >= 2 && comps[1].len() * 100 / comps[0].len().max(1) < 42);
        if thin {
            balance_continents(bounds, &mut layer, seed ^ 0xA11);
            prune_axis_corridors(bounds, &mut layer, 4);
        }
    }
    apply_shore_fringe(bounds, &mut layer, character, seed ^ 0x51DE);
    mark_inland_seas(bounds, &mut layer);
    layer
}

/// Sync elevation from `land_mask`: land = 1, non-land = 0.
pub fn elevation_from_land_mask(bounds: &MapBounds, land_mask: &DenseLayer) -> DenseLayer {
    let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
    for index in 0..bounds.len() {
        let value = match land_mask.state(index) {
            DenseState::Value(LayerValue::Text(kind)) if kind == LAND_MASK_LAND => 1,
            _ => 0,
        };
        elevation.set(index, DenseState::Value(LayerValue::Int(value)));
    }
    elevation
}

pub fn normalize_kind(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        LAND_MASK_LAND => LAND_MASK_LAND,
        LAND_MASK_INLAND_SEA => LAND_MASK_INLAND_SEA,
        _ => LAND_MASK_OCEAN,
    }
}
fn mark_inland_seas(bounds: &MapBounds, layer: &mut DenseLayer) {
    let mut seen = vec![false; bounds.len()];
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for (index, seen_cell) in seen.iter_mut().enumerate().take(bounds.len()) {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        if !is_boundary_cell(bounds, cell) || !is_non_land(layer, index) {
            continue;
        }
        *seen_cell = true;
        queue.push_back(index);
    }
    while let Some(index) = queue.pop_front() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        for n in cell.neighbors() {
            let Some(next) = bounds.index_of(n) else {
                continue;
            };
            if seen[next] || !is_non_land(layer, next) {
                continue;
            }
            seen[next] = true;
            queue.push_back(next);
        }
    }
    for (index, ocean_connected) in seen.into_iter().enumerate() {
        if ocean_connected || !is_non_land(layer, index) {
            continue;
        }
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(LAND_MASK_INLAND_SEA.to_string())),
        );
    }
}
