//! Precip-aware lake generation from depression analysis (hydrology-lake-generation-v1).

use std::collections::HashSet;

use crate::hex::MapBounds;
use crate::hydro::{DEFAULT_LAND_ELEVATION, SEA_LEVEL};
use crate::lakes::{Lake, LakeCatalog, LAKE_CATALOG_SCHEMA_VERSION};
use crate::layer::DenseLayer;
use crate::worldgen::hydrology::depression_fill::provisional_drainage;
use crate::worldgen::hydrology::terminal_routing::{classify_basin_terminal, SpillTerminal};
use crate::worldgen::hydrology::types::{classify_precip_input, DepressionAnalysis, LakeDensity, PrecipInputState};

/// Generate a lake catalog from depression analysis + climate inputs.
pub fn generate_lakes(
    analysis: &DepressionAnalysis,
    elevation: &DenseLayer,
    precipitation: Option<&DenseLayer>,
    bounds: &MapBounds,
    density: LakeDensity,
    seed: u64,
) -> LakeCatalog {
    let n = bounds.len();
    if n == 0 {
        return LakeCatalog::default();
    }

    let precip_state = classify_precip_input(elevation, precipitation);

    let depression_basins = collect_depression_basins(analysis);
    if depression_basins.is_empty() {
        return LakeCatalog::default();
    }

    let provisional = provisional_drainage(analysis, precipitation, bounds, precip_state);

    let land_cells = land_cell_count(elevation, n);
    let max_lake_cells = max_lake_cell_budget(land_cells, density);
    let min_supply = min_supply_threshold(land_cells, density);

    let mut candidates: Vec<BasinCandidate> = depression_basins
        .into_iter()
        .filter_map(|bid| {
            let cells = lake_cells_for_basin(bid, analysis);
            if cells.is_empty() {
                return None;
            }
            let supply = provisional.basin_supply.get(&bid).copied().unwrap_or(0);
            let outlet = analysis.spill_cell.get(&bid).copied();
            let terminal = classify_basin_terminal(bid, analysis);
            let (endorheic, outlet) = match terminal {
                SpillTerminal::Ocean => (false, outlet),
                SpillTerminal::Endorheic => (true, outlet),
                SpillTerminal::Cycle | SpillTerminal::Unresolved => return None,
            };
            let fill_volume = cells
                .iter()
                .map(|&cell| analysis.fill_depth[cell].max(0) as u64)
                .sum();
            Some(BasinCandidate {
                bid,
                cells,
                supply,
                outlet,
                endorheic,
                fill_volume,
                hierarchy_depth: basin_hierarchy_depth(bid, analysis),
            })
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.supply
            .cmp(&a.supply)
            .then_with(|| b.fill_volume.cmp(&a.fill_volume))
            .then_with(|| b.hierarchy_depth.cmp(&a.hierarchy_depth))
            .then_with(|| a.bid.cmp(&b.bid))
            .then_with(|| tie_break(seed, a.bid, b.bid))
    });

    let mut catalog = LakeCatalog::default();
    let mut used_cells = 0usize;
    let mut next_id = 1u32;

    for cand in candidates {
        if cand.supply < min_supply {
            continue;
        }
        let new_cells = cand.cells.len();
        if used_cells + new_cells > max_lake_cells {
            continue;
        }
        catalog.lakes.push(Lake {
            id: next_id,
            cells: cand.cells,
            outlet_cell: if cand.endorheic { None } else { cand.outlet },
            endorheic: cand.endorheic,
            name: None,
        });
        used_cells += new_cells;
        next_id += 1;
    }

    catalog.next_id = next_id.max(1);
    catalog.schema_version = LAKE_CATALOG_SCHEMA_VERSION;
    catalog
}

struct BasinCandidate {
    bid: u32,
    cells: Vec<usize>,
    supply: u64,
    outlet: Option<usize>,
    endorheic: bool,
    fill_volume: u64,
    hierarchy_depth: u32,
}

fn collect_depression_basins(analysis: &DepressionAnalysis) -> Vec<u32> {
    let mut ids: HashSet<u32> = HashSet::new();
    for i in 0..analysis.basin_id.len() {
        if analysis.fill_depth[i] > 0 && analysis.basin_id[i] > 0 {
            ids.insert(analysis.basin_id[i]);
        }
    }
    let mut out: Vec<u32> = ids.into_iter().collect();
    out.sort_unstable();
    out
}

