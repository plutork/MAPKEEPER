//! Layer-first map state (Hex Map Model Foundation, D-36).
//!
//! The map is machine-readable world state split into **layers**, kept
//! separate from author-facing `profiles/` (which are NOT a layer). Each
//! layer stores one aspect of world state, anchored by `cell_id`.
//!
//! **Partial state** is first-class — a cell may be `unknown` / `none` /
//! `value` per layer:
//!
//! - `unknown` — not filled / not decided → the cell key is simply absent.
//! - `none` — explicitly absent → stored as `{ "state": "none" }`.
//! - `value` — a concrete known value → `{ "state": "value", "value": <T> }`.
//!
//! On-disk shape (`map/layers/<id>.json`, sparse):
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "layer_id": "terrain",
//!   "value_type": "categorical",
//!   "cells": {
//!     "world.hex.q0.r0": { "state": "value", "value": "forest" },
//!     "world.hex.q1.r0": { "state": "none" }
//!   }
//! }
//! ```
//!
//! This module owns the model, (de)serialization and `unknown/none/value`
//! resolution — pure, no filesystem. `server`/`cli` do the actual I/O (D-20).
//!
//! V0 proof slice: only `terrain`, a `categorical` (string-valued) layer. No
//! generators, validators, or other layers yet — but the shape is designed so
//! those are additive later (future generators are local product tools over
//! these layers, not AI runtime).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const TERRAIN_LAYER_ID: &str = "terrain";
pub const ELEVATION_LAYER_ID: &str = "elevation";

/// Kind of value a layer stores. Only `categorical` (string) exists in the V0
/// proof slice; numeric/enum kinds (elevation, …) are future additions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    Categorical,
    Integer,
}

/// A stored cell entry — only the two *stored* partial states (`none` /
/// `value`). `unknown` is represented by the **absence** of a key, so it
/// never appears on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Entry {
    None,
    Value { value: String },
}

/// Resolved partial state for a cell in a layer — the full `unknown / none /
/// value` trio. Used at the API boundary (get/set) and by callers; `Unknown`
/// on a `set` clears the cell (removes the key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum CellState {
    Unknown,
    None,
    Value { value: String },
}

impl CellState {
    pub fn value(v: impl Into<String>) -> Self {
        CellState::Value { value: v.into() }
    }
}

/// A single map layer: sparse `cell_id -> Entry`, with a typed header. Missing
/// keys resolve to `unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub schema_version: u32,
    pub layer_id: String,
    pub value_type: ValueType,
    #[serde(default)]
    pub cells: BTreeMap<String, Entry>,
}

impl Layer {
    pub fn new(layer_id: impl Into<String>, value_type: ValueType) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            layer_id: layer_id.into(),
            value_type,
            cells: BTreeMap::new(),
        }
    }

    /// A fresh, empty `terrain` layer (categorical) — the V0 proof layer.
    pub fn terrain() -> Self {
        Self::new(TERRAIN_LAYER_ID, ValueType::Categorical)
    }

    /// Resolve a cell to its full partial state. Absent key => `Unknown`.
    pub fn state(&self, cell_id: &str) -> CellState {
        match self.cells.get(cell_id) {
            None => CellState::Unknown,
            Some(Entry::None) => CellState::None,
            Some(Entry::Value { value }) => CellState::Value { value: value.clone() },
        }
    }

    /// Set a cell's partial state. `Unknown` removes the key (back to not
    /// stored); `None`/`Value` store the corresponding entry.
    pub fn set(&mut self, cell_id: impl Into<String>, state: CellState) {
        let cell_id = cell_id.into();
        match state {
            CellState::Unknown => {
                self.cells.remove(&cell_id);
            }
            CellState::None => {
                self.cells.insert(cell_id, Entry::None);
            }
            CellState::Value { value } => {
                self.cells.insert(cell_id, Entry::Value { value });
            }
        }
    }

    /// Parse a layer from JSON.
    pub fn from_json(raw: &str) -> serde_json::Result<Layer> {
        serde_json::from_str(raw)
    }

    /// Serialize a layer to pretty JSON (stable key order via `BTreeMap`).
    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// Spatial bounds declared by a map manifest. Only a radial hexagon centered
/// at the origin exists in V0 (matches [`crate::hex::MapBounds`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Bounds {
    HexRadius { radius: i32 },
}

