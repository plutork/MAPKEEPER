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

/// Radial hex bounds centered on the origin — the V0 blank map is a
/// radius-`radius` hexagon. Owned here so renderer/hit-testing/import share
/// one in-bounds rule instead of re-deriving it (spatial contract, 2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapBounds {
    pub radius: i32,
}

impl MapBounds {
    pub fn new(radius: i32) -> Self {
        Self { radius }
    }

    /// True if `cell` lies within the bounded hexagon.
    pub fn contains(&self, cell: Axial) -> bool {
        Axial::new(0, 0).distance(cell) <= self.radius
    }

    /// Every in-bounds cell, row-major by `q` then `r`.
    pub fn cells(&self) -> Vec<Axial> {
        Axial::new(0, 0).range(self.radius)
    }

    // scale-layers: cell index — canonical `(q,r) <-> linear index` so map
    // layers can be stored dense/index-addressed (D-46). `cell_id` string stays
    // the external identity; this index is the internal key. Order matches
    // `cells()` (column-major by `q`, then `r`), so `index_of(cells()[i]) == i`.

    /// Number of cells in the bounded hexagon (`1 + 3r(r+1)`).
    pub fn len(&self) -> usize {
        let r = self.radius.max(0) as usize;
        1 + 3 * r * (r + 1)
    }

    /// A radius-0 hexagon still has one cell, so bounds are never empty; kept
    /// for clippy/ergonomics.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Height (cell count) of column `q`, or 0 if `q` is out of bounds.
    fn column_height(&self, q: i32) -> usize {
        if q.abs() > self.radius {
            return 0;
        }
        (2 * self.radius + 1 - q.abs()) as usize
    }

    /// Lowest `r` present in column `q`.
    fn column_r_lo(&self, q: i32) -> i32 {
        (-self.radius).max(-q - self.radius)
    }

    /// Linear index of `cell` within [`Self::cells`] order, or `None` if the
    /// cell is out of bounds.
    pub fn index_of(&self, cell: Axial) -> Option<usize> {
        if !self.contains(cell) {
            return None;
        }
        let mut prefix = 0usize;
        for q in -self.radius..cell.q {
            prefix += self.column_height(q);
        }
        let offset = (cell.r - self.column_r_lo(cell.q)) as usize;
        Some(prefix + offset)
    }

    /// Inverse of [`Self::index_of`] — the cell at linear index `index`.
    pub fn from_index(&self, index: usize) -> Option<Axial> {
        if index >= self.len() {
            return None;
        }
        let mut remaining = index;
        for q in -self.radius..=self.radius {
            let h = self.column_height(q);
            if remaining < h {
                let r = self.column_r_lo(q) + remaining as i32;
                return Some(Axial::new(q, r));
            }
            remaining -= h;
        }
        None
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
    fn bounds_contains() {
        let bounds = MapBounds::new(2);
        assert!(bounds.contains(Axial::new(0, 0)));
        assert!(bounds.contains(Axial::new(2, 0)));
        assert!(!bounds.contains(Axial::new(3, 0)));
        assert_eq!(bounds.cells().len(), 19);
    }

    // scale-layers: cell index (D-46).
    #[test]
    fn len_matches_cells() {
        for radius in 0..=8 {
            let bounds = MapBounds::new(radius);
            assert_eq!(bounds.len(), bounds.cells().len());
        }
    }

    #[test]
    fn index_roundtrip_matches_cells_order() {
        for radius in 0..=6 {
            let bounds = MapBounds::new(radius);
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
        let bounds = MapBounds::new(3);
        assert_eq!(bounds.index_of(Axial::new(9, 9)), None);
    }
}
