//! Canvas layout, redraw, and rAF coalescing (D-94 B2).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::hydro::{hydro_from_elevation, HydroKind};

use crate::brush::{
    brush_preview_uses_circle, effective_paint_radius, paint_stamp_cells, river_brush,
    sync_brush_effective_label, sync_brush_radius_active, sync_brush_step_active,
    sync_falloff_active,
};
use crate::dom::{canvas, context, perf_now, set_text, window};
use crate::elevation_view::{
    draw_elevation_label, draw_mountain_glyph, elevation_fill_rgb, labels_status_label,
    overlays_lod_ok, peaks_status_label, set_fill_rgb, ColorMode, MOUNTAIN_THRESHOLD,
};
use crate::state::{
    count_visible_in_bounds, draw_snapshot, elevation_at, geology_tint, grid_lines_stats_label,
    grid_lines_toggle_label, show_profile_markers, stroke_grid_enabled, AppState, Brush,
    BRUSH_PREVIEW_GAP, CANVAS_PAD, FILL_SCALE_GRID_OFF, FILL_SCALE_GRID_ON, GRID_LINE_WIDTH,
    MIN_ZOOM, ZOOM_CLOSEUP_HEX_PX, ZOOM_MAX_HARD,
};
use crate::wizard::wizard_is_active;
use wasm_bindgen::prelude::*;
use web_sys::CanvasRenderingContext2d;

pub(crate) fn redraw_and_sample(state: &Rc<RefCell<AppState>>) {
    let t0 = perf_now();
    let drawn = redraw(&state.borrow());
    let ms = perf_now() - t0;
    let mut s = state.borrow_mut();
    s.perf.redraw_ms = Some(ms);
    s.perf.drawn_cells = Some(drawn);
    s.last_draw_snapshot = Some(draw_snapshot(&s));
    set_text("view-perf", &s.perf.view_line());
}
pub(crate) fn schedule_redraw(state: Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        s.redraw_dirty = true;
        if s.redraw_raf_pending {
            return;
        }
        s.redraw_raf_pending = true;
    }

    let state_cb = state.clone();
    let closure = Closure::once(move || {
        flush_scheduled_redraw(state_cb);
    });
    let _ = window()
        .request_animation_frame(closure.as_ref().unchecked_ref())
        .expect("request_animation_frame");
    closure.forget();
}

pub(crate) fn flush_scheduled_redraw(state: Rc<RefCell<AppState>>) {
    let should_draw = {
        let mut s = state.borrow_mut();
        s.redraw_raf_pending = false;
        if !s.redraw_dirty {
            false
        } else {
            let snap = draw_snapshot(&s);
            if s.last_draw_snapshot == Some(snap) {
                s.redraw_dirty = false;
                false
            } else {
                s.redraw_dirty = false;
                true
            }
        }
    };
    if should_draw {
        redraw_and_sample(&state);
    }
    if state.borrow().redraw_dirty {
        schedule_redraw(state);
    }
}
pub(crate) fn draw_preview_boundary(
    state: &AppState,
    ctx: &CanvasRenderingContext2d,
    size: f64,
    ox: f64,
    oy: f64,
) {
    let wizard_edit = wizard_is_active() && state.wizard_edit_mode;
    if !wizard_edit && (matches!(state.brush, Brush::Inspect) || river_brush(&state.brush)) {
        return;
    }
    let Some(center) = state.hover_cell else {
        return;
    };
    let radius = effective_paint_radius(state);
    let (cx, cy) = {
        let (x, y) = Axial::new(center.0, center.1).to_pixel(size);
        (ox + x, oy + y)
    };
    ctx.set_line_width(2.0);
    ctx.set_stroke_style_str("#9fe3c4");
    // Large M/L/XL: per-hex boundary walk freezes the app (esp. with labels on).
    if brush_preview_uses_circle(radius) {
        let hex_w = (3f64.sqrt() * size).max(1.0);
        let r_px = hex_w * (radius as f64 + 0.55);
        ctx.begin_path();
        let _ = ctx.arc(cx, cy, r_px, 0.0, std::f64::consts::PI * 2.0);
        ctx.stroke();
        return;
    }
    let cells = paint_stamp_cells(center, radius, state.map_bounds);
    if cells.is_empty() {
        return;
    }
    let cell_set: HashSet<(i32, i32)> = cells.iter().copied().collect();
    for (q, r) in cells {
        let axial = Axial::new(q, r);
        let is_boundary = axial
            .neighbors()
            .iter()
            .any(|n| !cell_set.contains(&(n.q, n.r)));
        if !is_boundary {
            continue;
        }
        let (x, y) = axial.to_pixel(size);
        let corners = hex_corners(ox + x, oy + y, size * BRUSH_PREVIEW_GAP * 0.98);
        ctx.begin_path();
        ctx.move_to(corners[0].0, corners[0].1);
        for corner in &corners[1..] {
            ctx.line_to(corner.0, corner.1);
        }
        ctx.close_path();
        ctx.stroke();
    }
}

