//! Step 3 world pipeline: land silhouette (`land_mask`) generators.
//!
//! D-66 / step3-organic-silhouette-v1: layout_class → growth recipe (bias) →
//! seeded layered land growth → cleanup → land_mask. Not ellipse-union drawings.

use crate::hex::{Axial, MapBounds};
use crate::layer::{DenseLayer, DenseState, LayerValue};

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

macro_rules! zones {
    ($($cx:expr, $cy:expr, $rx:expr, $ry:expr);+ $(;)?) => {
        &[$(LayoutBlob { cx: $cx, cy: $cy, rx: $rx, ry: $ry }),+]
    };
}

/// Growth-plan catalog (D-66): ~5 plans × 6 classes. Zones bias seeds; growth makes form.
pub static RECIPE_CATALOG: &[LayoutRecipe] = &[
    LayoutRecipe {
        id: "pangea_irregular",
        layout_class: LayoutClass::Pangea,
        seed_zones: zones!(-0.1, 0.05, 0.35, 0.30; 0.25, -0.1, 0.28, 0.25),
        hole: None,
        land_fraction: 0.52,
        primary_count: 1,
        satellite_count: 8,
        merge_bias: 0.85,
        elongation: 0.35,
        irregularity: 0.55,
    },
    LayoutRecipe {
        id: "pangea_l_mass",
        layout_class: LayoutClass::Pangea,
        seed_zones: zones!(-0.35, 0.0, 0.22, 0.45; 0.15, 0.35, 0.40, 0.20),
        hole: None,
        land_fraction: 0.48,
        primary_count: 2,
        satellite_count: 6,
        merge_bias: 0.9,
        elongation: 0.55,
        irregularity: 0.5,
    },
    LayoutRecipe {
        id: "pangea_crescent",
        layout_class: LayoutClass::Pangea,
        seed_zones: zones!(0.0, 0.0, 0.45, 0.40),
        hole: Some(LayoutBlob { cx: 0.28, cy: 0.05, rx: 0.28, ry: 0.32 }),
        land_fraction: 0.50,
        primary_count: 1,
        satellite_count: 7,
        merge_bias: 0.8,
        elongation: 0.4,
        irregularity: 0.6,
    },
    LayoutRecipe {
        id: "pangea_c_shape",
        layout_class: LayoutClass::Pangea,
        seed_zones: zones!(
            -0.45, -0.3, 0.22, 0.18;
            -0.5, 0.05, 0.20, 0.22;
            -0.35, 0.4, 0.25, 0.18;
            0.15, 0.45, 0.22, 0.16
        ),
        hole: None,
        land_fraction: 0.46,
        primary_count: 3,
        satellite_count: 5,
        merge_bias: 0.75,
        elongation: 0.45,
        irregularity: 0.58,
    },
    LayoutRecipe {
        id: "pangea_hooked",
        layout_class: LayoutClass::Pangea,
        seed_zones: zones!(-0.2, 0.0, 0.35, 0.28; 0.4, -0.2, 0.22, 0.18; 0.5, 0.2, 0.15, 0.22),
        hole: None,
        land_fraction: 0.47,
        primary_count: 2,
        satellite_count: 7,
        merge_bias: 0.7,
        elongation: 0.5,
        irregularity: 0.62,
    },
    LayoutRecipe {
        id: "continents_l_and_blob",
        layout_class: LayoutClass::Continents,
        seed_zones: zones!(-0.5, -0.05, 0.22, 0.38; 0.48, 0.08, 0.28, 0.32),
        hole: None,
        land_fraction: 0.42,
        primary_count: 2,
        satellite_count: 5,
        merge_bias: 0.15,
        elongation: 0.45,
        irregularity: 0.55,
    },
    LayoutRecipe {
        id: "continents_crescent_pair",
        layout_class: LayoutClass::Continents,
        seed_zones: zones!(-0.48, 0.0, 0.28, 0.35; 0.48, 0.08, 0.26, 0.32),
        hole: Some(LayoutBlob { cx: -0.28, cy: 0.0, rx: 0.16, ry: 0.22 }),
        land_fraction: 0.40,
        primary_count: 2,
        satellite_count: 4,
        merge_bias: 0.1,
        elongation: 0.4,
        irregularity: 0.58,
    },
    LayoutRecipe {
        id: "continents_broken_chain",
        layout_class: LayoutClass::Continents,
        seed_zones: zones!(
            -0.65, 0.2, 0.18, 0.22;
            -0.2, -0.15, 0.20, 0.24;
            0.35, 0.25, 0.18, 0.20;
            0.68, -0.2, 0.16, 0.18
        ),
        hole: None,
        land_fraction: 0.38,
        primary_count: 3,
        satellite_count: 4,
        merge_bias: 0.25,
        elongation: 0.5,
        irregularity: 0.6,
    },
    LayoutRecipe {
        id: "continents_c_and_mass",
        layout_class: LayoutClass::Continents,
        seed_zones: zones!(
            -0.55, -0.25, 0.18, 0.16;
            -0.58, 0.1, 0.16, 0.20;
            -0.45, 0.38, 0.20, 0.14;
            0.45, 0.0, 0.30, 0.35
        ),
        hole: None,
        land_fraction: 0.40,
        primary_count: 2,
        satellite_count: 5,
        merge_bias: 0.2,
        elongation: 0.42,
        irregularity: 0.55,
    },
    LayoutRecipe {
        id: "continents_irregular_dual",
        layout_class: LayoutClass::Continents,
        seed_zones: zones!(-0.4, -0.1, 0.28, 0.28; 0.42, 0.12, 0.26, 0.30),
        hole: None,
        land_fraction: 0.41,
        primary_count: 2,
        satellite_count: 6,
        merge_bias: 0.12,
        elongation: 0.38,
        irregularity: 0.57,
    },
    LayoutRecipe {
        id: "archipelago_chain",
        layout_class: LayoutClass::Archipelago,
        seed_zones: zones!(
            -0.55, 0.2, 0.12, 0.12;
            -0.25, -0.15, 0.12, 0.12;
            0.05, 0.25, 0.12, 0.12;
            0.35, -0.1, 0.12, 0.12;
            0.6, 0.15, 0.12, 0.12
        ),
        hole: None,
        land_fraction: 0.22,
        primary_count: 5,
        satellite_count: 6,
        merge_bias: 0.08,
        elongation: 0.55,
        irregularity: 0.65,
    },
    LayoutRecipe {
        id: "archipelago_ring_gap",
        layout_class: LayoutClass::Archipelago,
        seed_zones: zones!(
            0.0, -0.45, 0.12, 0.10;
            0.4, -0.15, 0.12, 0.10;
            0.35, 0.35, 0.12, 0.10;
            -0.35, 0.35, 0.12, 0.10;
            -0.4, -0.15, 0.12, 0.10
        ),
        hole: None,
        land_fraction: 0.20,
        primary_count: 5,
        satellite_count: 4,
        merge_bias: 0.05,
        elongation: 0.35,
        irregularity: 0.62,
    },
    LayoutRecipe {
        id: "archipelago_cluster_west",
        layout_class: LayoutClass::Archipelago,
        seed_zones: zones!(
            -0.45, -0.2, 0.14, 0.14;
            -0.55, 0.15, 0.12, 0.12;
            -0.25, 0.25, 0.12, 0.12;
            0.35, 0.0, 0.14, 0.14;
            0.55, -0.25, 0.10, 0.10
        ),
        hole: None,
        land_fraction: 0.21,
        primary_count: 4,
        satellite_count: 7,
        merge_bias: 0.12,
        elongation: 0.4,
        irregularity: 0.6,
    },
    LayoutRecipe {
        id: "archipelago_scattered",
        layout_class: LayoutClass::Archipelago,
        seed_zones: zones!(
            -0.6, -0.3, 0.10, 0.10;
            -0.15, 0.35, 0.10, 0.10;
            0.2, -0.35, 0.10, 0.10;
            0.55, 0.25, 0.10, 0.10;
            0.0, 0.0, 0.10, 0.10
        ),
        hole: None,
        land_fraction: 0.18,
        primary_count: 5,
        satellite_count: 5,
        merge_bias: 0.05,
        elongation: 0.3,
        irregularity: 0.68,
    },
    LayoutRecipe {
        id: "archipelago_twin_groups",
        layout_class: LayoutClass::Archipelago,
        seed_zones: zones!(
            -0.5, 0.0, 0.18, 0.22;
            -0.35, 0.25, 0.12, 0.12;
            0.4, -0.1, 0.18, 0.20;
            0.55, 0.2, 0.12, 0.12
        ),
        hole: None,
        land_fraction: 0.23,
        primary_count: 4,
        satellite_count: 6,
        merge_bias: 0.15,
        elongation: 0.45,
        irregularity: 0.6,
    },
    LayoutRecipe {
        id: "island_hooked",
        layout_class: LayoutClass::Island,
        seed_zones: zones!(-0.05, 0.0, 0.22, 0.18; 0.25, -0.15, 0.12, 0.10),
        hole: None,
        land_fraction: 0.14,
        primary_count: 1,
        satellite_count: 4,
        merge_bias: 0.7,
        elongation: 0.55,
        irregularity: 0.65,
    },
    LayoutRecipe {
        id: "island_crescent",
        layout_class: LayoutClass::Island,
        seed_zones: zones!(0.0, 0.05, 0.22, 0.20),
        hole: Some(LayoutBlob { cx: 0.12, cy: 0.0, rx: 0.12, ry: 0.14 }),
        land_fraction: 0.13,
        primary_count: 1,
        satellite_count: 3,
        merge_bias: 0.75,
        elongation: 0.45,
        irregularity: 0.62,
    },
    LayoutRecipe {
        id: "island_l_mass",
        layout_class: LayoutClass::Island,
        seed_zones: zones!(-0.1, -0.05, 0.14, 0.22; 0.12, 0.15, 0.18, 0.10),
        hole: None,
        land_fraction: 0.13,
        primary_count: 1,
        satellite_count: 3,
        merge_bias: 0.8,
        elongation: 0.5,
        irregularity: 0.6,
    },
    LayoutRecipe {
        id: "island_long",
        layout_class: LayoutClass::Island,
        seed_zones: zones!(0.0, 0.0, 0.32, 0.12),
        hole: None,
        land_fraction: 0.12,
        primary_count: 1,
        satellite_count: 3,
        merge_bias: 0.85,
        elongation: 0.75,
        irregularity: 0.58,
    },
    LayoutRecipe {
        id: "island_irregular",
        layout_class: LayoutClass::Island,
        seed_zones: zones!(0.05, -0.05, 0.20, 0.18),
        hole: None,
        land_fraction: 0.15,
        primary_count: 1,
        satellite_count: 5,
        merge_bias: 0.7,
        elongation: 0.4,
        irregularity: 0.7,
    },
    LayoutRecipe {
        id: "cai_irregular_main",
        layout_class: LayoutClass::ContinentAndIslands,
        seed_zones: zones!(
            0.15, 0.0, 0.32, 0.30;
            -0.55, -0.2, 0.12, 0.12;
            -0.6, 0.25, 0.10, 0.10
        ),
        hole: None,
        land_fraction: 0.36,
        primary_count: 1,
        satellite_count: 7,
        merge_bias: 0.2,
        elongation: 0.4,
        irregularity: 0.58,
    },
    LayoutRecipe {
        id: "cai_west_sats",
        layout_class: LayoutClass::ContinentAndIslands,
        seed_zones: zones!(
            0.25, 0.05, 0.30, 0.28;
            -0.55, 0.0, 0.14, 0.16;
            -0.65, -0.3, 0.10, 0.10;
            -0.45, 0.3, 0.10, 0.10
        ),
        hole: None,
        land_fraction: 0.35,
        primary_count: 1,
        satellite_count: 8,
        merge_bias: 0.15,
        elongation: 0.42,
        irregularity: 0.6,
    },
    LayoutRecipe {
        id: "cai_crescent_sats",
        layout_class: LayoutClass::ContinentAndIslands,
        seed_zones: zones!(0.1, 0.0, 0.32, 0.28; -0.55, 0.15, 0.12, 0.12),
        hole: Some(LayoutBlob { cx: 0.28, cy: 0.05, rx: 0.14, ry: 0.16 }),
        land_fraction: 0.34,
        primary_count: 1,
        satellite_count: 6,
        merge_bias: 0.18,
        elongation: 0.45,
        irregularity: 0.62,
    },
    LayoutRecipe {
        id: "cai_south_chain",
        layout_class: LayoutClass::ContinentAndIslands,
        seed_zones: zones!(
            0.0, -0.15, 0.35, 0.28;
            -0.4, 0.4, 0.10, 0.10;
            0.0, 0.45, 0.10, 0.10;
            0.4, 0.4, 0.10, 0.10
        ),
        hole: None,
        land_fraction: 0.35,
        primary_count: 1,
        satellite_count: 7,
        merge_bias: 0.12,
        elongation: 0.4,
        irregularity: 0.58,
    },
    LayoutRecipe {
        id: "cai_split_main",
        layout_class: LayoutClass::ContinentAndIslands,
        seed_zones: zones!(
            -0.15, 0.1, 0.28, 0.26;
            0.35, -0.15, 0.22, 0.22;
            -0.6, -0.25, 0.10, 0.10
        ),
        hole: None,
        land_fraction: 0.37,
        primary_count: 2,
        satellite_count: 5,
        merge_bias: 0.35,
        elongation: 0.38,
        irregularity: 0.55,
    },
    LayoutRecipe {
        id: "med_ring_gap",
        layout_class: LayoutClass::Mediterranean,
        seed_zones: zones!(0.0, 0.0, 0.48, 0.42),
        hole: Some(LayoutBlob { cx: 0.0, cy: 0.0, rx: 0.28, ry: 0.24 }),
        land_fraction: 0.44,
        primary_count: 1,
        satellite_count: 6,
        merge_bias: 0.85,
        elongation: 0.35,
        irregularity: 0.55,
    },
    LayoutRecipe {
        id: "med_center_basin",
        layout_class: LayoutClass::Mediterranean,
        seed_zones: zones!(0.0, 0.05, 0.50, 0.40),
        hole: Some(LayoutBlob { cx: 0.05, cy: 0.0, rx: 0.32, ry: 0.26 }),
        land_fraction: 0.46,
        primary_count: 1,
        satellite_count: 5,
        merge_bias: 0.9,
        elongation: 0.3,
        irregularity: 0.5,
    },
    LayoutRecipe {
        id: "med_c_basin",
        layout_class: LayoutClass::Mediterranean,
        seed_zones: zones!(
            -0.35, -0.25, 0.22, 0.18;
            -0.4, 0.15, 0.20, 0.22;
            0.1, 0.35, 0.28, 0.18;
            0.35, -0.1, 0.22, 0.24
        ),
        hole: Some(LayoutBlob { cx: -0.05, cy: 0.05, rx: 0.22, ry: 0.20 }),
        land_fraction: 0.42,
        primary_count: 3,
        satellite_count: 4,
        merge_bias: 0.8,
        elongation: 0.4,
        irregularity: 0.58,
    },
    LayoutRecipe {
        id: "med_offset_basin",
        layout_class: LayoutClass::Mediterranean,
        seed_zones: zones!(0.1, -0.05, 0.48, 0.40),
        hole: Some(LayoutBlob { cx: -0.15, cy: 0.1, rx: 0.24, ry: 0.22 }),
        land_fraction: 0.43,
        primary_count: 1,
        satellite_count: 6,
        merge_bias: 0.85,
        elongation: 0.42,
        irregularity: 0.55,
    },
    LayoutRecipe {
        id: "med_twin_split",
        layout_class: LayoutClass::Mediterranean,
        seed_zones: zones!(-0.25, 0.0, 0.32, 0.38; 0.35, 0.05, 0.28, 0.32),
        hole: Some(LayoutBlob { cx: 0.05, cy: 0.0, rx: 0.18, ry: 0.28 }),
        land_fraction: 0.45,
        primary_count: 2,
        satellite_count: 5,
        merge_bias: 0.7,
        elongation: 0.4,
        irregularity: 0.55,
    },
];

