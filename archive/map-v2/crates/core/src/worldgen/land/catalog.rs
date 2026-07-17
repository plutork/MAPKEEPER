//! Growth-plan catalog and recipe pickers (D-66).

use super::types::*;
use super::util::mix64;

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
        hole: Some(LayoutBlob {
            cx: 0.28,
            cy: 0.05,
            rx: 0.28,
            ry: 0.32,
        }),
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
        elongation: 0.28,
        irregularity: 0.55,
    },
    LayoutRecipe {
        id: "continents_crescent_pair",
        layout_class: LayoutClass::Continents,
        seed_zones: zones!(-0.48, 0.0, 0.28, 0.35; 0.48, 0.08, 0.26, 0.32),
        hole: Some(LayoutBlob {
            cx: -0.28,
            cy: 0.0,
            rx: 0.16,
            ry: 0.22,
        }),
        land_fraction: 0.40,
        primary_count: 2,
        satellite_count: 4,
        merge_bias: 0.1,
        elongation: 0.26,
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
        elongation: 0.3,
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
        elongation: 0.28,
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
        elongation: 0.26,
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
            -0.5, 0.0, 0.14, 0.16;
            -0.35, 0.25, 0.10, 0.10;
            0.4, -0.1, 0.14, 0.14;
            0.55, 0.2, 0.10, 0.10
        ),
        hole: None,
        land_fraction: 0.20,
        primary_count: 4,
        satellite_count: 7,
        merge_bias: 0.03,
        elongation: 0.42,
        irregularity: 0.64,
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
        hole: Some(LayoutBlob {
            cx: 0.12,
            cy: 0.0,
            rx: 0.12,
            ry: 0.14,
        }),
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
        hole: Some(LayoutBlob {
            cx: 0.28,
            cy: 0.05,
            rx: 0.14,
            ry: 0.16,
        }),
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
        hole: Some(LayoutBlob {
            cx: 0.0,
            cy: 0.0,
            rx: 0.28,
            ry: 0.24,
        }),
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
        hole: Some(LayoutBlob {
            cx: 0.05,
            cy: 0.0,
            rx: 0.32,
            ry: 0.26,
        }),
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
        hole: Some(LayoutBlob {
            cx: -0.05,
            cy: 0.05,
            rx: 0.22,
            ry: 0.20,
        }),
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
        hole: Some(LayoutBlob {
            cx: -0.15,
            cy: 0.1,
            rx: 0.24,
            ry: 0.22,
        }),
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
        hole: Some(LayoutBlob {
            cx: 0.05,
            cy: 0.0,
            rx: 0.18,
            ry: 0.28,
        }),
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
