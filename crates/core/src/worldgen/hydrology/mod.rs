//! Hydrology: depression analysis, river flux, lake generation.

pub mod depression_fill;
pub mod diagnostics;
pub mod drainage_graph;
pub mod lakes;
pub mod river_flux;
pub mod river_validate;
pub mod types;

pub use depression_fill::{analyze_depressions, provisional_drainage};
pub use diagnostics::{
    diagnose_legacy_hydrology, LegacyHydrologyDiagnostics, LegacyRiverTerminal,
    LegacyTerminalReason,
};
pub use drainage_graph::{
    build_drainage_graph, DrainageGraph, DrainageGraphError, DrainageNode, DrainageNodeId,
};
pub use lakes::generate_lakes;
pub use river_flux::*;
pub use river_validate::{
    classify_terminal, enforce_strict_generated_catalog, mouth_diagnostics,
    prune_invalid_river_trees, validate_catalog, validate_generated_catalog_strict,
    would_assign_parent_cycle, RiverTerminal, RiverValidationContext, RiverValidationReport,
};
pub use types::{DepressionAnalysis, LakeDensity, ProvisionalDrainage, RiverDensity};
