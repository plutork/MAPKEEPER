use super::despeckle::{count_land_cells, fill_land_disk, isolated_minor_count};
use super::kind::{geology_kind, is_orogenic_kind};
use super::land_helpers::is_land_cell;
use super::mapping::coast_proximity;
use super::*;
use crate::hex::MapBounds;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::worldgen::land::{
    generate_land_mask, LayoutClass, ShoreCharacter, LAND_MASK_INLAND_SEA, LAND_MASK_LAND,
    LAND_MASK_OCEAN,
};
use crate::worldgen::plates::{build_boundary_distances, build_hidden_plates};

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
    let mask = generate_land_mask(&bounds, LayoutClass::Island, ShoreCharacter::Smooth, 3);
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
    let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 5);
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
    let elev =
        elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 5, ElevationIntensity::Standard);
    assert!(elev.int_or(0, 0) > elev.int_or(1, 0));
    assert!((48..=64).contains(&elev.int_or(0, 0)));
    assert!((8..=14).contains(&elev.int_or(1, 0)));
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

fn mean_coast_of_kind(bounds: &MapBounds, mask: &DenseLayer, geo: &DenseLayer, kind: &str) -> f64 {
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

fn boundary_adjacent_ridge_fraction(
    bounds: &MapBounds,
    _mask: &DenseLayer,
    geo: &DenseLayer,
    seed: u64,
) -> f64 {
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
    let elev = elevation_from_land_mask_and_geology(
        &bounds,
        &mask,
        &geo,
        21,
        ElevationIntensity::Standard,
    );
    let high = (0..bounds.len())
        .filter(|&i| elev.int_or(i, 0) >= 55)
        .count();
    assert!(high > 4, "expected visible highland/ridge elevation cells");
    let ridge_cells = count_kind(&geo, GEOLOGY_RIDGE) + count_kind(&geo, GEOLOGY_VOLCANIC_ARC);
    assert!(ridge_cells > 0);
}
