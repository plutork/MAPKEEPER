//! Source→mouth pin path for legacy manual rivers (D-55 / track 2).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::hex::{Axial, MapBounds};
use crate::hydro::DEFAULT_LAND_ELEVATION;
use crate::layer::DenseLayer;
use crate::rivers::{river_at_cell, River, RiverCatalog, RiverError};

const UPHILL_COST: u32 = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiverPinError {
    InvalidCell,
    SameEndpoint,
    NoPath,
    CellOccupied { river_id: u32 },
    RiverNotFound,
}

impl std::fmt::Display for RiverPinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiverPinError::InvalidCell => write!(f, "cell out of map bounds"),
            RiverPinError::SameEndpoint => write!(f, "source and mouth must differ"),
            RiverPinError::NoPath => write!(f, "no downhill path between source and mouth"),
            RiverPinError::CellOccupied { river_id } => {
                write!(f, "path crosses river {river_id}")
            }
            RiverPinError::RiverNotFound => write!(f, "river not found"),
        }
    }
}

impl From<RiverError> for RiverPinError {
    fn from(err: RiverError) -> Self {
        match err {
            RiverError::InvalidCell => Self::InvalidCell,
            RiverError::RiverNotFound => Self::RiverNotFound,
            RiverError::CellOccupied { river_id } => Self::CellOccupied { river_id },
            _ => Self::NoPath,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct HeapState {
    cost: u32,
    index: usize,
}

impl Ord for HeapState {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.index.cmp(&other.index))
    }
}

impl PartialOrd for HeapState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn elevation_at(elevation: &DenseLayer, index: usize) -> i32 {
    elevation.int_or(index, DEFAULT_LAND_ELEVATION)
}

fn neighbors(bounds: &MapBounds, index: usize) -> impl Iterator<Item = usize> + '_ {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
}

fn step_cost(from_elev: i32, to_elev: i32) -> u32 {
    let rise = (to_elev - from_elev).max(0) as u32;
    1 + rise.saturating_mul(UPHILL_COST)
}

/// Bounded elevation-biased walk from source to mouth on the hex grid.
pub fn pin_path(
    elevation: &DenseLayer,
    bounds: &MapBounds,
    source: usize,
    mouth: usize,
) -> Result<Vec<usize>, RiverPinError> {
    if source >= bounds.len() || mouth >= bounds.len() {
        return Err(RiverPinError::InvalidCell);
    }
    if source == mouth {
        return Err(RiverPinError::SameEndpoint);
    }
    let mut dist = HashMap::from([(source, 0u32)]);
    let mut prev = HashMap::new();
    let mut heap = BinaryHeap::from([HeapState {
        cost: 0,
        index: source,
    }]);
    while let Some(HeapState { cost, index }) = heap.pop() {
        if index == mouth {
            break;
        }
        if dist.get(&index).copied().unwrap_or(u32::MAX) < cost {
            continue;
        }
        let from_elev = elevation_at(elevation, index);
        for next in neighbors(bounds, index) {
            let next_cost = cost.saturating_add(step_cost(from_elev, elevation_at(elevation, next)));
            if next_cost < dist.get(&next).copied().unwrap_or(u32::MAX) {
                dist.insert(next, next_cost);
                prev.insert(next, index);
                heap.push(HeapState {
                    cost: next_cost,
                    index: next,
                });
            }
        }
    }
    if !dist.contains_key(&mouth) {
        return Err(RiverPinError::NoPath);
    }
    let mut path = vec![mouth];
    let mut current = mouth;
    while current != source {
        let Some(parent) = prev.get(&current).copied() else {
            return Err(RiverPinError::NoPath);
        };
        path.push(parent);
        current = parent;
    }
    path.reverse();
    Ok(path)
}

fn path_clear_for_river(
    catalog: &RiverCatalog,
    path: &[usize],
    mouth: usize,
    except: Option<u32>,
) -> Result<(), RiverPinError> {
    for &index in path {
        if index == mouth {
            continue;
        }
        if let Some(other) = river_at_cell(catalog, index) {
            if except != Some(other) {
                return Err(RiverPinError::CellOccupied { river_id: other });
            }
        }
    }
    Ok(())
}

