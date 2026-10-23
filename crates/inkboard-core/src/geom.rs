//! 2D geometry: points, rectangles, rotated frames, distance utilities.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Neg, Sub};

/// A point (or a vector) of the plane, in world units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub const ZERO: Pt = Pt { x: 0.0, y: 0.0 };

    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    #[must_use]
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    #[must_use]
    pub fn dist(self, o: Pt) -> f64 {
        (self - o).len()
    }
    #[must_use]
    pub fn dot(self, o: Pt) -> f64 {
        self.x * o.x + self.y * o.y
    }
    #[must_use]
    pub fn lerp(self, o: Pt, t: f64) -> Pt {
        self + (o - self) * t
    }
    /// Unit vector (or zero when the vector is zero).
    #[must_use]
    pub fn normalized(self) -> Pt {
        let l = self.len();
        if l < 1e-12 { Pt::ZERO } else { self * (1.0 / l) }
    }
    /// Rotation by an angle `a` (radians) around `c`.
    #[must_use]
    pub fn rotate(self, c: Pt, a: f64) -> Pt {
        let (s, co) = a.sin_cos();
        let d = self - c;
        Pt::new(c.x + d.x * co - d.y * s, c.y + d.x * s + d.y * co)
    }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for Pt {
    type Output = Pt;
    fn mul(self, k: f64) -> Pt {
        Pt::new(self.x * k, self.y * k)
    }
}
impl Neg for Pt {
    type Output = Pt;
    fn neg(self) -> Pt {
        Pt::new(-self.x, -self.y)
    }
}

/// Axis-aligned rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    /// Normalized rectangle through any two corners.
    #[must_use]
    pub fn from_corners(a: Pt, b: Pt) -> Self {
        Self::new(a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
    }
    /// Bounding box of a set of points; `None` when empty.
    pub fn from_points<I: IntoIterator<Item = Pt>>(it: I) -> Option<Self> {
        let mut it = it.into_iter();
        let first = it.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in it {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Some(Self::new(x0, y0, x1 - x0, y1 - y0))
    }
    #[must_use]
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    #[must_use]
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    #[must_use]
    pub fn center(&self) -> Pt {
        Pt::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    #[must_use]
    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }
    #[must_use]
    pub fn contains_rect(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }
    #[must_use]
    pub fn intersects(&self, o: &Rect) -> bool {
        !(o.x > self.right() || o.right() < self.x || o.y > self.bottom() || o.bottom() < self.y)
    }
    #[must_use]
    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }
    #[must_use]
    pub fn expand(&self, m: f64) -> Rect {
        Rect::new(self.x - m, self.y - m, self.w + 2.0 * m, self.h + 2.0 * m)
    }
    #[must_use]
    pub fn translate(&self, d: Pt) -> Rect {
        Rect::new(self.x + d.x, self.y + d.y, self.w, self.h)
    }
}

/// Box (x, y, width, height) with rotation around its center.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub angle: f64,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

impl Frame {
    #[must_use]
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h, angle: 0.0 }
    }
    #[must_use]
    pub fn from_rect(r: Rect) -> Self {
        Self::new(r.x, r.y, r.w, r.h)
    }
    #[must_use]
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
    #[must_use]
    pub fn center(&self) -> Pt {
        Pt::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    /// Position in the local frame: origin at the center, no rotation.
    #[must_use]
    pub fn to_local(&self, p: Pt) -> Pt {
        let c = self.center();
        let d = p - c;
        let (s, co) = (-self.angle).sin_cos();
        Pt::new(d.x * co - d.y * s, d.x * s + d.y * co)
    }
    /// Inverse de [`Frame::to_local`].
    #[must_use]
    pub fn to_world(&self, l: Pt) -> Pt {
        let c = self.center();
        let (s, co) = self.angle.sin_cos();
        Pt::new(c.x + l.x * co - l.y * s, c.y + l.x * s + l.y * co)
    }
    #[must_use]
    pub fn corners(&self) -> [Pt; 4] {
        let (hw, hh) = (self.w / 2.0, self.h / 2.0);
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].map(|(x, y)| self.to_world(Pt::new(x, y)))
    }
    /// Axis-aligned bounding box (takes the rotation into account).
    #[must_use]
    pub fn aabb(&self) -> Rect {
        if self.angle == 0.0 {
            return self.rect();
        }
        Rect::from_points(self.corners()).unwrap_or_default()
    }
}

