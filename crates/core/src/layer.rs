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

// ---------------------------------------------------------------------------
// scale-layers (D-46): generic dense, index-addressed typed layer.
//
// Evolution of the sparse string-keyed model above (kept intact until the
// adapters switch over in the `scale-layers--adapters` slice). Under the fixed
// ~100k ceiling the map bounds are known, so a layer is stored as dense columns
// addressed by cell index (see `core::hex::MapBounds::index_of`), not by a map
// of `cell_id` strings. Categorical values are palette/dictionary-encoded so a
// fully-painted terrain layer is a byte array of small codes, not repeated
// strings. Partial state (unknown/none/value, D-36) is preserved.
//
// This model is bounds-agnostic itself: it just holds `cell_count` slots; the
// caller maps `(q,r) <-> index` through `MapBounds`. Profiles (D-22) are
// unaffected — they stay per-cell JSON.
// ---------------------------------------------------------------------------

/// Format version for the dense on-disk layer shape (distinct from the sparse
/// `CURRENT_SCHEMA_VERSION == 1`).
pub const DENSE_SCHEMA_VERSION: u32 = 2;

/// State tag stored per cell in a dense layer. Kept as a small integer on disk.
mod state_tag {
    pub const UNKNOWN: u8 = 0;
    pub const NONE: u8 = 1;
    pub const VALUE: u8 = 2;
}

/// A concrete value carried by a dense layer cell — variant matches the layer's
/// [`ValueType`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerValue {
    Text(String),
    Int(i32),
}

/// Resolved partial state for a dense-layer cell (generic over value kind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenseState {
    Unknown,
    None,
    Value(LayerValue),
}

/// A dense, index-addressed layer: one state slot per cell plus a typed value
/// column. `states[i]` is one of `state_tag::{UNKNOWN,NONE,VALUE}`; the value
/// column (`codes` for categorical, `values` for integer) is only meaningful
/// where the state is `VALUE`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenseLayer {
    pub schema_version: u32,
    pub layer_id: String,
    pub value_type: ValueType,
    pub cell_count: usize,
    pub states: Vec<u8>,
    /// Categorical dictionary — distinct values; `codes[i]` indexes into this.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette: Vec<String>,
    /// Categorical value column (palette index per cell).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codes: Vec<u32>,
    /// Integer value column.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<i32>,
}

impl DenseLayer {
    /// A fresh categorical layer with `cell_count` unknown cells.
    pub fn new_categorical(layer_id: impl Into<String>, cell_count: usize) -> Self {
        Self {
            schema_version: DENSE_SCHEMA_VERSION,
            layer_id: layer_id.into(),
            value_type: ValueType::Categorical,
            cell_count,
            states: vec![state_tag::UNKNOWN; cell_count],
            palette: Vec::new(),
            codes: vec![0; cell_count],
            values: Vec::new(),
        }
    }

    /// A fresh integer layer with `cell_count` unknown cells.
    pub fn new_integer(layer_id: impl Into<String>, cell_count: usize) -> Self {
        Self {
            schema_version: DENSE_SCHEMA_VERSION,
            layer_id: layer_id.into(),
            value_type: ValueType::Integer,
            cell_count,
            states: vec![state_tag::UNKNOWN; cell_count],
            palette: Vec::new(),
            codes: Vec::new(),
            values: vec![0; cell_count],
        }
    }

    pub fn len(&self) -> usize {
        self.cell_count
    }

    pub fn is_empty(&self) -> bool {
        self.cell_count == 0
    }

    /// Resolve the partial state at `index`. Out-of-range => `Unknown`.
    pub fn state(&self, index: usize) -> DenseState {
        if index >= self.cell_count {
            return DenseState::Unknown;
        }
        match self.states[index] {
            state_tag::NONE => DenseState::None,
            state_tag::VALUE => match self.value_type {
                ValueType::Categorical => {
                    let code = self.codes[index] as usize;
                    match self.palette.get(code) {
                        Some(v) => DenseState::Value(LayerValue::Text(v.clone())),
                        None => DenseState::Unknown,
                    }
                }
                ValueType::Integer => DenseState::Value(LayerValue::Int(self.values[index])),
            },
            _ => DenseState::Unknown,
        }
    }

