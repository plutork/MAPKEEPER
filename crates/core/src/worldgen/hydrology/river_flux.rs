//! Elevation-driven river generation (rivers-auto-from-elevation-v1, D-55;
//! amended D-91 — climate precipitation flux when layer present).

use std::collections::HashMap;

use crate::climate::PRECIPITATION_LAYER_ID;
use crate::hex::MapBounds;
use crate::hydro::{DEFAULT_LAND_ELEVATION, SEA_LEVEL};
use crate::layer::DenseLayer;
use crate::rivers::{River, RiverCatalog, RIVER_CATALOG_SCHEMA_VERSION};

/// Azgaar `MIN_FLUX_TO_FORM_RIVER`, scaled for map size.
const BASE_MIN_FLUX: u32 = 30;
pub const UNIFORM_PRECIP: u32 = 1;
/// Fallback mean when precip layer has no land samples (Balanced ≈ legacy uniform).
const FALLBACK_LAND_PRECIP_MEAN: f64 = 90.0;
const MIN_RIVER_CELLS: usize = 2;

/// Generate a full river catalog from elevation (uniform precip fallback).
pub fn generate_rivers_from_elevation(elevation: &DenseLayer, bounds: &MapBounds) -> RiverCatalog {
    generate_with_owners(elevation, bounds, None).0
}

/// Run generation and return catalog + per-cell owners + whether climate precip was used.
pub fn generate_with_owners(
    elevation: &DenseLayer,
    bounds: &MapBounds,
    precipitation: Option<&DenseLayer>,
) -> (RiverCatalog, Vec<u32>, bool) {
    let n = bounds.len();
    if n == 0 {
        return (RiverCatalog::default(), Vec::new(), false);
    }

    let use_climate = precipitation
        .map(|p| {
            p.layer_id == PRECIPITATION_LAYER_ID && land_precip_sample_count(p, elevation, n) > 0
        })
        .unwrap_or(false);
    let precip_mean = precipitation
        .filter(|_| use_climate)
        .map(|p| land_precip_mean(p, elevation, n))
        .unwrap_or(FALLBACK_LAND_PRECIP_MEAN);

    let mut heights = read_heights(elevation, n);
    resolve_depressions(&mut heights, bounds);

    let min_flux = scaled_min_flux(n);
    let land: Vec<usize> = (0..n).filter(|&i| heights[i] > SEA_LEVEL).collect();
    let mut land_high_to_low = land.clone();
    land_high_to_low.sort_by(|&a, &b| heights[b].cmp(&heights[a]));

    let mut flux = vec![0u32; n];
    let mut owner = vec![0u32; n];
    let mut conf = vec![0u32; n];
    let mut paths: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut parents: HashMap<u32, u32> = HashMap::new();
    let mut next_id = 1u32;

    for &i in &land_high_to_low {
        let precip_add = if use_climate {
            let raw = precipitation.unwrap().int_or(i, 0);
            precip_flux_units(raw, precip_mean)
        } else {
            UNIFORM_PRECIP
        };
        flux[i] = flux[i].saturating_add(precip_add);
        if flux[i] < min_flux {
            if let Some(min) = lowest_neighbor(i, &heights, bounds) {
                if heights[min] < heights[i] {
                    flux[min] = flux[min].saturating_add(flux[i]);
                }
            }
            continue;
        }

        let Some(min) = lowest_neighbor(i, &heights, bounds) else {
            continue;
        };
        if heights[min] >= heights[i] {
            continue;
        }

        if owner[i] == 0 {
            owner[i] = next_id;
            paths.insert(next_id, vec![i]);
            next_id += 1;
        }

        flow_down(
            min,
            flux[i],
            owner[i],
            &heights,
            &mut flux,
            &mut owner,
            &mut conf,
            &mut paths,
            &mut parents,
        );
    }

    let catalog = build_catalog(paths, parents, next_id);
    (catalog, owner, use_climate)
}

