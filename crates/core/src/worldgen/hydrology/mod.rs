//! Hydrology: depression analysis, river flux, lake generation.

pub mod depression_fill;
pub mod lakes;
pub mod river_flux;
pub mod river_validate;
pub mod types;

pub use depression_fill::analyze_depressions;
pub use lakes::generate_lakes;
pub use river_flux::*;
pub use river_validate::{
    classify_terminal, mouth_diagnostics, prune_invalid_river_trees, validate_catalog,
    validate_generated_catalog_strict, RiverTerminal, RiverValidationContext, RiverValidationReport,
};
pub use types::{DepressionAnalysis, LakeDensity, RiverDensity};
