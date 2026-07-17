//! Hydrology: depression analysis, river flux, lake generation.

pub mod catalog;
pub mod channel_graph;
pub mod depression_fill;
pub mod diagnostics;
pub mod drainage_graph;
pub mod lakes;
pub mod render;
pub mod river_polygon;
pub mod snapshot;
pub mod terminal_routing;
pub mod types;

pub use catalog::{
    compatibility_river_id_layer, HydrologyCatalog, NameMigrationReport, NamedRiverBinding,
    NamedRiverStore, PhysicalRiverSegment, NAMED_RIVERS_FILE, NAMED_RIVER_STORE_SCHEMA_VERSION,
};
pub use channel_graph::{
    build_channel_graph, validate_channel_graph, ChannelGraph, ChannelGraphError, ChannelPolicy,
    HydrologyFlow, PhysicalSegment, RiverGraph, RiverGraphNode, RiverGraphNodeKind,
};
pub use depression_fill::{analyze_depressions, provisional_drainage};
pub use diagnostics::{diagnose_hydrology, HydrologyDiagnostics};
pub use drainage_graph::{
    build_drainage_graph, DrainageGraph, DrainageGraphError, DrainageNode, DrainageNodeId,
};
pub use lakes::generate_lakes;
pub use render::{legacy_river_render_paths, river_render_paths, RiverRenderPaths};
pub use river_polygon::river_ribbon_polygon;
pub use snapshot::{
    derive_effective_seed, hydrology_policy_version, is_derived_hydrology_layer_id,
    HydrologySnapshot, HydrologySnapshotError, CHANNEL_NODE_LAYER_ID, CHANNEL_SEGMENT_LAYER_ID,
    HYDROLOGY_CHANNEL_POLICY_BASE, HYDROLOGY_GENERATOR_VERSION, HYDROLOGY_SNAPSHOT_FILE,
    HYDROLOGY_SNAPSHOT_SCHEMA_VERSION,
};
pub use terminal_routing::{classify_basin_terminal, SpillTerminal};
pub use types::{
    classify_precip_input, terrain_runoff, DepressionAnalysis, LakeDensity, PrecipInputState,
    ProvisionalDrainage, RiverDensity, FALLBACK_LAND_RUNOFF,
};
