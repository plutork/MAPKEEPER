//! Land silhouette types and constants (D-66).

pub const LAND_MASK_LAYER_ID: &str = "land_mask";
pub const LAND_MASK_OCEAN: &str = "ocean";
pub const LAND_MASK_LAND: &str = "land";
pub const LAND_MASK_INLAND_SEA: &str = "inland_sea";

/// Macro silhouette layout (D-62). Shore character is orthogonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutClass {
    Pangea,
    Continents,
    Archipelago,
    Island,
    ContinentAndIslands,
    Mediterranean,
}

impl LayoutClass {
    pub const ALL: [LayoutClass; 6] = [
        LayoutClass::Pangea,
        LayoutClass::Continents,
        LayoutClass::Archipelago,
        LayoutClass::Island,
        LayoutClass::ContinentAndIslands,
        LayoutClass::Mediterranean,
    ];

    pub fn parse(raw: &str) -> LayoutClass {
        match raw.trim().to_ascii_lowercase().as_str() {
            "continents" | "dual" | "two-landmasses" => LayoutClass::Continents,
            "archipelago" => LayoutClass::Archipelago,
            "island" => LayoutClass::Island,
            "continent_and_islands" | "continent-and-islands" => LayoutClass::ContinentAndIslands,
            "mediterranean" => LayoutClass::Mediterranean,
            _ => LayoutClass::Pangea,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            LayoutClass::Pangea => "pangea",
            LayoutClass::Continents => "continents",
            LayoutClass::Archipelago => "archipelago",
            LayoutClass::Island => "island",
            LayoutClass::ContinentAndIslands => "continent_and_islands",
            LayoutClass::Mediterranean => "mediterranean",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LayoutClass::Pangea => "Pangea",
            LayoutClass::Continents => "Continents",
            LayoutClass::Archipelago => "Archipelago",
            LayoutClass::Island => "Island",
            LayoutClass::ContinentAndIslands => "Continent + islands",
            LayoutClass::Mediterranean => "Mediterranean",
        }
    }
}

/// Backward-compatible alias.
pub type SilhouetteStyle = LayoutClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShoreCharacter {
    Smooth,
    Jagged,
}

impl ShoreCharacter {
    pub fn parse(raw: &str) -> ShoreCharacter {
        match raw.trim().to_ascii_lowercase().as_str() {
            "jagged" => ShoreCharacter::Jagged,
            _ => ShoreCharacter::Smooth,
        }
    }
}

/// Bias zone in normalized map space (~[-1,1]) — seed placement, not final land.
#[derive(Debug, Clone, Copy)]
pub struct LayoutBlob {
    pub cx: f64,
    pub cy: f64,
    pub rx: f64,
    pub ry: f64,
}

/// Growth plan / bias skeleton for a layout class (D-66).
#[derive(Debug, Clone, Copy)]
pub struct LayoutRecipe {
    pub id: &'static str,
    pub layout_class: LayoutClass,
    /// Preferred seed zones (centers + soft radii for jitter).
    pub seed_zones: &'static [LayoutBlob],
    /// Optional basin seed (mediterranean / crescent carve).
    pub hole: Option<LayoutBlob>,
    /// Target land fraction of map cells (approx).
    pub land_fraction: f64,
    /// Primary growth blobs (large).
    pub primary_count: u8,
    /// Smaller overlay / satellite blobs.
    pub satellite_count: u8,
    /// 0 = keep masses apart · 1 = allow merge.
    pub merge_bias: f64,
    /// Stretch growth along a seeded axis (0..1).
    pub elongation: f64,
    /// Base coastal irregularity (shore character scales this).
    pub irregularity: f64,
}
