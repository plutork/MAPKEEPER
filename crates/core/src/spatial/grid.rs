use serde::{Deserialize, Serialize};

use super::presets::{alpha_default_preset, ALPHA_NEIGHBOR_CENTER_DISTANCE_M};

/// Named hex lattice identity (alpha: exactly one per world).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HexGrid {
    pub id: String,
    /// Distance between centres of neighbouring cells (meters).
    pub neighbor_center_distance_m: f64,
    pub width: u32,
    pub height: u32,
}

impl HexGrid {
    pub fn default_probe() -> Self {
        let preset = alpha_default_preset();
        Self {
            id: "primary".to_string(),
            neighbor_center_distance_m: ALPHA_NEIGHBOR_CENTER_DISTANCE_M,
            width: preset.cols,
            height: preset.rows,
        }
    }

    pub fn contains_axial(&self, q: i32, r: i32) -> bool {
        let Some((col, row)) = axial_to_offset(q, r) else {
            return false;
        };
        col >= 0 && row >= 0 && (col as u32) < self.width && (row as u32) < self.height
    }

    pub fn iter_axial(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (0..self.height).flat_map(move |row| {
            (0..self.width).filter_map(move |col| offset_to_axial(col as i32, row as i32))
        })
    }
}

/// odd-r offset → axial
pub fn offset_to_axial(col: i32, row: i32) -> Option<(i32, i32)> {
    let q = col - (row - (row & 1)) / 2;
    Some((q, row))
}

/// axial → odd-r offset
pub fn axial_to_offset(q: i32, r: i32) -> Option<(i32, i32)> {
    let col = q + (r - (r & 1)) / 2;
    Some((col, r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_axial_round_trip_in_bounds() {
        let grid = HexGrid::default_probe();
        for (q, r) in grid.iter_axial() {
            let (col, row) = axial_to_offset(q, r).unwrap();
            let (q2, r2) = offset_to_axial(col, row).unwrap();
            assert_eq!((q, r), (q2, r2));
            assert!(grid.contains_axial(q, r));
        }
        assert_eq!(grid.iter_axial().count(), (grid.width * grid.height) as usize);
    }
}
