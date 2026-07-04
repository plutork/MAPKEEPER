//! World folder scaffold — pure data; `cli`/`server` perform the actual
//! filesystem writes (roadmap 3.5). Cell existence = a profile file on disk;
//! no separate "painted cells" list.

/// Relative directories created for a new world project.
pub const SCAFFOLD_DIRS: &[&str] = &["map", "canon", "profiles", "data", "journal"];

/// `mapkeeper.toml` content for a freshly scaffolded world.
pub fn manifest_toml(world_id: &str) -> String {
    format!(
        "# mapkeeper world project\n\n[world]\nid = \"{world_id}\"\nname = \"{world_id}\"\nversion = \"0.1.0\"\n"
    )
}

/// World id must be filesystem- and `cell_id`-safe: lowercase alnum + `-`/`_`,
/// and must not contain '.' (used as a separator in `cell_id`).
pub fn is_valid_world_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_dotted_ids() {
        assert!(!is_valid_world_id(""));
        assert!(!is_valid_world_id("my.world"));
        assert!(!is_valid_world_id("My-World"));
        assert!(is_valid_world_id("main"));
        assert!(is_valid_world_id("north-continent_2"));
    }
}