/// Distance from a point `p` to the segment `[a, b]`.
#[must_use]
pub fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let frac = if l2 == 0.0 { 0.0 } else { ((p - a).dot(d) / l2).clamp(0.0, 1.0) };
    p.dist(a + d * frac)
}

/// Point-in-polygon test (even-odd rule).
#[must_use]
pub fn in_poly(poly: &[Pt], p: Pt) -> bool {
    let mut inside = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// First intersection (t > 0) of a ray with the edges of a closed polygon.
#[must_use]
pub fn ray_poly_hit(poly: &[Pt], origin: Pt, dir: Pt) -> Option<Pt> {
    let mut best: Option<f64> = None;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let edge = b - a;
        let den = dir.x * edge.y - dir.y * edge.x;
        if den.abs() < 1e-12 {
            continue;
        }
        let ao = a - origin;
        let t = (ao.x * edge.y - ao.y * edge.x) / den;
        let u = (ao.x * dir.y - ao.y * dir.x) / den;
        if t > 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&u) && best.is_none_or(|b| t < b) {
            best = Some(t);
        }
    }
    best.map(|t| origin + dir * t)
}

/// Indices of the points kept by the Ramer–Douglas–Peucker simplification.
#[must_use]
pub fn rdp_indices(pts: &[Pt], eps: f64) -> Vec<usize> {
    if pts.len() < 3 {
        return (0..pts.len()).collect();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (mut md, mut mi) = (0.0, 0usize);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = seg_dist(*p, pts[a], pts[b]);
            if d > md {
                md = d;
                mi = i;
            }
        }
        if md > eps && mi > 0 {
            keep[mi] = true;
            stack.push((a, mi));
            stack.push((mi, b));
        }
    }
    keep.iter().enumerate().filter_map(|(i, k)| k.then_some(i)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip_with_rotation() {
        let f = Frame { x: 10.0, y: 20.0, w: 100.0, h: 40.0, angle: 0.7 };
        let p = Pt::new(33.0, -4.0);
        let back = f.to_world(f.to_local(p));
        assert!(back.dist(p) < 1e-9);
    }

    #[test]
    fn aabb_of_rotated_square() {
        let f = Frame { x: 0.0, y: 0.0, w: 10.0, h: 10.0, angle: std::f64::consts::FRAC_PI_4 };
        let r = f.aabb();
        assert!((r.w - 10.0 * 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn segment_distance() {
        assert!((seg_dist(Pt::new(5.0, 3.0), Pt::ZERO, Pt::new(10.0, 0.0)) - 3.0).abs() < 1e-9);
        assert!((seg_dist(Pt::new(-4.0, 3.0), Pt::ZERO, Pt::new(10.0, 0.0)) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn polygon_and_ray() {
        let sq = [Pt::new(-1.0, -1.0), Pt::new(1.0, -1.0), Pt::new(1.0, 1.0), Pt::new(-1.0, 1.0)];
        assert!(in_poly(&sq, Pt::ZERO));
        assert!(!in_poly(&sq, Pt::new(2.0, 0.0)));
        let hit = ray_poly_hit(&sq, Pt::ZERO, Pt::new(1.0, 0.0)).unwrap();
        assert!(hit.dist(Pt::new(1.0, 0.0)) < 1e-9);
    }

    #[test]
    fn rdp_indices_match_points_and_keep_ends() {
        let pts = [Pt::new(0.0, 0.0), Pt::new(5.0, 0.01), Pt::new(10.0, 0.0), Pt::new(10.0, 10.0)];
        assert_eq!(rdp_indices(&pts, 0.5), vec![0, 2, 3]);
        assert_eq!(rdp_indices(&pts[..2], 0.5), vec![0, 1]);
        assert_eq!(rdp_indices(&[], 1.0), [] as [usize; 0]);
    }
}
