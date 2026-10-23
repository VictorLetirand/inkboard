//! Freehand strokes: smoothing and variable width according to the pen pressure.

use crate::geom::Pt;

/// Width of a point according to the pressure (0..1): from 30 % to 160 % of the base width.
pub fn pressure_width(base: f64, p: f32) -> f64 {
    base * (0.3 + 1.3 * f64::from(p.clamp(0.0, 1.0)))
}

/// Smoothing with quadratic curves through the segment midpoints. Returns the sampled points
/// (`steps` per segment) and the interpolated pressure (0.5 when no pressure is provided).
pub fn smooth(pts: &[Pt], pressures: Option<&[f32]>, steps: usize) -> (Vec<Pt>, Vec<f32>) {
    let n = pts.len();
    let pr = |i: usize| pressures.and_then(|p| p.get(i)).copied().unwrap_or(0.5);
    if n < 3 {
        return (pts.to_vec(), (0..n).map(pr).collect());
    }
    let steps = steps.max(1);
    let mut out = vec![pts[0]];
    let mut out_p = vec![pr(0)];
    let (mut prev, mut prev_p) = (pts[0], pr(0));
    for i in 1..n - 1 {
        let mid = pts[i].lerp(pts[i + 1], 0.5);
        let mid_p = (pr(i) + pr(i + 1)) / 2.0;
        for k in 1..=steps {
            let t = k as f64 / steps as f64;
            let u = 1.0 - t;
            out.push(prev * (u * u) + pts[i] * (2.0 * u * t) + mid * (t * t));
            out_p.push(prev_p + (mid_p - prev_p) * t as f32);
        }
        prev = mid;
        prev_p = mid_p;
    }
    out.push(pts[n - 1]);
    out_p.push(pr(n - 1));
    (out, out_p)
}

/// Left and right edges of a variable-width stroke: two points offset by `±w/2` perpendicular to the path.
pub fn stroke_sides(pts: &[Pt], widths: &[f64]) -> (Vec<Pt>, Vec<Pt>) {
    let n = pts.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
        let d = (b - a).normalized();
        let normal = if d == Pt::ZERO { Pt::new(0.0, 1.0) } else { Pt::new(-d.y, d.x) };
        let h = widths.get(i).copied().unwrap_or(0.0) / 2.0;
        left.push(pts[i] + normal * h);
        right.push(pts[i] - normal * h);
    }
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_grows_with_pressure_and_is_clamped() {
        assert!(pressure_width(4.0, 0.0) < pressure_width(4.0, 0.5));
        assert!(pressure_width(4.0, 0.5) < pressure_width(4.0, 1.0));
        assert_eq!(pressure_width(4.0, 2.0), pressure_width(4.0, 1.0));
        assert!((pressure_width(10.0, 0.0) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn smoothing_keeps_endpoints_and_interpolates_pressure() {
        let pts = [Pt::new(0.0, 0.0), Pt::new(10.0, 10.0), Pt::new(20.0, 0.0), Pt::new(30.0, 10.0)];
        let pr = [0.0, 1.0, 0.0, 1.0];
        let (p, q) = smooth(&pts, Some(&pr), 4);
        assert_eq!(p.len(), q.len());
        assert_eq!(p[0], pts[0]);
        assert_eq!(*p.last().unwrap(), pts[3]);
        assert_eq!(q[0], 0.0);
        assert_eq!(*q.last().unwrap(), 1.0);
        assert!(q.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(p.len() > pts.len());
    }

    #[test]
    fn short_strokes_and_missing_pressure_are_handled() {
        let (p, q) = smooth(&[Pt::ZERO, Pt::new(1.0, 1.0)], None, 4);
        assert_eq!((p.len(), q), (2, vec![0.5, 0.5]));
        assert!(smooth(&[], None, 4).0.is_empty());
    }

    #[test]
    fn sides_are_offset_by_half_the_width() {
        let pts = [Pt::new(0.0, 0.0), Pt::new(10.0, 0.0), Pt::new(20.0, 0.0)];
        let (l, r) = stroke_sides(&pts, &[2.0, 4.0, 6.0]);
        for i in 0..3 {
            assert!((l[i].dist(r[i]) - [2.0, 4.0, 6.0][i]).abs() < 1e-9);
            assert!((l[i].x - pts[i].x).abs() < 1e-9, "offset perpendicular to a horizontal path");
        }
        // a single point: no division by zero
        let (l, r) = stroke_sides(&[Pt::new(5.0, 5.0)], &[4.0]);
        assert!(l[0].dist(r[0]) > 3.9);
    }
}