fn lake_cells_for_basin(bid: u32, analysis: &DepressionAnalysis) -> Vec<usize> {
    analysis
        .basin_id
        .iter()
        .enumerate()
        .filter(|(i, b)| **b == bid && analysis.fill_depth[*i] > 0)
        .map(|(i, _)| i)
        .collect()
}

fn basin_hierarchy_depth(bid: u32, analysis: &DepressionAnalysis) -> u32 {
    let mut depth = 0u32;
    let mut current = Some(bid);
    while let Some(basin) = current {
        current = analysis.basin_parent.get(&basin).copied().flatten();
        depth = depth.saturating_add(1);
        if depth > analysis.spill_cell.len() as u32 {
            break;
        }
    }
    depth
}

fn land_cell_count(elevation: &DenseLayer, n: usize) -> usize {
    (0..n)
        .filter(|&i| elevation.int_or(i, DEFAULT_LAND_ELEVATION) > SEA_LEVEL)
        .count()
}

fn min_supply_threshold(land_cells: usize, density: LakeDensity) -> u64 {
    let scale = ((land_cells as f64) / 80.0).sqrt().max(1.0);
    let base = match density {
        LakeDensity::Sparse => 140.0,
        LakeDensity::Balanced => 65.0,
        LakeDensity::LakeRich => 28.0,
    };
    (base * scale) as u64
}

fn max_lake_cell_budget(land_cells: usize, density: LakeDensity) -> usize {
    let frac = match density {
        LakeDensity::Sparse => 0.04,
        LakeDensity::Balanced => 0.09,
        LakeDensity::LakeRich => 0.16,
    };
    ((land_cells as f64) * frac).max(1.0) as usize
}

fn tie_break(seed: u64, a: u32, b: u32) -> std::cmp::Ordering {
    let ha = mix_seed(seed, a);
    let hb = mix_seed(seed, b);
    ha.cmp(&hb)
}

