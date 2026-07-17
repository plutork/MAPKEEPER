//! World map size presets — hex-rectangle 16:9 bounds for new worlds (D-40, D-49, D-73).
//!
//! Author-facing presets map to `hex-rectangle` in `map/manifest.json`.
//! Canvas/viewport is separate (pan/zoom — roadmap 4.2).

use crate::hex::MapBounds;

/// Author-facing map size presets for create/generate wizards (D-40, D-48, D-49, D-73).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapPreset {
    Small,
    Medium,
    Large,
    Epic,
    Grand,
    World,
}

impl MapPreset {
    pub fn id(self) -> &'static str {
        match self {
            MapPreset::Small => "small",
            MapPreset::Medium => "medium",
            MapPreset::Large => "large",
            MapPreset::Epic => "epic",
            MapPreset::Grand => "grand",
            MapPreset::World => "world",
        }
    }

    /// Width × height (odd-r offset rectangle, 16:9) for this tier.
    pub fn dimensions(self) -> (i32, i32) {
        match self {
            // map-preset--ladder-retune-500 (D-73): geo ×~3 from Small~500 → World~100k
            MapPreset::Small => (30, 17),
            MapPreset::Medium => (52, 29),
            MapPreset::Large => (89, 50),
            MapPreset::Epic => (155, 87),
            MapPreset::Grand => (267, 150),
            MapPreset::World => (421, 237),
        }
    }

    pub fn bounds(self) -> MapBounds {
        let (w, h) = self.dimensions();
        MapBounds::new(w, h)
    }

    /// In-bounds cell count for author-facing labels.
    pub fn approx_cell_count(self) -> u32 {
        let (w, h) = self.dimensions();
        rect_cell_count(w, h)
    }

    pub fn author_label(self) -> &'static str {
        match self {
            MapPreset::Small => "Small (~510 cells)",
            MapPreset::Medium => "Medium (~1,508 cells)",
            MapPreset::Large => "Large (~4,450 cells)",
            MapPreset::Epic => "Epic (~13,485 cells)",
            MapPreset::Grand => "Grand (~40,050 cells)",
            MapPreset::World => "World (~99,777 cells, not stable)",
        }
    }

    /// Presets allowed in create/generate wizards.
    pub fn wizard_presets() -> &'static [MapPreset] {
        &[
            MapPreset::Small,
            MapPreset::Medium,
            MapPreset::Large,
            MapPreset::Epic,
            MapPreset::Grand,
            MapPreset::World,
        ]
    }
}

pub fn parse_map_preset(raw: &str) -> Option<MapPreset> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "small" | "s" => Some(MapPreset::Small),
        "medium" | "m" => Some(MapPreset::Medium),
        "large" | "l" => Some(MapPreset::Large),
        "epic" | "e" => Some(MapPreset::Epic),
        "grand" | "g" => Some(MapPreset::Grand),
        "world" | "w" => Some(MapPreset::World),
        _ => None,
    }
}

/// Cell count for a filled hex rectangle (offset W×H).
pub fn rect_cell_count(width: i32, height: i32) -> u32 {
    (width.max(0) * height.max(0)) as u32
}

/// Default bounds when `map/manifest.json` is missing (pre-D-36 folders).
pub fn legacy_default_bounds() -> MapBounds {
    MapPreset::Small.bounds()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_cell_counts() {
        assert_eq!(MapPreset::Small.approx_cell_count(), 510);
        assert_eq!(MapPreset::Medium.approx_cell_count(), 1508);
        assert_eq!(MapPreset::Large.approx_cell_count(), 4450);
        assert_eq!(MapPreset::Epic.approx_cell_count(), 13485);
        assert_eq!(MapPreset::Grand.approx_cell_count(), 40050);
        assert_eq!(MapPreset::World.approx_cell_count(), 99777);
    }

    #[test]
    fn preset_aspect_ratio_roughly_16_9() {
        for preset in MapPreset::wizard_presets() {
            let (w, h) = preset.dimensions();
            let ratio = w as f64 / h as f64;
            assert!(
                (ratio - 16.0 / 9.0).abs() < 0.08,
                "preset {:?} ratio {ratio}",
                preset
            );
        }
    }

    #[test]
    fn parse_preset_ids() {
        assert_eq!(parse_map_preset("large"), Some(MapPreset::Large));
        assert_eq!(parse_map_preset("M"), Some(MapPreset::Medium));
        assert_eq!(parse_map_preset("epic"), Some(MapPreset::Epic));
        assert_eq!(parse_map_preset("grand"), Some(MapPreset::Grand));
        assert_eq!(parse_map_preset("W"), Some(MapPreset::World));
    }
}
