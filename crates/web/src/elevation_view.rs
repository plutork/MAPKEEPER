//! elevation-authoring-v2: palette, overlays, LOD — web projection only.

use web_sys::CanvasRenderingContext2d;

pub const ELEVATION_TINT_CAP: i32 = 100;
pub const MOUNTAIN_THRESHOLD: i32 = 75;
pub const OVERLAY_LOD_MAX_VISIBLE: usize = 2000;
pub const OVERLAY_LOD_MIN_ZOOM: f64 = 1.6;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Hydro,
    Elevation,
}

pub fn overlays_lod_ok(visible_cells: usize, zoom: f64) -> bool {
    visible_cells <= OVERLAY_LOD_MAX_VISIBLE || zoom >= OVERLAY_LOD_MIN_ZOOM
}

pub fn peaks_status_label(
    show_peaks: bool,
    color_mode: ColorMode,
    visible_cells: usize,
    zoom: f64,
) -> &'static str {
    if !show_peaks || color_mode != ColorMode::Elevation {
        "Peaks Off"
    } else if !overlays_lod_ok(visible_cells, zoom) {
        "Peaks LOD-off"
    } else {
        "Peaks On"
    }
}

pub fn labels_status_label(show_labels: bool, visible_cells: usize, zoom: f64) -> &'static str {
    if !show_labels {
        "Elev Off"
    } else if !overlays_lod_ok(visible_cells, zoom) {
        "Elev LOD-off"
    } else {
        "Elev On"
    }
}

/// Land tint 1..cap and bathymetric water (elev <= 0).
pub fn elevation_fill_rgb(elevation: i32) -> (u8, u8, u8) {
    if elevation <= 0 {
        let depth = (-elevation).min(50);
        let t = depth as f64 / 50.0;
        let r = (30.0 + t * 20.0) as u8;
        let g = (90.0 + t * 40.0) as u8;
        let b = (160.0 + t * 60.0) as u8;
        return (r, g, b);
    }
    let capped = elevation.min(ELEVATION_TINT_CAP).max(1);
    let t = (capped - 1) as f64 / (ELEVATION_TINT_CAP - 1) as f64;
    if t < 0.5 {
        let u = t * 2.0;
        let r = (40.0 + u * 180.0) as u8;
        let g = (120.0 + u * 100.0) as u8;
        let b = (50.0 * (1.0 - u)) as u8;
        (r, g, b)
    } else {
        let u = (t - 0.5) * 2.0;
        let r = 220u8;
        let g = (200.0 * (1.0 - u)) as u8;
        let b = 40u8;
        (r, g, b)
    }
}

pub fn set_fill_rgb(ctx: &CanvasRenderingContext2d, rgb: (u8, u8, u8), buf: &mut String) {
    buf.clear();
    use std::fmt::Write;
    let _ = write!(buf, "rgb({},{},{})", rgb.0, rgb.1, rgb.2);
    ctx.set_fill_style_str(buf);
}

pub fn draw_mountain_glyph(ctx: &CanvasRenderingContext2d, cx: f64, cy: f64, hex_size: f64) {
    // elevation-authoring-v2: large silhouette — ~cell fill, not a tiny marker.
    let h = (hex_size * 0.78).clamp(10.0, 56.0);
    let w = h * 1.05;
    ctx.set_fill_style_str("#e8e0d0");
    ctx.set_stroke_style_str("#3a2e22");
    ctx.set_line_width((hex_size * 0.04).clamp(1.0, 2.5));
    ctx.begin_path();
    ctx.move_to(cx, cy - h * 0.46);
    ctx.line_to(cx + w * 0.5, cy + h * 0.38);
    ctx.line_to(cx - w * 0.5, cy + h * 0.38);
    ctx.close_path();
    ctx.fill();
    ctx.stroke();
}

pub fn draw_elevation_label(
    ctx: &CanvasRenderingContext2d,
    cx: f64,
    cy: f64,
    hex_size: f64,
    elevation: i32,
    below_center: bool,
) {
    let font_px = (hex_size * 0.32).clamp(7.0, 12.0);
    ctx.set_font(&format!("{font_px}px system-ui,sans-serif"));
    ctx.set_fill_style_str("#f0f4f8");
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    let y = if below_center {
        cy + hex_size * 0.28
    } else {
        cy
    };
    let _ = ctx.fill_text(&elevation.to_string(), cx, y);
}