/// Lake basin fill overlay (hydrology-water-generation-ui-v1).
pub(crate) fn draw_lakes(
    state: &AppState,
    ctx: &CanvasRenderingContext2d,
    size: f64,
    ox: f64,
    oy: f64,
) {
    if state.lakes.lakes.is_empty() {
        return;
    }
    ctx.set_fill_style_str("rgba(58, 143, 217, 0.62)");
    let bounds = state.map_bounds;
    let fill_scale = if state.show_grid {
        FILL_SCALE_GRID_ON
    } else {
        FILL_SCALE_GRID_OFF
    };
    for lake in &state.lakes.lakes {
        for &idx in &lake.cells {
            let Some(cell) = bounds.from_index(idx) else {
                continue;
            };
            let (x, y) = cell.to_pixel(size);
            let (cx, cy) = (ox + x, oy + y);
            let corners = hex_corners(cx, cy, size * fill_scale);
            ctx.begin_path();
            ctx.move_to(corners[0].0, corners[0].1);
            for corner in &corners[1..] {
                ctx.line_to(corner.0, corner.1);
            }
            ctx.close_path();
            ctx.fill();
        }
    }
}

/// Hydrology v2 physical-segment projection: center-to-center polyline strokes.
pub(crate) fn draw_rivers(
    state: &AppState,
    ctx: &CanvasRenderingContext2d,
    size: f64,
    ox: f64,
    oy: f64,
) {
    if state.rivers.rivers.is_empty() {
        return;
    }
    ctx.set_stroke_style_str("#4da6ff");
    ctx.set_fill_style_str("#4da6ff");
    ctx.set_line_width(2.0);
    ctx.set_line_cap("round");
    ctx.set_line_join("round");
    let bounds = state.map_bounds;
    let dot_r = (size * 0.12).clamp(2.0, 6.0);
    for river in &state.rivers.rivers {
        if river.cells.is_empty() {
            continue;
        }
        if river.cells.len() == 1 {
            let Some(cell) = bounds.from_index(river.cells[0]) else {
                continue;
            };
            let (x, y) = cell.to_pixel(size);
            ctx.begin_path();
            let _ = ctx.arc(ox + x, oy + y, dot_r, 0.0, std::f64::consts::TAU);
            ctx.fill();
            continue;
        }
        ctx.begin_path();
        let mut started = false;
        for &idx in &river.cells {
            let Some(cell) = bounds.from_index(idx) else {
                continue;
            };
            let (x, y) = cell.to_pixel(size);
            let (cx, cy) = (ox + x, oy + y);
            if started {
                ctx.line_to(cx, cy);
            } else {
                ctx.move_to(cx, cy);
                started = true;
            }
        }
        if started {
            ctx.stroke();
        }
    }
}
fn hex_corners(cx: f64, cy: f64, size: f64) -> [(f64, f64); 6] {
    std::array::from_fn(|i| {
        let angle = (60.0 * i as f64 - 30.0).to_radians();
        (cx + size * angle.cos(), cy + size * angle.sin())
    })
}

/// Half-extent (in unit-size pixels) of the whole hex map, including the
/// outer cells' corner reach. Pointy-top corners stick out `√3/2` sideways
/// and `1.0` vertically. Used to fit the map into the current canvas.
fn map_half_extent(bounds: MapBounds) -> (f64, f64) {
    let mut mx = 0.0_f64;
    let mut my = 0.0_f64;
    for cell in bounds.cells() {
        let (x, y) = cell.to_pixel(1.0);
        mx = mx.max(x.abs());
        my = my.max(y.abs());
    }
    (mx + 3f64.sqrt() / 2.0, my + 1.0)
}

/// Fit-to-window layout: hex `size` and origin so the rectangle map fills the
/// canvas with padding. Centered on the axial origin.
pub(crate) fn hex_layout(width: f64, height: f64, bounds: MapBounds) -> (f64, f64, f64) {
    let (hx, hy) = map_half_extent(bounds);
    let avail_w = (width - 2.0 * CANVAS_PAD).max(1.0);
    let avail_h = (height - 2.0 * CANVAS_PAD).max(1.0);
    let size = (avail_w / (2.0 * hx)).min(avail_h / (2.0 * hy)).max(1.0);
    (size, width / 2.0, height / 2.0)
}