/// A layer declared by the map manifest — its id, value kind, and the file
/// (relative to `map/`) that stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerRef {
    pub layer_id: String,
    pub value_type: ValueType,
    pub file: String,
}

/// `map/manifest.json` — the machine-readable index of a world's map state:
/// bounds + the set of declared layers. The source of truth for map logic
/// (renderer is a projection of this model, not the other way round).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapManifest {
    pub schema_version: u32,
    pub bounds: Bounds,
    #[serde(default)]
    pub layers: Vec<LayerRef>,
}

impl MapManifest {
    /// The V0 scaffold manifest: radius-`radius` bounds + typed layers.
    pub fn default_v0(radius: i32) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            bounds: Bounds::HexRadius { radius },
            layers: vec![
                LayerRef {
                    layer_id: TERRAIN_LAYER_ID.to_string(),
                    value_type: ValueType::Categorical,
                    file: format!("layers/{TERRAIN_LAYER_ID}.json"),
                },
                LayerRef {
                    layer_id: ELEVATION_LAYER_ID.to_string(),
                    value_type: ValueType::Integer,
                    file: format!("layers/{ELEVATION_LAYER_ID}.json"),
                },
            ],
        }
    }

    pub fn from_json(raw: &str) -> serde_json::Result<MapManifest> {
        serde_json::from_str(raw)
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_none_value_resolution() {
        let mut layer = Layer::terrain();
        assert_eq!(layer.state("w.hex.q0.r0"), CellState::Unknown);

        layer.set("w.hex.q0.r0", CellState::value("forest"));
        layer.set("w.hex.q1.r0", CellState::None);

        assert_eq!(layer.state("w.hex.q0.r0"), CellState::value("forest"));
        assert_eq!(layer.state("w.hex.q1.r0"), CellState::None);
        assert_eq!(layer.state("w.hex.q9.r9"), CellState::Unknown);
    }

    #[test]
    fn setting_unknown_clears_the_cell() {
        let mut layer = Layer::terrain();
        layer.set("w.hex.q0.r0", CellState::value("water"));
        assert!(layer.cells.contains_key("w.hex.q0.r0"));
        layer.set("w.hex.q0.r0", CellState::Unknown);
        assert!(!layer.cells.contains_key("w.hex.q0.r0"));
        assert_eq!(layer.state("w.hex.q0.r0"), CellState::Unknown);
    }

    #[test]
    fn serializes_to_authored_shape() {
        let mut layer = Layer::terrain();
        layer.set("world.hex.q0.r0", CellState::value("forest"));
        layer.set("world.hex.q1.r0", CellState::None);
        let json = layer.to_json_pretty().unwrap();

        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"layer_id\": \"terrain\""));
        assert!(json.contains("\"value_type\": \"categorical\""));
        assert!(json.contains("\"state\": \"value\""));
        assert!(json.contains("\"value\": \"forest\""));
        assert!(json.contains("\"state\": \"none\""));

        // Round trips.
        let back = Layer::from_json(&json).unwrap();
        assert_eq!(back, layer);
    }

    #[test]
    fn parses_sparse_file_with_missing_cells_as_unknown() {
        let raw = r#"{
            "schema_version": 1,
            "layer_id": "terrain",
            "value_type": "categorical",
            "cells": {
                "w.hex.q0.r0": { "state": "value", "value": "mountain" }
            }
        }"#;
        let layer = Layer::from_json(raw).unwrap();
        assert_eq!(layer.state("w.hex.q0.r0"), CellState::value("mountain"));
        assert_eq!(layer.state("w.hex.q5.r5"), CellState::Unknown);
    }

    #[test]
    fn manifest_declares_terrain_layer() {
        let manifest = MapManifest::default_v0(6);
        assert_eq!(manifest.bounds, Bounds::HexRadius { radius: 6 });
        assert_eq!(manifest.layers.len(), 2);
        assert_eq!(manifest.layers[0].layer_id, TERRAIN_LAYER_ID);
        assert_eq!(manifest.layers[0].file, "layers/terrain.json");
        assert_eq!(manifest.layers[1].layer_id, ELEVATION_LAYER_ID);
        assert_eq!(manifest.layers[1].value_type, ValueType::Integer);
        assert_eq!(manifest.layers[1].file, "layers/elevation.json");

        let json = manifest.to_json_pretty().unwrap();
        assert_eq!(MapManifest::from_json(&json).unwrap(), manifest);
    }
}
