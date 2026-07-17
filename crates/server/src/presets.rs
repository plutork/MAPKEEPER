//! Create preset catalog API (N-016…N-018) — values from core only.

use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router};
use mapkeeper_core::spatial::{
    cell_count, create_presets, is_default_preset, map_area_km2, ALPHA_NEIGHBOR_CENTER_DISTANCE_M,
};
use serde::Serialize;

use crate::state::ServerState;

#[derive(Serialize)]
struct PresetCard {
    id: &'static str,
    display_name: &'static str,
    cols: u32,
    rows: u32,
    cells: u32,
    width_m: f64,
    height_m: f64,
    /// Approx km labels for Create cards (UI must not recompute footprint).
    width_km: f64,
    height_km: f64,
    /// Derived hex-sum area (N-017); UI must not recompute.
    area_km2: f64,
    is_default: bool,
    neighbor_center_distance_m: f64,
}

#[derive(Serialize)]
struct PresetsResponse {
    presets: Vec<PresetCard>,
    default_preset_id: &'static str,
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new().route("/api/map-presets", get(list_presets))
}

async fn list_presets() -> Json<PresetsResponse> {
    let presets = create_presets()
        .iter()
        .map(|preset| {
            let cells = cell_count(preset);
            let area = map_area_km2(cells, ALPHA_NEIGHBOR_CENTER_DISTANCE_M);
            PresetCard {
                id: preset.id,
                display_name: preset.display_name,
                cols: preset.cols,
                rows: preset.rows,
                cells,
                width_m: preset.width_m,
                height_m: preset.height_m,
                width_km: (preset.width_m / 1000.0 * 10.0).round() / 10.0,
                height_km: (preset.height_m / 1000.0 * 10.0).round() / 10.0,
                area_km2: (area * 10.0).round() / 10.0,
                is_default: is_default_preset(preset),
                neighbor_center_distance_m: ALPHA_NEIGHBOR_CENTER_DISTANCE_M,
            }
        })
        .collect();
    Json(PresetsResponse {
        presets,
        default_preset_id: mapkeeper_core::spatial::alpha_default_preset().id,
    })
}
