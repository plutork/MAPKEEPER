//! Step 4 world pipeline: intermediate `geology` layer (D-63 / D-87 hidden plates).
//!
//! Explanatory categorical layer between accepted `land_mask` and elevation.
//! Does **not** write final elevation values.

use crate::hex::{Axial, MapBounds};
use crate::land_mask::LAND_MASK_LAND;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::plates::{
    build_boundary_distances, build_hidden_plates, classify_plate_boundary_at, hash01,
    BoundaryKind,
};

pub use crate::layer::GEOLOGY_LAYER_ID;

pub const GEOLOGY_NONE: &str = "none";
pub const GEOLOGY_STABLE: &str = "stable";
pub const GEOLOGY_BASIN: &str = "basin";
pub const GEOLOGY_RIDGE: &str = "ridge";
pub const GEOLOGY_RIFT: &str = "rift";
pub const GEOLOGY_VOLCANIC_ARC: &str = "volcanic_arc";

/// Author-facing geology generation styles (wizard buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeologyStyle {
    /// Orogenic chains along convergent/transform boundaries.
    Belts,
    /// Stable interiors; restrained boundary mountains.
    Shields,
    /// Volcanic arcs at coast-adjacent convergent boundaries.
    Arcs,
    /// Tectonically constrained but varied placement.
    Random,
}

impl GeologyStyle {
    pub fn parse(raw: &str) -> GeologyStyle {
        match raw.trim().to_ascii_lowercase().as_str() {
            "shields" | "shield" => GeologyStyle::Shields,
            "arcs" | "arc" => GeologyStyle::Arcs,
            "random" => GeologyStyle::Random,
            _ => GeologyStyle::Belts,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            GeologyStyle::Belts => "belts",
            GeologyStyle::Shields => "shields",
            GeologyStyle::Arcs => "arcs",
            GeologyStyle::Random => "random",
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
    let plates = build_hidden_plates(bounds, seed);
    let boundary_dist = build_boundary_distances(bounds, &plates);
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
            let (boundary, influence) =
                classify_plate_boundary_at(bounds, &plates, cell, index);
            map_hidden_tectonics_to_geology_style(
                style,
                boundary,
                influence,
                boundary_dist[index],
                nx,
                ny,
                coast,
                cell,
                seed,
            )
        };
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(kind.to_string())),
        );
    }
    despeckle_isolated_minors(bounds, &mut layer);
    layer
}

/// Map hidden plate boundary signal + author style → geology category.
pub fn map_hidden_tectonics_to_geology_style(
    style: GeologyStyle,
    boundary: BoundaryKind,
    influence: f64,
    boundary_dist: u8,
    nx: f64,
    ny: f64,
    coast: f64,
    cell: Axial,
    seed: u64,
) -> &'static str {
    let n = hash01(seed ^ 0x6E01, cell.q, cell.r);

    match style {
        GeologyStyle::Belts => {
            if should_place_orogenic(style, boundary, influence, boundary_dist, cell, seed) {
                orogenic_class_for_boundary(boundary, n, coast, seed, cell)
            } else if coast > 0.30 && n > 0.62 {
                GEOLOGY_VOLCANIC_ARC
            } else if boundary_dist > 3 && (nx * nx + ny * ny).sqrt() < 0.28 && n > 0.82 {
                GEOLOGY_BASIN
            } else {
                GEOLOGY_STABLE
            }
        }
        GeologyStyle::Shields => {
            if should_place_orogenic(style, boundary, influence, boundary_dist, cell, seed) {
                orogenic_class_for_boundary(boundary, n, coast, seed, cell)
            } else if boundary_dist > 3 && (nx * nx + ny * ny).sqrt() < 0.30 && n > 0.75 {
                GEOLOGY_BASIN
            } else if coast > 0.40 && n > 0.78 {
                GEOLOGY_VOLCANIC_ARC
            } else {
                GEOLOGY_STABLE
            }
        }
        GeologyStyle::Arcs => {
            if coast > 0.12 && n > 0.38 {
                if n > 0.58 {
                    GEOLOGY_VOLCANIC_ARC
                } else {
                    GEOLOGY_RIDGE
                }
            } else if should_place_orogenic(style, boundary, influence, boundary_dist, cell, seed) {
                orogenic_class_for_boundary(boundary, n, coast, seed, cell)
            } else if boundary_dist > 3 && n > 0.90 {
                GEOLOGY_BASIN
            } else {
                GEOLOGY_STABLE
            }
        }
        GeologyStyle::Random => {
            if should_place_orogenic(style, boundary, influence, boundary_dist, cell, seed) {
                orogenic_class_for_boundary(boundary, n, coast, seed, cell)
            } else if n > 0.88 {
                GEOLOGY_BASIN
            } else {
                GEOLOGY_STABLE
            }
        }
    }
}

