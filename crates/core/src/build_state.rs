//! World build wizard draft state in `mapkeeper.toml` (D-59, home-build-draft-v1).
//! Steps 1–4 after D-71 (size+grid merged); scheme marks numbering.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// D-71 numbering (current). Absent/`1` = D-69 five-step (size, grid, land, tect, elev).
pub const BUILD_STEP_SCHEME_V71: u32 = 2;

pub const BUILD_STEP_SIZE: u32 = 1;
pub const BUILD_STEP_LAND_SILHOUETTE: u32 = 2;
pub const BUILD_STEP_TECTONICS: u32 = 3;
pub const BUILD_STEP_ELEVATION: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSection {
    pub status: String,
    pub step: u32,
    /// Numbering scheme; missing = D-69 (pre-merge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheme: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MapkeeperToml {
    world: WorldSection,
    #[serde(skip_serializing_if = "Option::is_none")]
    build: Option<BuildSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorldSection {
    id: String,
    name: String,
    version: String,
}

pub fn is_draft(section: &BuildSection) -> bool {
    section.status == "draft"
}

/// Map stored step to D-71 numbering (1 Size · 2 Land · 3 Tectonics · 4 Elevation).
pub fn normalize_wizard_step(section: &BuildSection) -> u32 {
    let raw = section.step.max(1);
    if section.scheme.unwrap_or(1) >= BUILD_STEP_SCHEME_V71 {
        return raw.min(BUILD_STEP_ELEVATION);
    }
    // D-69: 1 size, 2 grid, 3 land, 4 tect, 5 elev → drop grid
    match raw {
        1 | 2 => BUILD_STEP_SIZE,
        n => (n - 1).min(BUILD_STEP_ELEVATION),
    }
}

pub fn read_build(world_path: &Path) -> Option<BuildSection> {
    let raw = std::fs::read_to_string(world_path.join("mapkeeper.toml")).ok()?;
    let doc: MapkeeperToml = toml::from_str(&raw).ok()?;
    doc.build
}

pub fn write_build_draft(world_path: &Path, step: u32) -> Result<(), String> {
    let path = world_path.join("mapkeeper.toml");
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut doc: MapkeeperToml = toml::from_str(&raw).map_err(|e| e.to_string())?;
    doc.build = Some(BuildSection {
        status: "draft".into(),
        step: step.max(1).min(BUILD_STEP_ELEVATION),
        scheme: Some(BUILD_STEP_SCHEME_V71),
    });
    let out = toml::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(&path, out).map_err(|e| e.to_string())
}

pub fn clear_build(world_path: &Path) -> Result<(), String> {
    let path = world_path.join("mapkeeper.toml");
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut doc: MapkeeperToml = toml::from_str(&raw).map_err(|e| e.to_string())?;
    doc.build = None;
    let out = toml::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(&path, out).map_err(|e| e.to_string())
}

pub fn manifest_toml_with_build(world_id: &str, draft: bool) -> String {
    if draft {
        format!(
            "# mapkeeper world project\n\n[world]\nid = \"{world_id}\"\nname = \"{world_id}\"\nversion = \"0.2.1\"\n\n[build]\nstatus = \"draft\"\nstep = {BUILD_STEP_SIZE}\nscheme = {BUILD_STEP_SCHEME_V71}\n"
        )
    } else {
        crate::world::manifest_toml(world_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn draft_round_trip() {
        let dir = std::env::temp_dir().join(format!("mk-build-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("mapkeeper.toml"),
            manifest_toml_with_build("test", true),
        )
        .unwrap();
        let b = read_build(&dir).unwrap();
        assert!(is_draft(&b));
        assert_eq!(b.step, BUILD_STEP_SIZE);
        assert_eq!(b.scheme, Some(BUILD_STEP_SCHEME_V71));
        clear_build(&dir).unwrap();
        assert!(read_build(&dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_d69_steps() {
        let old_grid = BuildSection {
            status: "draft".into(),
            step: 2,
            scheme: None,
        };
        assert_eq!(normalize_wizard_step(&old_grid), BUILD_STEP_SIZE);
        let old_land = BuildSection {
            status: "draft".into(),
            step: 3,
            scheme: None,
        };
        assert_eq!(normalize_wizard_step(&old_land), BUILD_STEP_LAND_SILHOUETTE);
        let old_elev = BuildSection {
            status: "draft".into(),
            step: 5,
            scheme: None,
        };
        assert_eq!(normalize_wizard_step(&old_elev), BUILD_STEP_ELEVATION);
        let v71_land = BuildSection {
            status: "draft".into(),
            step: 2,
            scheme: Some(BUILD_STEP_SCHEME_V71),
        };
        assert_eq!(normalize_wizard_step(&v71_land), BUILD_STEP_LAND_SILHOUETTE);
    }
}
