/// Axial hex coordinate — V0 supports hex grids only (roadmap 2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Axial {
    pub q: i32,
    pub r: i32,
}

impl Axial {
    pub fn new(q: i32, r: i32) -> Self {
        Self { q, r }
    }

    /// Six neighbors (spatial contract, roadmap 2.4).
    pub fn neighbors(self) -> [Axial; 6] {
        const DIRECTIONS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
        DIRECTIONS.map(|(dq, dr)| Axial::new(self.q + dq, self.r + dr))
    }

    /// Hex grid distance (number of steps) between two cells.
    pub fn distance(self, other: Axial) -> i32 {
        ((self.q - other.q).abs()
            + (self.q + self.r - other.q - other.r).abs()
            + (self.r - other.r).abs())
            / 2
    }

    /// Cells whose distance from `self` is exactly `radius` (the hex ring).
    /// `radius == 0` yields just `self`.
    pub fn ring(self, radius: i32) -> Vec<Axial> {
        if radius <= 0 {
            return vec![self];
        }
        // Start at one corner (radius steps along direction 4), then walk the
        // six edges, radius steps each — standard hex-ring traversal.
        const DIRECTIONS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
        let mut cell = Axial::new(self.q + DIRECTIONS[4].0 * radius, self.r + DIRECTIONS[4].1 * radius);
        let mut out = Vec::with_capacity((6 * radius) as usize);
        for dir in DIRECTIONS {
            for _ in 0..radius {
                out.push(cell);
                cell = Axial::new(cell.q + dir.0, cell.r + dir.1);
            }
        }
        out
    }

    /// All cells within `radius` of `self` (filled hexagon, inclusive).
    pub fn range(self, radius: i32) -> Vec<Axial> {
        let mut out = Vec::new();
        for dq in -radius..=radius {
            let lo = (-radius).max(-dq - radius);
            let hi = radius.min(-dq + radius);
            for dr in lo..=hi {
                out.push(Axial::new(self.q + dq, self.r + dr));
            }
        }
        out
    }

    /// Center of this cell in pixel space (pointy-top axial layout), given `size`
    /// (center-to-corner radius). Shared by web renderer and hit-testing (roadmap 4.2).
    pub fn to_pixel(self, size: f64) -> (f64, f64) {
        let q = self.q as f64;
        let r = self.r as f64;
        let x = size * (3f64.sqrt() * q + 3f64.sqrt() / 2.0 * r);
        let y = size * (3.0 / 2.0 * r);
        (x, y)
    }

    /// Inverse of `to_pixel` — rounds a pixel position to the containing cell.
    pub fn from_pixel(x: f64, y: f64, size: f64) -> Axial {
        let q = (3f64.sqrt() / 3.0 * x - 1.0 / 3.0 * y) / size;
        let r = (2.0 / 3.0 * y) / size;
        round_axial(q, r)
    }
}

/// Pointy-top odd-r offset rectangle centered on the axial origin (D-49).
/// `width` × `height` cells; index order matches row-major offset (col, row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapBounds {
    pub width: i32,
    pub height: i32,
}

impl MapBounds {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
        }
    }

    /// True if `cell` lies within the bounded rectangle.
    pub fn contains(&self, cell: Axial) -> bool {
        self.axial_to_offset(cell).is_some()
    }

    /// Every in-bounds cell, row-major by offset row then col.
    pub fn cells(&self) -> Vec<Axial> {
        let mut out = Vec::with_capacity(self.len());
        for row in 0..self.height {
            for col in 0..self.width {
                out.push(self.offset_to_axial(col, row));
            }
        }
        out
    }

    // map-bounds--hex-rectangle-16x9: cell index for dense layers (D-46).

    /// Number of cells (`width * height`).
    pub fn len(&self) -> usize {
        (self.width.max(0) * self.height.max(0)) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn offset_to_axial(&self, col: i32, row: i32) -> Axial {
        let col_c = col - self.width / 2;
        let row_c = row - self.height / 2;
        let q = col_c - (row_c - (row_c & 1)) / 2;
        let r = row_c;
        Axial::new(q, r)
    }

    fn axial_to_offset(&self, cell: Axial) -> Option<(i32, i32)> {
        let row_c = cell.r;
        let col_c = cell.q + (row_c - (row_c & 1)) / 2;
        let col = col_c + self.width / 2;
        let row = row_c + self.height / 2;
        if (0..self.width).contains(&col) && (0..self.height).contains(&row) {
            Some((col, row))
        } else {
            None
        }
    }

    /// Linear index of `cell` within [`Self::cells`] order, or `None` if OOB.
    pub fn index_of(&self, cell: Axial) -> Option<usize> {
        let (col, row) = self.axial_to_offset(cell)?;
        Some((row * self.width + col) as usize)
    }

    /// Inverse of [`Self::index_of`].
    pub fn from_index(&self, index: usize) -> Option<Axial> {
        let w = self.width as usize;
        if w == 0 || index >= self.len() {
            return None;
        }
        let col = (index % w) as i32;
        let row = (index / w) as i32;
        Some(self.offset_to_axial(col, row))
    }

    /// Min/max axial q/r over in-bounds cells (for viewport culling).
    pub fn axial_limits(&self) -> (i32, i32, i32, i32) {
        let cells = self.cells();
        let mut min_q = i32::MAX;
        let mut max_q = i32::MIN;
        let mut min_r = i32::MAX;
        let mut max_r = i32::MIN;
        for c in cells {
            min_q = min_q.min(c.q);
            max_q = max_q.max(c.q);
            min_r = min_r.min(c.r);
            max_r = max_r.max(c.r);
        }
        (min_q, max_q, min_r, max_r)
    }
}