fn mix_seed(seed: u64, bid: u32) -> u64 {
    seed ^ ((bid as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

pub fn lake_outflow_supply(
    lake: &Lake,
    analysis: &DepressionAnalysis,
    precipitation: Option<&DenseLayer>,
    _elevation: &DenseLayer,
    bounds: &MapBounds,
    precip_state: PrecipInputState,
) -> u64 {
    let provisional = provisional_drainage(analysis, precipitation, bounds, precip_state);
    let mut basins = HashSet::new();
    for &c in &lake.cells {
        let bid = analysis.basin_id[c];
        if bid > 0 {
            basins.insert(bid);
        }
    }
    basins
        .iter()
        .map(|b| provisional.basin_supply.get(b).copied().unwrap_or(0))
        .sum()
}

pub fn lake_acceptance_stats(catalog: &LakeCatalog) -> (usize, usize) {
    let cells: usize = catalog.lakes.iter().map(|l| l.cells.len()).sum();
    (catalog.lakes.len(), cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::PRECIPITATION_LAYER_ID;
    use crate::hex::Axial;
    use crate::layer::{DenseLayer, DenseState, LayerValue};
    use crate::worldgen::hydrology::terminal_routing::{classify_basin_terminal, SpillTerminal};
    use crate::worldgen::hydrology::analyze_depressions;

    fn set_elev(layer: &mut DenseLayer, bounds: &MapBounds, q: i32, r: i32, v: i32) {
        let i = bounds.index_of(Axial::new(q, r)).unwrap();
        layer.set(i, DenseState::Value(LayerValue::Int(v)));
    }

    fn set_precip(layer: &mut DenseLayer, i: usize, v: i32) {
        layer.set(i, DenseState::Value(LayerValue::Int(v)));
    }

    fn island_with_pit(bounds: &MapBounds, elev: &mut DenseLayer, pit: i32) {
        let w = bounds.width;
        for row in 0..bounds.height {
            for col in 0..w {
                let i = (row * w + col) as usize;
                let edge = row == 0 || col == 0 || row == bounds.height - 1 || col == w - 1;
                elev.set(
                    i,
                    DenseState::Value(LayerValue::Int(if edge { 0 } else { 25 })),
                );
            }
        }
        let center = (bounds.height / 2 * w + w / 2) as usize;
        elev.set(center, DenseState::Value(LayerValue::Int(pit)));
    }

    fn wet_dry_pair() -> (MapBounds, DenseLayer, DenseLayer, DenseLayer) {
        let bounds = MapBounds::new(12, 10);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        island_with_pit(&bounds, &mut elev, 6);
        let mut wet = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        let mut dry = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        for i in 0..bounds.len() {
            if elev.int_or(i, 0) > SEA_LEVEL {
                set_precip(&mut wet, i, 180);
                set_precip(&mut dry, i, 8);
            }
        }
        (bounds, elev, wet, dry)
    }

    #[test]
    fn wet_catchment_accepted_at_least_as_many_lakes_as_dry() {
        let (bounds, elev, wet, dry) = wet_dry_pair();
        let analysis = analyze_depressions(&elev, &bounds);
        let wet_cat = generate_lakes(
            &analysis,
            &elev,
            Some(&wet),
            &bounds,
            LakeDensity::Balanced,
            7,
        );
        let dry_cat = generate_lakes(
            &analysis,
            &elev,
            Some(&dry),
            &bounds,
            LakeDensity::Balanced,
            7,
        );
        let (wet_n, wet_cells) = lake_acceptance_stats(&wet_cat);
        let (dry_n, dry_cells) = lake_acceptance_stats(&dry_cat);
        assert!(
            wet_n >= dry_n && wet_cells >= dry_cells,
            "wet={wet_n}/{wet_cells} dry={dry_n}/{dry_cells}"
        );
    }

    #[test]
    fn density_monotonic_non_decreasing() {
        let (bounds, elev, wet, _) = wet_dry_pair();
        let analysis = analyze_depressions(&elev, &bounds);
        let sparse = generate_lakes(
            &analysis,
            &elev,
            Some(&wet),
            &bounds,
            LakeDensity::Sparse,
            11,
        );
        let balanced = generate_lakes(
            &analysis,
            &elev,
            Some(&wet),
            &bounds,
            LakeDensity::Balanced,
            11,
        );
        let rich = generate_lakes(
            &analysis,
            &elev,
            Some(&wet),
            &bounds,
            LakeDensity::LakeRich,
            11,
        );
        let (s_n, s_c) = lake_acceptance_stats(&sparse);
        let (b_n, b_c) = lake_acceptance_stats(&balanced);
        let (r_n, r_c) = lake_acceptance_stats(&rich);
        assert!(
            s_n <= b_n && b_n <= r_n,
            "counts sparse={s_n} bal={b_n} rich={r_n}"
        );
        assert!(
            s_c <= b_c && b_c <= r_c,
            "cells sparse={s_c} bal={b_c} rich={r_c}"
        );
    }

    #[test]
    fn lake_rich_dry_interior_not_map_filling() {
        let bounds = MapBounds::new(40, 24);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        for q in 0..bounds.width {
            for r in 0..bounds.height {
                if bounds.contains(Axial::new(q, r)) {
                    set_elev(&mut elev, &bounds, q, r, 10 + q / 2);
                }
            }
        }
        let mut precip = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        for i in 0..bounds.len() {
            if elev.int_or(i, 0) > SEA_LEVEL {
                set_precip(&mut precip, i, 15);
            }
        }
        let analysis = analyze_depressions(&elev, &bounds);
        let catalog = generate_lakes(
            &analysis,
            &elev,
            Some(&precip),
            &bounds,
            LakeDensity::LakeRich,
            3,
        );
        let (_, cells) = lake_acceptance_stats(&catalog);
        let land = land_cell_count(&elev, bounds.len());
        assert!(
            cells <= (land as f64 * 0.20) as usize,
            "lake cells {cells} vs land {land}"
        );
    }

    #[test]
    fn sparse_wet_coasts_may_keep_one_lake() {
        let bounds = MapBounds::new(14, 8);
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        island_with_pit(&bounds, &mut elev, 5);
        let mut precip = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, bounds.len());
        for i in 0..bounds.len() {
            if elev.int_or(i, 0) > SEA_LEVEL {
                set_precip(&mut precip, i, 200);
            }
        }
        let analysis = analyze_depressions(&elev, &bounds);
        let catalog = generate_lakes(
            &analysis,
            &elev,
            Some(&precip),
            &bounds,
            LakeDensity::Sparse,
            5,
        );
        let (n, _) = lake_acceptance_stats(&catalog);
        assert!(n <= 1, "sparse should not flood tiny map, got {n}");
    }

    #[test]
    fn enclosed_basin_yields_lake_or_dry_not_playa_stub() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/worlds/enclosed-basin/map");
        let manifest_raw = std::fs::read_to_string(root.join("manifest.json")).unwrap();
        let manifest: crate::layer::MapManifest =
            crate::layer::MapManifest::from_json(&manifest_raw).unwrap();
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
        let analysis = analyze_depressions(&elev, &bounds);
        let catalog = generate_lakes(&analysis, &elev, None, &bounds, LakeDensity::Balanced, 99);
        for lake in &catalog.lakes {
            assert!(!lake.cells.is_empty());
            assert!(!lake
                .name
                .as_deref()
                .unwrap_or("")
                .eq_ignore_ascii_case("playa"));
        }
    }

    #[test]
    fn generation_is_deterministic_for_seed() {
        let (bounds, elev, wet, _) = wet_dry_pair();
        let analysis = analyze_depressions(&elev, &bounds);
        let a = generate_lakes(
            &analysis,
            &elev,
            Some(&wet),
            &bounds,
            LakeDensity::Balanced,
            42,
        );
        let b = generate_lakes(
            &analysis,
            &elev,
            Some(&wet),
            &bounds,
            LakeDensity::Balanced,
            42,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn catalog_outlet_matches_terminal_routing() {
        let (bounds, elev, wet, _) = wet_dry_pair();
        let analysis = analyze_depressions(&elev, &bounds);
        let catalog = generate_lakes(
            &analysis,
            &elev,
            Some(&wet),
            &bounds,
            LakeDensity::LakeRich,
            1,
        );
        for lake in &catalog.lakes {
            if lake.endorheic {
                assert!(lake.outlet_cell.is_none());
            } else {
                assert!(lake.outlet_cell.is_some());
            }
        }
    }

    #[test]
    fn lake_chain_drains_through_downstream_basin() {
        use crate::lakes::{Lake, LakeCatalog};
        use crate::worldgen::hydrology::{build_drainage_graph, DrainageNode, DrainageNodeId};

        let analysis = mock_terminal_chain_analysis();
        assert_eq!(
            classify_basin_terminal(1, &analysis),
            SpillTerminal::Ocean
        );
        let bounds = MapBounds::new(3, 2);
        let catalog = LakeCatalog {
            schema_version: 1,
            next_id: 3,
            lakes: vec![
                Lake {
                    id: 1,
                    cells: vec![1, 2],
                    outlet_cell: Some(2),
                    endorheic: false,
                    name: None,
                },
                Lake {
                    id: 2,
                    cells: vec![3, 4],
                    outlet_cell: Some(4),
                    endorheic: false,
                    name: None,
                },
            ],
        };
        let graph = build_drainage_graph(&analysis, &catalog, &bounds).unwrap();
        let upstream = graph
            .nodes
            .iter()
            .position(|node| matches!(node, DrainageNode::Lake(1)))
            .expect("upstream lake node");
        let downstream = graph
            .nodes
            .iter()
            .position(|node| matches!(node, DrainageNode::Lake(2)))
            .expect("downstream lake node");
        assert_eq!(graph.receiver[upstream], Some(DrainageNodeId(downstream)));
    }

    fn mock_terminal_chain_analysis() -> DepressionAnalysis {
        use std::collections::HashMap;

        DepressionAnalysis {
            original_heights: vec![0, 20, 20, 18, 16, 0],
            conditioned_heights: vec![0, 20, 20, 18, 16, 0],
            flood_rank: vec![0; 6],
            provisional_receiver: vec![None, Some(2), Some(3), Some(4), Some(5), None],
            fill_depth: vec![0, 2, 2, 2, 2, 0],
            basin_id: vec![0, 1, 1, 2, 2, 0],
            spill_cell: HashMap::from([(1, 2), (2, 4)]),
            spill_elevation: HashMap::new(),
            basin_parent: HashMap::from([(1, Some(2)), (2, None)]),
        }
    }
}