/// Full layout with camera applied on top of the fit-to-window base.
pub(crate) fn map_layout(state: &AppState, width: f64, height: f64) -> (f64, f64, f64) {
    let (base_size, base_ox, base_oy) = hex_layout(width, height, state.map_bounds);
    let size = base_size * state.zoom;
    let ox = base_ox + state.pan_x;
    let oy = base_oy + state.pan_y;
    (size, ox, oy)
}

pub(crate) fn max_zoom_for_base_hex(base_size_px: f64) -> f64 {
    // Fit (1.0) always allowed; zoom-in only until hex reaches ZOOM_CLOSEUP_HEX_PX.
    let from_target = ZOOM_CLOSEUP_HEX_PX / base_size_px.max(1.0);
    from_target.clamp(1.0, ZOOM_MAX_HARD)
}

pub(crate) fn clamp_zoom(base_size_px: f64, value: f64) -> f64 {
    value.clamp(MIN_ZOOM, max_zoom_for_base_hex(base_size_px))
}
pub(crate) fn current_hex_size_px(state: &AppState) -> f64 {
    let canvas = canvas();
    let rect = canvas.get_bounding_client_rect();
    let (size, _, _) = map_layout(state, rect.width(), rect.height());
    size
}
fn visible_scan_bounds(
    width: f64,
    height: f64,
    size: f64,
    ox: f64,
    oy: f64,
    bounds: MapBounds,
) -> (i32, i32, i32, i32) {
    let (bmin_q, bmax_q, bmin_r, bmax_r) = bounds.axial_limits();
    let mut min_q = i32::MAX;
    let mut max_q = i32::MIN;
    let mut min_r = i32::MAX;
    let mut max_r = i32::MIN;
    for (sx, sy) in [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)] {
        let cell = Axial::from_pixel(sx - ox, sy - oy, size);
        min_q = min_q.min(cell.q);
        max_q = max_q.max(cell.q);
        min_r = min_r.min(cell.r);
        max_r = max_r.max(cell.r);
    }
    let pad = ((2.0 / size).ceil() as i32).max(2);
    (
        min_q.saturating_sub(pad).max(bmin_q),
        max_q.saturating_add(pad).min(bmax_q),
        min_r.saturating_sub(pad).max(bmin_r),
        max_r.saturating_add(pad).min(bmax_r),
    )
}

/// Match the canvas backing store to its CSS box so the map scales with the
/// window (no browser upscaling blur). Returns the current pixel dimensions.
fn sync_canvas_size() -> (f64, f64) {
    let canvas = canvas();
    let rect = canvas.get_bounding_client_rect();
    let w = rect.width().max(1.0);
    let h = rect.height().max(1.0);
    if (canvas.width() as f64 - w).abs() >= 1.0 {
        canvas.set_width(w as u32);
    }
    if (canvas.height() as f64 - h).abs() >= 1.0 {
        canvas.set_height(h as u32);
    }
    (canvas.width() as f64, canvas.height() as f64)
}