    /// Set the partial state at `index`. Mismatched value kinds are ignored
    /// (defensive — callers should match the layer's `value_type`).
    pub fn set(&mut self, index: usize, state: DenseState) {
        if index >= self.cell_count {
            return;
        }
        match state {
            DenseState::Unknown => self.states[index] = state_tag::UNKNOWN,
            DenseState::None => self.states[index] = state_tag::NONE,
            DenseState::Value(LayerValue::Text(v)) if self.value_type == ValueType::Categorical => {
                let code = self.intern(&v);
                self.states[index] = state_tag::VALUE;
                self.codes[index] = code;
            }
            DenseState::Value(LayerValue::Int(v)) if self.value_type == ValueType::Integer => {
                self.states[index] = state_tag::VALUE;
                self.values[index] = v;
            }
            DenseState::Value(_) => { /* kind mismatch — ignore */ }
        }
    }

    /// Integer value at `index`, or `default` when the cell is not a concrete
    /// integer value (unknown/none). Basis for derived layers like hydro.
    pub fn int_or(&self, index: usize, default: i32) -> i32 {
        match self.state(index) {
            DenseState::Value(LayerValue::Int(v)) => v,
            _ => default,
        }
    }

    /// Intern a categorical value into the palette, returning its code.
    fn intern(&mut self, value: &str) -> u32 {
        if let Some(pos) = self.palette.iter().position(|v| v == value) {
            return pos as u32;
        }
        self.palette.push(value.to_string());
        (self.palette.len() - 1) as u32
    }

