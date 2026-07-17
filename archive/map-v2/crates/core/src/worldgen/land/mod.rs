//! Step 3 world pipeline: land silhouette (land_mask) generators.
//!
//! D-66 / step3-organic-silhouette-v1: layout_class → growth recipe (bias) →
//! seeded layered land growth → cleanup → land_mask. Not ellipse-union drawings.
//!
//! land-rs-split track A (D-104): modular layout; behavior unchanged from monolithic land.rs.

mod catalog;
mod enforce;
mod generate;
mod growth;
#[cfg(test)]
mod tests;
mod types;
mod util;

pub use catalog::{
    find_recipe, next_recipe, pick_compare_trio, pick_recipe, recipes_for, RECIPE_CATALOG,
};
pub use generate::{
    elevation_from_land_mask, generate_land_mask, generate_land_mask_recipe, normalize_kind,
};
pub use types::*;
