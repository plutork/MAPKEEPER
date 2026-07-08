//! Step 3 world pipeline foundation: land silhouette (`land_mask`) generators.
//!
//! `land_mask` is a categorical dense layer (`ocean` | `land` | `inland_sea`)
//! used as land/water source of truth for the build wizard.

use crate::hex::{Axial, MapBounds};
use crate::layer::{DenseLayer, DenseState, LayerValue};

pub const LAND_MASK_LAYER_ID: &str = "land_mask";
pub const LAND_MASK_OCEAN: &str = "ocean";
pub const LAND_MASK_LAND: &str = "land";
pub const LAND_MASK_INLAND_SEA: &str = "inland_sea";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilhouetteStyle {
    Continent,
    Archipelago,
    Dual,
    Island,
}

impl SilhouetteStyle {
    pub fn parse(raw: &str) -> SilhouetteStyle {
        match raw.trim().to_ascii_lowercase().as_str() {
            "archipelago" => SilhouetteStyle::Archipelago,
            "dual" | "two-landmasses" => SilhouetteStyle::Dual,
            "island" => SilhouetteStyle::Island,
            _ => SilhouetteStyle::Continent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShoreCharacter {
    Smooth,
    Jagged,
}

impl ShoreCharacter {
    pub fn parse(raw: &str) -> ShoreCharacter {
        match raw.trim().to_ascii_lowercase().as_str() {
            "jagged" => ShoreCharacter::Jagged,
            _ => ShoreCharacter::Smooth,
        }
    }
}

/// Generate a silhouette layer for the provided style/character and seed.
pub fn generate_land_mask(
    bounds: &MapBounds,
    style: SilhouetteStyle,
    character: ShoreCharacter,
    seed: u64,
) -> DenseLayer {
    let mut layer = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
    let (max_x, max_y) = half_extent(bounds);
    let roughness = match character {
        ShoreCharacter::Smooth => 0.13,
        ShoreCharacter::Jagged => 0.29,
    };
    for index in 0..bounds.len() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let (x, y) = cell.to_pixel(1.0);
        let nx = if max_x > 0.0 { x / max_x } else { 0.0 };
        let ny = if max_y > 0.0 { y / max_y } else { 0.0 };
        let value = if is_land(style, nx, ny, cell, seed, roughness) {
            LAND_MASK_LAND
        } else {
            LAND_MASK_OCEAN
        };
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(value.to_string())),
        );
    }
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

fn is_land(
    style: SilhouetteStyle,
    nx: f64,
    ny: f64,
    cell: Axial,
    seed: u64,
    roughness: f64,
) -> bool {
    let distance = (nx * nx + ny * ny).sqrt();
    let noise = octave_noise(cell, seed);
    match style {
        SilhouetteStyle::Continent => {
            let threshold = 0.84 + roughness * noise;
            distance < threshold
        }
        SilhouetteStyle::Island => {
            let threshold = 0.57 + roughness * 0.8 * noise;
            distance < threshold
        }
        SilhouetteStyle::Dual => {
            let left = island_metric(nx + 0.52, ny, 0.53 + roughness * 0.6 * noise);
            let right = island_metric(nx - 0.52, ny, 0.53 + roughness * 0.6 * noise);
            left || right
        }
        SilhouetteStyle::Archipelago => {
            let mut islands = false;
            let count = 6;
            let angle_seed = hash_noise(seed ^ 0xA13F, cell.q, cell.r);
            for i in 0..count {
                let ang = ((i as f64) / (count as f64) + angle_seed * 0.05) * std::f64::consts::TAU;
                let radius = 0.15 + 0.62 * ((i as f64 + 1.0) / (count as f64 + 1.0));
                let cx = ang.cos() * radius;
                let cy = ang.sin() * radius * 0.74;
                let local = hash_noise(
                    seed ^ (i as u64 * 0x9E37),
                    cell.q + i as i32,
                    cell.r - i as i32,
                );
                if island_metric(
                    nx - cx,
                    ny - cy,
                    0.24 + roughness * 0.8 * local + (if i % 2 == 0 { 0.06 } else { 0.0 }),
                ) {
                    islands = true;
                    break;
                }
            }
            islands
        }
    }
}

fn island_metric(dx: f64, dy: f64, radius: f64) -> bool {
    (dx * dx + dy * dy).sqrt() < radius
}

fn mark_inland_seas(bounds: &MapBounds, layer: &mut DenseLayer) {
    let mut seen = vec![false; bounds.len()];
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for index in 0..bounds.len() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        if !is_boundary_cell(bounds, cell) || !is_non_land(layer, index) {
            continue;
        }
        seen[index] = true;
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

fn is_non_land(layer: &DenseLayer, index: usize) -> bool {
    !matches!(
        layer.state(index),
        DenseState::Value(LayerValue::Text(kind)) if kind == LAND_MASK_LAND
    )
}

fn is_boundary_cell(bounds: &MapBounds, cell: Axial) -> bool {
    cell.neighbors().iter().any(|n| !bounds.contains(*n))
}

fn half_extent(bounds: &MapBounds) -> (f64, f64) {
    let mut max_x: f64 = 0.0;
    let mut max_y: f64 = 0.0;
    for c in bounds.cells() {
        let (x, y) = c.to_pixel(1.0);
        max_x = max_x.max(x.abs());
        max_y = max_y.max(y.abs());
    }
    (max_x.max(1.0), max_y.max(1.0))
}

fn octave_noise(cell: Axial, seed: u64) -> f64 {
    let n1 = hash_noise(seed ^ 0x9E37_79B9, cell.q, cell.r);
    let n2 = hash_noise(seed ^ 0x85EB_CA6B, cell.q * 2, cell.r * 2);
    let n3 = hash_noise(seed ^ 0xC2B2_AE35, cell.q * 4, cell.r * 4);
    (n1 * 0.58 + n2 * 0.29 + n3 * 0.13).clamp(-1.0, 1.0)
}

fn hash_noise(seed: u64, q: i32, r: i32) -> f64 {
    let mut x = seed
        ^ ((q as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        ^ ((r as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    let unit = (x as f64) / (u64::MAX as f64);
    unit * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_keeps_bounds_length() {
        let bounds = MapBounds::new(14, 8);
        let layer = generate_land_mask(
            &bounds,
            SilhouetteStyle::Continent,
            ShoreCharacter::Smooth,
            42,
        );
        assert_eq!(layer.len(), bounds.len());
        assert_eq!(layer.layer_id, LAND_MASK_LAYER_ID);
    }

    #[test]
    fn land_mask_syncs_to_elevation() {
        let bounds = MapBounds::new(4, 3);
        let mut mask = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
        mask.set(
            0,
            DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
        );
        mask.set(
            1,
            DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
        );
        let elev = elevation_from_land_mask(&bounds, &mask);
        assert_eq!(elev.int_or(0, 0), 1);
        assert_eq!(elev.int_or(1, 1), 0);
    }

    #[test]
    fn normalizes_unknown_kind_to_ocean() {
        assert_eq!(normalize_kind("land"), LAND_MASK_LAND);
        assert_eq!(normalize_kind("inland_sea"), LAND_MASK_INLAND_SEA);
        assert_eq!(normalize_kind("mystery"), LAND_MASK_OCEAN);
    }
}