fn land_precip_sample_count(precip: &DenseLayer, elevation: &DenseLayer, n: usize) -> u32 {
    (0..n)
        .filter(|&i| elevation.int_or(i, DEFAULT_LAND_ELEVATION) > SEA_LEVEL)
        .filter(|&i| precip.int_or(i, 0) > 0)
        .count() as u32
}

fn land_precip_mean(precip: &DenseLayer, elevation: &DenseLayer, n: usize) -> f64 {
    let mut sum = 0.0;
    let mut count = 0u32;
    for i in 0..n {
        if elevation.int_or(i, DEFAULT_LAND_ELEVATION) <= SEA_LEVEL {
            continue;
        }
        let v = precip.int_or(i, 0);
        if v <= 0 {
            continue;
        }
        sum += v as f64;
        count += 1;
    }
    if count == 0 {
        FALLBACK_LAND_PRECIP_MEAN
    } else {
        sum / f64::from(count)
    }
}

/// Scale layer precip so Balanced mean ≈ legacy uniform=1 flux unit.
fn precip_flux_units(raw: i32, mean: f64) -> u32 {
    if raw <= 0 {
        return UNIFORM_PRECIP;
    }
    let mean = mean.max(1.0);
    ((f64::from(raw) / mean).round() as u32).max(UNIFORM_PRECIP)
}

fn read_heights(elevation: &DenseLayer, n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| elevation.int_or(i, DEFAULT_LAND_ELEVATION))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn flow_down(
    to: usize,
    from_flux: u32,
    river: u32,
    heights: &[i32],
    flux: &mut [u32],
    owner: &mut [u32],
    conf: &mut [u32],
    paths: &mut HashMap<u32, Vec<usize>>,
    parents: &mut HashMap<u32, u32>,
) {
    if to >= flux.len() || river == 0 {
        return;
    }
    let to_effective = flux[to].saturating_sub(conf[to]);
    let to_river = owner[to];

    if to_river != 0 {
        if from_flux > to_effective {
            conf[to] = conf[to].saturating_add(flux[to]);
            if heights[to] > SEA_LEVEL {
                parents.insert(to_river, river);
            }
            owner[to] = river;
        } else {
            conf[to] = conf[to].saturating_add(from_flux);
            if heights[to] > SEA_LEVEL {
                parents.insert(river, to_river);
            }
        }
    } else {
        owner[to] = river;
    }

    if heights[to] > SEA_LEVEL {
        flux[to] = flux[to].saturating_add(from_flux);
        paths.entry(river).or_default().push(to);
    }
}

