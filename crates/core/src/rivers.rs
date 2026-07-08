//! River catalog + dense `river_id` sync (river-overlay-layer-v1, D-54).
//!
//! Catalog truth: `map/rivers.json` (`RiverCatalog`). Derived dense layer:
//! `map/layers/river_id.json` — integer `0` = none, `N` = river id.

use serde::{Deserialize, Serialize};

use crate::hex::{Axial, MapBounds};
use crate::layer::{DenseLayer, DenseState, LayerValue, RIVER_ID_LAYER_ID};

pub const RIVER_CATALOG_SCHEMA_VERSION: u32 = 1;
pub const RIVER_CATALOG_FILE: &str = "rivers.json";

/// One open polyline river — ordered linear cell indices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct River {
    pub id: u32,
    pub cells: Vec<usize>,
    #[serde(default)]
    pub source: usize,
    #[serde(default)]
    pub mouth: usize,
    /// Main stem id; equals `id` when this river is the stem (D-55).
    #[serde(default)]
    pub parent: u32,
    /// River-system root id (D-55).
    #[serde(default)]
    pub basin: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// On-disk catalog under `map/rivers.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiverCatalog {
    pub schema_version: u32,
    pub rivers: Vec<River>,
    pub next_id: u32,
}

impl Default for RiverCatalog {
    fn default() -> Self {
        Self {
            schema_version: RIVER_CATALOG_SCHEMA_VERSION,
            rivers: Vec::new(),
            next_id: 1,
        }
    }
}

impl RiverCatalog {
    pub fn from_json(raw: &str) -> serde_json::Result<Self> {
        serde_json::from_str(raw)
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiverError {
    InvalidCell,
    NotNeighbor,
    DuplicateCell,
    CellOccupied { river_id: u32 },
    RiverNotFound,
    EmptyRiver,
}

impl std::fmt::Display for RiverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiverError::InvalidCell => write!(f, "cell out of map bounds"),
            RiverError::NotNeighbor => write!(f, "new cell must neighbor the previous one"),
            RiverError::DuplicateCell => write!(f, "cell already in this river"),
            RiverError::CellOccupied { river_id } => {
                write!(f, "cell belongs to river {river_id}")
            }
            RiverError::RiverNotFound => write!(f, "river not found"),
            RiverError::EmptyRiver => write!(f, "river has no cells"),
        }
    }
}

/// River id occupying `index`, if any.
pub fn river_at_cell(catalog: &RiverCatalog, index: usize) -> Option<u32> {
    catalog
        .rivers
        .iter()
        .find(|r| r.cells.contains(&index))
        .map(|r| r.id)
}

fn cell_occupied_by_other(
    catalog: &RiverCatalog,
    index: usize,
    except: Option<u32>,
) -> Option<u32> {
    catalog.rivers.iter().find_map(|r| {
        if except == Some(r.id) {
            return None;
        }
        r.cells.contains(&index).then_some(r.id)
    })
}

fn validate_index(bounds: &MapBounds, index: usize) -> Result<(), RiverError> {
    if index >= bounds.len() {
        return Err(RiverError::InvalidCell);
    }
    Ok(())
}

fn neighbors(bounds: &MapBounds, index: usize) -> impl Iterator<Item = usize> + '_ {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
}

/// Start a new river with one cell; returns the allocated id.
pub fn create_river(
    catalog: &mut RiverCatalog,
    bounds: &MapBounds,
    index: usize,
) -> Result<u32, RiverError> {
    validate_index(bounds, index)?;
    if let Some(other) = cell_occupied_by_other(catalog, index, None) {
        return Err(RiverError::CellOccupied { river_id: other });
    }
    let id = catalog.next_id;
    catalog.next_id = catalog.next_id.saturating_add(1).max(2);
    catalog.rivers.push(River {
        id,
        cells: vec![index],
        source: index,
        mouth: index,
        parent: id,
        basin: id,
        name: None,
    });
    Ok(id)
}

/// Append `index` to an existing river chain.
pub fn append_cell(
    catalog: &mut RiverCatalog,
    bounds: &MapBounds,
    river_id: u32,
    index: usize,
) -> Result<(), RiverError> {
    validate_index(bounds, index)?;
    let pos = catalog
        .rivers
        .iter()
        .position(|r| r.id == river_id)
        .ok_or(RiverError::RiverNotFound)?;
    let river = &catalog.rivers[pos];
    if river.cells.contains(&index) {
        return Err(RiverError::DuplicateCell);
    }
    if let Some(other) = cell_occupied_by_other(catalog, index, Some(river_id)) {
        return Err(RiverError::CellOccupied { river_id: other });
    }
    let Some(&last) = river.cells.last() else {
        return Err(RiverError::EmptyRiver);
    };
    let is_neighbor = neighbors(bounds, last).any(|n| n == index);
    if !is_neighbor {
        return Err(RiverError::NotNeighbor);
    }
    catalog.rivers[pos].cells.push(index);
    catalog.rivers[pos].mouth = index;
    Ok(())
}

/// Remove the last cell; drops the river when empty.
pub fn pop_last_cell(catalog: &mut RiverCatalog, river_id: u32) -> Result<(), RiverError> {
    let pos = catalog
        .rivers
        .iter()
        .position(|r| r.id == river_id)
        .ok_or(RiverError::RiverNotFound)?;
    if catalog.rivers[pos].cells.is_empty() {
        return Err(RiverError::EmptyRiver);
    }
    catalog.rivers[pos].cells.pop();
    if catalog.rivers[pos].cells.is_empty() {
        catalog.rivers.remove(pos);
    }
    Ok(())
}

