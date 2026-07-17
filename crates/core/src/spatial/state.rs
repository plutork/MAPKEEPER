use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::field::GridField;
use super::frame::WorldFrame;
use super::geometry::GeometryStub;
use super::grid::HexGrid;
use super::presets::ALPHA_NEIGHBOR_CENTER_DISTANCE_M;

pub const SPATIAL_STATE_RELATIVE: &str = "spatial/state.json";

/// Persisted spatial content — frame/grid mirror immutable toml config (N-014).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialState {
    pub version: u32,
    /// Content revision for OCC (N-025). Missing on disk → 0.
    #[serde(default)]
    pub revision: u64,
    pub frame: WorldFrame,
    pub grid: HexGrid,
    pub field: GridField,
    pub geometry_stub: GeometryStub,
}

pub fn default_spatial_state() -> SpatialState {
    let frame = WorldFrame::default_probe();
    let grid = HexGrid::default_probe();
    let geometry_stub = GeometryStub::default_probe(&frame, &grid);
    SpatialState {
        version: 1,
        revision: 0,
        frame,
        grid,
        field: GridField::default_relief(),
        geometry_stub,
    }
}

impl SpatialState {
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(raw: &str) -> Result<Self, serde_json::Error> {
        let value: Value = serde_json::from_str(raw)?;
        let mut state = migrate_spatial_value(value)?;
        state.field.normalize_author_field();
        Ok(state)
    }

    pub fn assert_no_screen_keys(raw: &str) -> Result<(), String> {
        for forbidden in ["screen_", "viewport", "camera", "zoom", "pan_x", "pan_y"] {
            if raw.contains(forbidden) {
                return Err(format!("persisted spatial state must not contain `{forbidden}`"));
            }
        }
        Ok(())
    }

    /// Apply immutable spatial config (toml wins over state mirror).
    pub fn apply_spatial_config(&mut self, config: &crate::world::SpatialConfig) {
        self.frame.id = "world".to_string();
        self.frame.origin_x = config.origin_x_m;
        self.frame.origin_y = config.origin_y_m;
        self.grid.id = config.grid_id.clone();
        self.grid.neighbor_center_distance_m = config.neighbor_center_distance_m;
        self.grid.width = config.cols;
        self.grid.height = config.rows;
    }

    pub fn refresh_geometry_stub_from_probe(&mut self) {
        self.geometry_stub = GeometryStub::default_probe(&self.frame, &self.grid);
    }

    /// Bump content revision after a successful durable mutation (N-025).
    pub fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1).max(1);
    }
}

fn migrate_spatial_value(mut value: Value) -> Result<SpatialState, serde_json::Error> {
    if let Some(grid) = value.get_mut("grid").and_then(|g| g.as_object_mut()) {
        if !grid.contains_key("neighbor_center_distance_m") {
            // One-time abstract probe → metric cutover (axial relief kept).
            let legacy = grid
                .remove("cell_size")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            let distance = if (legacy - 1.0).abs() < 1e-9 {
                ALPHA_NEIGHBOR_CENTER_DISTANCE_M
            } else {
                // Already-metric-ish value stored under old key.
                legacy
            };
            grid.insert(
                "neighbor_center_distance_m".into(),
                Value::from(distance),
            );
        } else {
            grid.remove("cell_size");
        }
    }
    if let Some(frame) = value.get_mut("frame").and_then(|f| f.as_object_mut()) {
        frame.remove("unit_scale");
    }
    let mut state: SpatialState = serde_json::from_value(value)?;
    // Rebuild stub so world meters match grid after cutover.
    if state.geometry_stub.id == "probe" {
        state.refresh_geometry_stub_from_probe();
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_round_trip_without_screen() {
        let state = default_spatial_state();
        let raw = state.to_json_pretty().unwrap();
        SpatialState::assert_no_screen_keys(&raw).unwrap();
        assert!(raw.contains("neighbor_center_distance_m"));
        assert!(!raw.contains("cell_size"));
        assert!(!raw.contains("unit_scale"));
        let loaded = SpatialState::from_json(&raw).unwrap();
        assert_eq!(loaded, state);
        assert_eq!(loaded.grid.id, "primary");
        assert_eq!(loaded.field.id, "relief");
    }

    #[test]
    fn loads_legacy_mark_as_relief() {
        let mut state = default_spatial_state();
        state.field.id = "mark".to_string();
        let raw = state.to_json_pretty().unwrap();
        let loaded = SpatialState::from_json(&raw).unwrap();
        assert_eq!(loaded.field.id, "relief");
    }

    #[test]
    fn migrates_abstract_cell_size_to_metric() {
        let raw = r#"{
          "version": 1,
          "frame": { "id": "world", "origin_x": 0.0, "origin_y": 0.0, "unit_scale": 1.0 },
          "grid": { "id": "primary", "cell_size": 1.0, "width": 12, "height": 8 },
          "field": { "id": "relief", "cells": { "0,0": 3 } },
          "geometry_stub": { "id": "probe", "points": [[0.0, 0.0]] }
        }"#;
        let loaded = SpatialState::from_json(raw).unwrap();
        assert_eq!(
            loaded.grid.neighbor_center_distance_m,
            ALPHA_NEIGHBOR_CENTER_DISTANCE_M
        );
        assert_eq!(loaded.field.cells.get("0,0").copied(), Some(3));
        assert!(!loaded.to_json_pretty().unwrap().contains("cell_size"));
    }
}
