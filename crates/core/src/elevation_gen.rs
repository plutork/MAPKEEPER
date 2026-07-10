//! Step 5 elevation bridge (D-72 / D-88): geology → continuous-ish integer relief.

use crate::geology::{
    geology_kind_at, GEOLOGY_BASIN, GEOLOGY_NONE, GEOLOGY_RIFT, GEOLOGY_RIDGE, GEOLOGY_STABLE,
    GEOLOGY_VOLCANIC_ARC,
};
use crate::hex::MapBounds;
use crate::land_mask::LAND_MASK_LAND;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::plates::hash01;

const SAME_CLASS_WEIGHT: f64 = 1.0;
const CROSS_CLASS_WEIGHT: f64 = 0.35;
const SELF_WEIGHT: f64 = 2.0;

/// Center/base Z per geology class (D-72 readability anchors).
pub fn base_elevation_for_geology(kind: &str) -> i32 {
    match kind {
        GEOLOGY_BASIN => 11,
        GEOLOGY_RIFT => 19,
        GEOLOGY_STABLE | GEOLOGY_NONE => 30,
        GEOLOGY_RIDGE => 56,
        GEOLOGY_VOLCANIC_ARC => 72,
        _ => 30,
    }
}

/// Inclusive jitter band per geology class (D-88).
pub fn elevation_band_for_geology(kind: &str) -> (i32, i32) {
    match kind {
        GEOLOGY_BASIN => (8, 14),
        GEOLOGY_RIFT => (14, 24),
        GEOLOGY_STABLE | GEOLOGY_NONE => (26, 34),
        GEOLOGY_RIDGE => (48, 64),
        GEOLOGY_VOLCANIC_ARC => (62, 82),
        _ => (26, 34),
    }
}

/// Deterministic jitter Z inside the class band.
pub fn deterministic_cell_jitter(kind: &str, cell_q: i32, cell_r: i32, seed: u64) -> i32 {
    let (lo, hi) = elevation_band_for_geology(kind);
    if lo >= hi {
        return lo;
    }
    let t = hash01(seed ^ 0xE1E8_01, cell_q, cell_r);
    let span = (hi - lo) as f64;
    lo + (t * span).round() as i32
}

fn is_land_cell(land_mask: &DenseLayer, index: usize) -> bool {
    matches!(
        land_mask.state(index),
        DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
    )
}

fn clamp_to_band(z: i32, kind: &str) -> i32 {
    let (lo, hi) = elevation_band_for_geology(kind);
    z.clamp(lo, hi).clamp(1, 100)
}

/// Clamp land elevation to geology band; water forced to 0.
pub fn clamp_elevation_by_geology(
    z: i32,
    kind: &str,
    land: bool,
) -> i32 {
    if !land {
        return 0;
    }
    clamp_to_band(z, kind)
}

/// One light hex smooth over land; same-geology neighbors weigh more.
pub fn smooth_elevation_once(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    geology: &DenseLayer,
    heights: &mut [i32],
) {
    let len = heights.len();
    let mut next = heights.to_vec();
    for index in 0..len {
        if !is_land_cell(land_mask, index) {
            next[index] = 0;
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let my_kind = geology_kind_at(geology, index);
        let mut sum = heights[index] as f64 * SELF_WEIGHT;
        let mut weight = SELF_WEIGHT;
        for nb in cell.neighbors() {
            let Some(ni) = bounds.index_of(nb) else {
                continue;
            };
            if !is_land_cell(land_mask, ni) {
                continue;
            }
            let w = if geology_kind_at(geology, ni) == my_kind {
                SAME_CLASS_WEIGHT
            } else {
                CROSS_CLASS_WEIGHT
            };
            sum += heights[ni] as f64 * w;
            weight += w;
        }
        let smoothed = (sum / weight).round() as i32;
        next[index] = clamp_to_band(smoothed, my_kind);
    }
    heights.copy_from_slice(&next);
}

/// Step 5 bridge: land_mask + geology + seed → dense integer elevation.
pub fn elevation_from_land_mask_and_geology(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    geology: &DenseLayer,
    seed: u64,
) -> DenseLayer {
    let len = bounds.len();
    let mut heights = vec![0i32; len];

    for index in 0..len {
        if !is_land_cell(land_mask, index) {
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let kind = geology_kind_at(geology, index);
        let z = deterministic_cell_jitter(kind, cell.q, cell.r, seed);
        heights[index] = clamp_to_band(z, kind);
    }

    smooth_elevation_once(bounds, land_mask, geology, &mut heights);

    let mut elevation = DenseLayer::new_integer("elevation", len);
    for index in 0..len {
        let land = is_land_cell(land_mask, index);
        let kind = if land {
            geology_kind_at(geology, index)
        } else {
            GEOLOGY_NONE
        };
        let z = clamp_elevation_by_geology(heights[index], kind, land);
        elevation.set(index, DenseState::Value(LayerValue::Int(z)));
    }
    elevation
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geology::{
        generate_geology, GeologyStyle, GEOLOGY_LAYER_ID,
    };
    use crate::land_mask::{generate_land_mask, LayoutClass, ShoreCharacter, LAND_MASK_LAND};

    fn median_of_kind(
        elev: &DenseLayer,
        geo: &DenseLayer,
        mask: &DenseLayer,
        kind: &str,
    ) -> f64 {
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

    #[test]
    fn bridge_is_deterministic_for_seed() {
        let bounds = MapBounds::new(24, 14);
        let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 3);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 9);
        let a = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 42);
        let b = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 42);
        for i in 0..bounds.len() {
            assert_eq!(a.int_or(i, -1), b.int_or(i, -1));
        }
    }

    #[test]
    fn land_not_flat_plateau_per_geology_class() {
        let bounds = MapBounds::new(64, 36);
        let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 5);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 21);
        let elev = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 7);
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
        let elev = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 1);
        for i in 0..bounds.len() {
            let land = matches!(
                mask.state(i),
                DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
            );
            let z = elev.int_or(i, -1);
            if land {
                assert!(z >= 1, "land cell {i} got {z}");
                assert!(z <= 100);
            } else {
                assert_eq!(z, 0, "water cell {i} got {z}");
            }
        }
    }

    #[test]
    fn class_median_ordering_preserved() {
        let bounds = MapBounds::new(48, 28);
        let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 4);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
        let elev = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 99);
        let m_basin = median_of_kind(&elev, &geo, &mask, GEOLOGY_BASIN);
        let m_stable = median_of_kind(&elev, &geo, &mask, GEOLOGY_STABLE);
        let m_ridge = median_of_kind(&elev, &geo, &mask, GEOLOGY_RIDGE);
        let m_arc = median_of_kind(&elev, &geo, &mask, GEOLOGY_VOLCANIC_ARC);
        if m_basin > 0.0 && m_stable > 0.0 {
            assert!(m_basin < m_stable);
        }
        if m_stable > 0.0 && m_ridge > 0.0 {
            assert!(m_stable < m_ridge);
        }
        if m_ridge > 0.0 && m_arc > 0.0 {
            assert!(m_ridge < m_arc);
        }
    }

    #[test]
    fn different_nonce_changes_elevation_layout() {
        let bounds = MapBounds::new(48, 28);
        let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 4);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
        let a = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 1);
        let b = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 2);
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
        let elev = elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 5);
        assert!(elev.int_or(0, 0) > elev.int_or(1, 0));
        assert!((48..=64).contains(&elev.int_or(0, 0)));
        assert!((8..=14).contains(&elev.int_or(1, 0)));
    }
}