pub fn recipes_for(class: LayoutClass) -> Vec<&'static LayoutRecipe> {
    RECIPE_CATALOG
        .iter()
        .filter(|r| r.layout_class == class)
        .collect()
}

pub fn find_recipe(id: &str) -> Option<&'static LayoutRecipe> {
    RECIPE_CATALOG.iter().find(|r| r.id == id)
}

pub fn pick_recipe(class: LayoutClass, seed: u64) -> &'static LayoutRecipe {
    let list = recipes_for(class);
    debug_assert!(!list.is_empty());
    let idx = (seed as usize) % list.len().max(1);
    list[idx]
}

/// Next growth plan for the same class (D-65/D-66).
pub fn next_recipe(class: LayoutClass, current_id: &str, seed: u64) -> &'static LayoutRecipe {
    let list = recipes_for(class);
    if list.is_empty() {
        return pick_recipe(class, seed);
    }
    if list.len() == 1 {
        return list[0];
    }
    let start = (seed as usize) % list.len();
    for offset in 0..list.len() {
        let r = list[(start + offset) % list.len()];
        if r.id != current_id {
            return r;
        }
    }
    list[start]
}

/// Deprecated D-64 helper — kept for tests.
pub fn pick_compare_trio(seed: u64) -> [&'static LayoutRecipe; 3] {
    let mut classes = LayoutClass::ALL;
    for i in (1..classes.len()).rev() {
        let j = (mix64(seed ^ (i as u64 * 0x9E37)) as usize) % (i + 1);
        classes.swap(i, j);
    }
    let a = pick_recipe(classes[0], mix64(seed ^ 0xA11));
    let b = pick_recipe(classes[1], mix64(seed ^ 0xB22));
    let c = pick_recipe(classes[2], mix64(seed ^ 0xC33));
    [a, b, c]
}

