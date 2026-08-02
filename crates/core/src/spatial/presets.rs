//! Product catalog for extent-primary map presets (N-014 + N-016 + N-017).
//! Create catalog SoT — web must not invent footprints or area.

/// Alpha primary-grid default neighbour-center distance (meters).
pub const ALPHA_NEIGHBOR_CENTER_DISTANCE_M: f64 = 1000.0;

/// Provisional alpha Create catalog ceiling (cells). Not a spatial-model limit.
pub const ALPHA_CREATE_CATALOG_MAX_CELLS: u32 = 50_000;

/// Pre-agreed physical-extent preset (full-hex rectangular footprint).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapExtentPreset {
    pub id: &'static str,
    /// Mnemonic Create-card title (N-017); not identity.
    pub display_name: &'static str,
    pub width_m: f64,
    pub height_m: f64,
    pub cols: u32,
    pub rows: u32,
}

/// Cost tier label (N-016) — not the Default role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostTier {
    Light,
    Standard,
    Large,
    Heavy,
}

impl CostTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Standard => "Standard",
            Self::Large => "Large",
            Self::Heavy => "Heavy",
        }
    }
}

/// Bounding-box extent (meters) for a pointy-top odd-r full-hex rectangle.
pub fn metric_extent_m(cols: u32, rows: u32, neighbor_center_distance_m: f64) -> (f64, f64) {
    let d = neighbor_center_distance_m;
    let size = red_blob_radius_m(d);
    let x_step = d;
    let y_step = d * 3.0_f64.sqrt() / 2.0;
    let center_w = if cols == 0 {
        0.0
    } else {
        (cols - 1) as f64 * x_step + if rows > 1 { x_step / 2.0 } else { 0.0 }
    };
    let center_h = if rows == 0 {
        0.0
    } else {
        (rows - 1) as f64 * y_step
    };
    (center_w + 2.0 * size, center_h + 2.0 * size)
}

/// Derived map area from full hex cells (N-017). Not a hand-authored SoT.
/// `map_area_km2 = cells × (√3/2) × neighbor_center_distance_km²`
pub fn map_area_km2(cell_count: u32, neighbor_center_distance_m: f64) -> f64 {
    let d_km = neighbor_center_distance_m / 1000.0;
    cell_count as f64 * (3.0_f64.sqrt() / 2.0) * d_km * d_km
}

const fn preset(
    id: &'static str,
    display_name: &'static str,
    cols: u32,
    rows: u32,
    width_m: f64,
    height_m: f64,
) -> MapExtentPreset {
    MapExtentPreset {
        id,
        display_name,
        width_m,
        height_m,
        cols,
        rows,
    }
}

/// Smallest Create rung — keeps historical id; wide ~16:9 family (N-016).
pub const PRESET_REGIONAL_12X8: MapExtentPreset =
    preset("regional_12x8", "Pocket", 12, 8, 12_654.701, 7_216.878);

pub const PRESET_WIDE_250: MapExtentPreset =
    preset("wide_250", "Hamlet", 20, 13, 20_654.701, 11_547.005);
pub const PRESET_WIDE_500: MapExtentPreset =
    preset("wide_500", "Vale", 28, 18, 28_654.701, 15_877.132);
pub const PRESET_WIDE_1000: MapExtentPreset =
    preset("wide_1000", "Shire", 40, 26, 40_654.701, 22_805.336);
/// Default Create selection (~2k cells).
pub const PRESET_WIDE_2000: MapExtentPreset =
    preset("wide_2000", "Frontier", 55, 36, 55_654.701, 31_465.590);
pub const PRESET_WIDE_4000: MapExtentPreset =
    preset("wide_4000", "County", 78, 51, 78_654.701, 44_455.971);
pub const PRESET_WIDE_7500: MapExtentPreset =
    preset("wide_7500", "March", 107, 70, 107_654.701, 60_910.453);
pub const PRESET_WIDE_12000: MapExtentPreset =
    preset("wide_12000", "Duchy", 136, 88, 136_654.701, 76_498.911);
pub const PRESET_WIDE_18000: MapExtentPreset =
    preset("wide_18000", "Kingdom", 166, 108, 166_654.701, 93_819.419);
pub const PRESET_WIDE_26000: MapExtentPreset =
    preset("wide_26000", "Empire", 200, 130, 200_654.701, 112_871.978);
pub const PRESET_WIDE_36000: MapExtentPreset =
    preset("wide_36000", "Realm", 235, 153, 235_654.701, 132_790.562);
pub const PRESET_WIDE_50000: MapExtentPreset =
    preset("wide_50000", "Dominion", 277, 180, 277_654.701, 156_173.248);

/// Legacy N-014 rungs — resolvable for existing worlds; not Create cards.
pub const PRESET_REGIONAL_26X16: MapExtentPreset =
    preset("regional_26x16", "Legacy", 26, 16, 26_000.0, 16_000.0);
