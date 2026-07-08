//! Step 3 world pipeline: land silhouette (`land_mask`) generators.
//!
//! `land_mask` is a categorical dense layer (`ocean` | `land` | `inland_sea`)
//! used as land/water source of truth for the build wizard.
//!
//! Layout classes (D-62 / `step3-geo-variant-classes-v1`): macro land/water
//! arrangement only — not elevation or geology.

use crate::hex::{Axial, MapBounds};
use crate::layer::{DenseLayer, DenseState, LayerValue};

pub const LAND_MASK_LAYER_ID: &str = "land_mask";
pub const LAND_MASK_OCEAN: &str = "ocean";
pub const LAND_MASK_LAND: &str = "land";
pub const LAND_MASK_INLAND_SEA: &str = "inland_sea";

/// Macro silhouette layout (D-62). Shore character is orthogonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutClass {
    Pangea,
    Continents,
    Archipelago,
    Island,
    ContinentAndIslands,
    Mediterranean,
}

impl LayoutClass {
    pub fn parse(raw: &str) -> LayoutClass {
        match raw.trim().to_ascii_lowercase().as_str() {
            "continents" | "dual" | "two-landmasses" => LayoutClass::Continents,
            "archipelago" => LayoutClass::Archipelago,
            "island" => LayoutClass::Island,
            "continent_and_islands" | "continent-and-islands" => LayoutClass::ContinentAndIslands,
            "mediterranean" => LayoutClass::Mediterranean,
            // legacy "continent" + default
            _ => LayoutClass::Pangea,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            LayoutClass::Pangea => "pangea",
            LayoutClass::Continents => "continents",
            LayoutClass::Archipelago => "archipelago",
            LayoutClass::Island => "island",
            LayoutClass::ContinentAndIslands => "continent_and_islands",
            LayoutClass::Mediterranean => "mediterranean",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LayoutClass::Pangea => "Pangea",
            LayoutClass::Continents => "Continents",
            LayoutClass::Archipelago => "Archipelago",
            LayoutClass::Island => "Island",
            LayoutClass::ContinentAndIslands => "Continent + islands",
            LayoutClass::Mediterranean => "Mediterranean",
        }
    }
}

/// A/B/C compare sets: three different layout classes (not seeds of one style).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutCompareSet {
    /// A=pangea · B=continents · C=archipelago
    Macro,
    /// A=island · B=continent_and_islands · C=mediterranean
    Coastal,
}

impl LayoutCompareSet {
    pub fn parse(raw: &str) -> LayoutCompareSet {
        match raw.trim().to_ascii_lowercase().as_str() {
            "coastal" => LayoutCompareSet::Coastal,
            _ => LayoutCompareSet::Macro,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            LayoutCompareSet::Macro => "macro",
            LayoutCompareSet::Coastal => "coastal",
        }
    }

    pub fn class_for_variant(self, variant: char) -> LayoutClass {
        let v = variant.to_ascii_uppercase();
        match (self, v) {
            (LayoutCompareSet::Macro, 'B') => LayoutClass::Continents,
            (LayoutCompareSet::Macro, 'C') => LayoutClass::Archipelago,
            (LayoutCompareSet::Macro, _) => LayoutClass::Pangea,
            (LayoutCompareSet::Coastal, 'B') => LayoutClass::ContinentAndIslands,
            (LayoutCompareSet::Coastal, 'C') => LayoutClass::Mediterranean,
            (LayoutCompareSet::Coastal, _) => LayoutClass::Island,
        }
    }
}

/// Backward-compatible alias used by older call sites.
pub type SilhouetteStyle = LayoutClass;

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

/// Generate a silhouette layer for the provided layout class / shore / seed.
pub fn generate_land_mask(
    bounds: &MapBounds,
    style: LayoutClass,
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
    style: LayoutClass,
    nx: f64,
    ny: f64,
    cell: Axial,
    seed: u64,
    roughness: f64,
) -> bool {
    let noise = octave_noise(cell, seed);
    match style {
        LayoutClass::Pangea => {
            let distance = (nx * nx + ny * ny).sqrt();
            let threshold = 0.84 + roughness * noise;
            distance < threshold
        }
        LayoutClass::Island => {
            let distance = (nx * nx + ny * ny).sqrt();
            let threshold = 0.57 + roughness * 0.8 * noise;
            distance < threshold
        }
        LayoutClass::Continents => {
            let left = island_metric(nx + 0.52, ny, 0.53 + roughness * 0.6 * noise);
            let right = island_metric(nx - 0.52, ny, 0.53 + roughness * 0.6 * noise);
            left || right
        }
        LayoutClass::Archipelago => archipelago_islands(nx, ny, cell, seed, roughness, 6),
        LayoutClass::ContinentAndIslands => {
            // Readable main mass + a few mid-size satellites (not 1-hex dust).
            let main = island_metric(nx + 0.10, ny * 0.95, 0.56 + roughness * 0.45 * noise);
            let sat1 = island_metric(nx - 0.78, ny + 0.28, 0.20 + roughness * 0.28 * noise);
            let sat2 = island_metric(nx - 0.70, ny - 0.48, 0.17 + roughness * 0.25 * noise);
            let sat3 = island_metric(nx + 0.78, ny - 0.22, 0.15 + roughness * 0.22 * noise);
            main || sat1 || sat2 || sat3
        }
        LayoutClass::Mediterranean => {
            // Land ring / basin: outer land, enclosed water → inland_sea after mark.
            let distance = (nx * nx + ny * ny * 1.15).sqrt();
            let outer = 0.88 + roughness * 0.55 * noise;
            let inner = 0.38 + roughness * 0.35 * noise;
            distance < outer && distance > inner
        }
    }
}