pub fn generate_land_mask(
    bounds: &MapBounds,
    style: LayoutClass,
    character: ShoreCharacter,
    seed: u64,
) -> DenseLayer {
    let recipe = pick_recipe(style, seed);
    generate_land_mask_recipe(bounds, recipe, character, seed)
}

/// D-66 Track A: seeded layered land growth from growth-plan recipe.
pub fn generate_land_mask_recipe(
    bounds: &MapBounds,
    recipe: &LayoutRecipe,
    character: ShoreCharacter,
    seed: u64,
) -> DenseLayer {
    let n = bounds.len();
    let mut heights = vec![0.0f64; n];
    let (max_x, max_y) = half_extent(bounds);
    let mut rng = seed ^ 0xD660_06A1_1C00;

    let shore_scale = match character {
        ShoreCharacter::Smooth => 0.55,
        ShoreCharacter::Jagged => 1.15,
    };
    let sharpness = (recipe.irregularity * shore_scale).clamp(0.05, 1.2);
    let decay = match character {
        ShoreCharacter::Smooth => 0.90,
        ShoreCharacter::Jagged => 0.86,
    };

    let zones = recipe.seed_zones;
    let zone_n = zones.len().max(1);

    for i in 0..recipe.primary_count as usize {
        rng = mix64(rng ^ (i as u64 * 0x9E37) ^ 0x0041);
        let zone = &zones[i % zone_n];
        let start = pick_seed_cell(bounds, zone, rng, max_x, max_y, recipe.merge_bias);
        let h0 = 0.92 + unit01(rng ^ 0xA1) * 0.08;
        let (ex, ey) = elongation_axis(rng ^ 0xE1, recipe.elongation);
        grow_blob(
            bounds,
            &mut heights,
            start,
            h0,
            decay,
            sharpness,
            recipe.elongation,
            ex,
            ey,
            rng,
        );
    }

    for i in 0..recipe.satellite_count as usize {
        rng = mix64(rng ^ (i as u64 * 0xC2B2) ^ 0x5A70);
        let zone = &zones[(i + recipe.primary_count as usize) % zone_n];
        let start = pick_seed_cell(bounds, zone, rng, max_x, max_y, recipe.merge_bias);
        let h0 = 0.35 + unit01(rng ^ 0xB2) * 0.35;
        let sat_decay = decay * (0.88 + unit01(rng ^ 0xD2) * 0.08);
        let (ex, ey) = elongation_axis(rng ^ 0xE2, recipe.elongation * 0.7);
        grow_blob(
            bounds,
            &mut heights,
            start,
            h0,
            sat_decay,
            sharpness * 0.9,
            recipe.elongation * 0.7,
            ex,
            ey,
            rng,
        );
    }

    if let Some(hole) = recipe.hole {
        rng = mix64(rng ^ 0x401E);
        let start = pick_seed_cell(bounds, &hole, rng, max_x, max_y, 1.0);
        carve_pit(bounds, &mut heights, start, 0.55, 0.88, rng);
    }

    let threshold = threshold_for_fraction(&heights, recipe.land_fraction);
    let mut layer = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, n);
    for index in 0..n {
        let value = if heights[index] > threshold {
            LAND_MASK_LAND
        } else {
            LAND_MASK_OCEAN
        };
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(value.to_string())),
        );
    }

    apply_shore_fringe(bounds, &mut layer, character, seed);
    // D-66 dogfood: kill 1-hex-wide axis strips left by growth.
    prune_thin_corridors(bounds, &mut layer, 4);
    remove_tiny_islands(bounds, &mut layer, min_island_cells(recipe));
    // Third pass: remember layout_class identity (e.g. Pangea = one mass).
    enforce_layout_class(bounds, &mut layer, recipe.layout_class);
    mark_inland_seas(bounds, &mut layer);
    layer
}

