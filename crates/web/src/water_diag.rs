//! Dogfood diagnostics for wizard/editor water generation (maintainer copy-paste).

use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::hydro::SEA_LEVEL;
use mapkeeper_core::lakes::LakeCatalog;
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue};
use mapkeeper_core::rivers::RiverCatalog;

use crate::dom::set_water_diagnostics;
use crate::state::{AppState, WaterGenTrace};

pub(crate) fn lake_catalog_stats(catalog: &LakeCatalog) -> (usize, usize, usize) {
    let lakes = catalog.lakes.len();
    let cells: usize = catalog.lakes.iter().map(|l| l.cells.len()).sum();
    let endorheic = catalog
        .lakes
        .iter()
        .filter(|l| l.endorheic)
        .count();
    (lakes, cells, endorheic)
}

pub(crate) fn river_catalog_stats(catalog: &RiverCatalog) -> (usize, usize) {
    let rivers = catalog.rivers.len();
    let path_cells: usize = catalog.rivers.iter().map(|r| r.cells.len()).sum();
    (rivers, path_cells)
}

fn land_cell_count(elevation: &DenseLayer, bounds: MapBounds) -> usize {
    (0..bounds.len())
        .filter(|&i| match elevation.state(i) {
            DenseState::Value(LayerValue::Int(v)) => v > SEA_LEVEL,
            _ => false,
        })
        .count()
}

pub(crate) fn format_water_diagnostics(state: &AppState) -> String {
    let bounds = state.map_bounds;
    let (lake_n, lake_cells, endorheic) = lake_catalog_stats(&state.lakes);
    let (river_n, path_cells) = river_catalog_stats(&state.rivers);
    let land = land_cell_count(&state.elevation, bounds);
    let precip = match state.precip_layer_present {
        Some(true) => "present",
        Some(false) => "missing",
        None => "unknown",
    };
    let mut out = String::new();
    out.push_str("=== snapshot ===\n");
    out.push_str(&format!(
        "world: {}×{} · {} cells · land {}\n",
        bounds.width,
        bounds.height,
        bounds.len(),
        land
    ));
    out.push_str(&format!(
        "lakes: {lake_n} · {lake_cells} cells · endorheic {endorheic} · next_id {}\n",
        state.lakes.next_id
    ));
    out.push_str(&format!(
        "rivers: {river_n} · {path_cells} path cells\n",
    ));
    out.push_str(&format!("precip layer: {precip}\n"));
    out.push('\n');
    out.push_str("=== last action ===\n");
    let trace = &state.water_gen_trace;
    if trace.action.is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(&format!("action: {}\n", trace.action));
        if !trace.request.is_empty() {
            out.push_str(&format!("request: {}\n", trace.request));
        }
        if !trace.result.is_empty() {
            out.push_str(&format!("result: {}\n", trace.result));
        }
        if !trace.error.is_empty() {
            out.push_str(&format!("error: {}\n", trace.error));
        }
    }
    out
}

pub(crate) fn sync_water_diagnostics(state: &AppState) {
    set_water_diagnostics(&format_water_diagnostics(state));
}

pub(crate) fn set_water_gen_trace(state: &mut AppState, trace: WaterGenTrace) {
    state.water_gen_trace = trace;
    sync_water_diagnostics(state);
}