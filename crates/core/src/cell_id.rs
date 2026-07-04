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

    /// Parses the canonical form `{world_id}.hex.q{q}.r{r}` (e.g. from a
    /// profile filename stem). Returns `None` on any malformed input.
    pub fn parse(s: &str) -> Option<CellId> {
        let (world_id, rest) = s.split_once(".hex.q")?;
        let (q_str, r_str) = rest.split_once(".r")?;
        if world_id.is_empty() {
            return None;
        }
        let q = q_str.parse().ok()?;
        let r = r_str.parse().ok()?;
        Some(CellId::new(world_id, q, r))
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

    #[test]
    fn parses_canonical_id() {
        let id = CellId::parse("main.hex.q3.r-1").unwrap();
        assert_eq!(id.world_id, "main");
        assert_eq!(id.q, 3);
        assert_eq!(id.r, -1);
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(CellId::parse("not-a-cell-id").is_none());
        assert!(CellId::parse(".hex.q1.r2").is_none());
    }
}