/// Sync elevation from `land_mask`: land = 1, non-land = 0.
pub fn elevation_from_land_mask(bounds: &MapBounds, land_mask: &DenseLayer) -> DenseLayer {
    let mut elevation = DenseLayer::new_integer("elevation", bounds.len());
    for index in 0..bounds.len() {
        let value = match land_mask.state(index) {
            DenseState::Value(LayerValue::Text(kind)) if kind == LAND_MASK_LAND => 1,
            _ => 0,
        };
        elevation.set(index, DenseState::Value(LayerValue::Int(value)));
    }
    elevation
}

pub fn normalize_kind(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        LAND_MASK_LAND => LAND_MASK_LAND,
        LAND_MASK_INLAND_SEA => LAND_MASK_INLAND_SEA,
        _ => LAND_MASK_OCEAN,
    }
}

fn min_island_cells(recipe: &LayoutRecipe) -> usize {
    match recipe.layout_class {
        LayoutClass::Archipelago => 2,
        LayoutClass::Island => 3,
        _ => 4,
    }
}

fn elongation_axis(seed: u64, amount: f64) -> (f64, f64) {
    if amount < 0.05 {
        return (1.0, 0.0);
    }
    let angle = unit01(seed) * std::f64::consts::TAU;
    (angle.cos(), angle.sin())
}

