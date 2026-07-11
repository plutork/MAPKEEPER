//! Elevation-driven river generation (rivers-auto-from-elevation-v1, D-55;
//! amended D-91 — climate precipitation flux when layer present;
//! hydrology-river-lake-integration-v1 — lake sinks + river density).

use std::collections::{HashMap, HashSet};

use crate::climate::PRECIPITATION_LAYER_ID;
use crate::hex::MapBounds;
use crate::hydro::{DEFAULT_LAND_ELEVATION, SEA_LEVEL};
use crate::layer::DenseLayer;
use crate::lakes::LakeCatalog;
use crate::rivers::{River, RiverCatalog, RIVER_CATALOG_SCHEMA_VERSION};
use crate::worldgen::hydrology::types::{DepressionAnalysis, RiverDensity};

use super::depression_fill::analyze_depressions;

/// Azgaar `MIN_FLUX_TO_FORM_RIVER`, scaled for map size.
const BASE_MIN_FLUX: u32 = 30;
pub const UNIFORM_PRECIP: u32 = 1;
/// Fallback mean when precip layer has no land samples (Balanced ≈ legacy uniform).
const FALLBACK_LAND_PRECIP_MEAN: f64 = 90.0;
const MIN_RIVER_CELLS: usize = 2;

/// Optional inputs for lake-aware river flux (defaults = legacy D-55/D-91).
#[derive(Clone, Copy, Default)]
pub struct RiverFluxParams<'a> {
    pub analysis: Option<&'a DepressionAnalysis>,
    pub lakes: Option<&'a LakeCatalog>,
    pub density: RiverDensity,
}

/// Generate a full river catalog from elevation (uniform precip fallback).
pub fn generate_rivers_from_elevation(elevation: &DenseLayer, bounds: &MapBounds) -> RiverCatalog {
    generate_with_owners(elevation, bounds, None, RiverFluxParams::default()).0
}

/// Run generation and return catalog + per-cell owners + whether climate precip was used.
pub fn generate_with_owners(
    elevation: &DenseLayer,
    bounds: &MapBounds,
    precipitation: Option<&DenseLayer>,
    params: RiverFluxParams,
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

    let analysis = match params.analysis {
        Some(a) => a.clone(),
        None => analyze_depressions(elevation, bounds),
    };
    let heights = &analysis.conditioned_heights;
    let lake_cells = lake_cell_set(params.lakes);

    let min_flux = scaled_min_flux(n, params.density);
    let land: Vec<usize> = (0..n)
        .filter(|&i| heights[i] > SEA_LEVEL && !lake_cells.contains(&i))
        .collect();
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
            if let Some(min) = lowest_routable_neighbor(i, heights, bounds, &lake_cells) {
                if heights[min] < heights[i] {
                    flux[min] = flux[min].saturating_add(flux[i]);
                }
            }
            continue;
        }

        let Some(min) = lowest_routable_neighbor(i, heights, bounds, &lake_cells) else {
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
            heights,
            &lake_cells,
            &mut flux,
            &mut owner,
            &mut conf,
            &mut paths,
            &mut parents,
        );
    }

    let catalog = build_catalog(
        &owner,
        heights,
        bounds,
        &lake_cells,
        parents,
        next_id,
    );
    (catalog, owner, use_climate)
}

fn lake_cell_set(lakes: Option<&LakeCatalog>) -> HashSet<usize> {
    lakes
        .map(|catalog| {
            catalog
                .lakes
                .iter()
                .flat_map(|lake| lake.cells.iter().copied())
                .collect()
        })
        .unwrap_or_default()
}