fn should_place_orogenic(
    style: GeologyStyle,
    boundary: BoundaryKind,
    influence: f64,
    boundary_dist: u8,
    cell: Axial,
    seed: u64,
) -> bool {
    if boundary == BoundaryKind::Interior || boundary_dist > 3 {
        return false;
    }
    let roll = hash01(seed ^ 0x0A0E_6E, cell.q, cell.r);
    let gap = hash01(seed ^ 0x6A70_5, cell.q, cell.r);
    if boundary_dist == 0 && gap < 0.26 {
        return false;
    }

    let mut chance = match boundary_dist {
        0 => 0.50 + influence * 0.36,
        1 => 0.26 + influence * 0.30,
        2 => 0.11 + influence * 0.20,
        3 => 0.05,
        _ => 0.0,
    };
    if boundary_dist <= 1 && hash01(seed ^ 0x8B1D_E6, cell.q, cell.r) > 0.86 {
        chance = chance.max(0.82);
    }
    chance *= match style {
        GeologyStyle::Belts => 1.0,
        GeologyStyle::Shields => 0.52,
        GeologyStyle::Arcs => 0.72,
        GeologyStyle::Random => 1.08,
    };
    roll < chance.clamp(0.0, 0.92)
}

fn orogenic_class_for_boundary(
    boundary: BoundaryKind,
    n: f64,
    coast: f64,
    seed: u64,
    cell: Axial,
) -> &'static str {
    let pick = hash01(seed ^ 0xA4D_0, cell.q, cell.r);
    match boundary {
        BoundaryKind::Divergent => {
            if pick > 0.42 {
                GEOLOGY_RIFT
            } else {
                GEOLOGY_RIDGE
            }
        }
        BoundaryKind::Convergent => {
            if coast > 0.22 && n > 0.55 {
                GEOLOGY_VOLCANIC_ARC
            } else {
                GEOLOGY_RIDGE
            }
        }
        BoundaryKind::Transform => GEOLOGY_RIDGE,
        BoundaryKind::Interior => GEOLOGY_STABLE,
    }
}

/// Step 5 bridge: elevation from land_mask + geology (no plate sim).
/// Water → 0; land heights biased by geology class (D-72 readable contrast).
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
                GEOLOGY_BASIN => 10,
                GEOLOGY_RIFT => 18,
                GEOLOGY_STABLE | GEOLOGY_NONE => 30,
                GEOLOGY_RIDGE => 55,
                GEOLOGY_VOLCANIC_ARC => 72,
                _ => 30,
            }
        };
        elevation.set(index, DenseState::Value(LayerValue::Int(z)));
    }
    elevation
}

fn despeckle_isolated_minors(bounds: &MapBounds, layer: &mut DenseLayer) {
    let mut demote = Vec::new();
    for index in 0..bounds.len() {
        let kind = geology_kind(layer, index);
        if !is_minor_geology(kind) {
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let mut same = 0usize;
        for n in cell.neighbors() {
            let Some(ni) = bounds.index_of(n) else {
                continue;
            };
            if geology_kind(layer, ni) == kind {
                same += 1;
            }
        }
        if same == 0 {
            demote.push(index);
        }
    }
    for index in demote {
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(GEOLOGY_STABLE.to_string())),
        );
    }
}

