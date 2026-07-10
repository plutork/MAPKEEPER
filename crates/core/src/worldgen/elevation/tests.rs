use super::*;
use crate::hex::MapBounds;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::worldgen::geology::{
    generate_geology, geology_kind_at, GeologyStyle, GEOLOGY_BASIN, GEOLOGY_LAYER_ID,
    GEOLOGY_RIDGE, GEOLOGY_STABLE, GEOLOGY_VOLCANIC_ARC,
};
use crate::worldgen::land::{generate_land_mask, LayoutClass, ShoreCharacter, LAND_MASK_LAND};

fn median_of_kind(elev: &DenseLayer, geo: &DenseLayer, mask: &DenseLayer, kind: &str) -> f64 {
    let mut vals = Vec::new();
    for i in 0..elev.len() {
        if !matches!(
            mask.state(i),
            DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
        ) {
            continue;
        }
        if geology_kind_at(geo, i) != kind {
            continue;
        }
        vals.push(elev.int_or(i, 0));
    }
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort();
    let mid = vals.len() / 2;
    if vals.len() % 2 == 0 {
        (vals[mid - 1] + vals[mid]) as f64 / 2.0
    } else {
        vals[mid] as f64
    }
}

fn distinct_land_values(elev: &DenseLayer, mask: &DenseLayer) -> usize {
    let mut set = std::collections::BTreeSet::new();
    for i in 0..elev.len() {
        if matches!(
            mask.state(i),
            DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
        ) {
            set.insert(elev.int_or(i, 0));
        }
    }
    set.len()
}

fn land_value_range(elev: &DenseLayer, mask: &DenseLayer) -> i32 {
    let mut min = 100i32;
    let mut max = 0i32;
    for i in 0..elev.len() {
        if matches!(
            mask.state(i),
            DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
        ) {
            let z = elev.int_or(i, 0);
            min = min.min(z);
            max = max.max(z);
        }
    }
    max - min
}

#[test]
fn intensity_parse_aliases() {
    assert_eq!(
        ElevationIntensity::parse("standard"),
        ElevationIntensity::Standard
    );
    assert_eq!(ElevationIntensity::parse("bold"), ElevationIntensity::Bold);
    assert_eq!(
        ElevationIntensity::parse("chaos"),
        ElevationIntensity::Chaos
    );
    assert_eq!(ElevationIntensity::parse(""), ElevationIntensity::Standard);
}

#[test]
fn bridge_is_deterministic_for_seed() {
    let bounds = MapBounds::new(24, 14);
    let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 3);
    let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 9);
    let a = elevation_from_land_mask_and_geology(
        &bounds,
        &mask,
        &geo,
        42,
        ElevationIntensity::Standard,
    );
    let b = elevation_from_land_mask_and_geology(
        &bounds,
        &mask,
        &geo,
        42,
        ElevationIntensity::Standard,
    );
    for i in 0..bounds.len() {
        assert_eq!(a.int_or(i, -1), b.int_or(i, -1));
    }
}

#[test]
fn land_not_flat_plateau_per_geology_class() {
    let bounds = MapBounds::new(64, 36);
    let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 5);
    let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 21);
    let elev =
        elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 7, ElevationIntensity::Standard);
    assert!(
        distinct_land_values(&elev, &mask) > 4,
        "elevation should vary across land"
    );
}

#[test]
fn water_zero_land_at_least_one() {
    let bounds = MapBounds::new(20, 12);
    let mask = generate_land_mask(&bounds, LayoutClass::Island, ShoreCharacter::Smooth, 2);
    let geo = generate_geology(&bounds, &mask, GeologyStyle::Shields, 3);
    for intensity in [
        ElevationIntensity::Standard,
        ElevationIntensity::Bold,
        ElevationIntensity::Chaos,
    ] {
        let elev = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 1, intensity);
        for i in 0..bounds.len() {
            let land = matches!(
                mask.state(i),
                DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
            );
            let z = elev.int_or(i, -1);
            if land {
                assert!(z >= 1, "land cell {i} got {z} ({intensity:?})");
                assert!(z <= 100);
            } else {
                assert_eq!(z, 0, "water cell {i} got {z} ({intensity:?})");
            }
        }
    }
}

