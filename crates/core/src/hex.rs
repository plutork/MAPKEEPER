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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_six_neighbors() {
        let center = Axial::new(0, 0);
        assert_eq!(center.neighbors().len(), 6);
    }
}
