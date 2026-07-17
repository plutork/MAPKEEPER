//! Asymmetric hybrid spatial foundation (N-008) + author relief (N-010).
//! Local metric frame + extent presets (N-014); Create catalog (N-016);
//! card labels + derived area (N-017); hard-disk brush (N-021).
//! World space owns continuous geometry; hex lattice owns grid-bound data.

mod brush;
mod convert;
mod field;
mod frame;
mod geometry;
mod grid;
mod presets;
mod state;

pub use brush::{
    disk_cell_count_estimate, disk_footprint, disk_from_offsets, disk_offsets, hex_distance,
    max_brush_radius, pulse_interval_ms, AIRBRUSH_DEFAULT_RATE, AIRBRUSH_RATES_STEPS_PER_SEC,
    BRUSH_RADIUS_PERF_CAP, FIELD_FLUSH_BATCH_MAX,
};
pub use convert::{
    axial_to_world, screen_to_world, world_to_axial, world_to_screen, Axial, Viewport,
};
pub use field::{GridField, RELIEF_MAX, RELIEF_MIN};
pub use frame::WorldFrame;
pub use geometry::{cells_for_stub, GeometryStub};
pub use grid::HexGrid;
pub use presets::{
    alpha_default_preset, cell_count, cost_tier, create_presets, is_default_preset, map_area_km2,
    metric_extent_m, preset_by_id, presets, red_blob_radius_m, CostTier, MapExtentPreset,
    ALPHA_CREATE_CATALOG_MAX_CELLS, ALPHA_NEIGHBOR_CENTER_DISTANCE_M, PRESET_REGIONAL_12X8,
    PRESET_REGIONAL_26X16, PRESET_REGIONAL_52X32, PRESET_WIDE_2000,
};
pub use state::{default_spatial_state, SpatialState, SPATIAL_STATE_RELATIVE};