#[test]
fn class_median_ordering_preserved() {
    let bounds = MapBounds::new(48, 28);
    let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 4);
    let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
    for intensity in [ElevationIntensity::Standard, ElevationIntensity::Bold] {
        let elev = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 99, intensity);
        let m_basin = median_of_kind(&elev, &geo, &mask, GEOLOGY_BASIN);
        let m_stable = median_of_kind(&elev, &geo, &mask, GEOLOGY_STABLE);
        let m_ridge = median_of_kind(&elev, &geo, &mask, GEOLOGY_RIDGE);
        let m_arc = median_of_kind(&elev, &geo, &mask, GEOLOGY_VOLCANIC_ARC);
        if m_basin > 0.0 && m_stable > 0.0 {
            assert!(m_basin < m_stable, "{intensity:?}");
        }
        if m_stable > 0.0 && m_ridge > 0.0 {
            assert!(m_stable < m_ridge, "{intensity:?}");
        }
        if m_ridge > 0.0 && m_arc > 0.0 {
            assert!(m_ridge < m_arc, "{intensity:?}");
        }
    }
}

#[test]
fn different_nonce_changes_elevation_layout() {
    let bounds = MapBounds::new(48, 28);
    let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 4);
    let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
    let a =
        elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 1, ElevationIntensity::Standard);
    let b =
        elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 2, ElevationIntensity::Standard);
    let mut diff = 0usize;
    for i in 0..bounds.len() {
        if a.int_or(i, -1) != b.int_or(i, -1) {
            diff += 1;
        }
    }
    assert!(
        diff > 10,
        "regenerate nonce should shift elevation (got {diff} differing cells)"
    );
}

#[test]
fn bold_bands_wider_than_standard() {
    for kind in [
        GEOLOGY_BASIN,
        GEOLOGY_STABLE,
        GEOLOGY_RIDGE,
        GEOLOGY_VOLCANIC_ARC,
    ] {
        let (s_lo, s_hi) = crate::worldgen::elevation::bands::elevation_band_for_intensity(
            kind,
            ElevationIntensity::Standard,
        );
        let (b_lo, b_hi) = crate::worldgen::elevation::bands::elevation_band_for_intensity(
            kind,
            ElevationIntensity::Bold,
        );
        assert!(
            (b_hi - b_lo) > (s_hi - s_lo),
            "bold band should be wider for {kind}"
        );
    }
}

#[test]
fn bold_differs_from_standard_at_same_seed() {
    let bounds = MapBounds::new(64, 36);
    let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 5);
    let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 21);
    let standard =
        elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 7, ElevationIntensity::Standard);
    let bold =
        elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 7, ElevationIntensity::Bold);
    let mut diff = 0usize;
    for i in 0..bounds.len() {
        if standard.int_or(i, -1) != bold.int_or(i, -1) {
            diff += 1;
        }
    }
    assert!(diff > 20, "bold should change layout vs standard");
}

#[test]
fn chaos_differs_from_standard() {
    let bounds = MapBounds::new(48, 28);
    let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 4);
    let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
    let standard =
        elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 5, ElevationIntensity::Standard);
    let chaos =
        elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 5, ElevationIntensity::Chaos);
    let mut diff = 0usize;
    for i in 0..bounds.len() {
        if standard.int_or(i, -1) != chaos.int_or(i, -1) {
            diff += 1;
        }
    }
    assert!(diff > 20, "chaos layout should diverge from standard");
    assert!(
        land_value_range(&chaos, &mask) > land_value_range(&standard, &mask) / 2,
        "chaos should use more of the 1..100 range"
    );
}

#[test]
fn ridge_higher_than_basin_on_fixture_cells() {
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
