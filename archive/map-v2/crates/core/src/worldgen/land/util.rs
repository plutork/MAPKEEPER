//! Shared land-mask helpers.

use crate::hex::{Axial, MapBounds};
use crate::layer::{DenseLayer, DenseState, LayerValue};

use super::types::{LayoutBlob, LayoutClass, LayoutRecipe, LAND_MASK_LAND, LAND_MASK_OCEAN};

pub(crate) fn min_island_cells(recipe: &LayoutRecipe) -> usize {
    match recipe.layout_class {
        LayoutClass::Archipelago => 2,
        LayoutClass::Island => 3,
        _ => 4,
    }
}

pub(crate) fn elongation_axis(seed: u64, amount: f64) -> (f64, f64) {
    if amount < 0.05 {
        return (1.0, 0.0);
    }
    let angle = unit01(seed) * std::f64::consts::TAU;
    (angle.cos(), angle.sin())
}

pub(crate) fn pick_seed_cell(
    bounds: &MapBounds,
    zone: &LayoutBlob,
    seed: u64,
    max_x: f64,
    max_y: f64,
    merge_bias: f64,
) -> usize {
    let jitter = 0.35 + (1.0 - merge_bias) * 0.4;
    let nx = zone.cx + (unit01(seed ^ 0x11) * 2.0 - 1.0) * zone.rx * jitter;
    let ny = zone.cy + (unit01(seed ^ 0x22) * 2.0 - 1.0) * zone.ry * jitter;
    nearest_index(bounds, nx, ny, max_x, max_y)
}

pub(crate) fn nearest_index(bounds: &MapBounds, nx: f64, ny: f64, max_x: f64, max_y: f64) -> usize {
    let mut best = 0usize;
    let mut best_d = f64::MAX;
    for index in 0..bounds.len() {
        let Some(cell) = bounds.from_index(index) else {
            continue;
        };
        let (x, y) = cell.to_pixel(1.0);
        let cx = if max_x > 0.0 { x / max_x } else { 0.0 };
        let cy = if max_y > 0.0 { y / max_y } else { 0.0 };
        let d = (cx - nx).hypot(cy - ny);
        if d < best_d {
            best_d = d;
            best = index;
        }
    }
    best
}

pub(crate) fn shuffle_six(items: &mut [Axial; 6], seed: u64) {
    let mut s = seed;
    for i in (1..6).rev() {
        s = mix64(s ^ (i as u64 * 0x9E37));
        let j = (s as usize) % (i + 1);
        items.swap(i, j);
    }
}

pub(crate) fn opposite_hex_pair(center: Axial, a: Axial, b: Axial) -> bool {
    let da = (a.q - center.q, a.r - center.r);
    let db = (b.q - center.q, b.r - center.r);
    da.0 + db.0 == 0 && da.1 + db.1 == 0
}

/// Connected land components, largest first.
pub(crate) fn land_components(bounds: &MapBounds, layer: &DenseLayer) -> Vec<Vec<usize>> {
    let n = bounds.len();
    let mut seen = vec![false; n];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for start in 0..n {
        if seen[start] || !is_land_cell(layer, start) {
            continue;
        }
        let mut stack = vec![start];
        let mut component = Vec::new();
        seen[start] = true;
        while let Some(i) = stack.pop() {
            component.push(i);
            let Some(cell) = bounds.from_index(i) else {
                continue;
            };
            for nb in cell.neighbors() {
                let Some(ni) = bounds.index_of(nb) else {
                    continue;
                };
                if seen[ni] || !is_land_cell(layer, ni) {
                    continue;
                }
                seen[ni] = true;
                stack.push(ni);
            }
        }
        out.push(component);
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.len()));
    out
}

pub(crate) fn flood_ocean(layer: &mut DenseLayer, cells: &[usize]) {
    for &i in cells {
        layer.set(
            i,
            DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
        );
    }
}

pub(crate) fn component_centroid(bounds: &MapBounds, cells: &[usize]) -> (f64, f64) {
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut n = 0.0;
    for &i in cells {
        let Some(cell) = bounds.from_index(i) else {
            continue;
        };
        let (x, y) = cell.to_pixel(1.0);
        sx += x;
        sy += y;
        n += 1.0;
    }
    if n <= 0.0 {
        (0.0, 0.0)
    } else {
        (sx / n, sy / n)
    }
}

pub(crate) fn set_land(layer: &mut DenseLayer, index: usize) {
    layer.set(
        index,
        DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
    );
}

pub(crate) fn is_land_cell(layer: &DenseLayer, index: usize) -> bool {
    matches!(
        layer.state(index),
        DenseState::Value(LayerValue::Text(kind)) if kind == LAND_MASK_LAND
    )
}

pub(crate) fn is_non_land(layer: &DenseLayer, index: usize) -> bool {
    !matches!(
        layer.state(index),
        DenseState::Value(LayerValue::Text(kind)) if kind == LAND_MASK_LAND
    )
}

pub(crate) fn is_boundary_cell(bounds: &MapBounds, cell: Axial) -> bool {
    cell.neighbors().iter().any(|n| !bounds.contains(*n))
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

pub(crate) fn unit01(seed: u64) -> f64 {
    let x = mix64(seed);
    (x as f64) / (u64::MAX as f64)
}

pub(crate) fn mix64(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}