fn is_minor_geology(kind: &str) -> bool {
    matches!(
        kind,
        GEOLOGY_BASIN | GEOLOGY_RIDGE | GEOLOGY_RIFT | GEOLOGY_VOLCANIC_ARC
    )
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::land_mask::{
        generate_land_mask, LayoutClass, ShoreCharacter, LAND_MASK_INLAND_SEA, LAND_MASK_LAND,
        LAND_MASK_OCEAN,
    };
    use crate::plates::{build_boundary_distances, build_hidden_plates};

    fn count_land_cells(land_mask: &DenseLayer) -> usize {
        (0..land_mask.len())
            .filter(|&i| is_land_cell(land_mask, i))
            .count()
    }

    fn valid_geology_kind(kind: &str) -> bool {
        matches!(
            kind,
            GEOLOGY_NONE
                | GEOLOGY_STABLE
                | GEOLOGY_BASIN
                | GEOLOGY_RIDGE
                | GEOLOGY_RIFT
                | GEOLOGY_VOLCANIC_ARC
        )
    }

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
        assert_eq!(elev.int_or(0, 0), 55);
        assert_eq!(elev.int_or(1, 0), 10);
    }

    #[test]
    fn styles_parse() {
        assert_eq!(GeologyStyle::parse("belts"), GeologyStyle::Belts);
        assert_eq!(GeologyStyle::parse("shields"), GeologyStyle::Shields);
        assert_eq!(GeologyStyle::parse("arcs"), GeologyStyle::Arcs);
        assert_eq!(GeologyStyle::parse("random"), GeologyStyle::Random);
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

    fn fill_land_disk(bounds: &MapBounds, mask: &mut DenseLayer, radius: i32) {
        let center = bounds
            .from_index(bounds.len() / 2)
            .unwrap_or(Axial::new(0, 0));
        for i in 0..bounds.len() {
            let Some(c) = bounds.from_index(i) else {
                continue;
            };
            let land = c.distance(center) <= radius;
            let v = if land {
                LAND_MASK_LAND
            } else {
                LAND_MASK_OCEAN
            };
            mask.set(i, DenseState::Value(LayerValue::Text(v.to_string())));
        }
    }

    fn mean_coast_of_kind(
        bounds: &MapBounds,
        mask: &DenseLayer,
        geo: &DenseLayer,
        kind: &str,
    ) -> f64 {
        let mut sum = 0.0;
        let mut n = 0usize;
        for i in 0..bounds.len() {
            if geology_kind(geo, i) != kind {
                continue;
            }
            let Some(cell) = bounds.from_index(i) else {
                continue;
            };
            sum += coast_proximity(bounds, mask, cell);
            n += 1;
        }
        if n == 0 {
            0.0
        } else {
            sum / n as f64
        }
    }

    fn isolated_minor_count(bounds: &MapBounds, geo: &DenseLayer) -> usize {
        let mut n = 0usize;
        for i in 0..bounds.len() {
            let kind = geology_kind(geo, i);
            if !is_minor_geology(kind) {
                continue;
            }
            let Some(cell) = bounds.from_index(i) else {
                continue;
            };
            let same = cell
                .neighbors()
                .into_iter()
                .filter(|nb| {
                    bounds
                        .index_of(*nb)
                        .is_some_and(|ni| geology_kind(geo, ni) == kind)
                })
                .count();
            if same == 0 {
                n += 1;
            }
        }
        n
    }

    #[test]
    fn arcs_volcanic_prefer_coast_over_interior() {
        let bounds = MapBounds::new(40, 24);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        fill_land_disk(&bounds, &mut mask, 10);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Arcs, 42);
        let arcs = count_kind(&geo, GEOLOGY_VOLCANIC_ARC);
        assert!(arcs > 0, "Arcs style should place volcanic_arc");
        let mean_arc = mean_coast_of_kind(&bounds, &mask, &geo, GEOLOGY_VOLCANIC_ARC);
        let mean_stable = mean_coast_of_kind(&bounds, &mask, &geo, GEOLOGY_STABLE);
        assert!(
            mean_arc > mean_stable,
            "volcanic_arc mean coast {mean_arc} should exceed stable {mean_stable}"
        );
        assert!(
            mean_arc > 0.25,
            "volcanic_arc should sit near coast, got mean {mean_arc}"
        );
    }

    #[test]
    fn generated_geology_has_few_isolated_minors() {
        let bounds = MapBounds::new(48, 28);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        fill_land_disk(&bounds, &mut mask, 12);
        for style in [
            GeologyStyle::Belts,
            GeologyStyle::Shields,
            GeologyStyle::Arcs,
            GeologyStyle::Random,
        ] {
            let geo = generate_geology(&bounds, &mask, style, 7);
            let isolated = isolated_minor_count(&bounds, &geo);
            assert_eq!(
                isolated, 0,
                "{:?} left {isolated} isolated minor cells",
                style
            );
        }
    }

    #[test]
    fn all_styles_produce_valid_categories() {
        let bounds = MapBounds::new(32, 20);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        fill_land_disk(&bounds, &mut mask, 8);
        for style in [
            GeologyStyle::Belts,
            GeologyStyle::Shields,
            GeologyStyle::Arcs,
            GeologyStyle::Random,
        ] {
            let geo = generate_geology(&bounds, &mask, style, 13);
            for i in 0..bounds.len() {
                let kind = geology_kind(&geo, i);
                assert!(valid_geology_kind(kind), "{style:?} invalid {kind}");
            }
        }
    }

    #[test]
    fn geology_is_deterministic_for_seed() {
        let bounds = MapBounds::new(24, 14);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        fill_land_disk(&bounds, &mut mask, 6);
        let a = generate_geology(&bounds, &mask, GeologyStyle::Belts, 55);
        let b = generate_geology(&bounds, &mask, GeologyStyle::Belts, 55);
        for i in 0..bounds.len() {
            assert_eq!(geology_kind(&a, i), geology_kind(&b, i));
        }
    }

    fn orogenic_mean_pixel(bounds: &MapBounds, geo: &DenseLayer) -> (f64, f64) {
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut n = 0usize;
        for i in 0..bounds.len() {
            let kind = geology_kind(geo, i);
            if kind != GEOLOGY_RIDGE && kind != GEOLOGY_RIFT {
                continue;
            }
            let Some(cell) = bounds.from_index(i) else {
                continue;
            };
            let (x, y) = cell.to_pixel(1.0);
            sx += x;
            sy += y;
            n += 1;
        }
        if n == 0 {
            (0.0, 0.0)
        } else {
            (sx / n as f64, sy / n as f64)
        }
    }

    #[test]
    fn belts_seed_changes_layout_not_only_speckle() {
        let bounds = MapBounds::new(48, 28);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        fill_land_disk(&bounds, &mut mask, 12);
        let a = generate_geology(&bounds, &mask, GeologyStyle::Belts, 1);
        let b = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
        let (ax, ay) = orogenic_mean_pixel(&bounds, &a);
        let (bx, by) = orogenic_mean_pixel(&bounds, &b);
        assert!(count_kind(&a, GEOLOGY_RIDGE) + count_kind(&a, GEOLOGY_RIFT) > 0);
        assert!(count_kind(&b, GEOLOGY_RIDGE) + count_kind(&b, GEOLOGY_RIFT) > 0);
        let dist = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
        assert!(
            dist > 1.0,
            "orogenic centroid should shift between seeds (got {dist})"
        );
    }

    fn ridge_q_span(bounds: &MapBounds, geo: &DenseLayer) -> i32 {
        let mut min_q = i32::MAX;
        let mut max_q = i32::MIN;
        for i in 0..bounds.len() {
            if geology_kind(geo, i) != GEOLOGY_RIDGE {
                continue;
            }
            let Some(cell) = bounds.from_index(i) else {
                continue;
            };
            min_q = min_q.min(cell.q);
            max_q = max_q.max(cell.q);
        }
        if min_q == i32::MAX {
            0
        } else {
            max_q - min_q
        }
    }

    #[test]
    fn large_land_gets_boundary_driven_ridge_spread() {
        let bounds_small = MapBounds::new(24, 14);
        let mut mask_small = DenseLayer::new_categorical("land_mask", bounds_small.len());
        fill_land_disk(&bounds_small, &mut mask_small, 4);

        let bounds_large = MapBounds::new(80, 45);
        let mut mask_large = DenseLayer::new_categorical("land_mask", bounds_large.len());
        fill_land_disk(&bounds_large, &mut mask_large, 28);

        let land_small = count_land_cells(&mask_small);
        let land_large = count_land_cells(&mask_large);
        assert!(land_large > land_small * 8);

        let geo_small = generate_geology(&bounds_small, &mask_small, GeologyStyle::Belts, 7);
        let geo_large = generate_geology(&bounds_large, &mask_large, GeologyStyle::Belts, 7);
        let span_small = ridge_q_span(&bounds_small, &geo_small);
        let span_large = ridge_q_span(&bounds_large, &geo_large);
        assert!(count_kind(&geo_large, GEOLOGY_RIDGE) > count_kind(&geo_small, GEOLOGY_RIDGE));
        assert!(
            span_large > span_small.saturating_mul(2),
            "large land ridge span {span_large} should exceed small {span_small}"
        );
    }

    fn boundary_adjacent_ridge_fraction(bounds: &MapBounds, _mask: &DenseLayer, geo: &DenseLayer, seed: u64) -> f64 {
        let plates = build_hidden_plates(bounds, seed);
        let boundary_dist = build_boundary_distances(bounds, &plates);
        let mut ridges = 0usize;
        let mut near_boundary = 0usize;
        for i in 0..bounds.len() {
            if geology_kind(geo, i) != GEOLOGY_RIDGE {
                continue;
            }
            ridges += 1;
            if boundary_dist[i] <= 2 {
                near_boundary += 1;
            }
        }
        if ridges == 0 {
            0.0
        } else {
            near_boundary as f64 / ridges as f64
        }
    }

    fn is_orogenic_kind(kind: &str) -> bool {
        matches!(
            kind,
            GEOLOGY_RIDGE | GEOLOGY_RIFT | GEOLOGY_VOLCANIC_ARC
        )
    }

    #[test]
    fn belts_orogenic_chains_are_irregular_not_uniform_corridors() {
        // failure_class: recipe_not_distinct — boundary lines need gaps + width variety
        let bounds = MapBounds::new(64, 36);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        fill_land_disk(&bounds, &mut mask, 22);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 21);
        let plates = build_hidden_plates(&bounds, 21);
        let boundary_dist = build_boundary_distances(&bounds, &plates);

        let mut edge_land = 0usize;
        let mut edge_gap = 0usize;
        let mut ridge_on_edge = false;
        let mut ridge_off_edge = false;
        for i in 0..bounds.len() {
            if !is_land_cell(&mask, i) {
                continue;
            }
            if boundary_dist[i] == 0 {
                edge_land += 1;
                if geology_kind(&geo, i) == GEOLOGY_STABLE {
                    edge_gap += 1;
                }
            }
            let kind = geology_kind(&geo, i);
            if !is_orogenic_kind(kind) {
                continue;
            }
            if boundary_dist[i] == 0 {
                ridge_on_edge = true;
            } else if boundary_dist[i] <= 2 {
                ridge_off_edge = true;
            }
        }
        assert!(edge_land > 0);
        let gap_frac = edge_gap as f64 / edge_land as f64;
        assert!(
            gap_frac > 0.10,
            "plate-edge land should include stable gaps, got {gap_frac}"
        );
        assert!(
            ridge_on_edge && ridge_off_edge,
            "orogenic cells should vary between edge and near-edge width"
        );
    }

    #[test]
    fn medium_large_ridges_follow_plate_boundaries() {
        let bounds = MapBounds::new(64, 36);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        fill_land_disk(&bounds, &mut mask, 22);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 21);
        let frac = boundary_adjacent_ridge_fraction(&bounds, &mask, &geo, 21);
        assert!(
            frac > 0.45,
            "ridges should mostly sit near plate boundaries, got {frac}"
        );
    }

    #[test]
    fn elevation_after_hidden_plates_has_highland_chains() {
        let bounds = MapBounds::new(48, 28);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        fill_land_disk(&bounds, &mut mask, 12);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 21);
        let elev = elevation_from_land_mask_and_geology(&bounds, &mask, &geo);
        let high = (0..bounds.len())
            .filter(|&i| elev.int_or(i, 0) >= 55)
            .count();
        assert!(high > 4, "expected visible highland/ridge elevation cells");
        let ridge_cells = count_kind(&geo, GEOLOGY_RIDGE) + count_kind(&geo, GEOLOGY_VOLCANIC_ARC);
        assert!(ridge_cells > 0);
    }
}
