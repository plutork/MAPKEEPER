//! Hidden tectonics → geology style mapping.

use crate::hex::{Axial, MapBounds};
use crate::layer::DenseLayer;
use crate::worldgen::plates::{hash01, BoundaryKind};

use super::types::{
    GeologyStyle, GEOLOGY_BASIN, GEOLOGY_RIDGE, GEOLOGY_RIFT, GEOLOGY_STABLE, GEOLOGY_VOLCANIC_ARC,
};

pub(crate) fn coast_proximity(bounds: &MapBounds, land_mask: &DenseLayer, cell: Axial) -> f64 {
    use super::land_helpers::is_land_cell;
    let mut water_n = 0usize;
    let mut total = 0usize;
    for n in cell.neighbors() {
        let Some(idx) = bounds.index_of(n) else {
            continue;
        };
        total += 1;
        if !is_land_cell(land_mask, idx) {
            water_n += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    water_n as f64 / total as f64
}

pub(crate) fn half_extent(bounds: &MapBounds) -> (f64, f64) {
    let mut max_x: f64 = 0.0;
    let mut max_y: f64 = 0.0;
    for c in bounds.cells() {
        let (x, y) = c.to_pixel(1.0);
        max_x = max_x.max(x.abs());
        max_y = max_y.max(y.abs());
    }
    (max_x.max(1.0), max_y.max(1.0))
}

/// Map hidden plate boundary signal + author style → geology category.
pub fn map_hidden_tectonics_to_geology_style(
    style: GeologyStyle,
    boundary: BoundaryKind,
    influence: f64,
    boundary_dist: u8,
    nx: f64,
    ny: f64,
    coast: f64,
    cell: Axial,
    seed: u64,
) -> &'static str {
    let n = hash01(seed ^ 0x6E01, cell.q, cell.r);

    match style {
        GeologyStyle::Belts => {
            if should_place_orogenic(style, boundary, influence, boundary_dist, cell, seed) {
                orogenic_class_for_boundary(boundary, n, coast, seed, cell)
            } else if coast > 0.30 && n > 0.62 {
                GEOLOGY_VOLCANIC_ARC
            } else if boundary_dist > 3 && (nx * nx + ny * ny).sqrt() < 0.28 && n > 0.82 {
                GEOLOGY_BASIN
            } else {
                GEOLOGY_STABLE
            }
        }
        GeologyStyle::Shields => {
            if should_place_orogenic(style, boundary, influence, boundary_dist, cell, seed) {
                orogenic_class_for_boundary(boundary, n, coast, seed, cell)
            } else if boundary_dist > 3 && (nx * nx + ny * ny).sqrt() < 0.30 && n > 0.75 {
                GEOLOGY_BASIN
            } else if coast > 0.40 && n > 0.78 {
                GEOLOGY_VOLCANIC_ARC
            } else {
                GEOLOGY_STABLE
            }
        }
        GeologyStyle::Arcs => {
            if coast > 0.12 && n > 0.38 {
                if n > 0.58 {
                    GEOLOGY_VOLCANIC_ARC
                } else {
                    GEOLOGY_RIDGE
                }
            } else if should_place_orogenic(style, boundary, influence, boundary_dist, cell, seed) {
                orogenic_class_for_boundary(boundary, n, coast, seed, cell)
            } else if boundary_dist > 3 && n > 0.90 {
                GEOLOGY_BASIN
            } else {
                GEOLOGY_STABLE
            }
        }
        GeologyStyle::Random => {
            if should_place_orogenic(style, boundary, influence, boundary_dist, cell, seed) {
                orogenic_class_for_boundary(boundary, n, coast, seed, cell)
            } else if n > 0.88 {
                GEOLOGY_BASIN
            } else {
                GEOLOGY_STABLE
            }
        }
    }
}

fn should_place_orogenic(
    style: GeologyStyle,
    boundary: BoundaryKind,
    influence: f64,
    boundary_dist: u8,
    cell: Axial,
    seed: u64,
) -> bool {
    if boundary == BoundaryKind::Interior || boundary_dist > 3 {
        return false;
    }
    let roll = hash01(seed ^ 0x0A0E_6E, cell.q, cell.r);
    let gap = hash01(seed ^ 0x6A70_5, cell.q, cell.r);
    if boundary_dist == 0 && gap < 0.26 {
        return false;
    }

    let mut chance = match boundary_dist {
        0 => 0.50 + influence * 0.36,
        1 => 0.26 + influence * 0.30,
        2 => 0.11 + influence * 0.20,
        3 => 0.05,
        _ => 0.0,
    };
    if boundary_dist <= 1 && hash01(seed ^ 0x8B1D_E6, cell.q, cell.r) > 0.86 {
        chance = chance.max(0.82);
    }
    chance *= match style {
        GeologyStyle::Belts => 1.0,
        GeologyStyle::Shields => 0.52,
        GeologyStyle::Arcs => 0.72,
        GeologyStyle::Random => 1.08,
    };
    roll < chance.clamp(0.0, 0.92)
}

fn orogenic_class_for_boundary(
    boundary: BoundaryKind,
    n: f64,
    coast: f64,
    seed: u64,
    cell: Axial,
) -> &'static str {
    let pick = hash01(seed ^ 0xA4D_0, cell.q, cell.r);
    match boundary {
        BoundaryKind::Divergent => {
            if pick > 0.42 {
                GEOLOGY_RIFT
            } else {
                GEOLOGY_RIDGE
            }
        }
        BoundaryKind::Convergent => {
            if coast > 0.22 && n > 0.55 {
                GEOLOGY_VOLCANIC_ARC
            } else {
                GEOLOGY_RIDGE
            }
        }
        BoundaryKind::Transform => GEOLOGY_RIDGE,
        BoundaryKind::Interior => GEOLOGY_STABLE,
    }
}
