//! World folder scaffold — pure data; `cli`/`server` perform the actual
//! filesystem writes (roadmap 3.5). Cell existence = a profile file on disk;
//! no separate "painted cells" list.
//!
//! **Single source of truth** (roadmap 5.2): every new world — wizard,
//! launcher `/api/projects create`, or CLI `init` — gets the *same* static
//! files as the GitHub Template (`toolchain/template/world/`, D-08),
//! embedded here at compile time. No copy/drift between the two onboarding
//! paths. `mapkeeper.toml` is the one exception — generated per-world by
//! `manifest_toml()` because it needs the author's world id substituted in.

/// Relative directories created for a new world project. `.cursor/commands`
/// is included so `SCAFFOLD_FILES`' `user.md` has somewhere to land;
/// `map/layers` holds the machine-readable map-state layers (D-36).
pub const SCAFFOLD_DIRS: &[&str] =
    &["map", "map/layers", "canon", "profiles", "data", "journal", ".cursor/commands"];

/// A static scaffold file: relative path inside the world folder + contents.
pub struct ScaffoldFile {
    pub rel_path: &'static str,
    pub contents: &'static str,
}

/// Static files copied as-is from `toolchain/template/world/` into every new
/// world. Keep in sync with that folder — it is the edited source (synced to
/// `mapkeeper-world-template` via CI, D-10); this list just embeds it.
pub const SCAFFOLD_FILES: &[ScaffoldFile] = &[
    ScaffoldFile {
        rel_path: "README.md",
        contents: include_str!("../../../toolchain/template/world/README.md"),
    },
    ScaffoldFile {
        rel_path: "AGENTS.md",
        contents: include_str!("../../../toolchain/template/world/AGENTS.md"),
    },
    ScaffoldFile {
        rel_path: ".gitignore",
        contents: include_str!("../../../toolchain/template/world/.gitignore"),
    },
    ScaffoldFile {
        rel_path: ".cursor/commands/user.md",
        contents: include_str!("../../../toolchain/template/world/.cursor/commands/user.md"),
    },
    ScaffoldFile {
        rel_path: "map/README.md",
        contents: include_str!("../../../toolchain/template/world/map/README.md"),
    },
    ScaffoldFile {
        rel_path: "map/manifest.json",
        contents: include_str!("../../../toolchain/template/world/map/manifest.json"),
    },
    ScaffoldFile {
        rel_path: "map/layers/terrain.json",
        contents: include_str!("../../../toolchain/template/world/map/layers/terrain.json"),
    },
    ScaffoldFile {
        rel_path: "map/layers/elevation.json",
        contents: include_str!("../../../toolchain/template/world/map/layers/elevation.json"),
    },
    ScaffoldFile {
        rel_path: "canon/README.md",
        contents: include_str!("../../../toolchain/template/world/canon/README.md"),
    },
    ScaffoldFile {
        rel_path: "profiles/README.md",
        contents: include_str!("../../../toolchain/template/world/profiles/README.md"),
    },
    ScaffoldFile {
        rel_path: "data/README.md",
        contents: include_str!("../../../toolchain/template/world/data/README.md"),
    },
    ScaffoldFile {
        rel_path: "journal/README.md",
        contents: include_str!("../../../toolchain/template/world/journal/README.md"),
    },
];

/// `mapkeeper.toml` content for a freshly scaffolded world — same shape as
/// the static template's `mapkeeper.toml`, with the author's id substituted.
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
