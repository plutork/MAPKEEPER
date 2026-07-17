use serde::{Deserialize, Serialize};

use crate::spatial::{
    alpha_default_preset, preset_by_id, MapExtentPreset, ALPHA_NEIGHBOR_CENTER_DISTANCE_M,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldManifest {
    pub world: WorldIdentity,
    /// Immutable spatial configuration (N-014). Absent → ensure writes default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<SpatialConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldIdentity {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// Immutable per-world spatial configuration stored in `mapkeeper.toml`.
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
}

pub fn manifest_toml(world_id: &str) -> String {
    manifest_toml_with_preset(world_id, alpha_default_preset())
}

pub fn manifest_toml_with_preset(world_id: &str, preset: &MapExtentPreset) -> String {
    let spatial = SpatialConfig::from_preset(preset);
    format!(
        "# mapkeeper world workspace\n\n\
         [world]\n\
         id = \"{world_id}\"\n\
         name = \"{world_id}\"\n\
         version = \"0.3.0\"\n\n\
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

pub fn parse_manifest(raw: &str) -> Result<WorldManifest, toml::de::Error> {
    toml::from_str(raw)
}

pub fn render_manifest(manifest: &WorldManifest) -> Result<String, toml::ser::Error> {
    let body = toml::to_string_pretty(manifest)?;
    Ok(format!("# mapkeeper world workspace\n\n{body}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_manifest_includes_spatial_config() {
        let raw = manifest_toml("my-world");
        let manifest = parse_manifest(&raw).unwrap();
        assert_eq!(manifest.world.id, "my-world");
        assert_eq!(manifest.world.version, "0.3.0");
        let spatial = manifest.spatial.expect("spatial section");
        assert_eq!(spatial.preset_id, "wide_2000");
        assert_eq!(spatial.cols, 55);
        assert_eq!(spatial.rows, 36);
        assert_eq!(spatial.neighbor_center_distance_m, 1000.0);
        assert!(!raw.contains("[map]"));
        assert!(!raw.contains("[build]"));
        assert!(!raw.contains("[history]"));
    }

    #[test]
    fn author_name_normalizes_to_safe_id() {
        assert_eq!(normalize_world_id(" My World ").unwrap(), "my-world");
        assert!(normalize_world_id("!!!").is_err());
    }
}
