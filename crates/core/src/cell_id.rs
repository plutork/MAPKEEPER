use std::fmt;

/// Canonical cell identifier: `{world_id}.hex.q{q}.r{r}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CellId {
    pub world_id: String,
    pub q: i32,
    pub r: i32,
}

impl CellId {
    pub fn new(world_id: impl Into<String>, q: i32, r: i32) -> Self {
        Self { world_id: world_id.into(), q, r }
    }

    pub fn filename(&self) -> String {
        format!("{self}.json")
    }
}

impl fmt::Display for CellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.hex.q{}.r{}", self.world_id, self.q, self.r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_canonical_id() {
        let id = CellId::new("main", 3, -1);
        assert_eq!(id.to_string(), "main.hex.q3.r-1");
        assert_eq!(id.filename(), "main.hex.q3.r-1.json");
    }
}
