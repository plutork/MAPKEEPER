//! Step 6 coast foundation (D-90): land → water distance in hex steps.

use std::collections::VecDeque;

use crate::hex::MapBounds;
use crate::land_mask::LAND_MASK_LAND;
use crate::layer::{DenseLayer, DenseState, LayerValue};

/// Hex steps from each cell to the nearest non-land cell (water = 0).
pub fn coast_distance_land_steps(bounds: &MapBounds, land_mask: &DenseLayer) -> Vec<u32> {
    let n = bounds.len();
    let mut dist = vec![u32::MAX; n];
    let mut queue = VecDeque::new();

    for index in 0..n {
        if !is_land_cell(land_mask, index) {
            dist[index] = 0;
            queue.push_back(index);
        }
    }

    while let Some(index) = queue.pop_front() {
        let base = dist[index];
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        for nb in cell.neighbors() {
            let Some(ni) = bounds.index_of(nb) else {
                continue;
            };
            if dist[ni] == u32::MAX {
                dist[ni] = base.saturating_add(1);
                queue.push_back(ni);
            }
        }
    }

    for d in &mut dist {
        if *d == u32::MAX {
            *d = 0;
        }
    }
    dist
}

fn is_land_cell(land_mask: &DenseLayer, index: usize) -> bool {
    matches!(
        land_mask.state(index),
        DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::land_mask::{generate_land_mask, LayoutClass, ShoreCharacter, LAND_MASK_LAND};

    #[test]
    fn water_zero_land_increases_inland() {
        let bounds = MapBounds::new(40, 24);
        let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 3);
        let dist = coast_distance_land_steps(&bounds, &mask);
        let mut coastal = 0i32;
        let mut interior = 0i32;
        for index in 0..bounds.len() {
            if !matches!(
                mask.state(index),
                DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
            ) {
                assert_eq!(dist[index], 0);
                continue;
            }
            assert!(dist[index] >= 1);
            if dist[index] <= 2 {
                coastal += 1;
            } else if dist[index] >= 5 {
                interior += 1;
            }
        }
        assert!(coastal > 0);
        assert!(interior > 0);
    }
}
