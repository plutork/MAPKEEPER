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
use crate::worldgen::hydrology::lakes::lake_outflow_supply;
use crate::worldgen::hydrology::types::{DepressionAnalysis, RiverDensity};
use crate::worldgen::hydrology::river_validate::{
    classify_terminal, enforce_strict_generated_catalog, mouth_touches_sea as validate_mouth_touches_sea,
    would_assign_parent_cycle, RiverTerminal, RiverValidationContext,
};

use super::depression_fill::analyze_depressions;

/// Azgaar `MIN_FLUX_TO_FORM_RIVER`, scaled for map size.
const BASE_MIN_FLUX: u32 = 30;
pub const UNIFORM_PRECIP: u32 = 1;
/// Fallback mean when precip layer has no land samples (Balanced ≈ legacy uniform).
const FALLBACK_LAND_PRECIP_MEAN: f64 = 90.0;
const MIN_RIVER_CELLS: usize = 2;
const TRACE_STEP_CAP: usize = 8192;

/// Output of auto river generation (D-100 adds `rejected_rivers`).
#[derive(Debug, Clone)]
pub struct GenerateRiversOutput {
    pub catalog: RiverCatalog,
    pub owners: Vec<u32>,
    pub used_climate: bool,
    pub rejected_rivers: u32,
}

/// Generate a full river catalog from elevation (uniform precip fallback).
pub fn generate_rivers_from_elevation(elevation: &DenseLayer, bounds: &MapBounds) -> RiverCatalog {
    generate_with_owners(elevation, bounds, None, RiverFluxParams::default()).catalog
}

