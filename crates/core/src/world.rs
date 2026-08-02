use serde::{Deserialize, Serialize};

use crate::spatial::{
    alpha_default_preset, preset_by_id, MapExtentPreset, ALPHA_NEIGHBOR_CENTER_DISTANCE_M,
};

/// World-level schema for N-035 / N-037 greenfield layout.
pub const WORLD_SCHEMA_VERSION: u32 = 1;

/// Default first map id when Create makes world + first map.
pub const DEFAULT_FIRST_MAP_ID: &str = "main";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldManifest {
    pub world: WorldIdentity,
    #[serde(default = "default_world_schema")]
    pub schema_version: u32,
    /// Ordered maps in this world (N-035). Empty → incomplete / not openable.
    #[serde(default)]
    pub maps: Vec<WorldMapRef>,
    /// Legacy single-level only — never written for new worlds (N-037 refuse).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<SpatialConfig>,
}

fn default_world_schema() -> u32 {
    WORLD_SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldIdentity {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldMapRef {
    pub id: String,
    pub name: String,
    /// Relative path from world root, e.g. `maps/main`.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapManifest {
    pub map: MapIdentity,
    pub spatial: SpatialConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapIdentity {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// Immutable per-map spatial configuration (N-014 / N-035).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialConfig {
    pub preset_id: String,
    pub grid_id: String,
    pub width_m: f64,
    pub height_m: f64,
    pub cols: u32,
    pub rows: u32,
    pub neighbor_center_distance_m: f64,
    pub origin_x_m: f64,
    pub origin_y_m: f64,
    pub orientation: String,
}

impl SpatialConfig {
    pub fn from_preset_id(preset_id: &str) -> Result<Self, String> {
        let preset = preset_by_id(preset_id)
            .ok_or_else(|| format!("unknown map extent preset `{preset_id}`"))?;
        Ok(Self::from_preset(preset))
    }

    pub fn from_preset(preset: &MapExtentPreset) -> Self {
        Self {
            preset_id: preset.id.to_string(),
            grid_id: "primary".to_string(),
            width_m: preset.width_m,
            height_m: preset.height_m,
            cols: preset.cols,
            rows: preset.rows,
            neighbor_center_distance_m: ALPHA_NEIGHBOR_CENTER_DISTANCE_M,
            origin_x_m: 0.0,
            origin_y_m: 0.0,
            orientation: "pointy-top".to_string(),
        }
    }

    pub fn alpha_default() -> Self {
        Self::from_preset(alpha_default_preset())
    }

    /// Full preset validation (id + metric fields), not id-only.
    pub fn assert_matches_catalog(&self) -> Result<(), String> {
        let expected = Self::from_preset_id(&self.preset_id)?;
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-6;
        if self.grid_id != expected.grid_id
            || self.cols != expected.cols
            || self.rows != expected.rows
            || !close(self.width_m, expected.width_m)
            || !close(self.height_m, expected.height_m)
            || !close(
                self.neighbor_center_distance_m,
                expected.neighbor_center_distance_m,
            )
            || self.orientation != expected.orientation
        {
            return Err(format!("manifest/preset mismatch for `{}`", self.preset_id));
        }
        Ok(())
    }
}

pub fn map_rel_path(map_id: &str) -> String {
    format!("maps/{map_id}")
}

/// World-only toml (no `[spatial]`).
pub fn world_manifest_toml(world_id: &str, maps: &[WorldMapRef]) -> String {
    let maps_toml = maps
        .iter()
        .map(|m| {
            format!(
                "[[maps]]\nid = \"{}\"\nname = \"{}\"\npath = \"{}\"\n",
                m.id, m.name, m.path
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# mapkeeper world workspace (N-035)\n\n\
         schema_version = {schema}\n\n\
         [world]\n\
         id = \"{world_id}\"\n\
         name = \"{world_id}\"\n\
         version = \"0.4.0\"\n\n\
         {maps_toml}",
        schema = WORLD_SCHEMA_VERSION,
    )
}

pub fn map_manifest_toml(map_id: &str, preset: &MapExtentPreset) -> String {
    let spatial = SpatialConfig::from_preset(preset);
    format!(
        "# mapkeeper map (N-035)\n\n\
         [map]\n\
         id = \"{map_id}\"\n\
         name = \"{map_id}\"\n\
         version = \"0.4.0\"\n\n\
         [spatial]\n\
         preset_id = \"{preset}\"\n\
         grid_id = \"{grid}\"\n\
         width_m = {width}\n\
         height_m = {height}\n\
         cols = {cols}\n\
         rows = {rows}\n\
         neighbor_center_distance_m = {neighbor}\n\
         origin_x_m = {ox}\n\
         origin_y_m = {oy}\n\
         orientation = \"{orientation}\"\n",
        preset = spatial.preset_id,
        grid = spatial.grid_id,
        width = spatial.width_m,
        height = spatial.height_m,
        cols = spatial.cols,
        rows = spatial.rows,
        neighbor = spatial.neighbor_center_distance_m,
        ox = spatial.origin_x_m,
        oy = spatial.origin_y_m,
        orientation = spatial.orientation,
    )
}

/// World-level toml with default first map ref (no map files written).
pub fn manifest_toml(world_id: &str) -> String {
    let maps = [WorldMapRef {
        id: DEFAULT_FIRST_MAP_ID.to_string(),
        name: DEFAULT_FIRST_MAP_ID.to_string(),
        path: map_rel_path(DEFAULT_FIRST_MAP_ID),
    }];
    world_manifest_toml(world_id, &maps)
}

pub fn parse_manifest(raw: &str) -> Result<WorldManifest, toml::de::Error> {
    toml::from_str(raw)
}

pub fn parse_map_manifest(raw: &str) -> Result<MapManifest, toml::de::Error> {
    toml::from_str(raw)
}

pub fn render_manifest(manifest: &WorldManifest) -> Result<String, toml::ser::Error> {
    let body = toml::to_string_pretty(manifest)?;
    Ok(format!("# mapkeeper world workspace (N-035)\n\n{body}"))
}

pub fn is_valid_world_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

pub fn normalize_world_id(input: &str) -> Result<String, &'static str> {
    let mut output = String::new();
    let mut previous_separator = false;
    for ch in input.trim().chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() || lower == '_' {
            output.push(lower);
            previous_separator = false;
        } else if !previous_separator {
            output.push('-');
            previous_separator = true;
        }
    }
    let normalized = output.trim_matches(['-', '_']);
    if normalized.is_empty() || !is_valid_world_id(normalized) {
        Err("world name must contain letters or digits")
    } else {
        Ok(normalized.to_string())
    }
}

/// True when folder is the pre-N-035 single-level layout (N-037 refuse).
pub fn looks_legacy_single_level(world_toml: &str, has_root_spatial_dir: bool) -> bool {
    if has_root_spatial_dir {
        return true;
    }
    match parse_manifest(world_toml) {
        Ok(m) => m.spatial.is_some(),
        Err(_) => world_toml.contains("[spatial]"),
    }
}

pub fn is_two_level_world(manifest: &WorldManifest) -> bool {
    manifest.spatial.is_none() && !manifest.maps.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_manifest_has_maps_not_spatial() {
        let maps = [WorldMapRef {
            id: "main".into(),
            name: "main".into(),
            path: "maps/main".into(),
        }];
        let raw = world_manifest_toml("my-world", &maps);
        let manifest = parse_manifest(&raw).unwrap();
        assert_eq!(manifest.world.id, "my-world");
        assert_eq!(manifest.schema_version, WORLD_SCHEMA_VERSION);
        assert!(manifest.spatial.is_none());
        assert_eq!(manifest.maps.len(), 1);
        assert_eq!(manifest.maps[0].path, "maps/main");
        assert!(!raw.contains("[spatial]"));
    }

    #[test]
    fn map_manifest_includes_spatial() {
        let raw = map_manifest_toml("main", alpha_default_preset());
        let manifest = parse_map_manifest(&raw).unwrap();
        assert_eq!(manifest.map.id, "main");
        assert_eq!(manifest.spatial.preset_id, "wide_2000");
        assert_eq!(manifest.spatial.cols, 55);
        assert!(!raw.contains("[world]"));
    }

    #[test]
    fn legacy_detects_root_spatial_section() {
        let legacy = manifest_toml_with_preset_legacy("old");
        assert!(looks_legacy_single_level(&legacy, false));
        let maps = [WorldMapRef {
            id: "main".into(),
            name: "main".into(),
            path: "maps/main".into(),
        }];
        assert!(!looks_legacy_single_level(
            &world_manifest_toml("new", &maps),
            false
        ));
        assert!(looks_legacy_single_level(
            &world_manifest_toml("new", &maps),
            true
        ));
    }

    fn manifest_toml_with_preset_legacy(world_id: &str) -> String {
        let spatial = SpatialConfig::alpha_default();
        format!(
            "[world]\nid = \"{world_id}\"\nname = \"{world_id}\"\nversion = \"0.3.0\"\n\n\
             [spatial]\npreset_id = \"{}\"\ngrid_id = \"{}\"\nwidth_m = {}\nheight_m = {}\n\
             cols = {}\nrows = {}\nneighbor_center_distance_m = {}\norigin_x_m = 0.0\n\
             origin_y_m = 0.0\norientation = \"pointy-top\"\n",
            spatial.preset_id,
            spatial.grid_id,
            spatial.width_m,
            spatial.height_m,
            spatial.cols,
            spatial.rows,
            spatial.neighbor_center_distance_m,
        )
    }

    #[test]
    fn author_name_normalizes_to_safe_id() {
        assert_eq!(normalize_world_id(" My World ").unwrap(), "my-world");
        assert!(normalize_world_id("!!!").is_err());
    }
}
