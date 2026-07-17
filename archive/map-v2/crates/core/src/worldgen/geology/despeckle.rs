//! Isolated minor geology cleanup.

use crate::hex::MapBounds;
use crate::layer::{DenseLayer, DenseState, LayerValue};

use super::kind::{geology_kind, is_minor_geology};
use super::types::GEOLOGY_STABLE;

#[cfg(test)]
use super::land_helpers::is_land_cell;

pub(crate) fn despeckle_isolated_minors(bounds: &MapBounds, layer: &mut DenseLayer) {
    let mut demote = Vec::new();
    for index in 0..bounds.len() {
        let kind = geology_kind(layer, index);
        if !is_minor_geology(kind) {
            continue;
        }
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let mut same = 0usize;
        for n in cell.neighbors() {
            let Some(ni) = bounds.index_of(n) else {
                continue;
            };
            if geology_kind(layer, ni) == kind {
                same += 1;
            }
        }
        if same == 0 {
            demote.push(index);
        }
    }
    for index in demote {
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(GEOLOGY_STABLE.to_string())),
        );
    }
}

#[cfg(test)]
pub(crate) fn isolated_minor_count(bounds: &MapBounds, geo: &DenseLayer) -> usize {
    let mut n = 0usize;
    for i in 0..bounds.len() {
        let kind = geology_kind(geo, i);
        if !is_minor_geology(kind) {
            continue;
        }
        let Some(cell) = bounds.from_index(i) else {
            continue;
        };
        let same = cell
            .neighbors()
            .into_iter()
            .filter(|nb| {
                bounds
                    .index_of(*nb)
                    .is_some_and(|ni| geology_kind(geo, ni) == kind)
            })
            .count();
        if same == 0 {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
pub(crate) fn fill_land_disk(bounds: &MapBounds, mask: &mut DenseLayer, radius: i32) {
    use crate::hex::Axial;
    use crate::worldgen::land::{LAND_MASK_LAND, LAND_MASK_OCEAN};

    let center = bounds
        .from_index(bounds.len() / 2)
        .unwrap_or(Axial::new(0, 0));
    for i in 0..bounds.len() {
        let Some(c) = bounds.from_index(i) else {
            continue;
        };
        let land = c.distance(center) <= radius;
        let v = if land {
            LAND_MASK_LAND
        } else {
            LAND_MASK_OCEAN
        };
        mask.set(i, DenseState::Value(LayerValue::Text(v.to_string())));
    }
}

#[cfg(test)]
pub(crate) fn count_land_cells(land_mask: &DenseLayer) -> usize {
    (0..land_mask.len())
        .filter(|&i| is_land_cell(land_mask, i))
        .count()
}