/// Delete a river entirely.
pub fn delete_river(catalog: &mut RiverCatalog, river_id: u32) -> Result<(), RiverError> {
    let pos = catalog
        .rivers
        .iter()
        .position(|r| r.id == river_id)
        .ok_or(RiverError::RiverNotFound)?;
    catalog.rivers.remove(pos);
    Ok(())
}

/// Rebuild the dense `river_id` layer from the catalog.
pub fn sync_river_id_layer(catalog: &RiverCatalog, bounds: &MapBounds) -> DenseLayer {
    let mut layer = DenseLayer::new_integer(RIVER_ID_LAYER_ID, bounds.len());
    for i in 0..bounds.len() {
        layer.set(i, DenseState::Value(LayerValue::Int(0)));
    }
    for river in &catalog.rivers {
        for &idx in &river.cells {
            if idx < bounds.len() {
                layer.set(idx, DenseState::Value(LayerValue::Int(river.id as i32)));
            }
        }
    }
    layer
}

/// Resolve `(q,r)` to a linear index.
pub fn cell_index(bounds: &MapBounds, q: i32, r: i32) -> Result<usize, RiverError> {
    bounds
        .index_of(Axial::new(q, r))
        .ok_or(RiverError::InvalidCell)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_bounds() -> MapBounds {
        MapBounds::new(14, 8)
    }

    #[test]
    fn create_and_append_neighbor_chain() {
        let bounds = small_bounds();
        let start = bounds.index_of(Axial::new(2, 0)).unwrap();
        let mut catalog = RiverCatalog::default();
        let id = create_river(&mut catalog, &bounds, start).unwrap();
        let n1 = neighbors(&bounds, start).next().unwrap();
        append_cell(&mut catalog, &bounds, id, n1).unwrap();
        assert_eq!(catalog.rivers[0].cells, vec![start, n1]);
    }

    #[test]
    fn rejects_non_neighbor_append() {
        let bounds = small_bounds();
        let a = bounds.index_of(Axial::new(0, 0)).unwrap();
        let b = bounds.index_of(Axial::new(3, 0)).unwrap();
        let mut catalog = RiverCatalog::default();
        let id = create_river(&mut catalog, &bounds, a).unwrap();
        assert_eq!(
            append_cell(&mut catalog, &bounds, id, b),
            Err(RiverError::NotNeighbor)
        );
    }

    #[test]
    fn rejects_cell_in_two_rivers() {
        let bounds = small_bounds();
        let a = bounds.index_of(Axial::new(1, 0)).unwrap();
        let b = neighbors(&bounds, a).next().unwrap();
        let mut catalog = RiverCatalog::default();
        let id1 = create_river(&mut catalog, &bounds, a).unwrap();
        append_cell(&mut catalog, &bounds, id1, b).unwrap();
        assert_eq!(
            create_river(&mut catalog, &bounds, b),
            Err(RiverError::CellOccupied { river_id: id1 })
        );
    }

    #[test]
    fn sync_layer_maps_ids() {
        let bounds = small_bounds();
        let a = bounds.index_of(Axial::new(0, 0)).unwrap();
        let b = neighbors(&bounds, a).next().unwrap();
        let mut catalog = RiverCatalog::default();
        let id = create_river(&mut catalog, &bounds, a).unwrap();
        append_cell(&mut catalog, &bounds, id, b).unwrap();
        let layer = sync_river_id_layer(&catalog, &bounds);
        assert_eq!(layer.int_or(a, -1), id as i32);
        assert_eq!(layer.int_or(b, -1), id as i32);
        assert_eq!(layer.int_or(0, -1), 0);
    }

    #[test]
    fn pop_removes_empty_river() {
        let bounds = small_bounds();
        let a = bounds.index_of(Axial::new(0, 0)).unwrap();
        let mut catalog = RiverCatalog::default();
        let id = create_river(&mut catalog, &bounds, a).unwrap();
        pop_last_cell(&mut catalog, id).unwrap();
        assert!(catalog.rivers.is_empty());
    }

    #[test]
    fn catalog_json_roundtrip() {
        let mut catalog = RiverCatalog::default();
        let bounds = small_bounds();
        let a = bounds.index_of(Axial::new(1, 1)).unwrap();
        create_river(&mut catalog, &bounds, a).unwrap();
        let json = catalog.to_json_pretty().unwrap();
        assert_eq!(RiverCatalog::from_json(&json).unwrap(), catalog);
    }

    /// Indices for coastal-slope dogfood fixture (westward chain, row ~center).
    pub fn coastal_slope_dogfood_chain() -> Vec<usize> {
        let bounds = small_bounds();
        let mut chain = Vec::new();
        let start = bounds.index_of(Axial::new(4, 0)).unwrap();
        chain.push(start);
        let mut current = start;
        for _ in 0..6 {
            let next = neighbors(&bounds, current)
                .filter(|n| {
                    bounds
                        .from_index(*n)
                        .map(|c| c.q < bounds.from_index(current).unwrap().q)
                        .unwrap_or(false)
                })
                .min_by_key(|n| bounds.from_index(*n).unwrap().q)
                .expect("west neighbor");
            chain.push(next);
            current = next;
        }
        chain
    }

    #[test]
    fn coastal_slope_fixture_chain_is_valid() {
        let bounds = small_bounds();
        let chain = coastal_slope_dogfood_chain();
        let mut catalog = RiverCatalog::default();
        let id = create_river(&mut catalog, &bounds, chain[0]).unwrap();
        for &idx in &chain[1..] {
            append_cell(&mut catalog, &bounds, id, idx).unwrap();
        }
        assert_eq!(catalog.rivers[0].cells, chain);
    }
}