pub(crate) fn redraw(state: &AppState) -> usize {
    let (width, height) = sync_canvas_size();
    let ctx = context();
    let bounds = state.map_bounds;
    let (size, ox, oy) = map_layout(state, width, height);

    ctx.clear_rect(0.0, 0.0, width, height);
    ctx.set_fill_style_str("#0e1113");
    ctx.fill_rect(0.0, 0.0, width, height);

    let (q_min, q_max, r_min, r_max) = visible_scan_bounds(width, height, size, ox, oy, bounds);
    let visible_cells = count_visible_in_bounds(q_min, q_max, r_min, r_max, bounds);
    let stroke_grid = stroke_grid_enabled(state.show_grid, visible_cells);
    let fill_scale = if state.show_grid {
        FILL_SCALE_GRID_ON
    } else {
        FILL_SCALE_GRID_OFF
    };
    let draw_profile_dots = show_profile_markers(state.zoom);
    let overlay_lod = overlays_lod_ok(visible_cells, state.zoom);
    let draw_labels = state.show_elevation_labels && overlay_lod;
    let draw_peaks = state.show_peaks && state.color_mode == ColorMode::Elevation && overlay_lod;
    let mut color_buf = String::with_capacity(20);
    let mut drawn_cells = 0usize;
    for q in q_min..=q_max {
        for r in r_min..=r_max {
            let cell = Axial::new(q, r);
            if !bounds.contains(cell) {
                continue;
            }
            let (x, y) = cell.to_pixel(size);
            let (cx, cy) = (ox + x, oy + y);
            let corners = hex_corners(cx, cy, size * fill_scale);

            ctx.begin_path();
            ctx.move_to(corners[0].0, corners[0].1);
            for corner in &corners[1..] {
                ctx.line_to(corner.0, corner.1);
            }
            ctx.close_path();

            let selected = state.selected == Some((q, r));
            let elevation = elevation_at(&state.elevation, bounds, q, r);
            match state.color_mode {
                ColorMode::Hydro => {
                    ctx.set_fill_style_str(hydro_fill(elevation));
                }
                ColorMode::Elevation => {
                    set_fill_rgb(&ctx, elevation_fill_rgb(elevation), &mut color_buf);
                }
            }
            ctx.fill();
            // geology-readable--preview-contrast: class fill on Tectonics step
            if wizard_is_active() && state.wizard_step == 3 {
                if let Some(geo) = state.geology.as_ref() {
                    if let Some(tint) = geology_tint(geo, bounds, q, r) {
                        ctx.set_fill_style_str(tint);
                        ctx.fill();
                    }
                }
            }
            if selected {
                ctx.set_line_width(3.0);
                ctx.set_stroke_style_str("#9fe3c4");
                ctx.stroke();
            } else if stroke_grid {
                ctx.set_line_width(GRID_LINE_WIDTH);
                ctx.set_stroke_style_str("#3a424b");
                ctx.stroke();
            }

            if draw_peaks && elevation > MOUNTAIN_THRESHOLD {
                draw_mountain_glyph(&ctx, cx, cy, size);
            }
            if draw_labels {
                let label_below = draw_peaks && elevation > MOUNTAIN_THRESHOLD;
                draw_elevation_label(&ctx, cx, cy, size, elevation, label_below);
            }

            // Profile-presence marker — a separate layer from terrain, so both
            // are visible at once (a cell can have terrain, a profile, or both).
            if draw_profile_dots && state.cells.contains_key(&(q, r)) {
                ctx.begin_path();
                let dot = (size * 0.13).clamp(2.5, 5.0);
                let _ = ctx.arc(cx, cy, dot, 0.0, std::f64::consts::PI * 2.0);
                ctx.set_fill_style_str("#e8d27a");
                ctx.fill();
            }
            drawn_cells += 1;
        }
    }
    let hover_elev = state
        .hover_cell
        .map(|(q, r)| elevation_at(&state.elevation, bounds, q, r));
    let hover_note = hover_elev
        .map(|e| format!(" · Hover elev {e}"))
        .unwrap_or_default();
    set_text(
        "view-stats",
        &format!(
            "Zoom {:.2}x · Draw {} / {} cells · Grid {} · {} · {}{}",
            state.zoom,
            drawn_cells,
            bounds.len(),
            grid_lines_stats_label(state.show_grid, visible_cells),
            labels_status_label(state.show_elevation_labels, visible_cells, state.zoom),
            peaks_status_label(
                state.show_peaks,
                state.color_mode,
                visible_cells,
                state.zoom,
            ),
            hover_note,
        ),
    );
    draw_preview_boundary(state, &ctx, size, ox, oy);
    draw_lakes(state, &ctx, size, ox, oy);
    draw_rivers(state, &ctx, size, ox, oy);
    set_text("toggle-grid", grid_lines_toggle_label(state.show_grid));
    set_text(
        "toggle-color-mode",
        if state.color_mode == ColorMode::Elevation {
            "Color: Elevation"
        } else {
            "Color: Hydro"
        },
    );
    set_text(
        "toggle-elevation-labels",
        if state.show_elevation_labels {
            "Show elevation: On"
        } else {
            "Show elevation: Off"
        },
    );
    set_text(
        "toggle-peaks",
        if state.show_peaks {
            "Peaks: On"
        } else {
            "Peaks: Off"
        },
    );
    sync_brush_radius_active(state.brush_radius);
    sync_brush_effective_label(state);
    let eff = effective_paint_radius(state);
    sync_falloff_active(state.falloff_even, eff);
    sync_brush_step_active(state.brush_step);
    drawn_cells
}

fn hydro_fill(elevation: i32) -> &'static str {
    match hydro_from_elevation(elevation) {
        HydroKind::Land => "#6a7b43",
        HydroKind::Water => "#2e5f8a",
    }
}
