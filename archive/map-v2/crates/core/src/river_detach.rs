//! Detach tributary from stem on legacy manual rivers (D-55 / track 3).

use crate::rivers::{RiverCatalog, RiverError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiverDetachError {
    RiverNotFound,
    NotTributary,
    NoConfluence,
    TooShort,
}

impl std::fmt::Display for RiverDetachError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiverDetachError::RiverNotFound => write!(f, "river not found"),
            RiverDetachError::NotTributary => write!(f, "river is not a tributary"),
            RiverDetachError::NoConfluence => {
                write!(f, "mouth is not at a confluence on the parent stem")
            }
            RiverDetachError::TooShort => write!(f, "tributary is too short to detach"),
        }
    }
}

impl From<RiverError> for RiverDetachError {
    fn from(err: RiverError) -> Self {
        match err {
            RiverError::RiverNotFound => Self::RiverNotFound,
            _ => Self::NoConfluence,
        }
    }
}

/// Tributary occupying `index` (parent ≠ self), if any.
pub fn tributary_at_cell(catalog: &RiverCatalog, index: usize) -> Option<u32> {
    catalog
        .rivers
        .iter()
        .find(|river| river.cells.contains(&index) && river.parent != river.id)
        .map(|river| river.id)
}

/// Truncate at confluence and reset parent/basin to self.
pub fn detach_tributary(catalog: &mut RiverCatalog, river_id: u32) -> Result<(), RiverDetachError> {
    let pos = catalog
        .rivers
        .iter()
        .position(|river| river.id == river_id)
        .ok_or(RiverDetachError::RiverNotFound)?;
    let river = &catalog.rivers[pos];
    if river.parent == river.id {
        return Err(RiverDetachError::NotTributary);
    }
    let parent_cells = catalog
        .rivers
        .iter()
        .find(|river| river.id == river.parent)
        .map(|river| river.cells.as_slice())
        .ok_or(RiverDetachError::NoConfluence)?;
    if river.cells.len() < 2 {
        return Err(RiverDetachError::TooShort);
    }
    if river.cells.last() != Some(&river.mouth) || !parent_cells.contains(&river.mouth) {
        return Err(RiverDetachError::NoConfluence);
    }
    catalog.rivers[pos].cells.pop();
    let new_mouth = *catalog
        .rivers
        .get(pos)
        .and_then(|river| river.cells.last())
        .ok_or(RiverDetachError::TooShort)?;
    catalog.rivers[pos].mouth = new_mouth;
    catalog.rivers[pos].parent = river_id;
    catalog.rivers[pos].basin = river_id;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::Axial;
    use crate::hydro::SEA_LEVEL;
    use crate::layer::{DenseLayer, DenseState, LayerValue};
    use crate::river_pin::upsert_river_pin;
    use crate::rivers::{append_cell, create_river, sync_river_id_layer, river_at_cell};

    fn slope_bounds() -> (crate::hex::MapBounds, DenseLayer) {
        let bounds = crate::hex::MapBounds::new(8, 4);
        let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
        for index in 0..bounds.len() {
            let cell = bounds.from_index(index).unwrap();
            let height = SEA_LEVEL + 20 - cell.q;
            elevation.set(index, DenseState::Value(LayerValue::Int(height)));
        }
        (bounds, elevation)
    }

    fn stem_with_tributary() -> (crate::hex::MapBounds, RiverCatalog, u32, u32) {
        let (bounds, elevation) = slope_bounds();
        let source = bounds.index_of(Axial::new(0, 0)).unwrap();
        let mut catalog = RiverCatalog::default();
        let stem_id = create_river(&mut catalog, &bounds, source).unwrap();
        for _ in 0..3 {
            let last = *catalog.rivers[0].cells.last().unwrap();
            let next = bounds
                .from_index(last)
                .unwrap()
                .neighbors()
                .into_iter()
                .filter_map(|n| bounds.index_of(n))
                .next()
                .unwrap();
            append_cell(&mut catalog, &bounds, stem_id, next).unwrap();
        }
        let mouth = *catalog.rivers[0].cells.last().unwrap();
        let trib_source = bounds.index_of(Axial::new(0, 1)).unwrap();
        let trib_id = upsert_river_pin(
            &mut catalog,
            &bounds,
            &elevation,
            trib_source,
            mouth,
            None,
        )
        .unwrap();
        (bounds, catalog, stem_id, trib_id)
    }

    #[test]
    fn detach_truncates_at_confluence_and_resets_parent() {
        let (bounds, mut catalog, stem_id, trib_id) = stem_with_tributary();
        let confluence = catalog
            .rivers
            .iter()
            .find(|river| river.id == trib_id)
            .unwrap()
            .mouth;
        let stem_cells_before = catalog
            .rivers
            .iter()
            .find(|river| river.id == stem_id)
            .unwrap()
            .cells
            .clone();
        let trib_cells_before = catalog
            .rivers
            .iter()
            .find(|river| river.id == trib_id)
            .unwrap()
            .cells
            .len();

        detach_tributary(&mut catalog, trib_id).unwrap();

        let trib = catalog.rivers.iter().find(|river| river.id == trib_id).unwrap();
        assert_eq!(trib.parent, trib_id);
        assert_eq!(trib.basin, trib_id);
        assert_eq!(trib.cells.len(), trib_cells_before - 1);
        assert!(!trib.cells.contains(&confluence));
        assert_eq!(
            catalog
                .rivers
                .iter()
                .find(|river| river.id == stem_id)
                .unwrap()
                .cells,
            stem_cells_before
        );
        let layer = sync_river_id_layer(&catalog, &bounds);
        assert_eq!(layer.int_or(confluence, -1), stem_id as i32);
        assert_ne!(river_at_cell(&catalog, confluence), Some(trib_id));
    }

    #[test]
    fn detach_rejects_stem() {
        let (_, mut catalog, stem_id, _) = stem_with_tributary();
        assert_eq!(
            detach_tributary(&mut catalog, stem_id),
            Err(RiverDetachError::NotTributary)
        );
    }

    #[test]
    fn tributary_at_cell_prefers_tributary_over_stem_at_confluence() {
        let (_, catalog, stem_id, trib_id) = stem_with_tributary();
        let confluence = catalog
            .rivers
            .iter()
            .find(|river| river.id == trib_id)
            .unwrap()
            .mouth;
        assert!(catalog
            .rivers
            .iter()
            .find(|river| river.id == stem_id)
            .unwrap()
            .cells
            .contains(&confluence));
        assert_eq!(tributary_at_cell(&catalog, confluence), Some(trib_id));
    }
}
