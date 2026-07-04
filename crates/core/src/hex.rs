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

    /// Six neighbors — placeholder ahead of spatial queries (roadmap 2.4).
    pub fn neighbors(self) -> [Axial; 6] {
        const DIRECTIONS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
        DIRECTIONS.map(|(dq, dr)| Axial::new(self.q + dq, self.r + dr))
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
}
