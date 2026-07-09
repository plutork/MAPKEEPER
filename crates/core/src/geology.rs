//! Step 4 world pipeline: intermediate `geology` layer (D-63 / world-pipeline--tectonics-v1).
//!
//! Explanatory categorical layer between accepted `land_mask` and elevation.
//! Does **not** write final elevation values.

use crate::hex::{Axial, MapBounds};
use crate::land_mask::LAND_MASK_LAND;
use crate::layer::{DenseLayer, DenseState, LayerValue};

pub use crate::layer::GEOLOGY_LAYER_ID;

pub const GEOLOGY_NONE: &str = "none";
pub const GEOLOGY_STABLE: &str = "stable";
pub const GEOLOGY_BASIN: &str = "basin";
pub const GEOLOGY_RIDGE: &str = "ridge";
pub const GEOLOGY_RIFT: &str = "rift";
pub const GEOLOGY_VOLCANIC_ARC: &str = "volcanic_arc";

/// Author-facing geology generation styles (2–3 buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeologyStyle {
    /// Interior ridge/rift belts + stable shields.
    Belts,
    /// Broader stable shields, sparse ridges.
    Shields,
    /// Coastal volcanic arcs + short spines.
    Arcs,
}

impl GeologyStyle {
    pub fn parse(raw: &str) -> GeologyStyle {
        match raw.trim().to_ascii_lowercase().as_str() {
            "shields" | "shield" => GeologyStyle::Shields,
            "arcs" | "arc" => GeologyStyle::Arcs,
            _ => GeologyStyle::Belts,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            GeologyStyle::Belts => "belts",
            GeologyStyle::Shields => "shields",
            GeologyStyle::Arcs => "arcs",
        }
    }
}

/// Generate dense categorical `geology` from accepted `land_mask`.
/// Non-land cells are always `none`. Does not write elevation.
pub fn generate_geology(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    style: GeologyStyle,
    seed: u64,
) -> DenseLayer {
    let mut layer = DenseLayer::new_categorical(GEOLOGY_LAYER_ID, bounds.len());
    let (max_x, max_y) = half_extent(bounds);
    for index in 0..bounds.len() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let kind = if !is_land_cell(land_mask, index) {
            GEOLOGY_NONE
        } else {
            let (x, y) = cell.to_pixel(1.0);
            let nx = if max_x > 0.0 { x / max_x } else { 0.0 };
            let ny = if max_y > 0.0 { y / max_y } else { 0.0 };
            let coast = coast_proximity(bounds, land_mask, cell);
            classify_land(style, nx, ny, coast, cell, seed)
        };
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(kind.to_string())),
        );
    }
    layer
}

/// Step 5 bridge: elevation from land_mask + geology (no plate sim).
/// Water → 0; land heights biased by geology class.
pub fn elevation_from_land_mask_and_geology(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    geology: &DenseLayer,
) -> DenseLayer {
    let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
    for index in 0..bounds.len() {
        let z = if !is_land_cell(land_mask, index) {
            0
        } else {
            match geology_kind(geology, index) {
                GEOLOGY_BASIN => 1,
                GEOLOGY_STABLE | GEOLOGY_NONE => 2,
                GEOLOGY_RIFT => 2,
                GEOLOGY_RIDGE | GEOLOGY_VOLCANIC_ARC => 3,
                _ => 2,
            }
        };
        elevation.set(index, DenseState::Value(LayerValue::Int(z)));
    }
    elevation
}

fn classify_land(
    style: GeologyStyle,
    nx: f64,
    ny: f64,
    coast: f64,
    cell: Axial,
    seed: u64,
) -> &'static str {
    let n = hash01(seed ^ 0x6E01, cell.q, cell.r);
    let band = (nx * 1.7 + ny * 0.9 + hash01(seed, cell.q, cell.r) * 0.35).sin();
    match style {
        GeologyStyle::Belts => {
            if band.abs() < 0.18 {
                if n > 0.55 {
                    GEOLOGY_RIFT
                } else {
                    GEOLOGY_RIDGE
                }
            } else if band.abs() < 0.32 && n > 0.7 {
                GEOLOGY_BASIN
            } else if coast < 0.22 && n > 0.82 {
                GEOLOGY_VOLCANIC_ARC
            } else {
                GEOLOGY_STABLE
            }
        }
        GeologyStyle::Shields => {
            if band.abs() < 0.10 && n > 0.6 {
                GEOLOGY_RIDGE
            } else if (nx * nx + ny * ny).sqrt() < 0.35 && n > 0.75 {
                GEOLOGY_BASIN
            } else if coast < 0.18 && n > 0.88 {
                GEOLOGY_VOLCANIC_ARC
            } else {
                GEOLOGY_STABLE
            }
        }
        GeologyStyle::Arcs => {
            if coast < 0.28 {
                if n > 0.45 {
                    GEOLOGY_VOLCANIC_ARC
                } else if n > 0.25 {
                    GEOLOGY_RIDGE
                } else {
                    GEOLOGY_STABLE
                }
            } else if band.abs() < 0.14 {
                GEOLOGY_RIDGE
            } else if n > 0.85 {
                GEOLOGY_BASIN
            } else {
                GEOLOGY_STABLE
            }
        }
    }
}