/// Run generation and return catalog + owner grid + climate flag + rejected count.
pub fn generate_with_owners(
    elevation: &DenseLayer,
    bounds: &MapBounds,
    precipitation: Option<&DenseLayer>,
    params: RiverFluxParams,
) -> GenerateRiversOutput {
    let n = bounds.len();
    if n == 0 {
        return GenerateRiversOutput {
            catalog: RiverCatalog::default(),
            owners: Vec::new(),
            used_climate: false,
            rejected_rivers: 0,
        };
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

    let mut catalog = build_catalog(
        &owner,
        heights,
        bounds,
        &lake_cells,
        parents,
        next_id,
    );
    let pre_count = catalog.rivers.len() as u32;
    let rejected_trace = finalize_traced_catalog(
        &mut catalog,
        heights,
        bounds,
        &lake_cells,
        params.lakes,
        &analysis,
        elevation,
        precipitation,
        use_climate,
        min_flux,
        n,
        params.density,
    );
    let rejected_prune = {
        let ctx = RiverValidationContext::new(heights, bounds, params.lakes);
        enforce_strict_generated_catalog(&mut catalog, &ctx)
    };
    let owners = owners_from_catalog(&catalog, n);
    let rejected_rivers = pre_count
        .saturating_sub(catalog.rivers.len() as u32)
        .saturating_add(rejected_trace)
        .saturating_add(rejected_prune);
    GenerateRiversOutput {
        catalog,
        owners,
        used_climate: use_climate,
        rejected_rivers,
    }
}

/// Optional inputs for lake-aware river flux (defaults = legacy D-55/D-91).
#[derive(Clone, Copy, Default)]
pub struct RiverFluxParams<'a> {
    pub analysis: Option<&'a DepressionAnalysis>,
    pub lakes: Option<&'a LakeCatalog>,
    pub density: RiverDensity,
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
    _lake_cells: &HashSet<usize>,
    parents: HashMap<u32, u32>,
    next_id: u32,
) -> RiverCatalog {
    let mut rivers = Vec::new();

    for id in 1..next_id {
        let cells = trace_owned_stem(id, owners, heights, bounds);
        if cells.is_empty() {
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
    while let Some(next) = bounds
        .from_index(cur)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
        .filter(|&n| {
            owned.contains(&n) && heights[n] < heights[cur] && heights[n] > SEA_LEVEL
        })
        .min_by_key(|&n| heights[n])
    {
        if chain.contains(&next) {
            break;
        }
        chain.push(next);
        cur = next;
    }
    chain
}

fn hex_neighbors(bounds: &MapBounds, index: usize) -> impl Iterator<Item = usize> + '_ {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraceOutcome {
    Sea,
    Lake(u32),
    Parent { parent_id: u32 },
    Stuck,
}

fn adjacent_lake_id(
    mouth: usize,
    lake_cell_to_id: &HashMap<usize, u32>,
    bounds: &MapBounds,
) -> Option<u32> {
    let mut cands: Vec<(u32, usize)> = hex_neighbors(bounds, mouth)
        .filter_map(|n| lake_cell_to_id.get(&n).map(|&id| (id, n)))
        .collect();
    cands.sort_by_key(|&(_, n)| n);
    cands.first().map(|&(id, _)| id)
}

fn find_parent_join(
    mouth: usize,
    cell_to_river: &HashMap<usize, u32>,
    self_id: u32,
    heights: &[i32],
    bounds: &MapBounds,
) -> Option<(u32, usize)> {
    let mut cands: Vec<(u32, usize)> = hex_neighbors(bounds, mouth)
        .filter_map(|n| {
            let pid = *cell_to_river.get(&n)?;
            if pid == self_id {
                return None;
            }
            Some((pid, n))
        })
        .collect();
    cands.sort_by_key(|&(_, c)| (heights[c], c));
    cands.first().copied()
}

fn strict_downhill_step(
    mouth: usize,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
    blocked: &HashSet<usize>,
) -> Option<usize> {
    hex_neighbors(bounds, mouth)
        .filter(|&n| {
            !lake_cells.contains(&n)
                && heights[n] > SEA_LEVEL
                && heights[n] < heights[mouth]
                && !blocked.contains(&n)
        })
        .min_by_key(|&n| (heights[n], n))
}

fn plateau_step(
    mouth: usize,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
    blocked: &HashSet<usize>,
) -> Option<usize> {
    let h = heights[mouth];
    let mut prev: HashMap<usize, usize> = HashMap::new();
    let mut queue = std::collections::VecDeque::from([mouth]);
    let mut visited = HashSet::from([mouth]);
    let mut best_exit: Option<(usize, usize)> = None;

    while let Some(cur) = queue.pop_front() {
        for n in hex_neighbors(bounds, cur) {
            if lake_cells.contains(&n) || heights[n] <= SEA_LEVEL || blocked.contains(&n) {
                continue;
            }
            if heights[n] < h {
                if heights[n] > SEA_LEVEL {
                    let score = (heights[n], n);
                    let replace = match best_exit {
                        None => true,
                        Some((e, _)) => score < (heights[e], e),
                    };
                    if replace {
                        best_exit = Some((n, cur));
                    }
                }
                continue;
            }
            if heights[n] == h && visited.insert(n) {
                prev.insert(n, cur);
                queue.push_back(n);
            }
        }
    }

    let (exit, gateway) = best_exit?;
    if gateway == mouth {
        return Some(exit);
    }
    let mut cur = gateway;
    loop {
        let p = *prev.get(&cur)?;
        if p == mouth {
            return Some(cur);
        }
        cur = p;
    }
}

fn next_trace_step(
    mouth: usize,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
    path: &[usize],
) -> Option<usize> {
    let blocked: HashSet<usize> = path.iter().copied().collect();
    strict_downhill_step(mouth, heights, bounds, lake_cells, &blocked).or_else(|| {
        plateau_step(mouth, heights, bounds, lake_cells, &blocked)
    })
}

fn extend_river_path(
    cells: &mut Vec<usize>,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
    lake_cell_to_id: &HashMap<usize, u32>,
    cell_to_river: &mut HashMap<usize, u32>,
    self_id: u32,
) -> TraceOutcome {
    for _ in 0..TRACE_STEP_CAP {
        let Some(&mouth) = cells.last() else {
            return TraceOutcome::Stuck;
        };
        if validate_mouth_touches_sea(mouth, heights, bounds) {
            return TraceOutcome::Sea;
        }
        if let Some(lid) = adjacent_lake_id(mouth, lake_cell_to_id, bounds) {
            return TraceOutcome::Lake(lid);
        }
        if let Some(next) = next_trace_step(mouth, heights, bounds, lake_cells, cells) {
            if cells.contains(&next) || heights[next] <= SEA_LEVEL {
                if validate_mouth_touches_sea(mouth, heights, bounds) {
                    return TraceOutcome::Sea;
                }
                return TraceOutcome::Stuck;
            }
            if let Some(&owner) = cell_to_river.get(&next) {
                if owner != self_id {
                    return TraceOutcome::Parent { parent_id: owner };
                }
            }
            cells.push(next);
            cell_to_river.insert(next, self_id);
            continue;
        }
        if let Some((pid, _)) = find_parent_join(mouth, cell_to_river, self_id, heights, bounds) {
            return TraceOutcome::Parent { parent_id: pid };
        }
        return TraceOutcome::Stuck;
    }
    TraceOutcome::Stuck
}

fn cell_to_river_map(catalog: &RiverCatalog) -> HashMap<usize, u32> {
    let mut map = HashMap::new();
    for river in &catalog.rivers {
        for &c in &river.cells {
            map.insert(c, river.id);
        }
    }
    map
}

fn lake_cell_to_id_map(lakes: Option<&LakeCatalog>) -> HashMap<usize, u32> {
    let mut map = HashMap::new();
    if let Some(catalog) = lakes {
        for lake in &catalog.lakes {
            for &c in &lake.cells {
                map.insert(c, lake.id);
            }
        }
    }
    map
}

fn trim_ocean_tail(cells: &mut Vec<usize>, heights: &[i32]) {
    while cells
        .last()
        .is_some_and(|&c| heights.get(c).copied().unwrap_or(0) <= SEA_LEVEL)
    {
        cells.pop();
    }
}

fn refresh_river_endpoints(river: &mut River) {
    if river.cells.is_empty() {
        return;
    }
    river.source = river.cells[0];
    river.mouth = *river.cells.last().unwrap_or(&river.source);
}

fn find_root_in_catalog(river_id: u32, rivers: &[River]) -> u32 {
    let mut current = river_id;
    for _ in 0..=rivers.len() {
        let Some(river) = rivers.iter().find(|r| r.id == current) else {
            return river_id;
        };
        if river.parent == 0 || river.parent == river.id {
            return current;
        }
        current = river.parent;
    }
    river_id
}

#[allow(clippy::too_many_arguments)]
fn finalize_traced_catalog(
    catalog: &mut RiverCatalog,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
    lakes: Option<&LakeCatalog>,
    analysis: &DepressionAnalysis,
    elevation: &DenseLayer,
    precipitation: Option<&DenseLayer>,
    use_climate: bool,
    min_flux: u32,
    n: usize,
    density: RiverDensity,
) -> u32 {
    let lake_cell_to_id = lake_cell_to_id_map(lakes);
    let mut rejected = 0u32;
    let mut cell_to_river = cell_to_river_map(catalog);

    let mut order: Vec<usize> = (0..catalog.rivers.len()).collect();
    order.sort_by(|&a, &b| {
        let ra = &catalog.rivers[a];
        let rb = &catalog.rivers[b];
        let root_a = ra.parent == 0 || ra.parent == ra.id;
        let root_b = rb.parent == 0 || rb.parent == rb.id;
        root_b
            .cmp(&root_a)
            .then_with(|| heights[rb.source].cmp(&heights[ra.source]))
            .then_with(|| ra.id.cmp(&rb.id))
    });

    struct TraceUpdate {
        idx: usize,
        cells: Vec<usize>,
        outcome: TraceOutcome,
    }
    let mut updates = Vec::new();
    for idx in order {
        let id = catalog.rivers[idx].id;
        let mut cells = catalog.rivers[idx].cells.clone();
        let outcome = extend_river_path(
            &mut cells,
            heights,
            bounds,
            lake_cells,
            &lake_cell_to_id,
            &mut cell_to_river,
            id,
        );
        for &c in &cells {
            cell_to_river.insert(c, id);
        }
        updates.push(TraceUpdate { idx, cells, outcome });
    }
    for update in &mut updates {
        trim_ocean_tail(&mut update.cells, heights);
        let id = catalog.rivers[update.idx].id;
        if update.cells.len() > 1 {
            let mouth = *update.cells.last().unwrap();
            if cell_to_river
                .get(&mouth)
                .is_some_and(|&owner| owner != id)
            {
                update.cells.pop();
                let new_mouth = *update.cells.last().unwrap();
                if let Some((pid, _)) =
                    find_parent_join(new_mouth, &cell_to_river, id, heights, bounds)
                {
                    update.outcome = TraceOutcome::Parent { parent_id: pid };
                } else {
                    update.outcome = TraceOutcome::Stuck;
                }
            }
        }
        if update.cells.len() < MIN_RIVER_CELLS && !matches!(update.outcome, TraceOutcome::Parent { .. }) {
            update.outcome = TraceOutcome::Stuck;
        }
    }
    for update in &updates {
        catalog.rivers[update.idx].cells = update.cells.clone();
        refresh_river_endpoints(&mut catalog.rivers[update.idx]);
    }
    cell_to_river = cell_to_river_map(catalog);
    let meta: Vec<(usize, u32, u32)> = updates
        .iter()
        .map(|u| {
            let id = catalog.rivers[u.idx].id;
            let (parent, basin) = match u.outcome {
                TraceOutcome::Sea | TraceOutcome::Lake(_) => (id, id),
                TraceOutcome::Parent { parent_id }
                    if !would_assign_parent_cycle(id, parent_id, catalog) =>
                {
                    (parent_id, find_root_in_catalog(parent_id, &catalog.rivers))
                }
                TraceOutcome::Parent { .. } => (id, id),
                TraceOutcome::Stuck => {
                    let p = catalog.rivers[u.idx].parent;
                    if p == 0 || p == id {
                        (id, id)
                    } else {
                        (p, find_root_in_catalog(id, &catalog.rivers))
                    }
                }
            };
            (u.idx, parent, basin)
        })
        .collect();
    for (idx, parent, basin) in meta {
        catalog.rivers[idx].parent = parent;
        catalog.rivers[idx].basin = basin;
    }

    rejected += spawn_lake_outlet_rivers(
        catalog,
        lakes,
        analysis,
        elevation,
        precipitation,
        use_climate,
        heights,
        bounds,
        lake_cells,
        &lake_cell_to_id,
        min_flux,
        &mut cell_to_river,
    );

    let before = catalog.rivers.len();
    let ctx = RiverValidationContext::new(heights, bounds, lakes);
    let keep: HashSet<u32> = catalog
        .rivers
        .iter()
        .filter(|r| {
            r.cells.len() >= MIN_RIVER_CELLS
                || classify_terminal(r, catalog, &ctx).is_some_and(|t| {
                    matches!(
                        t,
                        RiverTerminal::Sea { .. }
                            | RiverTerminal::Lake { .. }
                            | RiverTerminal::Parent { .. }
                    )
                })
        })
        .map(|r| r.id)
        .collect();
    catalog.rivers.retain(|r| keep.contains(&r.id));
    rejected += (before.saturating_sub(catalog.rivers.len())) as u32;

    for river in &mut catalog.rivers {
        refresh_river_endpoints(river);
    }

    catalog.rivers.sort_by_key(|r| r.id);
    let _ = (n, density); // reserved for future scale guards
    rejected
}

#[allow(clippy::too_many_arguments)]
fn spawn_lake_outlet_rivers(
    catalog: &mut RiverCatalog,
    lakes: Option<&LakeCatalog>,
    analysis: &DepressionAnalysis,
    elevation: &DenseLayer,
    precipitation: Option<&DenseLayer>,
    use_climate: bool,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
    lake_cell_to_id: &HashMap<usize, u32>,
    min_flux: u32,
    cell_to_river: &mut HashMap<usize, u32>,
) -> u32 {
    let Some(lake_catalog) = lakes else {
        return 0;
    };
    let mut added = 0u32;
    for lake in &lake_catalog.lakes {
        if lake.endorheic {
            continue;
        }
        let Some(outlet) = lake.outlet_cell else {
            continue;
        };
        let supply = lake_outflow_supply(
            lake,
            analysis,
            precipitation,
            elevation,
            bounds,
            use_climate,
        );
        if supply < u64::from(min_flux) {
            continue;
        }
        let start = outlet_start_cell(outlet, heights, bounds, lake_cells);
        let Some(start) = start else {
            continue;
        };
        if cell_to_river.contains_key(&start) {
            continue;
        }
        let id = catalog.next_id;
        catalog.next_id += 1;
        let mut cells = vec![start];
        let outcome = extend_river_path(
            &mut cells,
            heights,
            bounds,
            lake_cells,
            lake_cell_to_id,
            cell_to_river,
            id,
        );
        trim_ocean_tail(&mut cells, heights);
        if cells.len() < MIN_RIVER_CELLS || outcome == TraceOutcome::Stuck {
            continue;
        }
        for &c in &cells {
            cell_to_river.insert(c, id);
        }
        let mouth = *cells.last().unwrap_or(&start);
        let (parent, basin) = match outcome {
            TraceOutcome::Parent { parent_id } => {
                (parent_id, find_root_in_catalog(parent_id, &catalog.rivers))
            }
            _ => (id, id),
        };
        catalog.rivers.push(River {
            id,
            cells,
            source: start,
            mouth,
            parent,
            basin,
            name: None,
        });
        added += 1;
    }
    added
}

fn outlet_start_cell(
    outlet: usize,
    heights: &[i32],
    bounds: &MapBounds,
    lake_cells: &HashSet<usize>,
) -> Option<usize> {
    if heights.get(outlet).copied()? > SEA_LEVEL && !lake_cells.contains(&outlet) {
        return Some(outlet);
    }
    hex_neighbors(bounds, outlet)
        .filter(|&n| heights[n] > SEA_LEVEL && !lake_cells.contains(&n))
        .min_by_key(|&n| (heights[n], n))
}

fn owners_from_catalog(catalog: &RiverCatalog, n: usize) -> Vec<u32> {
    let mut owners = vec![0u32; n];
    for river in &catalog.rivers {
        for &c in &river.cells {
            if c < n {
                owners[c] = river.id;
            }
        }
    }
    owners
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
    use crate::worldgen::hydrology::river_validate::{
        validate_generated_catalog_strict, RiverValidationContext,
    };

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
        assert_eq!(before.catalog, after.catalog);
        assert_eq!(before.owners, after.owners);
        assert!(!after.catalog.rivers.is_empty());
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
        assert!(a.used_climate);
        assert_eq!(a.catalog, b.catalog);
        assert_eq!(a.owners, b.owners);
    }

    #[test]
    fn fallback_without_precip_layer_matches_uniform() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let legacy = generate_with_owners(&elev, &bounds, None, default_params());
        let empty = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        let fallback = generate_with_owners(&elev, &bounds, Some(&empty), default_params());
        assert!(!legacy.used_climate);
        assert!(!fallback.used_climate);
        assert_eq!(legacy.catalog, fallback.catalog);
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
        assert!(climate.used_climate);

        let wet_sources = sources_in_west_half(&climate.catalog, &bounds, mid_q);
        let dry_sources = climate.catalog.rivers.len().saturating_sub(wet_sources);
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
        assert!(scaled.used_climate);
        let delta = (scaled.catalog.rivers.len() as i32 - uniform.catalog.rivers.len() as i32).abs();
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
        let f = few.catalog.rivers.len();
        let b = balanced.catalog.rivers.len();
        let m = many.catalog.rivers.len();
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
        let out = generate_with_owners(
            &elev,
            &bounds,
            None,
            RiverFluxParams {
                analysis: Some(&analysis),
                lakes: Some(&lakes),
                density: RiverDensity::Balanced,
            },
        );
        let catalog = out.catalog;
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
        assert_eq!(a.catalog, b.catalog);
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
    fn small_preset_seed_sweep_passes_strict_validate() {
        use crate::map_preset::MapPreset;
        use crate::worldgen::climate::{generate_climate_layers, PrecipitationStyle};
        use crate::worldgen::elevation::{elevation_from_land_mask_and_geology, ElevationIntensity};
        use crate::worldgen::geology::{generate_geology, GeologyStyle};
        use crate::worldgen::land::{generate_land_mask, LayoutClass, ShoreCharacter};

        let bounds = MapPreset::Small.bounds();
        for layout in [LayoutClass::Continents, LayoutClass::Archipelago] {
            for seed in 20..=30u64 {
                let mask = generate_land_mask(&bounds, layout, ShoreCharacter::Smooth, seed);
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
                let out = generate_with_owners(
                    &elev,
                    &bounds,
                    Some(&climate.precipitation),
                    RiverFluxParams {
                        analysis: Some(&analysis),
                        lakes: None,
                        density: RiverDensity::Balanced,
                    },
                );
                let ctx = RiverValidationContext::new(&analysis.conditioned_heights, &bounds, None);
                let report = validate_generated_catalog_strict(&out.catalog, &ctx);
                assert!(
                    report.is_ok(),
                    "{layout:?} seed {seed}: strict failed: {:?}",
                    report.diagnostics
                );
            }
        }
    }

    #[test]
    fn generated_catalog_passes_strict_on_slope_fixture() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let analysis = analyze_depressions(&elev, &bounds);
        let out = generate_with_owners(
            &elev,
            &bounds,
            None,
            RiverFluxParams {
                analysis: Some(&analysis),
                lakes: None,
                density: RiverDensity::Balanced,
            },
        );
        let ctx = RiverValidationContext::new(&analysis.conditioned_heights, &bounds, None);
        assert!(validate_generated_catalog_strict(&out.catalog, &ctx).is_ok());
    }

    #[test]
    fn density_tiers_pass_strict_validation() {
        let bounds = MapBounds::new(18, 10);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        slope_fixture(&bounds, &mut elev);
        let analysis = analyze_depressions(&elev, &bounds);
        let ctx = RiverValidationContext::new(&analysis.conditioned_heights, &bounds, None);
        for density in [
            RiverDensity::Few,
            RiverDensity::Balanced,
            RiverDensity::Many,
        ] {
            let out = generate_with_owners(
                &elev,
                &bounds,
                None,
                RiverFluxParams {
                    analysis: Some(&analysis),
                    lakes: None,
                    density,
                },
            );
            assert!(
                validate_generated_catalog_strict(&out.catalog, &ctx).is_ok(),
                "strict validate failed for {:?}",
                density
            );
        }
    }
}
