//! Hydrology: depression analysis, river flux, lake generation.

pub mod depression_fill;
pub mod lakes;
pub mod river_flux;
pub mod types;

pub use depression_fill::analyze_depressions;
pub use lakes::generate_lakes;
pub use river_flux::*;
pub use types::{DepressionAnalysis, LakeDensity};