pub const PRESET_REGIONAL_52X32: MapExtentPreset =
    preset("regional_52x32", "Legacy", 52, 32, 52_000.0, 32_000.0);

/// Alpha Create catalog (N-016) — validated wide non-linear ladder.
const CREATE_PRESETS: &[MapExtentPreset] = &[
    PRESET_REGIONAL_12X8,
    PRESET_WIDE_250,
    PRESET_WIDE_500,
    PRESET_WIDE_1000,
    PRESET_WIDE_2000,
    PRESET_WIDE_4000,
    PRESET_WIDE_7500,
    PRESET_WIDE_12000,
    PRESET_WIDE_18000,
    PRESET_WIDE_26000,
    PRESET_WIDE_36000,
    PRESET_WIDE_50000,
];

const LEGACY_PRESETS: &[MapExtentPreset] = &[PRESET_REGIONAL_26X16, PRESET_REGIONAL_52X32];

pub fn create_presets() -> &'static [MapExtentPreset] {
    CREATE_PRESETS
}

/// All resolvable presets (Create + legacy lookup).
pub fn presets() -> &'static [MapExtentPreset] {
    CREATE_PRESETS
}

pub fn alpha_default_preset() -> &'static MapExtentPreset {
    &PRESET_WIDE_2000
}

pub fn preset_by_id(id: &str) -> Option<&'static MapExtentPreset> {
    CREATE_PRESETS
        .iter()
        .chain(LEGACY_PRESETS.iter())
        .find(|p| p.id == id)
}

pub fn cell_count(preset: &MapExtentPreset) -> u32 {
    preset.cols.saturating_mul(preset.rows)
}

pub fn cost_tier(preset: &MapExtentPreset) -> CostTier {
    let cells = cell_count(preset);
    if cells <= 5_000 {
        CostTier::Light
    } else if cells <= 15_000 {
        CostTier::Standard
    } else if cells <= 30_000 {
        CostTier::Large
    } else {
        CostTier::Heavy
    }
}

pub fn is_default_preset(preset: &MapExtentPreset) -> bool {
    preset.id == alpha_default_preset().id
}

/// Red Blob pointy-top radius (center→vertex) from neighbour-center distance.
pub fn red_blob_radius_m(neighbor_center_distance_m: f64) -> f64 {
    neighbor_center_distance_m / 3.0_f64.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbor_distance_matches_red_blob_geometry() {
        let d = ALPHA_NEIGHBOR_CENTER_DISTANCE_M;
        let size = red_blob_radius_m(d);
        let neighbor = size * 3.0_f64.sqrt();
        assert!((neighbor - d).abs() < 1e-9);
    }

    #[test]
    fn create_catalog_ladder_and_default() {
        assert_eq!(create_presets().len(), 12);
        assert_eq!(alpha_default_preset().id, "wide_2000");
        assert_eq!(alpha_default_preset().display_name, "Frontier");
        assert_eq!(cell_count(alpha_default_preset()), 1980);
        assert!(is_default_preset(alpha_default_preset()));
        assert!(!is_default_preset(&PRESET_REGIONAL_12X8));
        assert!(cell_count(&PRESET_WIDE_50000) <= ALPHA_CREATE_CATALOG_MAX_CELLS);
        assert_eq!(cost_tier(&PRESET_WIDE_2000), CostTier::Light);
        assert_eq!(cost_tier(&PRESET_WIDE_12000), CostTier::Standard);
        assert_eq!(cost_tier(&PRESET_WIDE_26000), CostTier::Large);
        assert_eq!(cost_tier(&PRESET_WIDE_50000), CostTier::Heavy);
        assert_eq!(PRESET_WIDE_18000.display_name, "Kingdom");
        assert_eq!(PRESET_WIDE_26000.display_name, "Empire");
    }

    #[test]
    fn derived_area_uses_hex_sum_formula() {
        let area = map_area_km2(1980, ALPHA_NEIGHBOR_CENTER_DISTANCE_M);
        let expected = 1980.0 * (3.0_f64.sqrt() / 2.0);
        assert!((area - expected).abs() < 1e-9);
        assert!((area - 1714.7).abs() < 0.1);
    }

    #[test]
    fn legacy_presets_still_resolve() {
        assert!(preset_by_id("regional_26x16").is_some());
        assert!(preset_by_id("regional_52x32").is_some());
        assert!(!create_presets().iter().any(|p| p.id == "regional_26x16"));
    }

    #[test]
    fn stored_extent_matches_metric_helper() {
        for preset in create_presets() {
            let (w, h) =
                metric_extent_m(preset.cols, preset.rows, ALPHA_NEIGHBOR_CENTER_DISTANCE_M);
            assert!((w - preset.width_m).abs() < 0.01, "{}", preset.id);
            assert!((h - preset.height_m).abs() < 0.01, "{}", preset.id);
        }
    }
}