/// Cube-coordinate rounding — standard fix for naive float axial rounding.
fn round_axial(q: f64, r: f64) -> Axial {
    let x = q;
    let z = r;
    let y = -x - z;

    let mut rx = x.round();
    let ry = y.round();
    let mut rz = z.round();

    let x_diff = (rx - x).abs();
    let y_diff = (ry - y).abs();
    let z_diff = (rz - z).abs();

    if x_diff > y_diff && x_diff > z_diff {
        rx = -ry - rz;
    } else if y_diff <= z_diff {
        rz = -rx - ry;
    }

    Axial::new(rx as i32, rz as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_six_neighbors() {
        let center = Axial::new(0, 0);
        assert_eq!(center.neighbors().len(), 6);
    }

    #[test]
    fn pixel_roundtrip() {
        for q in -3..=3 {
            for r in -3..=3 {
                let cell = Axial::new(q, r);
                let (x, y) = cell.to_pixel(20.0);
                assert_eq!(Axial::from_pixel(x, y, 20.0), cell);
            }
        }
    }

    #[test]
    fn distance_matches_neighbors() {
        let center = Axial::new(0, 0);
        assert_eq!(center.distance(center), 0);
        for n in center.neighbors() {
            assert_eq!(center.distance(n), 1);
        }
        assert_eq!(Axial::new(0, 0).distance(Axial::new(2, -1)), 2);
    }

    #[test]
    fn ring_size_and_distance() {
        let center = Axial::new(1, -2);
        assert_eq!(center.ring(0), vec![center]);
        for radius in 1..=4 {
            let ring = center.ring(radius);
            assert_eq!(ring.len() as i32, 6 * radius);
            assert!(ring.iter().all(|c| center.distance(*c) == radius));
        }
    }

    #[test]
    fn range_is_filled_hexagon() {
        let center = Axial::new(0, 0);
        // 1 + sum(6*k) for k in 1..=radius
        assert_eq!(center.range(0).len(), 1);
        assert_eq!(center.range(1).len(), 7);
        assert_eq!(center.range(2).len(), 19);
        assert!(center.range(3).iter().all(|c| center.distance(*c) <= 3));
    }

    #[test]
    fn bounds_contains_rectangle() {
        let bounds = MapBounds::new(4, 3);
        assert_eq!(bounds.len(), 12);
        assert!(bounds.contains(Axial::new(0, 0)));
        assert!(!bounds.contains(Axial::new(99, 99)));
    }

    // scale-layers: cell index (D-46).
    #[test]
    fn len_matches_cells() {
        for (w, h) in [(1, 1), (4, 3), (14, 8), (43, 24)] {
            let bounds = MapBounds::new(w, h);
            assert_eq!(bounds.len(), bounds.cells().len());
        }
    }

    #[test]
    fn index_roundtrip_matches_cells_order() {
        for (w, h) in [(1, 1), (4, 3), (7, 5), (14, 8)] {
            let bounds = MapBounds::new(w, h);
            let cells = bounds.cells();
            for (i, cell) in cells.iter().enumerate() {
                assert_eq!(bounds.index_of(*cell), Some(i));
                assert_eq!(bounds.from_index(i), Some(*cell));
            }
            assert_eq!(bounds.from_index(bounds.len()), None);
        }
    }

    #[test]
    fn index_of_rejects_out_of_bounds() {
        let bounds = MapBounds::new(3, 3);
        assert_eq!(bounds.index_of(Axial::new(9, 9)), None);
    }
}