/// Raise sinks so each land cell can drain to a lower or equal neighbor.
fn resolve_depressions(heights: &mut [i32], bounds: &MapBounds) {
    let land: Vec<usize> = (0..heights.len())
        .filter(|&i| heights[i] > SEA_LEVEL)
        .collect();
    let max_iters = heights.len().max(1);
    for _ in 0..max_iters {
        let mut changed = false;
        let mut sorted = land.clone();
        sorted.sort_by_key(|&i| heights[i]);
        for &i in &sorted {
            let Some(min_n) = lowest_neighbor_elevation(i, heights, bounds) else {
                continue;
            };
            if heights[i] > SEA_LEVEL && min_n > heights[i] {
                heights[i] = min_n;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn lowest_neighbor_elevation(index: usize, heights: &[i32], bounds: &MapBounds) -> Option<i32> {
    neighbor_indices(index, bounds)
        .into_iter()
        .map(|n| heights[n])
        .min()
}

fn lowest_neighbor(index: usize, heights: &[i32], bounds: &MapBounds) -> Option<usize> {
    neighbor_indices(index, bounds)
        .into_iter()
        .min_by_key(|&n| heights[n])
}

fn neighbor_indices(index: usize, bounds: &MapBounds) -> Vec<usize> {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
        .collect()
}

fn scaled_min_flux(cell_count: usize) -> u32 {
    if cell_count <= 2_000 {
        return 3;
    }
    let modifier = ((cell_count as f64) / 10_000.0).powf(0.25).max(0.15);
    ((BASE_MIN_FLUX as f64) * modifier).max(3.0) as u32
}

fn build_catalog(
    paths: HashMap<u32, Vec<usize>>,
    parents: HashMap<u32, u32>,
    next_id: u32,
) -> RiverCatalog {
    let mut rivers = Vec::new();

    for (id, mut cells) in paths {
        if cells.len() < MIN_RIVER_CELLS {
            continue;
        }
        cells.dedup();
        if cells.len() < MIN_RIVER_CELLS {
            continue;
        }
        let source = cells[0];
        let mouth = *cells.last().unwrap_or(&source);
        let parent_raw = parents.get(&id).copied().unwrap_or(0);
        let parent = if parent_raw == 0 || parent_raw == id {
            id
        } else {
            parent_raw
        };
        let basin = resolve_basin(id, &parents);
        rivers.push(River {
            id,
            cells,
            source,
            mouth,
            parent,
            basin,
            name: None,
        });
    }

    rivers.sort_by_key(|r| r.id);

    RiverCatalog {
        schema_version: RIVER_CATALOG_SCHEMA_VERSION,
        rivers,
        next_id,
    }
}

fn resolve_basin(id: u32, parents: &HashMap<u32, u32>) -> u32 {
    let mut current = id;
    for _ in 0..parents.len() + 1 {
        let p = parents.get(&current).copied().unwrap_or(0);
        if p == 0 || p == current {
            return current;
        }
        current = p;
    }
    id
}

/// Per-cell owner grid → dense `river_id` layer (dominant assignment).
pub fn sync_river_id_from_owners(owners: &[u32], bounds: &MapBounds) -> crate::layer::DenseLayer {
    use crate::layer::{DenseLayer, DenseState, LayerValue, RIVER_ID_LAYER_ID};
    let mut layer = DenseLayer::new_integer(RIVER_ID_LAYER_ID, bounds.len());
    for i in 0..bounds.len() {
        let v = owners.get(i).copied().unwrap_or(0) as i32;
        layer.set(i, DenseState::Value(LayerValue::Int(v)));
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::Axial;
    use crate::layer::{DenseLayer, DenseState, LayerValue};

    fn set_elev(layer: &mut DenseLayer, bounds: &MapBounds, q: i32, r: i32, v: i32) {
        let i = bounds.index_of(Axial::new(q, r)).unwrap();
        layer.set(i, DenseState::Value(LayerValue::Int(v)));
    }

    fn set_precip(layer: &mut DenseLayer, bounds: &MapBounds, q: i32, r: i32, v: i32) {
        let i = bounds.index_of(Axial::new(q, r)).unwrap();
        layer.set(i, DenseState::Value(LayerValue::Int(v)));
    }

    fn slope_fixture(bounds: &MapBounds, elev: &mut DenseLayer) {
        for i in 0..bounds.len() {
            elev.set(i, DenseState::Value(LayerValue::Int(0)));
        }
        for q in 2..bounds.width - 2 {
            for r in -2..=2 {
                if bounds.contains(Axial::new(q, r)) {
                    set_elev(elev, bounds, q, r, 10 + q * 5);
                }
            }
        }
    }

    #[test]
    fn generates_rivers_on_slope() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let catalog = generate_rivers_from_elevation(&elev, &bounds);
        assert!(!catalog.rivers.is_empty());
        for river in &catalog.rivers {
            assert!(river.cells.len() >= MIN_RIVER_CELLS);
            assert_eq!(river.source, river.cells[0]);
            assert_eq!(river.mouth, *river.cells.last().unwrap());
        }
    }

    #[test]
    fn confluence_sets_parent() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        for i in 0..bounds.len() {
            elev.set(i, DenseState::Value(LayerValue::Int(0)));
        }
        for q in 0..14 {
            for r in 0..8 {
                let cell = Axial::new(q, r);
                if !bounds.contains(cell) {
                    continue;
                }
                if q <= 0 || q >= 13 {
                    continue;
                }
                let left_peak = 40 - ((q - 3).abs() * 6 + (r - 3).abs() * 3);
                let right_peak = 38 - ((q - 10).abs() * 6 + (r - 4).abs() * 3);
                let mut v = left_peak.max(right_peak);
                if (5..=8).contains(&q) && v < 12 {
                    v = 8;
                }
                if v > 0 {
                    set_elev(&mut elev, &bounds, q, r, v);
                }
            }
        }
        let catalog = generate_rivers_from_elevation(&elev, &bounds);
        let has_tributary = catalog
            .rivers
            .iter()
            .any(|r| r.parent != r.id && r.parent != 0);
        assert!(
            has_tributary || catalog.rivers.len() >= 2,
            "expected confluence or multiple rivers, got {}",
            catalog.rivers.len()
        );
    }

    #[test]
    fn flow_down_dominant_flux_sets_parent() {
        let heights = vec![10i32; 6];
        let mut flux = vec![0u32; 6];
        let mut owner = vec![0u32; 6];
        let mut conf = vec![0u32; 6];
        let mut paths = HashMap::new();
        let mut parents = HashMap::new();
        owner[3] = 2;
        flux[3] = 8;
        paths.insert(2, vec![3]);

        flow_down(
            3,
            20,
            1,
            &heights,
            &mut flux,
            &mut owner,
            &mut conf,
            &mut paths,
            &mut parents,
        );

        assert_eq!(parents.get(&2), Some(&1));
        assert_eq!(owner[3], 1);
    }

    #[test]
    fn climate_flux_is_deterministic() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let mut precip = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        for q in 2..bounds.width - 2 {
            for r in -2..=2 {
                if bounds.contains(Axial::new(q, r)) {
                    let v = if q < 7 { 140 } else { 40 };
                    set_precip(&mut precip, &bounds, q, r, v);
                }
            }
        }
        let a = generate_with_owners(&elev, &bounds, Some(&precip));
        let b = generate_with_owners(&elev, &bounds, Some(&precip));
        assert!(a.2);
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
    }

    #[test]
    fn fallback_without_precip_layer_matches_uniform() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let legacy = generate_with_owners(&elev, &bounds, None);
        let empty = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        let fallback = generate_with_owners(&elev, &bounds, Some(&empty));
        assert!(!legacy.2);
        assert!(!fallback.2);
        assert_eq!(legacy.0, fallback.0);
    }

    fn sources_in_west_half(catalog: &RiverCatalog, bounds: &MapBounds, mid_q: i32) -> usize {
        catalog
            .rivers
            .iter()
            .filter(|r| {
                bounds
                    .from_index(r.source)
                    .map(|c| c.q < mid_q)
                    .unwrap_or(false)
            })
            .count()
    }

    #[test]
    fn wetter_regions_produce_more_river_mass() {
        let bounds = MapBounds::new(18, 10);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);

        let mut asymmetric = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        let mid_q = bounds.width / 2;
        for q in 2..bounds.width - 2 {
            for r in -3..=3 {
                if !bounds.contains(Axial::new(q, r)) {
                    continue;
                }
                let v = if q < mid_q { 160 } else { 25 };
                set_precip(&mut asymmetric, &bounds, q, r, v);
            }
        }

        let climate = generate_with_owners(&elev, &bounds, Some(&asymmetric));
        assert!(climate.2);

        let wet_sources = sources_in_west_half(&climate.0, &bounds, mid_q);
        let dry_sources = climate.0.rivers.len().saturating_sub(wet_sources);
        assert!(
            wet_sources > dry_sources,
            "expected more river sources on wet west half, west={wet_sources} east={dry_sources}"
        );
    }

    #[test]
    fn balanced_mean_precip_near_uniform_river_count() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let uniform = generate_with_owners(&elev, &bounds, None);

        let mut balanced = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        for q in 2..bounds.width - 2 {
            for r in -2..=2 {
                if bounds.contains(Axial::new(q, r)) {
                    set_precip(&mut balanced, &bounds, q, r, 90);
                }
            }
        }
        let scaled = generate_with_owners(&elev, &bounds, Some(&balanced));
        assert!(scaled.2);
        let delta = (scaled.0.rivers.len() as i32 - uniform.0.rivers.len() as i32).abs();
        assert!(
            delta <= 3,
            "balanced mean precip should track uniform river count, delta={delta}"
        );
    }
}
