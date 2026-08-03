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

    /// Raise/Lower one cell by delta (clamped, no gesture rule).
    pub fn adjust_cell(&mut self, grid: &HexGrid, axial: Axial, delta: i32) -> Result<i32, String> {
        if !grid.contains_axial(axial.q, axial.r) {
            return Err(format!("cell {},{} outside grid", axial.q, axial.r));
        }
        let next = (self.get(axial) + delta).clamp(RELIEF_MIN, RELIEF_MAX);
        self.set_cells(grid, &[(axial, next)])?;
        Ok(next)
    }
}

/// Relief gesture rule (N-015 meaning, N-030 placement): what one Raise/Lower
/// step does to a cell. `None` means the gesture leaves the cell untouched.
/// Every writer must use this instead of re-deriving the rule.
pub fn next_relief_value(current: i32, delta: i32, edit_ocean: bool) -> Option<i32> {
    if current < 0 && !edit_ocean {
        return None;
    }
    let mut next = current + delta;
    if delta < 0 && !edit_ocean {
        next = next.max(0);
    }
    next = next.clamp(RELIEF_MIN, RELIEF_MAX);
    (next != current).then_some(next)
}

/// Absolute set for Flatten/Align (N-038): target height with the same ocean
/// freeze/floor as Raise/Lower. `None` = leave cell untouched.
pub fn next_relief_absolute(current: i32, target: i32, edit_ocean: bool) -> Option<i32> {
    if current < 0 && !edit_ocean {
        return None;
    }
    let mut next = target.clamp(RELIEF_MIN, RELIEF_MAX);
    if !edit_ocean {
        next = next.max(0);
    }
    (next != current).then_some(next)
}

/// Axial neighbour offsets (cube-consistent; matches [`super::brush::hex_distance`]).
pub const AXIAL_NEIGHBOR_OFFSETS: [(i32, i32); 6] =
    [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

/// Integer average of center + neighbours for Smooth (N-038). Empty neighbours
/// → center unchanged average (= center).
pub fn smooth_relief_average(center: i32, neighbors: &[i32]) -> i32 {
    let n = 1 + neighbors.len() as i32;
    let sum = center + neighbors.iter().sum::<i32>();
    if sum >= 0 {
        (sum + n / 2) / n
    } else {
        (sum - n / 2) / n
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
        assert_eq!(
            field.adjust_cell(&grid, Axial { q: 0, r: 0 }, 1).unwrap(),
            1
        );
        assert_eq!(
            field.adjust_cell(&grid, Axial { q: 0, r: 0 }, -1).unwrap(),
            0
        );
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
    fn relief_gesture_respects_ocean_lock() {
        // Edit ocean off: below datum frozen, land Lower stops at 0 (N-015).
        assert_eq!(next_relief_value(-2, -1, false), None);
        assert_eq!(next_relief_value(-2, 1, false), None);
        assert_eq!(next_relief_value(1, -1, false), Some(0));
        assert_eq!(next_relief_value(0, -1, false), None);
        assert_eq!(next_relief_value(0, 1, false), Some(1));
        // Edit ocean on: digging and filling below datum allowed.
        assert_eq!(next_relief_value(-2, -1, true), Some(-3));
        assert_eq!(next_relief_value(-1, 1, true), Some(0));
        // Range clamp holds in both modes.
        assert_eq!(next_relief_value(RELIEF_MAX, 1, false), None);
        assert_eq!(next_relief_value(RELIEF_MIN, -1, true), None);
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

    #[test]
    fn flatten_respects_ocean_lock() {
        assert_eq!(next_relief_absolute(-2, 5, false), None);
        assert_eq!(next_relief_absolute(3, 5, false), Some(5));
        assert_eq!(next_relief_absolute(3, -4, false), Some(0));
        assert_eq!(next_relief_absolute(3, 3, false), None);
        assert_eq!(next_relief_absolute(-2, -5, true), Some(-5));
        assert_eq!(
            next_relief_absolute(0, RELIEF_MAX + 10, false),
            Some(RELIEF_MAX)
        );
    }

    #[test]
    fn smooth_average_rounds_nearest() {
        assert_eq!(smooth_relief_average(4, &[2, 2, 2, 2, 2, 2]), 2);
        assert_eq!(smooth_relief_average(0, &[]), 0);
        assert_eq!(smooth_relief_average(-3, &[-3, -1]), -2);
    }
}