fn pick_seed_cell(
    bounds: &MapBounds,
    zone: &LayoutBlob,
    seed: u64,
    max_x: f64,
    max_y: f64,
    merge_bias: f64,
) -> usize {
    let jitter = 0.35 + (1.0 - merge_bias) * 0.4;
    let nx = zone.cx + (unit01(seed ^ 0x11) * 2.0 - 1.0) * zone.rx * jitter;
    let ny = zone.cy + (unit01(seed ^ 0x22) * 2.0 - 1.0) * zone.ry * jitter;
    nearest_index(bounds, nx, ny, max_x, max_y)
}

fn nearest_index(bounds: &MapBounds, nx: f64, ny: f64, max_x: f64, max_y: f64) -> usize {
    let mut best = 0usize;
    let mut best_d = f64::MAX;
    for index in 0..bounds.len() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let (x, y) = cell.to_pixel(1.0);
        let cx = if max_x > 0.0 { x / max_x } else { 0.0 };
        let cy = if max_y > 0.0 { y / max_y } else { 0.0 };
        let d = (cx - nx).hypot(cy - ny);
        if d < best_d {
            best_d = d;
            best = index;
        }
    }
    best
}

/// Azgaar-style blob growth: parent height × decay × sharpness RNG; max-blend layers.
/// Neighbor order is shuffled each step so hex axes do not form persistent strips (D-66).
fn grow_blob(
    bounds: &MapBounds,
    heights: &mut [f64],
    start: usize,
    start_h: f64,
    decay: f64,
    sharpness: f64,
    elongation: f64,
    axis_x: f64,
    axis_y: f64,
    seed: u64,
) {
    let n = heights.len();
    let mut used = vec![false; n];
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    heights[start] = heights[start].max(start_h);
    used[start] = true;
    queue.push_back(start);
    let mut step = 0u64;
    // Mild elongation only — strong axis stretch made comb-like coasts.
    let elong = (elongation * 0.12).clamp(0.0, 0.12);
    while let Some(index) = queue.pop_front() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let parent_h = heights[index];
        if parent_h < 0.02 {
            continue;
        }
        let (px, py) = cell.to_pixel(1.0);
        let mut neigh = cell.neighbors();
        shuffle_six(&mut neigh, seed ^ step.wrapping_mul(0xC2B2) ^ (index as u64));
        for ncell in neigh {
            let Some(ni) = bounds.index_of(ncell) else {
                continue;
            };
            if used[ni] {
                continue;
            }
            used[ni] = true;
            step = step.wrapping_add(1);
            // Sharpness band kept narrow so ridges cannot runaway along one hex ray.
            let mut mod_v = if sharpness <= 0.05 {
                1.0
            } else {
                let band = sharpness.min(0.55);
                unit01(seed ^ step.wrapping_mul(0x9E37) ^ (ni as u64)) * band + (1.0 - band * 0.5)
            };
            mod_v = mod_v.clamp(0.72, 1.12);
            if elong > 0.01 {
                let (nx, ny) = ncell.to_pixel(1.0);
                let dx = nx - px;
                let dy = ny - py;
                let along = (dx * axis_x + dy * axis_y).abs();
                let across = (dx * -axis_y + dy * axis_x).abs();
                let stretch = 1.0 + elong * (along - across).tanh();
                mod_v *= stretch;
                mod_v = mod_v.clamp(0.70, 1.15);
            }
            let h = parent_h * decay * mod_v;
            if h > heights[ni] {
                heights[ni] = h;
            }
            if h > 0.02 {
                queue.push_back(ni);
            }
        }
    }
}

fn shuffle_six(items: &mut [Axial; 6], seed: u64) {
    let mut s = seed;
    for i in (1..6).rev() {
        s = mix64(s ^ (i as u64 * 0x9E37));
        let j = (s as usize) % (i + 1);
        items.swap(i, j);
    }
}