fn archipelago_islands(
    nx: f64,
    ny: f64,
    cell: Axial,
    seed: u64,
    roughness: f64,
    count: usize,
) -> bool {
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
            return true;
        }
    }
    false
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

    fn count_kind(layer: &DenseLayer, kind: &str) -> usize {
        (0..layer.len())
            .filter(|&i| {
                matches!(
                    layer.state(i),
                    DenseState::Value(LayerValue::Text(ref t)) if t == kind
                )
            })
            .count()
    }

    #[test]
    fn generate_keeps_bounds_length() {
        let bounds = MapBounds::new(14, 8);
        let layer = generate_land_mask(
            &bounds,
            LayoutClass::Pangea,
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

    #[test]
    fn parse_layout_aliases() {
        assert_eq!(LayoutClass::parse("continent"), LayoutClass::Pangea);
        assert_eq!(LayoutClass::parse("pangea"), LayoutClass::Pangea);
        assert_eq!(LayoutClass::parse("dual"), LayoutClass::Continents);
        assert_eq!(
            LayoutClass::parse("continent_and_islands"),
            LayoutClass::ContinentAndIslands
        );
    }

    #[test]
    fn compare_set_maps_distinct_classes() {
        let macro_set = LayoutCompareSet::Macro;
        assert_ne!(
            macro_set.class_for_variant('A'),
            macro_set.class_for_variant('B')
        );
        assert_ne!(
            macro_set.class_for_variant('B'),
            macro_set.class_for_variant('C')
        );
        let coastal = LayoutCompareSet::Coastal;
        assert_eq!(coastal.class_for_variant('A'), LayoutClass::Island);
        assert_eq!(
            coastal.class_for_variant('B'),
            LayoutClass::ContinentAndIslands
        );
        assert_eq!(coastal.class_for_variant('C'), LayoutClass::Mediterranean);
    }

    #[test]
    fn all_layout_classes_produce_land() {
        let bounds = MapBounds::new(24, 14);
        for class in [
            LayoutClass::Pangea,
            LayoutClass::Continents,
            LayoutClass::Archipelago,
            LayoutClass::Island,
            LayoutClass::ContinentAndIslands,
            LayoutClass::Mediterranean,
        ] {
            let layer = generate_land_mask(&bounds, class, ShoreCharacter::Smooth, 7);
            assert!(
                count_kind(&layer, LAND_MASK_LAND) > 0,
                "{} should produce land",
                class.id()
            );
        }
    }

    #[test]
    fn mediterranean_marks_inland_sea() {
        let bounds = MapBounds::new(28, 16);
        let layer = generate_land_mask(
            &bounds,
            LayoutClass::Mediterranean,
            ShoreCharacter::Smooth,
            11,
        );
        assert!(
            count_kind(&layer, LAND_MASK_INLAND_SEA) > 0,
            "mediterranean should enclose inland_sea"
        );
    }

    #[test]
    fn continent_and_islands_has_separated_land() {
        let bounds = MapBounds::new(36, 20);
        let layer = generate_land_mask(
            &bounds,
            LayoutClass::ContinentAndIslands,
            ShoreCharacter::Smooth,
            19,
        );
        let land = count_kind(&layer, LAND_MASK_LAND);
        let ocean = count_kind(&layer, LAND_MASK_OCEAN) + count_kind(&layer, LAND_MASK_INLAND_SEA);
        assert!(land > 40, "main continent should be substantial, got {land}");
        assert!(ocean > 20, "ocean should remain between masses");
        // At least one land cell near the left edge (satellite zone).
        let mut left_land = false;
        for index in 0..bounds.len() {
            let Some(cell) = bounds.from_index(index) else {
                continue;
            };
            let (x, _) = cell.to_pixel(1.0);
            let (max_x, _) = half_extent(&bounds);
            if x / max_x > -0.55 {
                continue;
            }
            if matches!(
                layer.state(index),
                DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
            ) {
                left_land = true;
                break;
            }
        }
        assert!(left_land, "expected satellite land on the far side");
    }
}