    pub fn from_json(raw: &str) -> serde_json::Result<DenseLayer> {
        serde_json::from_str(raw)
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Migrate a sparse categorical [`Layer`] (e.g. `terrain`, schema v1) into
    /// the dense model, mapping each `cell_id` to its index via `bounds`.
    /// Cells outside the bounds are dropped (defensive).
    pub fn from_sparse_layer(old: &Layer, bounds: &crate::hex::MapBounds) -> DenseLayer {
        let mut dense = DenseLayer::new_categorical(old.layer_id.clone(), bounds.len());
        for (cell_id, entry) in &old.cells {
            let Some(index) = index_for(cell_id, bounds) else { continue };
            match entry {
                Entry::None => dense.set(index, DenseState::None),
                Entry::Value { value } => {
                    dense.set(index, DenseState::Value(LayerValue::Text(value.clone())))
                }
            }
        }
        dense
    }

    /// Migrate a sparse [`crate::hydro::ElevationLayer`] (schema v1) into the
    /// dense integer model. Old semantics had no `unknown`: a missing key meant
    /// the default land elevation. Here stored cells become concrete values and
    /// absent cells stay `unknown`; read them back with
    /// `int_or(index, DEFAULT_LAND_ELEVATION)` to preserve the old default.
    pub fn from_sparse_elevation(
        old: &crate::hydro::ElevationLayer,
        bounds: &crate::hex::MapBounds,
    ) -> DenseLayer {
        let mut dense = DenseLayer::new_integer(old.layer_id.clone(), bounds.len());
        for (cell_id, elevation) in &old.cells {
            let Some(index) = index_for(cell_id, bounds) else { continue };
            dense.set(index, DenseState::Value(LayerValue::Int(*elevation as i32)));
        }
        dense
    }
}

/// Resolve a `cell_id` string to its dense index within `bounds`.
fn index_for(cell_id: &str, bounds: &crate::hex::MapBounds) -> Option<usize> {
    let cell = crate::cell_id::CellId::parse(cell_id)?;
    bounds.index_of(crate::hex::Axial::new(cell.q, cell.r))
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

    // scale-layers (D-46): dense typed-layer tests.
    use crate::hex::MapBounds;

    #[test]
    fn dense_categorical_partial_states() {
        let mut layer = DenseLayer::new_categorical("terrain", 5);
        assert_eq!(layer.state(0), DenseState::Unknown);

        layer.set(0, DenseState::Value(LayerValue::Text("forest".into())));
        layer.set(1, DenseState::Value(LayerValue::Text("forest".into())));
        layer.set(2, DenseState::None);

        assert_eq!(layer.state(0), DenseState::Value(LayerValue::Text("forest".into())));
        assert_eq!(layer.state(1), DenseState::Value(LayerValue::Text("forest".into())));
        assert_eq!(layer.state(2), DenseState::None);
        assert_eq!(layer.state(3), DenseState::Unknown);
        // palette dictionary-encodes the repeated value once.
        assert_eq!(layer.palette, vec!["forest".to_string()]);
    }

    #[test]
    fn dense_set_unknown_clears() {
        let mut layer = DenseLayer::new_categorical("terrain", 3);
        layer.set(0, DenseState::Value(LayerValue::Text("water".into())));
        assert_eq!(layer.state(0), DenseState::Value(LayerValue::Text("water".into())));
        layer.set(0, DenseState::Unknown);
        assert_eq!(layer.state(0), DenseState::Unknown);
    }

    #[test]
    fn dense_integer_values_and_int_or() {
        let mut layer = DenseLayer::new_integer("elevation", 4);
        layer.set(0, DenseState::Value(LayerValue::Int(5)));
        layer.set(1, DenseState::Value(LayerValue::Int(-2)));
        assert_eq!(layer.int_or(0, 1), 5);
        assert_eq!(layer.int_or(1, 1), -2);
        // unknown falls back to the supplied default (old land default = 1).
        assert_eq!(layer.int_or(2, 1), 1);
    }

    #[test]
    fn dense_kind_mismatch_is_ignored() {
        let mut layer = DenseLayer::new_integer("elevation", 2);
        layer.set(0, DenseState::Value(LayerValue::Text("nope".into())));
        assert_eq!(layer.state(0), DenseState::Unknown);
    }

    #[test]
    fn dense_json_roundtrip() {
        let mut layer = DenseLayer::new_categorical("terrain", 3);
        layer.set(0, DenseState::Value(LayerValue::Text("mountain".into())));
        layer.set(1, DenseState::None);
        let json = layer.to_json_pretty().unwrap();
        assert!(json.contains("\"schema_version\": 2"));
        assert!(json.contains("\"value_type\": \"categorical\""));
        let back = DenseLayer::from_json(&json).unwrap();
        assert_eq!(back, layer);
    }

    #[test]
    fn migrate_sparse_terrain_preserves_values() {
        let bounds = MapBounds::new(2);
        let mut sparse = Layer::terrain();
        sparse.set("w.hex.q0.r0", CellState::value("forest"));
        sparse.set("w.hex.q1.r0", CellState::None);

        let dense = DenseLayer::from_sparse_layer(&sparse, &bounds);
        let i0 = bounds.index_of(crate::hex::Axial::new(0, 0)).unwrap();
        let i1 = bounds.index_of(crate::hex::Axial::new(1, 0)).unwrap();
        assert_eq!(dense.state(i0), DenseState::Value(LayerValue::Text("forest".into())));
        assert_eq!(dense.state(i1), DenseState::None);
        assert_eq!(dense.len(), bounds.len());
    }

    #[test]
    fn migrate_sparse_elevation_preserves_default_semantics() {
        use crate::hydro::{ElevationLayer, DEFAULT_LAND_ELEVATION};
        let bounds = MapBounds::new(2);
        let mut sparse = ElevationLayer::new();
        sparse.set("w.hex.q0.r0", -3);

        let dense = DenseLayer::from_sparse_elevation(&sparse, &bounds);
        let i0 = bounds.index_of(crate::hex::Axial::new(0, 0)).unwrap();
        let i1 = bounds.index_of(crate::hex::Axial::new(1, 0)).unwrap();
        assert_eq!(dense.int_or(i0, DEFAULT_LAND_ELEVATION as i32), -3);
        // absent cell keeps the old "missing = default land" behavior via int_or.
        assert_eq!(dense.int_or(i1, DEFAULT_LAND_ELEVATION as i32), DEFAULT_LAND_ELEVATION as i32);
    }

    #[test]
    fn migrate_drops_out_of_bounds_cells() {
        let bounds = MapBounds::new(1);
        let mut sparse = Layer::terrain();
        sparse.set("w.hex.q9.r9", CellState::value("forest"));
        let dense = DenseLayer::from_sparse_layer(&sparse, &bounds);
        // nothing landed in-bounds
        assert!((0..dense.len()).all(|i| dense.state(i) == DenseState::Unknown));
    }
}
