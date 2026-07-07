//! World map size presets — hex-radius bounds for new worlds (D-40).
//!
//! Author-facing presets map to a `hex-radius` stored in `map/manifest.json`.
//! Canvas/viewport is separate (pan/zoom — roadmap 4.2 Slice 2).

/// Wizard presets exposed before viewport culling ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapPreset {
    Small,
    Medium,
    Large,
    Epic,
}

impl MapPreset {
    pub fn id(self) -> &'static str {
        match self {
            MapPreset::Small => "small",
            MapPreset::Medium => "medium",
            MapPreset::Large => "large",
            MapPreset::Epic => "epic",
        }
    }

    pub fn radius(self) -> i32 {
        match self {
            MapPreset::Small => 6,
            MapPreset::Medium => 18,
            MapPreset::Large => 50,
            // viewport-pan-zoom-culling: Epic preset unlock (~30K cells).
            MapPreset::Epic => 100,
        }
    }

    /// Approximate in-bounds cell count for author-facing labels.
    pub fn approx_cell_count(self) -> u32 {
        hex_cell_count(self.radius())
    }

    pub fn author_label(self) -> &'static str {
        match self {
            MapPreset::Small => "Small (~127 cells)",
            MapPreset::Medium => "Medium (~1,027 cells)",
            MapPreset::Large => "Large (~7,651 cells)",
            MapPreset::Epic => "Epic (~30,301 cells)",
        }
    }

    /// Presets allowed in create/generate wizards.
    pub fn wizard_presets() -> &'static [MapPreset] {
        &[MapPreset::Small, MapPreset::Medium, MapPreset::Large, MapPreset::Epic]
    }
}

pub fn parse_map_preset(raw: &str) -> Option<MapPreset> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "small" | "s" => Some(MapPreset::Small),
        "medium" | "m" => Some(MapPreset::Medium),
        "large" | "l" => Some(MapPreset::Large),
        "epic" | "e" => Some(MapPreset::Epic),
        _ => None,
    }
}

/// Cell count for a filled hex disk: N = 1 + 3·r·(r+1).
pub fn hex_cell_count(radius: i32) -> u32 {
    if radius <= 0 {
        return 1;
    }
    let r = radius as u32;
    1 + 3 * r * (r + 1)
}

/// Default bounds when `map/manifest.json` is missing (pre-D-36 worlds).
pub const LEGACY_DEFAULT_RADIUS: i32 = 6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_cell_counts() {
        assert_eq!(MapPreset::Small.approx_cell_count(), 127);
        assert_eq!(MapPreset::Medium.approx_cell_count(), 1027);
        assert_eq!(MapPreset::Large.approx_cell_count(), 7651);
        assert_eq!(MapPreset::Epic.approx_cell_count(), 30301);
    }

    #[test]
    fn parse_preset_ids() {
        assert_eq!(parse_map_preset("large"), Some(MapPreset::Large));
        assert_eq!(parse_map_preset("M"), Some(MapPreset::Medium));
        assert_eq!(parse_map_preset("epic"), Some(MapPreset::Epic));
    }
}
