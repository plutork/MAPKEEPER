//! Hydrology: depression analysis, river flux, (later) lakes.

pub mod depression_fill;
pub mod river_flux;
pub mod types;

pub use depression_fill::analyze_depressions;
pub use river_flux::*;
pub use types::DepressionAnalysis;
