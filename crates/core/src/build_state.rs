//! World build wizard draft state in `mapkeeper.toml` (D-59, home-build-draft-v1).

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const BUILD_STEP_LAND_SILHOUETTE: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSection {
    pub status: String,
    pub step: u32,
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
        step,
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
            "# mapkeeper world project\n\n[world]\nid = \"{world_id}\"\nname = \"{world_id}\"\nversion = \"0.1.0\"\n\n[build]\nstatus = \"draft\"\nstep = {BUILD_STEP_LAND_SILHOUETTE}\n"
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
        fs::write(dir.join("mapkeeper.toml"), manifest_toml_with_build("test", true)).unwrap();
        let b = read_build(&dir).unwrap();
        assert!(is_draft(&b));
        assert_eq!(b.step, BUILD_STEP_LAND_SILHOUETTE);
        clear_build(&dir).unwrap();
        assert!(read_build(&dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
