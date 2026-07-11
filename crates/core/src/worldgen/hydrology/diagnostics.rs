//! Read-only metrics for the legacy hydrology baseline.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::hex::MapBounds;
use crate::lakes::LakeCatalog;
use crate::rivers::RiverCatalog;

use super::river_validate::{
    classify_terminal, find_root_id, validate_catalog, RiverTerminal, RiverValidationContext,
};
use super::types::DepressionAnalysis;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum LegacyTerminalReason {
    Sea,
    Lake { lake_id: u32 },
    Parent { parent_id: u32 },
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegacyRiverTerminal {
    pub river_id: u32,
    pub reason: LegacyTerminalReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegacyHydrologyDiagnostics {
    pub depression_basin_count: usize,
    pub depression_cell_count: usize,
    pub unresolved_depression_count: usize,
    pub lake_count: usize,
    pub endorheic_lake_count: usize,
    pub lake_outlet_count: usize,
    pub river_count: usize,
    pub river_path_cell_count: usize,
    pub river_component_count: usize,
    pub lake_inlet_count: usize,
    pub short_coastal_river_count: usize,
    pub invalid_river_count: usize,
    pub terminals: Vec<LegacyRiverTerminal>,
}

/// Report the current catalog-based hydrology without changing routing behavior.
pub fn diagnose_legacy_hydrology(
    analysis: &DepressionAnalysis,
    rivers: &RiverCatalog,
    lakes: &LakeCatalog,
    bounds: &MapBounds,
) -> LegacyHydrologyDiagnostics {
    let basin_ids: BTreeSet<u32> = analysis
        .basin_id
        .iter()
        .copied()
        .filter(|&id| id != 0)
        .collect();
    let unresolved_depression_count = basin_ids
        .iter()
        .filter(|id| !analysis.spill_cell.contains_key(id))
        .count();
    let ctx = RiverValidationContext::new(&analysis.conditioned_heights, bounds, Some(lakes));
    let validation = validate_catalog(rivers, &ctx);
    let invalid_ids: BTreeSet<u32> = validation
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.river_id)
        .collect();

    let mut component_roots = BTreeSet::new();
    let mut lake_inlet_count = 0usize;
    let mut short_coastal_river_count = 0usize;
    let mut terminals = Vec::with_capacity(rivers.rivers.len());
    for river in &rivers.rivers {
        if let Some(root) = find_root_id(river.id, rivers) {
            component_roots.insert(root);
        }
        let reason = match classify_terminal(river, rivers, &ctx) {
            Some(RiverTerminal::Sea { .. }) => {
                if river.cells.len() <= 2 {
                    short_coastal_river_count += 1;
                }
                LegacyTerminalReason::Sea
            }
            Some(RiverTerminal::Lake { lake_id, .. }) => {
                lake_inlet_count += 1;
                LegacyTerminalReason::Lake { lake_id }
            }
            Some(RiverTerminal::Parent { parent_id, .. }) => {
                LegacyTerminalReason::Parent { parent_id }
            }
            None => LegacyTerminalReason::Invalid,
        };
        terminals.push(LegacyRiverTerminal {
            river_id: river.id,
            reason,
        });
    }
    terminals.sort_by_key(|terminal| terminal.river_id);

    LegacyHydrologyDiagnostics {
        depression_basin_count: basin_ids.len(),
        depression_cell_count: analysis
            .fill_depth
            .iter()
            .filter(|&&depth| depth > 0)
            .count(),
        unresolved_depression_count,
        lake_count: lakes.lakes.len(),
        endorheic_lake_count: lakes.lakes.iter().filter(|lake| lake.endorheic).count(),
        lake_outlet_count: lakes
            .lakes
            .iter()
            .filter(|lake| lake.outlet_cell.is_some() && !lake.endorheic)
            .count(),
        river_count: rivers.rivers.len(),
        river_path_cell_count: rivers.rivers.iter().map(|river| river.cells.len()).sum(),
        river_component_count: component_roots.len(),
        lake_inlet_count,
        short_coastal_river_count,
        invalid_river_count: invalid_ids.len(),
        terminals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::Axial;
    use crate::layer::{DenseLayer, DenseState, LayerValue};
    use crate::rivers::{River, RiverCatalog};
    use crate::worldgen::hydrology::analyze_depressions;

    #[test]
    fn reports_terminal_reasons_and_short_coastal_roots() {
        let bounds = MapBounds::new(5, 4);
        let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
        for index in 0..bounds.len() {
            elevation.set(index, DenseState::Value(LayerValue::Int(0)));
        }
        let land = bounds.index_of(Axial::new(2, 1)).unwrap();
        elevation.set(land, DenseState::Value(LayerValue::Int(10)));
        let analysis = analyze_depressions(&elevation, &bounds);
        let rivers = RiverCatalog {
            schema_version: 1,
            next_id: 2,
            rivers: vec![River {
                id: 1,
                cells: vec![land],
                source: land,
                mouth: land,
                parent: 1,
                basin: 1,
                name: None,
            }],
        };

        let diagnostics =
            diagnose_legacy_hydrology(&analysis, &rivers, &LakeCatalog::default(), &bounds);

        assert_eq!(diagnostics.short_coastal_river_count, 1);
        assert_eq!(diagnostics.terminals.len(), 1);
        assert_eq!(diagnostics.terminals[0].reason, LegacyTerminalReason::Sea);
    }

    #[test]
    fn generated_seed_baseline_is_deterministic() {
        use crate::map_preset::MapPreset;
        use crate::worldgen::climate::{generate_climate_layers, PrecipitationStyle};
        use crate::worldgen::elevation::{
            elevation_from_land_mask_and_geology, ElevationIntensity,
        };
        use crate::worldgen::geology::{generate_geology, GeologyStyle};
        use crate::worldgen::hydrology::{
            generate_lakes, generate_with_owners, LakeDensity, RiverDensity, RiverFluxParams,
        };
        use crate::worldgen::land::{generate_land_mask, LayoutClass, ShoreCharacter};

        let bounds = MapPreset::Small.bounds();
        for seed in [20_u64, 21] {
            let mask = generate_land_mask(
                &bounds,
                LayoutClass::Continents,
                ShoreCharacter::Smooth,
                seed,
            );
            let geology = generate_geology(&bounds, &mask, GeologyStyle::Random, seed ^ 0xAB);
            let elevation = elevation_from_land_mask_and_geology(
                &bounds,
                &mask,
                &geology,
                seed,
                ElevationIntensity::Standard,
            );
            let climate = generate_climate_layers(
                &bounds,
                &mask,
                &elevation,
                PrecipitationStyle::Balanced,
                seed,
            );
            let analysis = analyze_depressions(&elevation, &bounds);
            let lakes = generate_lakes(
                &analysis,
                &elevation,
                Some(&climate.precipitation),
                &bounds,
                LakeDensity::Balanced,
                seed,
            );
            let first = generate_with_owners(
                &elevation,
                &bounds,
                Some(&climate.precipitation),
                RiverFluxParams {
                    analysis: Some(&analysis),
                    lakes: Some(&lakes),
                    density: RiverDensity::Balanced,
                },
            );
            let second = generate_with_owners(
                &elevation,
                &bounds,
                Some(&climate.precipitation),
                RiverFluxParams {
                    analysis: Some(&analysis),
                    lakes: Some(&lakes),
                    density: RiverDensity::Balanced,
                },
            );
            assert_eq!(
                diagnose_legacy_hydrology(&analysis, &first.catalog, &lakes, &bounds),
                diagnose_legacy_hydrology(&analysis, &second.catalog, &lakes, &bounds),
                "seed {seed} changed baseline diagnostics"
            );
        }
    }
}
