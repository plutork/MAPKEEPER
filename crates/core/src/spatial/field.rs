use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::convert::Axial;
use super::grid::HexGrid;

/// Author-facing elevation range (integer cell height; not final unit system).
pub const RELIEF_MIN: i32 = -60;
pub const RELIEF_MAX: i32 = 100;

/// Persisted grid-bound field (authoritative on cells).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridField {
    pub id: String,
    /// Sparse cell values; missing key means baseline 0.
    pub cells: BTreeMap<String, i32>,
}

impl GridField {
    pub fn default_relief() -> Self {
        Self {
            id: "relief".to_string(),
            cells: BTreeMap::new(),
        }
    }

    /// Migrate retired probe `mark` id to author `relief`.
    pub fn normalize_author_field(&mut self) {
        if self.id == "mark" {
            self.id = "relief".to_string();
        }
    }

    pub fn key(axial: Axial) -> String {
        format!("{},{}", axial.q, axial.r)
    }

    pub fn get(&self, axial: Axial) -> i32 {
        self.cells.get(&Self::key(axial)).copied().unwrap_or(0)
    }

    pub fn set_cells(&mut self, grid: &HexGrid, updates: &[(Axial, i32)]) -> Result<(), String> {
        for &(axial, value) in updates {
            if !grid.contains_axial(axial.q, axial.r) {
                return Err(format!("cell {},{} outside grid", axial.q, axial.r));
            }
            if !(RELIEF_MIN..=RELIEF_MAX).contains(&value) {
                return Err(format!(
                    "relief {value} outside [{RELIEF_MIN}, {RELIEF_MAX}]"
                ));
            }
            let key = Self::key(axial);
            if value == 0 {
                self.cells.remove(&key);
            } else {
                self.cells.insert(key, value);
            }
        }
        Ok(())
    }

    /// Raise/Lower one cell by delta (clamped).
    pub fn adjust_cell(&mut self, grid: &HexGrid, axial: Axial, delta: i32) -> Result<i32, String> {
        if !grid.contains_axial(axial.q, axial.r) {
            return Err(format!("cell {},{} outside grid", axial.q, axial.r));
        }
        let next = (self.get(axial) + delta).clamp(RELIEF_MIN, RELIEF_MAX);
        self.set_cells(grid, &[(axial, next)])?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raise_lower_clamps() {
        let grid = HexGrid::default_probe();
        let mut field = GridField::default_relief();
        assert_eq!(field.id, "relief");
        assert_eq!(field.adjust_cell(&grid, Axial { q: 0, r: 0 }, 1).unwrap(), 1);
        assert_eq!(field.adjust_cell(&grid, Axial { q: 0, r: 0 }, -1).unwrap(), 0);
        for _ in 0..=RELIEF_MAX {
            field.adjust_cell(&grid, Axial { q: 0, r: 0 }, 1).unwrap();
        }
        assert_eq!(field.get(Axial { q: 0, r: 0 }), RELIEF_MAX);
        for _ in 0..=(RELIEF_MAX - RELIEF_MIN) {
            field.adjust_cell(&grid, Axial { q: 0, r: 0 }, -1).unwrap();
        }
        assert_eq!(field.get(Axial { q: 0, r: 0 }), RELIEF_MIN);
    }

    #[test]
    fn mark_migrates_to_relief() {
        let mut field = GridField {
            id: "mark".to_string(),
            cells: BTreeMap::from([("0,0".into(), 2)]),
        };
        field.normalize_author_field();
        assert_eq!(field.id, "relief");
        assert_eq!(field.get(Axial { q: 0, r: 0 }), 2);
    }
}
