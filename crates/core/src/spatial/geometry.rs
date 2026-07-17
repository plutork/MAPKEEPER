use serde::{Deserialize, Serialize};

use super::convert::{world_to_axial, Axial};
use super::frame::WorldFrame;
use super::grid::HexGrid;

/// Minimal world-space geometry stub (contract probe, not a type registry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryStub {
    pub id: String,
    /// Polyline in world coordinates (authoritative geometry).
    pub points: Vec<[f64; 2]>,
}

impl GeometryStub {
    pub fn default_probe(frame: &WorldFrame, grid: &HexGrid) -> Self {
        let a = super::convert::axial_to_world(frame, grid, Axial { q: 1, r: 1 });
        let b = super::convert::axial_to_world(frame, grid, Axial { q: 4, r: 2 });
        let c = super::convert::axial_to_world(frame, grid, Axial { q: 7, r: 3 });
        Self {
            id: "probe".to_string(),
            points: vec![[a.0, a.1], [b.0, b.1], [c.0, c.1]],
        }
    }
}

/// Derived cell membership — must not be persisted as SoT.
pub fn cells_for_stub(frame: &WorldFrame, grid: &HexGrid, stub: &GeometryStub) -> Vec<Axial> {
    let mut cells = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    if stub.points.is_empty() {
        return cells;
    }
    for window in stub.points.windows(2) {
        let [x0, y0] = window[0];
        let [x1, y1] = window[1];
        let steps = 24;
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let x = x0 + (x1 - x0) * t;
            let y = y0 + (y1 - y0) * t;
            let axial = world_to_axial(frame, grid, x, y);
            if grid.contains_axial(axial.q, axial.r) && seen.insert((axial.q, axial.r)) {
                cells.push(axial);
            }
        }
    }
    if stub.points.len() == 1 {
        let [x, y] = stub.points[0];
        let axial = world_to_axial(frame, grid, x, y);
        if grid.contains_axial(axial.q, axial.r) {
            cells.push(axial);
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_is_derived_from_world_geometry() {
        let frame = WorldFrame::default_probe();
        let grid = HexGrid::default_probe();
        let stub = GeometryStub::default_probe(&frame, &grid);
        let cells = cells_for_stub(&frame, &grid, &stub);
        assert!(cells.len() >= 3, "stub should cross multiple cells");
        // Mutating only membership is impossible — there is no membership field.
        let again = cells_for_stub(&frame, &grid, &stub);
        assert_eq!(cells, again);
    }
}
