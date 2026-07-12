//! Dogfood diagnostics for wizard/editor water generation (maintainer copy-paste).

use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::hydro::SEA_LEVEL;
use mapkeeper_core::lakes::LakeCatalog;
use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue};

use crate::brush::channel_topology_counts;
use crate::dom::set_water_diagnostics;
use crate::state::{AppState, WaterGenTrace};

pub(crate) fn lake_catalog_stats(catalog: &LakeCatalog) -> (usize, usize, usize) {
    let lakes = catalog.lakes.len();
    let cells: usize = catalog.lakes.iter().map(|l| l.cells.len()).sum();
    let endorheic = catalog.lakes.iter().filter(|l| l.endorheic).count();
    (lakes, cells, endorheic)
}

fn land_cell_count(elevation: &DenseLayer, bounds: MapBounds) -> usize {
    (0..bounds.len())
        .filter(|&i| match elevation.state(i) {
            DenseState::Value(LayerValue::Int(v)) => v > SEA_LEVEL,
            _ => false,
        })
        .count()
}

fn format_channel_topology_line(segments: usize, channel_cells: usize, read_only: bool) -> String {
    let mut line = format!(
        "channels: {segments} physical segments · {channel_cells} channel cells\n",
    );
    if read_only {
        line.push_str("rivers authoring: read-only (hydrology v2 snapshot)\n");
    }
    line
}

pub(crate) fn format_water_diagnostics(state: &AppState) -> String {
    let bounds = state.map_bounds;
    let (lake_n, lake_cells, endorheic) = lake_catalog_stats(&state.lakes);
    let (segments, channel_cells) = channel_topology_counts(state);
    let land = land_cell_count(&state.elevation, bounds);
    let precip = state
        .precip_input_state
        .as_deref()
        .or(match state.precip_layer_present {
            Some(true) => Some("present (unclassified)"),
            Some(false) => Some("missing"),
            None => Some("unknown"),
        })
        .unwrap_or("unknown");
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
    if state.rivers_compatibility_projection {
        out.push_str("rivers catalog: compatibility projection\n");
    }
    out.push_str(&format_channel_topology_line(
        segments,
        channel_cells,
        state.rivers_read_only,
    ));
    if !state.named_rivers.is_empty() {
        out.push_str(&format!("named rivers: {}\n", state.named_rivers.len()));
        for river in &state.named_rivers {
            out.push_str(&format!(
                "  #{id} \"{name}\" → segments {:?}\n",
                river.segment_ids,
                id = river.id,
                name = river.name
            ));
        }
    }
    let ambiguous: Vec<_> = state
        .name_migration
        .iter()
        .filter(|report| report.ambiguous)
        .collect();
    if !ambiguous.is_empty() {
        out.push_str(&format!(
            "name migration ambiguous: {} (review required)\n",
            ambiguous.len()
        ));
        for report in ambiguous {
            out.push_str(&format!("  \"{}\"\n", report.name));
        }
    }
    out.push_str(&format!("precip input: {precip}\n"));
    if let Some(source) = &state.precip_source {
        out.push_str(&format!("precip source: {source}\n"));
    }
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

#[cfg(test)]
mod tests {
    use super::format_channel_topology_line;

    #[test]
    fn diagnostics_use_segment_vocabulary() {
        let text = format_channel_topology_line(3, 12, true);
        assert!(text.contains("channels: 3 physical segments · 12 channel cells"));
        assert!(text.contains("rivers authoring: read-only"));
        assert!(!text.contains("rivers: 3"));
    }
}