fn tributary_parent(catalog: &RiverCatalog, mouth: usize) -> Option<(u32, u32)> {
    let stem = catalog.rivers.iter().find(|river| river.cells.contains(&mouth))?;
    Some((stem.parent, stem.basin))
}

/// Create or replace a legacy catalog river from source→mouth pin.
pub fn upsert_river_pin(
    catalog: &mut RiverCatalog,
    bounds: &MapBounds,
    elevation: &DenseLayer,
    source: usize,
    mouth: usize,
    river_id: Option<u32>,
) -> Result<u32, RiverPinError> {
    let path = pin_path(elevation, bounds, source, mouth)?;
    path_clear_for_river(catalog, &path, mouth, river_id)?;

    let tributary = tributary_parent(catalog, mouth);

    if let Some(id) = river_id {
        let pos = catalog
            .rivers
            .iter()
            .position(|river| river.id == id)
            .ok_or(RiverPinError::RiverNotFound)?;
        let (parent, basin) = tributary.unwrap_or((id, id));
        catalog.rivers[pos] = River {
            id,
            cells: path,
            source,
            mouth,
            parent,
            basin,
            name: catalog.rivers[pos].name.clone(),
        };
        Ok(id)
    } else {
        let id = catalog.next_id;
        catalog.next_id = catalog.next_id.saturating_add(1).max(2);
        let (parent, basin) = tributary.unwrap_or((id, id));
        catalog.rivers.push(River {
            id,
            cells: path,
            source,
            mouth,
            parent,
            basin,
            name: None,
        });
        Ok(id)
    }
}

/// Resolve axial coords to index for pin APIs.
pub fn pin_cell_index(bounds: &MapBounds, q: i32, r: i32) -> Result<usize, RiverPinError> {
    bounds
        .index_of(Axial::new(q, r))
        .ok_or(RiverPinError::InvalidCell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydro::SEA_LEVEL;
    use crate::layer::{DenseState, LayerValue};
    use crate::rivers::{append_cell, create_river};

    fn slope_bounds() -> (MapBounds, DenseLayer) {
        let bounds = MapBounds::new(8, 4);
        let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
        for index in 0..bounds.len() {
            let cell = bounds.from_index(index).unwrap();
            let height = SEA_LEVEL + 20 - cell.q;
            elevation.set(index, DenseState::Value(LayerValue::Int(height)));
        }
        (bounds, elevation)
    }

    #[test]
    fn pin_path_follows_downhill_slope() {
        let (bounds, elevation) = slope_bounds();
        let source = bounds.index_of(Axial::new(1, 0)).unwrap();
        let mouth = bounds.index_of(Axial::new(3, 0)).unwrap();
        let path = pin_path(&elevation, &bounds, source, mouth).unwrap();
        assert_eq!(path.first().copied(), Some(source));
        assert_eq!(path.last().copied(), Some(mouth));
        assert!(path.len() >= 2);
    }

    #[test]
    fn upsert_pin_sets_tributary_parent_when_mouth_on_stem() {
        let (bounds, elevation) = slope_bounds();
        let source = bounds.index_of(Axial::new(0, 0)).unwrap();
        let mut catalog = RiverCatalog::default();
        let stem_id = create_river(&mut catalog, &bounds, source).unwrap();
        for _ in 0..3 {
            let last = *catalog.rivers[0].cells.last().unwrap();
            let next = neighbors(&bounds, last).next().unwrap();
            append_cell(&mut catalog, &bounds, stem_id, next).unwrap();
        }
        let mouth = *catalog.rivers[0].cells.last().unwrap();
        let trib_source = bounds.index_of(Axial::new(0, 1)).unwrap();
        let id = upsert_river_pin(
            &mut catalog,
            &bounds,
            &elevation,
            trib_source,
            mouth,
            None,
        )
        .unwrap();
        let trib = catalog.rivers.iter().find(|river| river.id == id).unwrap();
        assert_eq!(trib.parent, stem_id);
        assert_eq!(trib.basin, stem_id);
    }
}
