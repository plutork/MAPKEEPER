//! Step 3 world pipeline: land silhouette (`land_mask`) generators.
//!
//! Layout classes (D-62) + recipe bank (D-64/D-65): ~5 non-ellipse recipes
//! per class. Regenerate rotates recipe within the selected class only.

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

/// Elliptical land (or hole) blob in normalized map space (~[-1,1]).
#[derive(Debug, Clone, Copy)]
pub struct LayoutBlob {
    pub cx: f64,
    pub cy: f64,
    pub rx: f64,
    pub ry: f64,
}

/// Crude macro recipe tagged with a layout class (step3-layout-pattern-bank-v1).
#[derive(Debug, Clone, Copy)]
pub struct LayoutRecipe {
    pub id: &'static str,
    pub layout_class: LayoutClass,
    pub blobs: &'static [LayoutBlob],
    /// Optional water hole (mediterranean inland basin).
    pub hole: Option<LayoutBlob>,
}

macro_rules! blobs {
    ($($cx:expr, $cy:expr, $rx:expr, $ry:expr);+ $(;)?) => {
        &[$(LayoutBlob { cx: $cx, cy: $cy, rx: $rx, ry: $ry }),+]
    };
}

/// Static catalog (D-65 / step3-layout-picker-ux-v2): distinctive non-ellipse macros.
/// ~5 recipes × 6 classes. Forms: crescent, C-shape, L-mass, broken chain,
/// ring-with-gap, multi-blob irregular, hooked island, split basin.
pub static RECIPE_CATALOG: &[LayoutRecipe] = &[
    // --- pangea: large irregular / L / crescent ---
    LayoutRecipe {
        id: "pangea_irregular",
        layout_class: LayoutClass::Pangea,
        blobs: blobs!(
            -0.15, 0.05, 0.55, 0.48;
            0.25, -0.15, 0.48, 0.42;
            0.05, 0.35, 0.40, 0.32;
            -0.35, -0.25, 0.32, 0.28
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "pangea_l_mass",
        layout_class: LayoutClass::Pangea,
        blobs: blobs!(
            -0.35, 0.0, 0.32, 0.72;
            0.15, 0.40, 0.62, 0.28
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "pangea_crescent",
        layout_class: LayoutClass::Pangea,
        blobs: blobs!(0.0, 0.0, 0.78, 0.70),
        hole: Some(LayoutBlob {
            cx: 0.28,
            cy: 0.05,
            rx: 0.42,
            ry: 0.48,
        }),
    },
    LayoutRecipe {
        id: "pangea_c_shape",
        layout_class: LayoutClass::Pangea,
        blobs: blobs!(
            -0.45, -0.35, 0.35, 0.28;
            -0.55, 0.05, 0.30, 0.32;
            -0.40, 0.42, 0.38, 0.26;
            0.05, 0.50, 0.35, 0.24;
            0.35, 0.30, 0.28, 0.30
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "pangea_hooked",
        layout_class: LayoutClass::Pangea,
        blobs: blobs!(
            -0.20, 0.0, 0.55, 0.42;
            0.35, -0.25, 0.38, 0.28;
            0.50, 0.15, 0.22, 0.35
        ),
        hole: None,
    },
    // --- continents: dual masses, not twin ellipses ---
    LayoutRecipe {
        id: "continents_l_and_blob",
        layout_class: LayoutClass::Continents,
        blobs: blobs!(
            -0.50, -0.10, 0.28, 0.58;
            -0.20, 0.35, 0.40, 0.24;
            0.48, 0.05, 0.38, 0.48
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "continents_crescent_pair",
        layout_class: LayoutClass::Continents,
        blobs: blobs!(-0.48, 0.0, 0.42, 0.55; 0.48, 0.08, 0.40, 0.50),
        hole: Some(LayoutBlob {
            cx: -0.28,
            cy: 0.0,
            rx: 0.22,
            ry: 0.32,
        }),
    },
    LayoutRecipe {
        id: "continents_broken_chain",
        layout_class: LayoutClass::Continents,
        blobs: blobs!(
            -0.65, 0.25, 0.28, 0.35;
            -0.20, -0.15, 0.32, 0.38;
            0.35, 0.30, 0.30, 0.32;
            0.68, -0.25, 0.26, 0.30
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "continents_c_and_mass",
        layout_class: LayoutClass::Continents,
        blobs: blobs!(
            -0.55, -0.30, 0.28, 0.24;
            -0.62, 0.05, 0.24, 0.28;
            -0.48, 0.38, 0.30, 0.22;
            0.45, 0.0, 0.42, 0.52
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "continents_irregular_dual",
        layout_class: LayoutClass::Continents,
        blobs: blobs!(
            -0.45, -0.20, 0.38, 0.35;
            -0.30, 0.25, 0.32, 0.30;
            0.40, 0.15, 0.35, 0.42;
            0.55, -0.30, 0.28, 0.26
        ),
        hole: None,
    },
    // --- archipelago: chains / rings with gap ---
    LayoutRecipe {
        id: "archipelago_broken_chain",
        layout_class: LayoutClass::Archipelago,
        blobs: blobs!(
            -0.72, 0.40, 0.14, 0.12;
            -0.40, 0.22, 0.16, 0.14;
            -0.05, 0.0, 0.15, 0.13;
            0.30, -0.22, 0.16, 0.14;
            0.62, -0.42, 0.14, 0.12
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "archipelago_ring_gap",
        layout_class: LayoutClass::Archipelago,
        blobs: blobs!(
            0.50, 0.15, 0.16, 0.14;
            0.25, 0.48, 0.15, 0.14;
            -0.20, 0.50, 0.16, 0.14;
            -0.52, 0.20, 0.15, 0.14;
            -0.48, -0.25, 0.15, 0.14;
            0.10, -0.52, 0.16, 0.14
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "archipelago_c_arc",
        layout_class: LayoutClass::Archipelago,
        blobs: blobs!(
            -0.55, -0.35, 0.15, 0.13;
            -0.65, 0.0, 0.14, 0.14;
            -0.50, 0.38, 0.16, 0.13;
            -0.10, 0.52, 0.15, 0.13;
            0.30, 0.42, 0.14, 0.13;
            0.55, 0.15, 0.13, 0.12
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "archipelago_scatter_irregular",
        layout_class: LayoutClass::Archipelago,
        blobs: blobs!(
            -0.60, -0.35, 0.18, 0.14;
            0.55, -0.40, 0.14, 0.16;
            -0.25, 0.45, 0.20, 0.14;
            0.40, 0.35, 0.15, 0.18;
            0.05, -0.05, 0.12, 0.11;
            -0.15, 0.10, 0.11, 0.13
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "archipelago_twin_clusters",
        layout_class: LayoutClass::Archipelago,
        blobs: blobs!(
            -0.45, 0.15, 0.18, 0.16;
            -0.25, -0.05, 0.14, 0.13;
            -0.55, -0.15, 0.12, 0.11;
            0.45, 0.20, 0.17, 0.15;
            0.60, 0.0, 0.13, 0.12;
            0.35, 0.40, 0.12, 0.11
        ),
        hole: None,
    },
    // --- island: hooked / crescent / L — not round blob ---
    LayoutRecipe {
        id: "island_hooked",
        layout_class: LayoutClass::Island,
        blobs: blobs!(
            -0.15, 0.05, 0.42, 0.22;
            0.25, -0.05, 0.28, 0.18;
            0.40, 0.25, 0.16, 0.28
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "island_crescent",
        layout_class: LayoutClass::Island,
        blobs: blobs!(0.0, 0.0, 0.48, 0.42),
        hole: Some(LayoutBlob {
            cx: 0.22,
            cy: 0.0,
            rx: 0.28,
            ry: 0.32,
        }),
    },
    LayoutRecipe {
        id: "island_l_mass",
        layout_class: LayoutClass::Island,
        blobs: blobs!(
            -0.15, 0.0, 0.18, 0.45;
            0.15, 0.25, 0.38, 0.16
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "island_c_shape",
        layout_class: LayoutClass::Island,
        blobs: blobs!(
            -0.25, -0.25, 0.20, 0.16;
            -0.32, 0.05, 0.16, 0.18;
            -0.20, 0.30, 0.22, 0.14;
            0.15, 0.28, 0.18, 0.14
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "island_broken_bar",
        layout_class: LayoutClass::Island,
        blobs: blobs!(
            -0.30, 0.05, 0.22, 0.16;
            0.05, -0.05, 0.18, 0.14;
            0.35, 0.10, 0.16, 0.18
        ),
        hole: None,
    },
    // --- continent_and_islands ---
    LayoutRecipe {
        id: "cai_irregular_main",
        layout_class: LayoutClass::ContinentAndIslands,
        blobs: blobs!(
            0.05, 0.0, 0.42, 0.38;
            -0.15, 0.25, 0.30, 0.28;
            0.25, -0.25, 0.28, 0.24;
            -0.70, 0.30, 0.16, 0.14;
            -0.65, -0.35, 0.14, 0.13
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "cai_l_main_chain",
        layout_class: LayoutClass::ContinentAndIslands,
        blobs: blobs!(
            -0.15, -0.05, 0.28, 0.52;
            0.20, 0.30, 0.45, 0.22;
            0.65, -0.35, 0.15, 0.13;
            0.70, 0.05, 0.12, 0.11;
            0.55, 0.40, 0.14, 0.12
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "cai_crescent_sats",
        layout_class: LayoutClass::ContinentAndIslands,
        blobs: blobs!(
            0.0, 0.0, 0.52, 0.48;
            0.70, 0.25, 0.14, 0.12;
            0.65, -0.30, 0.13, 0.12
        ),
        hole: Some(LayoutBlob {
            cx: 0.22,
            cy: 0.05,
            rx: 0.28,
            ry: 0.30,
        }),
    },
    LayoutRecipe {
        id: "cai_c_main",
        layout_class: LayoutClass::ContinentAndIslands,
        blobs: blobs!(
            -0.35, -0.30, 0.28, 0.22;
            -0.45, 0.05, 0.24, 0.26;
            -0.30, 0.38, 0.30, 0.20;
            0.10, 0.42, 0.28, 0.18;
            0.65, -0.20, 0.15, 0.13;
            0.70, 0.25, 0.13, 0.12
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "cai_hooked_main",
        layout_class: LayoutClass::ContinentAndIslands,
        blobs: blobs!(
            -0.10, 0.0, 0.45, 0.32;
            0.30, -0.20, 0.28, 0.22;
            0.42, 0.20, 0.18, 0.30;
            -0.70, 0.35, 0.14, 0.12;
            -0.72, -0.25, 0.13, 0.12
        ),
        hole: None,
    },
    // --- mediterranean: ring/gap / split basin ---
    LayoutRecipe {
        id: "med_ring_gap",
        layout_class: LayoutClass::Mediterranean,
        blobs: blobs!(
            -0.45, -0.35, 0.35, 0.28;
            -0.55, 0.05, 0.30, 0.32;
            -0.40, 0.42, 0.38, 0.26;
            0.10, 0.50, 0.40, 0.24;
            0.45, 0.25, 0.32, 0.30;
            0.40, -0.35, 0.35, 0.28;
            0.05, -0.50, 0.38, 0.24
        ),
        hole: None,
    },
    LayoutRecipe {
        id: "med_split_basin",
        layout_class: LayoutClass::Mediterranean,
        blobs: blobs!(0.0, 0.0, 0.88, 0.72),
        hole: Some(LayoutBlob {
            cx: -0.22,
            cy: 0.0,
            rx: 0.28,
            ry: 0.35,
        }),
    },
    LayoutRecipe {
        id: "med_c_basin",
        layout_class: LayoutClass::Mediterranean,
        blobs: blobs!(0.05, 0.0, 0.82, 0.70),
        hole: Some(LayoutBlob {
            cx: 0.25,
            cy: 0.0,
            rx: 0.40,
            ry: 0.42,
        }),
    },
    LayoutRecipe {
        id: "med_l_frame",
        layout_class: LayoutClass::Mediterranean,
        blobs: blobs!(
            -0.45, 0.0, 0.30, 0.70;
            0.10, 0.42, 0.65, 0.28;
            0.45, -0.15, 0.32, 0.35
        ),
        hole: Some(LayoutBlob {
            cx: 0.0,
            cy: -0.05,
            rx: 0.32,
            ry: 0.28,
        }),
    },
    LayoutRecipe {
        id: "med_twin_split",
        layout_class: LayoutClass::Mediterranean,
        blobs: blobs!(-0.25, 0.0, 0.52, 0.62; 0.35, 0.05, 0.48, 0.55),
        hole: Some(LayoutBlob {
            cx: 0.05,
            cy: 0.0,
            rx: 0.22,
            ry: 0.38,
        }),
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

/// Next recipe for the same class, preferring a different id than `current_id` (D-65).
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

/// Deprecated D-64 helper — kept for tests; UI no longer reshuffles classes (D-65).
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

/// Generate silhouette from layout class + shore + seed (picks recipe from bank).
pub fn generate_land_mask(
    bounds: &MapBounds,
    style: LayoutClass,
    character: ShoreCharacter,
    seed: u64,
) -> DenseLayer {
    let recipe = pick_recipe(style, seed);
    generate_land_mask_recipe(bounds, recipe, character, seed)
}

pub fn generate_land_mask_recipe(
    bounds: &MapBounds,
    recipe: &LayoutRecipe,
    character: ShoreCharacter,
    seed: u64,
) -> DenseLayer {
    let mut layer = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
    let (max_x, max_y) = half_extent(bounds);
    let roughness = match character {
        ShoreCharacter::Smooth => 0.13,
        ShoreCharacter::Jagged => 0.29,
    };
    for index in 0..bounds.len() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let (x, y) = cell.to_pixel(1.0);
        let nx = if max_x > 0.0 { x / max_x } else { 0.0 };
        let ny = if max_y > 0.0 { y / max_y } else { 0.0 };
        let noise = octave_noise(cell, seed);
        let value = if is_land_recipe(recipe, nx, ny, noise, roughness) {
            LAND_MASK_LAND
        } else {
            LAND_MASK_OCEAN
        };
        layer.set(
            index,
            DenseState::Value(LayerValue::Text(value.to_string())),
        );
    }
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

fn is_land_recipe(
    recipe: &LayoutRecipe,
    nx: f64,
    ny: f64,
    noise: f64,
    roughness: f64,
) -> bool {
    let in_land = recipe.blobs.iter().any(|b| {
        in_ellipse(
            nx - b.cx,
            ny - b.cy,
            b.rx + roughness * 0.55 * noise,
            b.ry + roughness * 0.55 * noise,
        )
    });
    if !in_land {
        return false;
    }
    if let Some(h) = recipe.hole {
        let in_hole = in_ellipse(
            nx - h.cx,
            ny - h.cy,
            (h.rx - roughness * 0.25 * noise).max(0.05),
            (h.ry - roughness * 0.25 * noise).max(0.05),
        );
        return !in_hole;
    }
    true
}

fn in_ellipse(dx: f64, dy: f64, rx: f64, ry: f64) -> bool {
    if rx <= 0.0 || ry <= 0.0 {
        return false;
    }
    (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) <= 1.0
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

fn octave_noise(cell: Axial, seed: u64) -> f64 {
    let n1 = hash_noise(seed ^ 0x9E37_79B9, cell.q, cell.r);
    let n2 = hash_noise(seed ^ 0x85EB_CA6B, cell.q * 2, cell.r * 2);
    let n3 = hash_noise(seed ^ 0xC2B2_AE35, cell.q * 4, cell.r * 4);
    (n1 * 0.58 + n2 * 0.29 + n3 * 0.13).clamp(-1.0, 1.0)
}

fn hash_noise(seed: u64, q: i32, r: i32) -> f64 {
    let mut x = seed
        ^ ((q as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        ^ ((r as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    let unit = (x as f64) / (u64::MAX as f64);
    unit * 2.0 - 1.0
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
        assert!(differ, "pangea recipes should not be identical masks");
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
}