/// Remove 1-cell-wide corridors / tips that read as hex-axis "strips".
fn prune_thin_corridors(bounds: &MapBounds, layer: &mut DenseLayer, passes: usize) {
    for _ in 0..passes {
        let mut drop: Vec<usize> = Vec::new();
        for index in 0..bounds.len() {
            if !is_land_cell(layer, index) {
                continue;
            }
            let Some(cell) = bounds.from_index(index) else {
                continue;
            };
            let mut land_ns: Vec<Axial> = Vec::new();
            for nb in cell.neighbors() {
                let Some(ni) = bounds.index_of(nb) else {
                    continue;
                };
                if is_land_cell(layer, ni) {
                    land_ns.push(nb);
                }
            }
            let kill = match land_ns.len() {
                0 | 1 => true,
                2 => opposite_hex_pair(cell, land_ns[0], land_ns[1]),
                _ => false,
            };
            if kill {
                drop.push(index);
            }
        }
        if drop.is_empty() {
            break;
        }
        for index in drop {
            layer.set(
                index,
                DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
            );
        }
    }
}

fn opposite_hex_pair(center: Axial, a: Axial, b: Axial) -> bool {
    let da = (a.q - center.q, a.r - center.r);
    let db = (b.q - center.q, b.r - center.r);
    da.0 + db.0 == 0 && da.1 + db.1 == 0
}

fn carve_pit(
    bounds: &MapBounds,
    heights: &mut [f64],
    start: usize,
    strength: f64,
    decay: f64,
    seed: u64,
) {
    let n = heights.len();
    let mut used = vec![false; n];
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let mut power = strength;
    heights[start] = (heights[start] - power).max(0.0);
    used[start] = true;
    queue.push_back(start);
    let mut step = 0u64;
    while let Some(index) = queue.pop_front() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        power *= decay;
        if power < 0.03 {
            continue;
        }
        for ncell in cell.neighbors() {
            let Some(ni) = bounds.index_of(ncell) else {
                continue;
            };
            if used[ni] {
                continue;
            }
            used[ni] = true;
            step = step.wrapping_add(1);
            let mod_v = 0.85 + unit01(seed ^ step) * 0.3;
            let cut = power * mod_v;
            heights[ni] = (heights[ni] - cut).max(0.0);
            if cut > 0.03 {
                queue.push_back(ni);
            }
        }
    }
}

fn threshold_for_fraction(heights: &[f64], target: f64) -> f64 {
    if heights.is_empty() {
        return 0.2;
    }
    let mut sorted: Vec<f64> = heights.iter().copied().filter(|h| *h > 0.0).collect();
    if sorted.is_empty() {
        return 1.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let want = (target.clamp(0.05, 0.85) * heights.len() as f64) as usize;
    let land_from_positive = want.min(sorted.len());
    if land_from_positive == 0 {
        return sorted[sorted.len() - 1] + 0.01;
    }
    let idx = sorted.len() - land_from_positive;
    sorted[idx] * 0.999
}

fn apply_shore_fringe(
    bounds: &MapBounds,
    layer: &mut DenseLayer,
    character: ShoreCharacter,
    seed: u64,
) {
    let chance = match character {
        ShoreCharacter::Smooth => 0.04,
        ShoreCharacter::Jagged => 0.11,
    };
    let n = bounds.len();
    let mut flips: Vec<(usize, bool)> = Vec::new();
    for index in 0..n {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let is_land = matches!(
            layer.state(index),
            DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
        );
        let mut land_n = 0;
        let mut ocean_n = 0;
        for nb in cell.neighbors() {
            let Some(ni) = bounds.index_of(nb) else {
                continue;
            };
            if matches!(
                layer.state(ni),
                DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
            ) {
                land_n += 1;
            } else {
                ocean_n += 1;
            }
        }
        let on_shore = land_n > 0 && ocean_n > 0;
        if !on_shore {
            continue;
        }
        let u = unit01(seed ^ (index as u64).wrapping_mul(0x85EB));
        if u > chance {
            continue;
        }
        if is_land && land_n <= 3 {
            flips.push((index, false));
        } else if !is_land && land_n >= 2 {
            flips.push((index, true));
        }
    }
    for (index, to_land) in flips {
        let value = if to_land {
            LAND_MASK_LAND
        } else {
            LAND_MASK_OCEAN
        };
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(value.to_string())),
        );
    }
}

fn remove_tiny_islands(bounds: &MapBounds, layer: &mut DenseLayer, min_cells: usize) {
    let components = land_components(bounds, layer);
    for component in components {
        if component.len() < min_cells {
            flood_ocean(layer, &component);
        }
    }
}

/// Connected land components, largest first.
fn land_components(bounds: &MapBounds, layer: &DenseLayer) -> Vec<Vec<usize>> {
    let n = bounds.len();
    let mut seen = vec![false; n];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for start in 0..n {
        if seen[start] || !is_land_cell(layer, start) {
            continue;
        }
        let mut stack = vec![start];
        let mut component = Vec::new();
        seen[start] = true;
        while let Some(i) = stack.pop() {
            component.push(i);
            let Some(cell) = bounds.from_index(i) else {
                continue;
            };
            for nb in cell.neighbors() {
                let Some(ni) = bounds.index_of(nb) else {
                    continue;
                };
                if seen[ni] || !is_land_cell(layer, ni) {
                    continue;
                }
                seen[ni] = true;
                stack.push(ni);
            }
        }
        out.push(component);
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.len()));
    out
}

fn flood_ocean(layer: &mut DenseLayer, cells: &[usize]) {
    for &i in cells {
        layer.set(
            i,
            DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
        );
    }
}