/// 0 = deep interior, 1 = on coast (land next to non-land).
fn coast_proximity(bounds: &MapBounds, land_mask: &DenseLayer, cell: Axial) -> f64 {
    let mut water_n = 0usize;
    let mut total = 0usize;
    for n in cell.neighbors() {
        let Some(idx) = bounds.index_of(n) else {
            continue;
        };
        total += 1;
        if !is_land_cell(land_mask, idx) {
            water_n += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    water_n as f64 / total as f64
}

fn is_land_cell(land_mask: &DenseLayer, index: usize) -> bool {
    matches!(
        land_mask.state(index),
        DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
    )
}

fn geology_kind(geology: &DenseLayer, index: usize) -> &'static str {
    match geology.state(index) {
        DenseState::Value(LayerValue::Text(ref t)) => match t.as_str() {
            GEOLOGY_STABLE => GEOLOGY_STABLE,
            GEOLOGY_BASIN => GEOLOGY_BASIN,
            GEOLOGY_RIDGE => GEOLOGY_RIDGE,
            GEOLOGY_RIFT => GEOLOGY_RIFT,
            GEOLOGY_VOLCANIC_ARC => GEOLOGY_VOLCANIC_ARC,
            _ => GEOLOGY_NONE,
        },
        _ => GEOLOGY_NONE,
    }
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

fn hash01(seed: u64, q: i32, r: i32) -> f64 {
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
    use crate::land_mask::{
        generate_land_mask, LayoutClass, ShoreCharacter, LAND_MASK_INLAND_SEA, LAND_MASK_OCEAN,
    };

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
    fn ocean_stays_none() {
        let bounds = MapBounds::new(20, 12);
        let mask = generate_land_mask(
            &bounds,
            LayoutClass::Island,
            ShoreCharacter::Smooth,
            3,
        );
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 9);
        for i in 0..bounds.len() {
            if !is_land_cell(&mask, i) {
                assert!(matches!(
                    geo.state(i),
                    DenseState::Value(LayerValue::Text(ref t)) if t == GEOLOGY_NONE
                ));
            }
        }
        assert!(count_kind(&geo, GEOLOGY_NONE) > 0);
    }

    #[test]
    fn land_gets_non_none_pattern() {
        let bounds = MapBounds::new(24, 14);
        let mask = generate_land_mask(
            &bounds,
            LayoutClass::Pangea,
            ShoreCharacter::Smooth,
            5,
        );
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
        let patterned = count_kind(&geo, GEOLOGY_RIDGE)
            + count_kind(&geo, GEOLOGY_RIFT)
            + count_kind(&geo, GEOLOGY_STABLE)
            + count_kind(&geo, GEOLOGY_BASIN)
            + count_kind(&geo, GEOLOGY_VOLCANIC_ARC);
        assert!(patterned > 0, "land should receive geology classes");
    }

    #[test]
    fn elevation_bridge_biases_ridges_higher() {
        let bounds = MapBounds::new(8, 6);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        let mut geo = DenseLayer::new_categorical(GEOLOGY_LAYER_ID, bounds.len());
        for i in 0..bounds.len() {
            mask.set(
                i,
                DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
            );
        }
        geo.set(
            0,
            DenseState::Value(LayerValue::Text(GEOLOGY_RIDGE.to_string())),
        );
        geo.set(
            1,
            DenseState::Value(LayerValue::Text(GEOLOGY_BASIN.to_string())),
        );
        let elev = elevation_from_land_mask_and_geology(&bounds, &mask, &geo);
        assert!(elev.int_or(0, 0) > elev.int_or(1, 0));
    }

    #[test]
    fn styles_parse() {
        assert_eq!(GeologyStyle::parse("belts"), GeologyStyle::Belts);
        assert_eq!(GeologyStyle::parse("shields"), GeologyStyle::Shields);
        assert_eq!(GeologyStyle::parse("arcs"), GeologyStyle::Arcs);
    }

    #[test]
    fn inland_sea_is_none() {
        let bounds = MapBounds::new(4, 3);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        mask.set(
            0,
            DenseState::Value(LayerValue::Text(LAND_MASK_INLAND_SEA.to_string())),
        );
        mask.set(
            1,
            DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
        );
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Arcs, 1);
        assert!(matches!(
            geo.state(0),
            DenseState::Value(LayerValue::Text(ref t)) if t == GEOLOGY_NONE
        ));
        assert!(matches!(
            geo.state(1),
            DenseState::Value(LayerValue::Text(ref t)) if t == GEOLOGY_NONE
        ));
    }
}
