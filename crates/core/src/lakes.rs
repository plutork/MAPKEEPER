//! Lake catalog + dense `lake_id` sync (hydrology-lake-domain-v1).
//!
//! Catalog truth: `map/lakes.json` (`LakeCatalog`). Derived dense layer:
//! `map/layers/lake_id.json` — integer `0` = none, `N` = lake id.

use serde::{Deserialize, Serialize};

use crate::hex::MapBounds;
use crate::layer::{DenseLayer, DenseState, LayerValue, LAKE_ID_LAYER_ID};

pub const LAKE_CATALOG_SCHEMA_VERSION: u32 = 1;
pub const LAKE_CATALOG_FILE: &str = "lakes.json";

/// One lake — unordered set of cell indices (polygon / basin fill).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lake {
    pub id: u32,
    pub cells: Vec<usize>,
    /// Spill outlet when the lake drains (H0 geometry; optional until set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outlet_cell: Option<usize>,
    /// Climatic endorheic flag — generation sets in Todo C; default false.
    #[serde(default)]
    pub endorheic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// On-disk catalog under `map/lakes.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LakeCatalog {
    pub schema_version: u32,
    pub lakes: Vec<Lake>,
    pub next_id: u32,
}

impl Default for LakeCatalog {
    fn default() -> Self {
        Self {
            schema_version: LAKE_CATALOG_SCHEMA_VERSION,
            lakes: Vec::new(),
            next_id: 1,
        }
    }
}

impl LakeCatalog {
    pub fn from_json(raw: &str) -> serde_json::Result<Self> {
        serde_json::from_str(raw)
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LakeError {
    InvalidCell,
    DuplicateCell,
    CellOccupied { lake_id: u32 },
    LakeNotFound,
    EmptyLake,
}

impl std::fmt::Display for LakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LakeError::InvalidCell => write!(f, "cell out of map bounds"),
            LakeError::DuplicateCell => write!(f, "duplicate cell in lake"),
            LakeError::CellOccupied { lake_id } => {
                write!(f, "cell belongs to lake {lake_id}")
            }
            LakeError::LakeNotFound => write!(f, "lake not found"),
            LakeError::EmptyLake => write!(f, "lake has no cells"),
        }
    }
}

/// Validate catalog invariants before persist.
pub fn validate_catalog(catalog: &LakeCatalog, bounds: &MapBounds) -> Result<(), LakeError> {
    let mut global_seen: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    for lake in &catalog.lakes {
        if lake.cells.is_empty() {
            return Err(LakeError::EmptyLake);
        }
        let mut lake_cells = std::collections::HashSet::new();
        for &idx in &lake.cells {
            if idx >= bounds.len() {
                return Err(LakeError::InvalidCell);
            }
            if !lake_cells.insert(idx) {
                return Err(LakeError::DuplicateCell);
            }
            if let Some(&other) = global_seen.get(&idx) {
                return Err(LakeError::CellOccupied { lake_id: other });
            }
            global_seen.insert(idx, lake.id);
        }
        if let Some(outlet) = lake.outlet_cell {
            if outlet >= bounds.len() {
                return Err(LakeError::InvalidCell);
            }
        }
    }
    Ok(())
}

/// Lake id occupying `index`, if any.
pub fn lake_at_cell(catalog: &LakeCatalog, index: usize) -> Option<u32> {
    catalog
        .lakes
        .iter()
        .find(|l| l.cells.contains(&index))
        .map(|l| l.id)
}

/// Rebuild the dense `lake_id` layer from the catalog.
pub fn sync_lake_id_layer(catalog: &LakeCatalog, bounds: &MapBounds) -> DenseLayer {
    let mut layer = DenseLayer::new_integer(LAKE_ID_LAYER_ID, bounds.len());
    for i in 0..bounds.len() {
        layer.set(i, DenseState::Value(LayerValue::Int(0)));
    }
    for lake in &catalog.lakes {
        for &idx in &lake.cells {
            if idx < bounds.len() {
                layer.set(idx, DenseState::Value(LayerValue::Int(lake.id as i32)));
            }
        }
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_bounds() -> MapBounds {
        MapBounds::new(14, 8)
    }

    #[test]
    fn sync_layer_maps_ids() {
        let bounds = small_bounds();
        let mut catalog = LakeCatalog::default();
        catalog.lakes.push(Lake {
            id: 1,
            cells: vec![3, 4, 5],
            outlet_cell: Some(3),
            endorheic: false,
            name: None,
        });
        let layer = sync_lake_id_layer(&catalog, &bounds);
        assert_eq!(layer.int_or(3, -1), 1);
        assert_eq!(layer.int_or(4, -1), 1);
        assert_eq!(layer.int_or(0, -1), 0);
    }

    #[test]
    fn empty_catalog_all_zero() {
        let bounds = small_bounds();
        let catalog = LakeCatalog::default();
        let layer = sync_lake_id_layer(&catalog, &bounds);
        for i in 0..bounds.len() {
            assert_eq!(layer.int_or(i, -1), 0);
        }
    }

    #[test]
    fn catalog_json_roundtrip() {
        let mut catalog = LakeCatalog::default();
        catalog.lakes.push(Lake {
            id: 1,
            cells: vec![10],
            outlet_cell: None,
            endorheic: true,
            name: Some("Basin".into()),
        });
        let json = catalog.to_json_pretty().unwrap();
        assert_eq!(LakeCatalog::from_json(&json).unwrap(), catalog);
    }

    #[test]
    fn rejects_overlapping_lakes() {
        let bounds = small_bounds();
        let catalog = LakeCatalog {
            schema_version: 1,
            next_id: 3,
            lakes: vec![
                Lake {
                    id: 1,
                    cells: vec![1, 2],
                    outlet_cell: None,
                    endorheic: false,
                    name: None,
                },
                Lake {
                    id: 2,
                    cells: vec![2, 3],
                    outlet_cell: None,
                    endorheic: false,
                    name: None,
                },
            ],
        };
        assert!(matches!(
            validate_catalog(&catalog, &bounds),
            Err(LakeError::CellOccupied { .. })
        ));
    }

    #[test]
    fn rejects_out_of_bounds_cell() {
        let bounds = small_bounds();
        let catalog = LakeCatalog {
            schema_version: 1,
            next_id: 2,
            lakes: vec![Lake {
                id: 1,
                cells: vec![bounds.len()],
                outlet_cell: None,
                endorheic: false,
                name: None,
            }],
        };
        assert_eq!(
            validate_catalog(&catalog, &bounds),
            Err(LakeError::InvalidCell)
        );
    }
}