/// Third pass after growth: enforce class identity (D-66 dogfood).
/// Pangea/Island → one mass; Continents → at most two; CAI → main + small sats.
fn enforce_layout_class(bounds: &MapBounds, layer: &mut DenseLayer, class: LayoutClass) {
    let components = land_components(bounds, layer);
    if components.is_empty() {
        return;
    }
    match class {
        LayoutClass::Pangea | LayoutClass::Island | LayoutClass::Mediterranean => {
            // One dominant landmass (med may enclose inland_sea after this).
            for component in components.into_iter().skip(1) {
                flood_ocean(layer, &component);
            }
        }
        LayoutClass::Continents => {
            for component in components.into_iter().skip(2) {
                flood_ocean(layer, &component);
            }
        }
        LayoutClass::ContinentAndIslands => {
            let main_len = components[0].len().max(1);
            let sat_cap = (main_len / 4).max(8);
            for (i, component) in components.into_iter().enumerate() {
                if i == 0 {
                    continue;
                }
                // Drop second continent-sized masses; keep only small satellites.
                if component.len() > sat_cap {
                    flood_ocean(layer, &component);
                }
            }
        }
        LayoutClass::Archipelago => {
            // Many islands is the identity — no merge/drop beyond tiny cleanup.
        }
    }
}

fn is_land_cell(layer: &DenseLayer, index: usize) -> bool {
    matches!(
        layer.state(index),
        DenseState::Value(LayerValue::Text(kind)) if kind == LAND_MASK_LAND
    )
}

fn mark_inland_seas(bounds: &MapBounds, layer: &mut DenseLayer) {
    let mut seen = vec![false; bounds.len()];
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for index in 0..bounds.len() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        if !is_boundary_cell(bounds, cell) || !is_non_land(layer, index) {
            continue;
        }
        seen[index] = true;
        queue.push_back(index);
    }
    while let Some(index) = queue.pop_front() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        for n in cell.neighbors() {
            let Some(next) = bounds.index_of(n) else {
                continue;
            };
            if seen[next] || !is_non_land(layer, next) {
                continue;
            }
            seen[next] = true;
            queue.push_back(next);
        }
    }
    for (index, ocean_connected) in seen.into_iter().enumerate() {
        if ocean_connected || !is_non_land(layer, index) {
            continue;
        }
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(LAND_MASK_INLAND_SEA.to_string())),
        );
    }
}

fn is_non_land(layer: &DenseLayer, index: usize) -> bool {
    !matches!(
        layer.state(index),
        DenseState::Value(LayerValue::Text(kind)) if kind == LAND_MASK_LAND
    )
}

fn is_boundary_cell(bounds: &MapBounds, cell: Axial) -> bool {
    cell.neighbors().iter().any(|n| !bounds.contains(*n))
}

fn half_extent(bounds: &MapBounds) -> (f64, f64) {
    let mut max_x: f64 = 0.0;
    let mut max_y: f64 = 0.0;
    for c in bounds.cells() {
        let (x, y) = c.to_pixel(1.0);
        max_x = max_x.max(x.abs());
        max_y = max_y.max(y.abs());
    }
    (max_x.max(1.0), max_y.max(1.0))
}

fn unit01(seed: u64) -> f64 {
    let x = mix64(seed);
    (x as f64) / (u64::MAX as f64)
}

