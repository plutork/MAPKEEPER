//! Variable-width river polygon helpers (presentation only; D-55 track C).

/// Smooth a polyline with uniform Catmull-Rom samples per segment.
pub fn smooth_centerline(points: &[(f64, f64)], samples_per_segment: usize) -> Vec<(f64, f64)> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let samples = samples_per_segment.max(2);
    let mut out = Vec::new();
    for i in 0..points.len() - 1 {
        let p0 = if i == 0 { points[0] } else { points[i - 1] };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < points.len() {
            points[i + 2]
        } else {
            points[i + 1]
        };
        let end = if i + 1 == points.len() - 1 {
            samples
        } else {
            samples - 1
        };
        for step in 0..end {
            let t = step as f64 / samples as f64;
            out.push(catmull_rom(p0, p1, p2, p3, t));
        }
    }
    out.push(*points.last().unwrap());
    out
}

fn catmull_rom(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    t: f64,
) -> (f64, f64) {
    let t2 = t * t;
    let t3 = t2 * t;
    let x = 0.5
        * ((2.0 * p1.0)
            + (-p0.0 + p2.0) * t
            + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
            + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3);
    let y = 0.5
        * ((2.0 * p1.1)
            + (-p0.1 + p2.1) * t
            + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
            + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);
    (x, y)
}

/// Half-widths growing from source (index 0) toward mouth (last index).
pub fn half_widths_along(n: usize, min_half_width: f64, max_half_width: f64) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![max_half_width];
    }
    (0..n)
        .map(|i| {
            let t = i as f64 / (n - 1) as f64;
            min_half_width + t * (max_half_width - min_half_width)
        })
        .collect()
}

/// Closed polygon ribbon from centerline + per-point half-widths.
pub fn offset_ribbon_polygon(
    centerline: &[(f64, f64)],
    half_widths: &[f64],
) -> Vec<(f64, f64)> {
    let n = centerline.len().min(half_widths.len());
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        let hw = half_widths[0];
        let (cx, cy) = centerline[0];
        return vec![
            (cx - hw, cy - hw),
            (cx + hw, cy - hw),
            (cx + hw, cy + hw),
            (cx - hw, cy + hw),
        ];
    }
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let (tx, ty) = tangent_at(centerline, i);
        let len = (tx * tx + ty * ty).sqrt().max(1e-6);
        let nx = -ty / len;
        let ny = tx / len;
        let hw = half_widths[i];
        let (cx, cy) = centerline[i];
        left.push((cx + nx * hw, cy + ny * hw));
        right.push((cx - nx * hw, cy - ny * hw));
    }
    let mut polygon = left;
    right.reverse();
    polygon.extend(right);
    polygon
}

fn tangent_at(line: &[(f64, f64)], i: usize) -> (f64, f64) {
    if line.len() < 2 {
        return (1.0, 0.0);
    }
    if i == 0 {
        let (x0, y0) = line[0];
        let (x1, y1) = line[1];
        (x1 - x0, y1 - y0)
    } else if i + 1 == line.len() {
        let (x0, y0) = line[i - 1];
        let (x1, y1) = line[i];
        (x1 - x0, y1 - y0)
    } else {
        let (x0, y0) = line[i - 1];
        let (x1, y1) = line[i + 1];
        (x1 - x0, y1 - y0)
    }
}

/// Build a variable-width ribbon polygon from map-space center points.
pub fn river_ribbon_polygon(
    centers: &[(f64, f64)],
    hex_size_px: f64,
    samples_per_segment: usize,
) -> Vec<(f64, f64)> {
    if centers.is_empty() {
        return Vec::new();
    }
    let min_hw = (hex_size_px * 0.08).clamp(1.5, 8.0);
    let max_hw = (hex_size_px * 0.30).clamp(min_hw + 1.0, 24.0);
    let smooth = smooth_centerline(centers, samples_per_segment);
    let widths = half_widths_along(smooth.len(), min_hw, max_hw);
    offset_ribbon_polygon(&smooth, &widths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_centerline_preserves_endpoints() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (20.0, 5.0), (30.0, 5.0)];
        let smooth = smooth_centerline(&pts, 4);
        assert_eq!(smooth.first().copied(), Some((0.0, 0.0)));
        assert_eq!(smooth.last().copied(), Some((30.0, 5.0)));
        assert!(smooth.len() > pts.len());
    }

    #[test]
    fn half_widths_grow_toward_mouth() {
        let widths = half_widths_along(5, 2.0, 10.0);
        assert_eq!(widths.first().copied(), Some(2.0));
        assert_eq!(widths.last().copied(), Some(10.0));
        assert!(widths[3] > widths[1]);
    }

    #[test]
    fn ribbon_polygon_is_closed_loop() {
        let centers = vec![(0.0, 0.0), (20.0, 0.0), (40.0, 10.0)];
        let poly = river_ribbon_polygon(&centers, 16.0, 4);
        assert!(poly.len() >= 6);
        let first = poly[0];
        let last = poly[poly.len() - 1];
        let dx = (first.0 - last.0).abs();
        let dy = (first.1 - last.1).abs();
        assert!(dx + dy > 0.0);
    }
}
