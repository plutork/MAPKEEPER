//! Step 4 world pipeline: intermediate `geology` layer (D-63 / D-87 hidden plates).

mod despeckle;
mod generate;
mod kind;
mod land_helpers;
mod mapping;
#[cfg(test)]
mod tests;
mod types;

pub use generate::generate_geology;
pub use kind::geology_kind_at;
pub use mapping::map_hidden_tectonics_to_geology_style;
pub use types::*;

/// Step 5 bridge — see `worldgen::elevation` (D-88 continuous intensity).
pub use crate::worldgen::elevation::{elevation_from_land_mask_and_geology, ElevationIntensity};