fn mix64(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_kind(layer: &DenseLayer, kind: &str) -> usize {
        (0..layer.len())
            .filter(|&i| {
                matches!(
                    layer.state(i),
                    DenseState::Value(LayerValue::Text(ref t)) if t == kind
                )
            })
            .count()
    }

    #[test]
    fn catalog_has_five_per_class() {
        assert_eq!(RECIPE_CATALOG.len(), 30);
        for class in LayoutClass::ALL {
            assert_eq!(recipes_for(class).len(), 5, "{}", class.id());
        }
    }

    #[test]
    fn next_recipe_changes_within_class() {
        let a = pick_recipe(LayoutClass::Island, 0);
        let b = next_recipe(LayoutClass::Island, a.id, 1);
        assert_eq!(a.layout_class, b.layout_class);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn compare_trio_has_distinct_classes() {
        for seed in [0u64, 1, 7, 42, 99, 1000] {
            let trio = pick_compare_trio(seed);
            assert_ne!(trio[0].layout_class, trio[1].layout_class);
            assert_ne!(trio[1].layout_class, trio[2].layout_class);
            assert_ne!(trio[0].layout_class, trio[2].layout_class);
        }
    }

    #[test]
    fn regenerating_changes_recipe_or_class_set() {
        let a = pick_compare_trio(0);
        let b = pick_compare_trio(1);
        let same = a[0].id == b[0].id && a[1].id == b[1].id && a[2].id == b[2].id;
        assert!(!same, "different nonce should reshuffle trio");
    }

    #[test]
    fn recipes_in_class_differ_macroform() {
        let bounds = MapBounds::new(28, 16);
        let recipes = recipes_for(LayoutClass::Pangea);
        let layers: Vec<_> = recipes
            .iter()
            .map(|r| generate_land_mask_recipe(&bounds, r, ShoreCharacter::Smooth, 0))
            .collect();
        let mut differ = false;
        'outer: for i in 0..layers.len() {
            for j in (i + 1)..layers.len() {
                for idx in 0..bounds.len() {
                    if layers[i].state(idx) != layers[j].state(idx) {
                        differ = true;
                        break 'outer;
                    }
                }
            }
        }
        assert!(differ, "pangea growth plans should not be identical masks");
    }

    #[test]
    fn different_seeds_change_form() {
        let bounds = MapBounds::new(28, 16);
        let recipe = find_recipe("island_irregular").expect("recipe");
        let a = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, 1);
        let b = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, 99);
        let mut differ = false;
        for idx in 0..bounds.len() {
            if a.state(idx) != b.state(idx) {
                differ = true;
                break;
            }
        }
        assert!(differ, "seed should change organic form");
    }

    #[test]
    fn prune_removes_opposite_corridor_cell() {
        let bounds = MapBounds::new(8, 6);
        let mut layer = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
        for index in 0..bounds.len() {
            layer.set(
                index,
                DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
            );
        }
        // Three-cell diagonal corridor: middle has exactly two opposite land neighbors.
        let a = Axial { q: 0, r: 0 };
        let b = Axial { q: 1, r: 0 };
        let c = Axial { q: 2, r: 0 };
        for cell in [a, b, c] {
            let i = bounds.index_of(cell).expect("in bounds");
            layer.set(
                i,
                DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
            );
        }
        prune_thin_corridors(&bounds, &mut layer, 4);
        let mid = bounds.index_of(b).unwrap();
        assert!(
            !is_land_cell(&layer, mid),
            "1-hex-wide corridor middle should be pruned"
        );
    }

    #[test]
    fn generate_keeps_bounds_length() {
        let bounds = MapBounds::new(14, 8);
        let layer = generate_land_mask(
            &bounds,
            LayoutClass::Pangea,
            ShoreCharacter::Smooth,
            42,
        );
        assert_eq!(layer.len(), bounds.len());
        assert_eq!(layer.layer_id, LAND_MASK_LAYER_ID);
    }

    #[test]
    fn land_mask_syncs_to_elevation() {
        let bounds = MapBounds::new(4, 3);
        let mut mask = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
        mask.set(
            0,
            DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
        );
        mask.set(
            1,
            DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
        );
        let elev = elevation_from_land_mask(&bounds, &mask);
        assert_eq!(elev.int_or(0, 0), 1);
        assert_eq!(elev.int_or(1, 1), 0);
    }

    #[test]
    fn normalizes_unknown_kind_to_ocean() {
        assert_eq!(normalize_kind("land"), LAND_MASK_LAND);
        assert_eq!(normalize_kind("inland_sea"), LAND_MASK_INLAND_SEA);
        assert_eq!(normalize_kind("mystery"), LAND_MASK_OCEAN);
    }

    #[test]
    fn all_layout_classes_produce_land() {
        let bounds = MapBounds::new(24, 14);
        for class in LayoutClass::ALL {
            let layer = generate_land_mask(&bounds, class, ShoreCharacter::Smooth, 7);
            assert!(
                count_kind(&layer, LAND_MASK_LAND) > 0,
                "{} should produce land",
                class.id()
            );
        }
    }

    #[test]
    fn mediterranean_marks_inland_sea() {
        let bounds = MapBounds::new(28, 16);
        let recipe = find_recipe("med_c_basin").expect("recipe");
        let layer = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, 11);
        assert!(
            count_kind(&layer, LAND_MASK_INLAND_SEA) > 0,
            "mediterranean should enclose inland_sea"
        );
    }

    #[test]
    fn continent_and_islands_has_separated_land() {
        let bounds = MapBounds::new(36, 20);
        let recipe = find_recipe("cai_irregular_main").expect("recipe");
        let layer = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, 19);
        let land = count_kind(&layer, LAND_MASK_LAND);
        assert!(land > 40, "main continent should be substantial, got {land}");
        let mut left_land = false;
        for index in 0..bounds.len() {
            let Some(cell) = bounds.from_index(index) else {
                continue;
            };
            let (x, _) = cell.to_pixel(1.0);
            let (max_x, _) = half_extent(&bounds);
            if x / max_x > -0.55 {
                continue;
            }
            if matches!(
                layer.state(index),
                DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
            ) {
                left_land = true;
                break;
            }
        }
        assert!(left_land, "expected satellite land on the far side");
    }

    #[test]
    fn pangea_is_single_landmass() {
        let bounds = MapBounds::new(28, 16);
        for seed in [0u64, 3, 11, 42, 99] {
            let layer = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Jagged, seed);
            let comps = land_components(&bounds, &layer);
            assert_eq!(
                comps.len(),
                1,
                "pangea seed {seed} should be one mass, got {}",
                comps.len()
            );
        }
    }

    #[test]
    fn enforce_keeps_largest_only_for_pangea() {
        let bounds = MapBounds::new(10, 8);
        let mut layer = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
        for index in 0..bounds.len() {
            layer.set(
                index,
                DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
            );
        }
        // Two separate blobs.
        for cell in [Axial { q: -2, r: 0 }, Axial { q: -1, r: 0 }, Axial { q: 0, r: 0 }] {
            let i = bounds.index_of(cell).expect("in bounds");
            layer.set(
                i,
                DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
            );
        }
        for cell in [Axial { q: 3, r: 1 }, Axial { q: 4, r: 1 }] {
            let i = bounds.index_of(cell).expect("in bounds");
            layer.set(
                i,
                DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
            );
        }
        enforce_layout_class(&bounds, &mut layer, LayoutClass::Pangea);
        assert_eq!(land_components(&bounds, &layer).len(), 1);
    }
}