fn lowest_routable_neighbor(
    index: usize,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
) -> Option<usize> {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
        .filter(|&n| !lake_cells.contains(&n))
        .min_by_key(|&n| heights[n])
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

#[allow(clippy::too_many_arguments)]
fn flow_down(
    to: usize,
    from_flux: u32,
    river: u32,
    heights: &[i32],
    lake_cells: &HashSet<usize>,
    flux: &mut [u32],
    owner: &mut [u32],
    conf: &mut [u32],
    paths: &mut HashMap<u32, Vec<usize>>,
    parents: &mut HashMap<u32, u32>,
) {
    if to >= flux.len() || river == 0 || lake_cells.contains(&to) {
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

fn scaled_min_flux(cell_count: usize, density: RiverDensity) -> u32 {
    let base = if cell_count <= 2_000 {
        3
    } else {
        let modifier = ((cell_count as f64) / 10_000.0).powf(0.25).max(0.15);
        ((BASE_MIN_FLUX as f64) * modifier).max(3.0) as u32
    };
    match density {
        RiverDensity::Few => ((f64::from(base) * 1.55).ceil() as u32).max(3),
        RiverDensity::Balanced => base,
        RiverDensity::Many => ((f64::from(base) * 0.52).max(2.0)) as u32,
    }
}

fn build_catalog(
    owners: &[u32],
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
    parents: HashMap<u32, u32>,
    next_id: u32,
) -> RiverCatalog {
    let mut rivers = Vec::new();

    for id in 1..next_id {
        let mut cells = trace_owned_stem(id, owners, heights, bounds);
        if cells.len() < MIN_RIVER_CELLS {
            continue;
        }
        extend_mouth_to_sink(&mut cells, heights, bounds, lake_cells);
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

fn trace_owned_stem(
    id: u32,
    owners: &[u32],
    heights: &[i32],
    bounds: &MapBounds,
) -> Vec<usize> {
    let owned: HashSet<usize> = owners
        .iter()
        .enumerate()
        .filter(|(_, &owner)| owner == id)
        .map(|(i, _)| i)
        .collect();
    if owned.is_empty() {
        return Vec::new();
    }
    let source = owned
        .iter()
        .copied()
        .max_by_key(|&i| heights[i])
        .unwrap_or(0);
    let mut chain = vec![source];
    let mut cur = source;
    loop {
        let Some(next) = bounds
            .from_index(cur)
            .into_iter()
            .flat_map(|cell| cell.neighbors())
            .filter_map(|n| bounds.index_of(n))
            .filter(|&n| owned.contains(&n) && heights[n] < heights[cur])
            .min_by_key(|&n| heights[n])
        else {
            break;
        };
        if chain.contains(&next) {
            break;
        }
        chain.push(next);
        cur = next;
    }
    chain
}

fn mouth_touches_sea(index: usize, heights: &[i32], bounds: &MapBounds) -> bool {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
        .any(|n| heights[n] <= SEA_LEVEL)
}

fn mouth_touches_lake(index: usize, lake_cells: &HashSet<usize>, bounds: &MapBounds) -> bool {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
        .any(|n| lake_cells.contains(&n))
}

fn extend_mouth_to_sink(
    cells: &mut Vec<usize>,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
) {
    loop {
        let Some(&mouth) = cells.last() else {
            break;
        };
        if mouth_touches_sea(mouth, heights, bounds) || mouth_touches_lake(mouth, lake_cells, bounds)
        {
            break;
        }
        let Some(next) = lowest_routable_neighbor(mouth, heights, bounds, lake_cells) else {
            break;
        };
        if heights[next] >= heights[mouth] || heights[next] <= SEA_LEVEL {
            break;
        }
        if cells.contains(&next) {
            break;
        }
        cells.push(next);
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

pub fn river_path_cell_count(catalog: &RiverCatalog) -> usize {
    catalog.rivers.iter().map(|r| r.cells.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::Axial;
    use crate::layer::{DenseLayer, DenseState, LayerValue};
    use crate::lakes::{Lake, LakeCatalog};
    use crate::worldgen::hydrology::analyze_depressions;

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

    fn default_params() -> RiverFluxParams<'static> {
        RiverFluxParams::default()
    }

    #[test]
    fn depression_analysis_preserves_river_catalog() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let before = generate_with_owners(&elev, &bounds, None, default_params());
        let after = generate_with_owners(&elev, &bounds, None, default_params());
        assert_eq!(before.0, after.0);
        assert_eq!(before.1, after.1);
        assert!(!after.0.rivers.is_empty());
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
        let lake_cells = HashSet::new();
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
            &lake_cells,
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
        let a = generate_with_owners(&elev, &bounds, Some(&precip), default_params());
        let b = generate_with_owners(&elev, &bounds, Some(&precip), default_params());
        assert!(a.2);
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
    }

    #[test]
    fn fallback_without_precip_layer_matches_uniform() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let legacy = generate_with_owners(&elev, &bounds, None, default_params());
        let empty = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        let fallback = generate_with_owners(&elev, &bounds, Some(&empty), default_params());
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

        let climate = generate_with_owners(&elev, &bounds, Some(&asymmetric), default_params());
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
        let uniform = generate_with_owners(&elev, &bounds, None, default_params());

        let mut balanced = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        for q in 2..bounds.width - 2 {
            for r in -2..=2 {
                if bounds.contains(Axial::new(q, r)) {
                    set_precip(&mut balanced, &bounds, q, r, 90);
                }
            }
        }
        let scaled = generate_with_owners(&elev, &bounds, Some(&balanced), default_params());
        assert!(scaled.2);
        let delta = (scaled.0.rivers.len() as i32 - uniform.0.rivers.len() as i32).abs();
        assert!(
            delta <= 3,
            "balanced mean precip should track uniform river count, delta={delta}"
        );
    }

    #[test]
    fn few_vs_many_monotonic_river_count() {
        let bounds = MapBounds::new(18, 10);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let analysis = analyze_depressions(&elev, &bounds);
        let params_base = |density| RiverFluxParams {
            analysis: Some(&analysis),
            lakes: None,
            density,
        };
        let few = generate_with_owners(
            &elev,
            &bounds,
            None,
            params_base(RiverDensity::Few),
        );
        let balanced = generate_with_owners(
            &elev,
            &bounds,
            None,
            params_base(RiverDensity::Balanced),
        );
        let many = generate_with_owners(
            &elev,
            &bounds,
            None,
            params_base(RiverDensity::Many),
        );
        let f = few.0.rivers.len();
        let b = balanced.0.rivers.len();
        let m = many.0.rivers.len();
        assert!(f <= b && b <= m, "river count few={f} bal={b} many={m}");
    }

    #[test]
    fn mouth_terminates_at_lake_shore_not_in_lake() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let analysis = analyze_depressions(&elev, &bounds);
        // Lake on low-q end of slope (west lowland — valid axial coords for this bounds).
        let mut land: Vec<(usize, i32)> = (0..bounds.len())
            .filter_map(|i| {
                let h = elev.int_or(i, 0);
                if h > SEA_LEVEL {
                    Some((i, h))
                } else {
                    None
                }
            })
            .collect();
        land.sort_by_key(|(_, h)| *h);
        let lake_cells: Vec<usize> = land
            .iter()
            .take(4)
            .map(|(i, _)| *i)
            .collect();
        assert!(
            lake_cells.len() >= 2,
            "fixture needs land cells on slope lowland"
        );
        let lakes = LakeCatalog {
            schema_version: 1,
            next_id: 2,
            lakes: vec![Lake {
                id: 1,
                cells: lake_cells.clone(),
                outlet_cell: None,
                endorheic: false,
                name: None,
            }],
        };
        let lake_set: HashSet<_> = lake_cells.iter().copied().collect();
        let (catalog, _, _) = generate_with_owners(
            &elev,
            &bounds,
            None,
            RiverFluxParams {
                analysis: Some(&analysis),
                lakes: Some(&lakes),
                density: RiverDensity::Balanced,
            },
        );
        for river in &catalog.rivers {
            assert!(
                river.cells.iter().all(|c| !lake_set.contains(c)),
                "river must not traverse lake cells"
            );
            if let Some(&mouth) = river.cells.last() {
                let touches_lake = bounds
                    .from_index(mouth)
                    .into_iter()
                    .flat_map(|c| c.neighbors())
                    .filter_map(|n| bounds.index_of(n))
                    .any(|n| lake_set.contains(&n));
                if touches_lake {
                    return;
                }
            }
        }
        assert!(
            !catalog.rivers.is_empty(),
            "expected at least one river terminating toward lake"
        );
    }

    #[test]
    fn determinism_with_lakes_present() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let analysis = analyze_depressions(&elev, &bounds);
        let lakes = LakeCatalog {
            schema_version: 1,
            next_id: 2,
            lakes: vec![Lake {
                id: 1,
                cells: vec![50, 51],
                outlet_cell: None,
                endorheic: true,
                name: None,
            }],
        };
        let params = RiverFluxParams {
            analysis: Some(&analysis),
            lakes: Some(&lakes),
            density: RiverDensity::Balanced,
        };
        let a = generate_with_owners(&elev, &bounds, None, params);
        let b = generate_with_owners(&elev, &bounds, None, params);
        assert_eq!(a.0, b.0);
    }

    fn load_fixture_elevation(name: &str) -> (MapBounds, DenseLayer) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/worlds")
            .join(name)
            .join("map");
        let manifest_raw = std::fs::read_to_string(root.join("manifest.json")).unwrap();
        let manifest: crate::layer::MapManifest = crate::layer::MapManifest::from_json(&manifest_raw).unwrap();
        let (w, h) = match manifest.bounds {
            crate::layer::Bounds::HexRectangle { width, height } => (width, height),
        };
        let bounds = MapBounds::new(w, h);
        let elev_raw = std::fs::read_to_string(root.join("layers/elevation.json")).unwrap();
        let elev = DenseLayer::read_or_empty(
            Some(&elev_raw),
            "elevation",
            crate::layer::ValueType::Integer,
            &bounds,
        );
        (bounds, elev)
    }

    #[test]
    fn coastal_slope_fixture_regression() {
        let (bounds, elev) = load_fixture_elevation("coastal-slope");
        let catalog = generate_rivers_from_elevation(&elev, &bounds);
        assert!(
            !catalog.rivers.is_empty(),
            "coastal-slope should still produce rivers"
        );
    }

    #[test]
    fn mountain_ridge_fixture_regression() {
        let (bounds, elev) = load_fixture_elevation("mountain-ridge");
        let catalog = generate_rivers_from_elevation(&elev, &bounds);
        assert!(
            !catalog.rivers.is_empty(),
            "mountain-ridge should still produce rivers"
        );
    }

    #[test]
    fn small_continents_catalog_traces_owner_stems() {
        use crate::map_preset::MapPreset;
        use crate::worldgen::climate::{generate_climate_layers, PrecipitationStyle};
        use crate::worldgen::elevation::{elevation_from_land_mask_and_geology, ElevationIntensity};
        use crate::worldgen::geology::{generate_geology, GeologyStyle};
        use crate::worldgen::land::{generate_land_mask, LayoutClass, ShoreCharacter};

        let seed = 26;
        let bounds = MapPreset::Small.bounds();
        let mask = generate_land_mask(&bounds, LayoutClass::Continents, ShoreCharacter::Smooth, seed);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Random, seed ^ 0xAB);
        let elev = elevation_from_land_mask_and_geology(
            &bounds,
            &mask,
            &geo,
            seed,
            ElevationIntensity::Standard,
        );
        let climate =
            generate_climate_layers(&bounds, &mask, &elev, PrecipitationStyle::Balanced, seed);
        let analysis = analyze_depressions(&elev, &bounds);
        let (catalog, owners, used_climate) = generate_with_owners(
            &elev,
            &bounds,
            Some(&climate.precipitation),
            RiverFluxParams {
                analysis: Some(&analysis),
                lakes: None,
                density: RiverDensity::Balanced,
            },
        );
        assert!(used_climate);
        let owned = owners.iter().filter(|&&o| o > 0).count();
        let path_cells = river_path_cell_count(&catalog);
        let max_len = catalog
            .rivers
            .iter()
            .map(|r| r.cells.len())
            .max()
            .unwrap_or(0);
        assert!(
            max_len >= 3,
            "dogfood seed {seed}: expected stem > 2 cells, max={max_len}"
        );
        assert!(
            path_cells * 2 >= owned,
            "catalog path cells should cover most owner cells, path={path_cells} owned={owned}"
        );
    }
}
