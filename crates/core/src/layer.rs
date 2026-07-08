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
//! On-disk shape (`map/layers/<id>.json`) is the **dense** [`DenseLayer`]
//! (scale-layers, D-46): cells are addressed by linear index within the map
//! bounds (`core::hex::MapBounds::index_of`), not by `cell_id` strings, and
//! categorical values are palette/dictionary-encoded. The sparse string-keyed
//! model was removed once adapters + web switched over.
//!
//! This module owns the model, (de)serialization and `unknown/none/value`
//! resolution — pure, no filesystem. `server`/`cli` do the actual I/O (D-20).
//! `cell_id` strings stay the external identity (API/profiles/agent); the
//! linear index is the internal storage key.

use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const TERRAIN_LAYER_ID: &str = "terrain";
pub const ELEVATION_LAYER_ID: &str = "elevation";
/// Dense integer layer: 0 = no river, N = river id (river-overlay-layer-v1).
pub const RIVER_ID_LAYER_ID: &str = "river_id";

/// Kind of value a layer stores: `categorical` (palette-encoded strings) or
/// `integer`. Future numeric/enum kinds are additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    Categorical,
    Integer,
}

/// Spatial bounds declared by a map manifest. V0: pointy-top hex rectangle
/// 16:9 (`map-bounds--hex-rectangle-16x9`, D-49).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Bounds {
    HexRectangle { width: i32, height: i32 },
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
    /// Scaffold manifest: `width`×`height` hex rectangle + typed layers.
    pub fn default_v0(width: i32, height: i32) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            bounds: Bounds::HexRectangle {
                width: width.max(1),
                height: height.max(1),
            },
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

    /// A fresh empty layer of `value_type` sized to `bounds`.
    pub fn empty(layer_id: &str, value_type: ValueType, bounds: &crate::hex::MapBounds) -> DenseLayer {
        match value_type {
            ValueType::Categorical => DenseLayer::new_categorical(layer_id, bounds.len()),
            ValueType::Integer => DenseLayer::new_integer(layer_id, bounds.len()),
        }
    }

    /// Read a dense layer from a file's raw contents, or start empty (typed by
    /// `value_type`, sized to `bounds`). `raw == None` means the file is absent;
    /// unparseable contents also fall back to empty (old formats are not kept).
    pub fn read_or_empty(
        raw: Option<&str>,
        layer_id: &str,
        value_type: ValueType,
        bounds: &crate::hex::MapBounds,
    ) -> DenseLayer {
        if let Some(raw) = raw {
            if let Ok(dense) = DenseLayer::from_json(raw) {
                return dense;
            }
        }
        DenseLayer::empty(layer_id, value_type, bounds)
    }
}

/// Wire form of a cell's partial state for the generic layer API — mirrors
/// [`DenseState`] but the value is JSON (string for categorical, number for
/// integer), resolved against the layer's [`ValueType`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum WireCellState {
    Unknown,
    None,
    Value { value: serde_json::Value },
}

impl WireCellState {
    /// Resolve to a [`DenseState`] for `value_type`. `None` when the JSON value
    /// kind does not match the layer (e.g. a string for an integer layer).
    pub fn to_dense(&self, value_type: ValueType) -> Option<DenseState> {
        match self {
            WireCellState::Unknown => Some(DenseState::Unknown),
            WireCellState::None => Some(DenseState::None),
            WireCellState::Value { value } => match value_type {
                ValueType::Categorical => {
                    value.as_str().map(|s| DenseState::Value(LayerValue::Text(s.to_string())))
                }
                ValueType::Integer => {
                    value.as_i64().map(|i| DenseState::Value(LayerValue::Int(i as i32)))
                }
            },
        }
    }

    pub fn from_dense(state: DenseState) -> WireCellState {
        match state {
            DenseState::Unknown => WireCellState::Unknown,
            DenseState::None => WireCellState::None,
            DenseState::Value(LayerValue::Text(v)) => {
                WireCellState::Value { value: serde_json::Value::String(v) }
            }
            DenseState::Value(LayerValue::Int(i)) => {
                WireCellState::Value { value: serde_json::Value::from(i) }
            }
        }
    }
}

/// One cell write in a generic layer batch (`PUT /api/layers/:id/batch`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerCellWrite {
    pub q: i32,
    pub r: i32,
    #[serde(flatten)]
    pub state: WireCellState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_declares_terrain_layer() {
        let manifest = MapManifest::default_v0(14, 8);
        assert_eq!(
            manifest.bounds,
            Bounds::HexRectangle { width: 14, height: 8 }
        );
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
    fn read_or_empty_parses_dense_or_starts_empty() {
        let bounds = MapBounds::new(4, 3);

        // None => empty typed layer sized to bounds.
        let empty = DenseLayer::read_or_empty(None, "terrain", ValueType::Categorical, &bounds);
        assert_eq!(empty.len(), bounds.len());
        assert_eq!(empty.value_type, ValueType::Categorical);

        // Unparseable => empty (old formats are not kept).
        let junk = DenseLayer::read_or_empty(Some("not json"), "elevation", ValueType::Integer, &bounds);
        assert_eq!(junk.value_type, ValueType::Integer);
        assert_eq!(junk.len(), bounds.len());

        // Valid dense round-trips.
        let mut layer = DenseLayer::new_categorical("terrain", bounds.len());
        layer.set(0, DenseState::Value(LayerValue::Text("forest".into())));
        let json = layer.to_json_pretty().unwrap();
        let back = DenseLayer::read_or_empty(Some(&json), "terrain", ValueType::Categorical, &bounds);
        assert_eq!(back, layer);
    }

    #[test]
    fn wire_cell_state_roundtrips_by_value_type() {
        // Categorical text.
        let dense = WireCellState::Value { value: serde_json::json!("forest") }
            .to_dense(ValueType::Categorical)
            .unwrap();
        assert_eq!(dense, DenseState::Value(LayerValue::Text("forest".into())));
        // Integer number.
        let dense = WireCellState::Value { value: serde_json::json!(-3) }
            .to_dense(ValueType::Integer)
            .unwrap();
        assert_eq!(dense, DenseState::Value(LayerValue::Int(-3)));
        // Kind mismatch => None.
        assert!(WireCellState::Value { value: serde_json::json!("x") }
            .to_dense(ValueType::Integer)
            .is_none());
        // from_dense inverse for the value cases.
        assert!(matches!(
            WireCellState::from_dense(DenseState::Value(LayerValue::Int(7))),
            WireCellState::Value { .. }
        ));
        assert!(matches!(WireCellState::from_dense(DenseState::None), WireCellState::None));
    }
}
