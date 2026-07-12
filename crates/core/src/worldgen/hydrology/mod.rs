//! Hydrology: depression analysis, river flux, lake generation.

pub mod catalog;
pub mod channel_graph;
pub mod depression_fill;
pub mod diagnostics;
pub mod drainage_graph;
pub mod lakes;
pub mod render;
pub mod snapshot;
pub mod types;

pub use catalog::{
    compatibility_river_id_layer, HydrologyCatalog, NameMigrationReport, NamedRiverBinding,
    PhysicalRiverSegment,
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
pub use render::{river_render_paths, RiverRenderPaths};
pub use snapshot::{
    is_derived_hydrology_layer_id, HydrologySnapshot, HydrologySnapshotError,
    CHANNEL_NODE_LAYER_ID, CHANNEL_SEGMENT_LAYER_ID, HYDROLOGY_SNAPSHOT_FILE,
    HYDROLOGY_SNAPSHOT_SCHEMA_VERSION,
};
pub use types::{DepressionAnalysis, LakeDensity, ProvisionalDrainage, RiverDensity};
