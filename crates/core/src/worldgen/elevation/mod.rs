//! Step 5 elevation bridge (D-72 / D-88 / D-89): geology → continuous-ish integer relief.

mod bands;
mod generate;
mod jitter;
mod smooth;
#[cfg(test)]
mod tests;
mod types;

pub use bands::{
    base_elevation_for_geology, clamp_elevation_by_geology, elevation_band_for_geology,
};
pub use generate::elevation_from_land_mask_and_geology;
pub use jitter::deterministic_cell_jitter;
pub use smooth::smooth_elevation_once;
pub use types::ElevationIntensity;
