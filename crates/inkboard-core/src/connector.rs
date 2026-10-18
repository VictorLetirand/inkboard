//! Connectors (arrows and lines) bound to objects.
//!
//! A connector only stores its ends: free (a point) or bound to
//! an object (id + anchor). The path is **recomputed on demand** from
//! the page's current state: moving, resizing or rotating a shape
//! therefore makes its arrows follow with no update at all.

use crate::geom::{Frame, Pt, Rect, ray_poly_hit};
use crate::model::{Id, Kind, Label, Object, Outline, Page};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Anchor {
    /// Point of the outline facing the other end.
    #[default]
    Auto,
    /// Relative position in the target's box (0..1 in x and y).
    Frac { fx: f64, fy: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum End {
    Free {
        at: Pt,
    },
    Bound {
        target: Id,
        #[serde(default)]
        anchor: Anchor,
    },
}

impl End {
    pub fn free(at: Pt) -> End {
        End::Free { at }
    }
    pub fn bound(target: Id) -> End {
        End::Bound { target, anchor: Anchor::Auto }
    }
    pub fn target(&self) -> Option<&Id> {
        match self {
            End::Bound { target, .. } => Some(target),
            End::Free { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    #[default]
    Straight,
    /// Segments orthogonaux.
    Elbow,
    Curve,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrowhead {
    #[default]
    None,
    Arrow,
    Triangle,
    Dot,
    Bar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connector {
    pub start: End,
    pub end: End,
    /// Intermediate waypoints.
    #[serde(default)]
    pub via: Vec<Pt>,
    #[serde(default)]
    pub route: Route,
    #[serde(default)]
    pub head_start: Arrowhead,
    #[serde(default)]
    pub head_end: Arrowhead,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,
}

impl Connector {
    pub fn new(start: End, end: End) -> Self {
        Self { start, end, via: Vec::new(), route: Route::Straight, head_start: Arrowhead::None, head_end: Arrowhead::Arrow, label: None }
    }
    pub fn straight(a: Pt, b: Pt) -> Self {
        Self::new(End::free(a), End::free(b))
    }
    /// Moves the free ends and the waypoints.
    pub fn translate_free(&mut self, d: Pt) {
        self.map_free(|p| p + d);
    }
    pub fn map_free(&mut self, f: impl Fn(Pt) -> Pt) {
        for e in [&mut self.start, &mut self.end] {
            if let End::Free { at } = e {
                *at = f(*at);
            }
        }
        for v in &mut self.via {
            *v = f(*v);
        }
    }
    pub fn targets(&self) -> impl Iterator<Item = &Id> {
        self.start.target().into_iter().chain(self.end.target())
    }
}

/* ---------------------------------------------------------- Geometry */

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    N,
    E,
    S,
    W,
}

impl Side {
    fn dir(self) -> Pt {
        match self {
            Side::N => Pt::new(0.0, -1.0),
            Side::E => Pt::new(1.0, 0.0),
            Side::S => Pt::new(0.0, 1.0),
            Side::W => Pt::new(-1.0, 0.0),
        }
    }
    fn horizontal(self) -> bool {
        matches!(self, Side::E | Side::W)
    }
}

fn outline_of(o: &Object) -> Outline {
    match &o.kind {
        Kind::Box(b) => b.geom.outline(),
        _ => Outline::Rect,
    }
}

fn side_toward(f: &Frame, toward: Pt) -> Side {
    let l = f.to_local(toward);
    let (nx, ny) = (l.x.abs() / (f.w / 2.0).max(1e-9), l.y.abs() / (f.h / 2.0).max(1e-9));
    if nx > ny {
        if l.x > 0.0 { Side::E } else { Side::W }
    } else if l.y > 0.0 {
        Side::S
    } else {
        Side::N
    }
}

/// Point of the object's outline, in the direction of `toward`.
pub fn boundary_hit(outline: Outline, f: &Frame, toward: Pt) -> Pt {
    let mut dir = f.to_local(toward);
    if dir.len() < 1e-9 {
        dir = Pt::new(1.0, 0.0);
    }
    let (hw, hh) = ((f.w / 2.0).max(1e-9), (f.h / 2.0).max(1e-9));
    let hit = match outline {
        Outline::Ellipse => dir * (1.0 / (dir.x / hw).hypot(dir.y / hh)),
        o => ray_poly_hit(&o.polygon(f.w, f.h), Pt::ZERO, dir).unwrap_or(Pt::ZERO),
    };
    f.to_world(hit)
}

struct Attach {
    pt: Pt,
    side: Option<Side>,
}

fn simplify(pts: Vec<Pt>) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last().is_some_and(|l| l.dist(p) < 1e-6) {
            continue;
        }
        while out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let cross = (b - a).x * (p - b).y - (b - a).y * (p - b).x;
            if cross.abs() < 1e-6 {
                out.pop();
            } else {
                break;
            }
        }
        out.push(p);
    }
    out
}

/// Length of the perpendicular exit from a shape side.
const STUB: f64 = 24.0;
/// Minimum clearance between an elbow connector and the shapes it goes around.
const CLEARANCE: f64 = 14.0;
/// Above this many objects, elbow connectors no longer try to go around shapes (linear cost per connector).
const AVOID_MAX_OBJECTS: usize = 5000;

/// Orthogonal path between two points, leaving perpendicular to the sides.
fn orthogonal(a: Pt, sa: Option<Side>, b: Pt, sb: Option<Side>) -> Vec<Pt> {
    let a1 = sa.map_or(a, |s| a + s.dir() * STUB);
    let b1 = sb.map_or(b, |s| b + s.dir() * STUB);
    let ha = sa.map_or_else(|| (b.x - a.x).abs() >= (b.y - a.y).abs(), Side::horizontal);
    let hb = sb.map_or(ha, Side::horizontal);
    let mut v = vec![a, a1];
    if ha && hb {
        let mx = match (sa, sb) {
            (Some(Side::E), Some(Side::E)) => a1.x.max(b1.x),
            (Some(Side::W), Some(Side::W)) => a1.x.min(b1.x),
            _ => (a1.x + b1.x) / 2.0,
        };
        v.extend([Pt::new(mx, a1.y), Pt::new(mx, b1.y)]);
    } else if !ha && !hb {
        let my = match (sa, sb) {
            (Some(Side::S), Some(Side::S)) => a1.y.max(b1.y),
            (Some(Side::N), Some(Side::N)) => a1.y.min(b1.y),
            _ => (a1.y + b1.y) / 2.0,
        };
        v.extend([Pt::new(a1.x, my), Pt::new(b1.x, my)]);
    } else if ha {
        v.push(Pt::new(b1.x, a1.y));
    } else {
        v.push(Pt::new(a1.x, b1.y));
    }
    v.extend([b1, b]);
    simplify(v)
}

fn flatten_cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, n: usize) -> Vec<Pt> {
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
        })
        .collect()
}

fn catmull_rom(pts: &[Pt]) -> Vec<Pt> {
    let mut out = Vec::new();
    for i in 0..pts.len() - 1 {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = if i + 2 < pts.len() { pts[i + 2] } else { pts[i + 1] };
        let (c1, c2) = (p1 + (p2 - p0) * (1.0 / 6.0), p2 - (p3 - p1) * (1.0 / 6.0));
        let seg = flatten_cubic(p1, c1, c2, p2, 12);
        out.extend(if i == 0 { seg } else { seg[1..].to_vec() });
    }
    out
}

/// Point at half the path length, and the tangent angle at that point.
pub fn path_midpoint(path: &[Pt]) -> (Pt, f64) {
    let total: f64 = path.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in path.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= total / 2.0 && l > 0.0 {
            let t = (total / 2.0 - acc) / l;
            let d = w[1] - w[0];
            return (w[0].lerp(w[1], t), d.y.atan2(d.x));
        }
        acc += l;
    }
    (path.first().copied().unwrap_or_default(), 0.0)
}

/// Vertices of the arrowhead: `tip` is the end, `from` a point upstream.
pub fn head_points(tip: Pt, from: Pt, size: f64, head: Arrowhead) -> Vec<Pt> {
    let d = (tip - from).normalized();
    let n = Pt::new(-d.y, d.x);
    match head {
        Arrowhead::None => Vec::new(),
        Arrowhead::Arrow | Arrowhead::Triangle => {
            let base = tip - d * size;
            vec![base + n * (size * 0.5), tip, base - n * (size * 0.5)]
        }
        Arrowhead::Bar => vec![tip + n * (size * 0.5), tip - n * (size * 0.5)],
        Arrowhead::Dot => vec![tip - d * (size * 0.4)],
    }
}

impl Page {
    fn end_ref<'a>(get: &dyn Fn(&Id) -> Option<&'a Object>, e: &End) -> Option<(Pt, Option<&'a Object>)> {
        match e {
            End::Free { at } => Some((*at, None)),
            End::Bound { target, .. } => {
                let o = get(target)?;
                Some((o.frame()?.center(), Some(o)))
            }
        }
    }

    fn attach(e: &End, obj: Option<&Object>, toward: Pt, elbow: bool) -> Option<Attach> {
        match e {
            End::Free { at } => Some(Attach { pt: *at, side: None }),
            End::Bound { anchor, .. } => {
                let o = obj?;
                let f = o.frame()?;
                let outline = outline_of(o);
                Some(match anchor {
                    Anchor::Frac { fx, fy } => {
                        let l = Pt::new((fx - 0.5) * f.w, (fy - 0.5) * f.h);
                        let (nx, ny) = (l.x.abs() / (f.w / 2.0).max(1e-9), l.y.abs() / (f.h / 2.0).max(1e-9));
                        let side = if nx > ny {
                            if l.x > 0.0 { Side::E } else { Side::W }
                        } else if l.y > 0.0 {
                            Side::S
                        } else {
                            Side::N
                        };
                        Attach { pt: f.to_world(l), side: Some(side) }
                    }
                    Anchor::Auto => {
                        let side = side_toward(&f, toward);
                        let aim = if elbow { f.to_world(side.dir() * 1.0e6) } else { toward };
                        Attach { pt: boundary_hit(outline, &f, aim), side: Some(side) }
                    }
                })
            }
        }
    }

    /// Index of objects by id: build it once per frame to draw many connectors
    /// (direct lookup is linear).
    pub fn index(&self) -> HashMap<&Id, &Object> {
        self.objects.iter().map(|o| (&o.id, o)).collect()
    }

    /// If the elbow path crosses another shape, looks for an orthogonal path around it;
    /// otherwise (or when nothing is found) returns the original path.
    fn avoid_obstacles(&self, path: Vec<Pt>, a: &Attach, b: &Attach, c: &Connector) -> Vec<Pt> {
        if self.objects.len() > AVOID_MAX_OBJECTS || path.len() < 2 {
            return path;
        }
        let ends: Vec<&Id> = c.targets().collect();
        let mut others: Vec<Rect> = Vec::new();
        let mut own: Vec<Rect> = Vec::new();
        for o in &self.objects {
            if !self.is_visible(o) {
                continue;
            }
            let Some(f) = o.frame() else { continue };
            if ends.contains(&&o.id) { own.push(f.aabb()) } else { others.push(f.aabb()) }
        }
        let inner = if path.len() >= 4 { &path[1..path.len() - 1] } else { &path[..0] };
        if !crate::route::crosses(&path, &others) && !crate::route::crosses(inner, &own) {
            return path;
        }
        let a1 = a.side.map_or(a.pt, |s| a.pt + s.dir() * STUB);
        let b1 = b.side.map_or(b.pt, |s| b.pt + s.dir() * STUB);
        let all: Vec<Rect> = others.into_iter().chain(own).collect();
        let Some(mid) = crate::route::route_around(a1, b1, a.side.map(Side::horizontal), &all, CLEARANCE) else { return path };
        let mut out = vec![a.pt];
        out.extend(mid);
        out.push(b.pt);
        simplify(out)
    }

    /// Path (polyline) of the connector; `None` when a target has disappeared.
    pub fn connector_path(&self, c: &Connector) -> Option<Vec<Pt>> {
        self.connector_path_with(c, &|id| self.get(id))
    }

    /// Like [`Page::connector_path`], with a supplied object lookup (e.g. [`Page::index`]).
    pub fn connector_path_with<'a>(&self, c: &Connector, get: &dyn Fn(&Id) -> Option<&'a Object>) -> Option<Vec<Pt>> {
        let (sref, sobj) = Self::end_ref(get, &c.start)?;
        let (eref, eobj) = Self::end_ref(get, &c.end)?;
        let elbow = c.route == Route::Elbow;
        let s_toward = c.via.first().copied().unwrap_or(eref);
        let e_toward = c.via.last().copied().unwrap_or(sref);
        let a = Self::attach(&c.start, sobj, s_toward, elbow)?;
        let b = Self::attach(&c.end, eobj, e_toward, elbow)?;
        Some(match c.route {
            Route::Straight => {
                let mut v = vec![a.pt];
                v.extend(&c.via);
                v.push(b.pt);
                v
            }
            Route::Elbow => {
                if c.via.is_empty() {
                    self.avoid_obstacles(orthogonal(a.pt, a.side, b.pt, b.side), &a, &b, c)
                } else {
                    let mut pts = vec![a.pt];
                    pts.extend(&c.via);
                    pts.push(b.pt);
                    let mut out = vec![pts[0]];
                    for w in pts.windows(2) {
                        out.extend(orthogonal(w[0], None, w[1], None).into_iter().skip(1));
                    }
                    simplify(out)
                }
            }
            Route::Curve => {
                if c.via.is_empty() {
                    let k = a.pt.dist(b.pt) * 0.4;
                    let c1 = a.pt + a.side.map_or((b.pt - a.pt).normalized(), Side::dir) * k;
                    let c2 = b.pt + b.side.map_or((a.pt - b.pt).normalized(), Side::dir) * k;
                    flatten_cubic(a.pt, c1, c2, b.pt, 24)
                } else {
                    let mut pts = vec![a.pt];
                    pts.extend(&c.via);
                    pts.push(b.pt);
                    catmull_rom(&pts)
                }
            }
        })
    }

    /// Detaches (frees) the ends bound to objects of `gone`, at their current position.
    pub fn detach_ends_to(&self, c: &Connector, gone: &[Id]) -> Option<Connector> {
        let hits = |e: &End| e.target().is_some_and(|t| gone.contains(t));
        if !hits(&c.start) && !hits(&c.end) {
            return None;
        }
        let path = self.connector_path(c)?;
        let mut n = c.clone();
        if hits(&c.start) {
            n.start = End::free(path[0]);
        }
        if hits(&c.end) {
            n.end = End::free(*path.last()?);
        }
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Frame;
    use crate::model::{BoxGeom, Label};
    use crate::style::Style;

    fn bx(page: &mut Page, geom: BoxGeom, x: f64, y: f64, w: f64, h: f64) -> Id {
        let o = Object::new_box(geom, Frame::new(x, y, w, h), Style::default(), Label::default());
        let id = o.id.clone();
        page.objects.push(o);
        id
    }

    #[test]
    fn straight_connector_touches_boundaries() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 300.0, 0.0, 100.0, 50.0);
        let c = Connector::new(End::bound(a), End::bound(b));
        let path = p.connector_path(&c).unwrap();
        assert!(path[0].dist(Pt::new(100.0, 25.0)) < 1e-6);
        assert!(path[1].dist(Pt::new(300.0, 25.0)) < 1e-6);
    }

    #[test]
    fn connector_follows_moved_target() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Ellipse, 300.0, 0.0, 100.0, 50.0);
        let c = Connector::new(End::bound(a), End::bound(b.clone()));
        let before = p.connector_path(&c).unwrap();
        let i = p.index_of(&b).unwrap();
        p.objects[i].translate(Pt::new(0.0, 200.0));
        let after = p.connector_path(&c).unwrap();
        assert!(after.last().unwrap().y > before.last().unwrap().y + 20.0);
    }

    #[test]
    fn ellipse_boundary_is_on_ellipse() {
        let f = Frame::new(0.0, 0.0, 200.0, 100.0);
        let p = boundary_hit(Outline::Ellipse, &f, Pt::new(1000.0, 50.0));
        assert!(p.dist(Pt::new(200.0, 50.0)) < 1e-6);
        let q = boundary_hit(Outline::Ellipse, &f, Pt::new(100.0, -1000.0));
        assert!(q.dist(Pt::new(100.0, 0.0)) < 1e-6);
    }

    #[test]
    fn rotated_target_attaches_on_rotated_outline() {
        let mut f = Frame::new(0.0, 0.0, 100.0, 100.0);
        f.angle = std::f64::consts::FRAC_PI_4;
        let p = boundary_hit(Outline::Rect, &f, Pt::new(1000.0, 50.0));
        // the tip of the square rotated by 45° is at 50*sqrt(2) from the center
        assert!((p.dist(Pt::new(50.0, 50.0)) - 50.0 * 2f64.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn elbow_is_orthogonal() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 300.0, 200.0, 100.0, 50.0);
        let mut c = Connector::new(End::bound(a), End::bound(b));
        c.route = Route::Elbow;
        let path = p.connector_path(&c).unwrap();
        assert!(path.len() >= 3);
        for w in path.windows(2) {
            assert!((w[0].x - w[1].x).abs() < 1e-6 || (w[0].y - w[1].y).abs() < 1e-6, "oblique segment: {w:?}");
        }
    }

    #[test]
    fn missing_target_gives_none_and_detach_works() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let c = Connector::new(End::bound(a.clone()), End::free(Pt::new(400.0, 25.0)));
        let d = p.detach_ends_to(&c, std::slice::from_ref(&a)).unwrap();
        assert!(matches!(d.start, End::Free { .. }));
        p.objects.clear();
        assert!(p.connector_path(&c).is_none());
        assert!(p.connector_path(&d).is_some());
    }

    #[test]
    fn curve_has_many_points_and_midpoint() {
        let p = Page::new("t");
        let mut c = Connector::straight(Pt::new(0.0, 0.0), Pt::new(100.0, 100.0));
        c.route = Route::Curve;
        let path = p.connector_path(&c).unwrap();
        assert!(path.len() > 10);
        let (m, _) = path_midpoint(&path);
        assert!(m.dist(Pt::new(50.0, 50.0)) < 5.0);
    }

    #[test]
    fn elbow_goes_around_a_shape_in_the_way_and_stays_orthogonal() {
        use crate::route::crosses;
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 500.0, 0.0, 100.0, 50.0);
        let wall = bx(&mut p, BoxGeom::Rect, 250.0, -60.0, 100.0, 170.0);
        let mut c = Connector::new(End::bound(a), End::bound(b));
        c.route = Route::Elbow;
        let path = p.connector_path(&c).unwrap();
        let wall_rect = p.get(&wall).unwrap().frame().unwrap().aabb();
        assert!(!crosses(&path, &[wall_rect]), "the path goes around the obstacle: {path:?}");
        for w in path.windows(2) {
            assert!((w[0].x - w[1].x).abs() < 1e-6 || (w[0].y - w[1].y).abs() < 1e-6, "oblique segment: {w:?}");
        }
        assert!(path[0].dist(Pt::new(100.0, 25.0)) < 1e-6 && path.last().unwrap().dist(Pt::new(500.0, 25.0)) < 1e-6);
    }

    #[test]
    fn elbow_is_unchanged_when_nothing_is_in_the_way() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 300.0, 200.0, 100.0, 50.0);
        bx(&mut p, BoxGeom::Rect, 900.0, 900.0, 100.0, 50.0); // far from everything
        let mut c = Connector::new(End::bound(a.clone()), End::bound(b.clone()));
        c.route = Route::Elbow;
        let with_far_shape = p.connector_path(&c).unwrap();
        let mut alone = Page::new("t");
        alone.objects = p.objects[..2].to_vec();
        assert_eq!(alone.connector_path(&c).unwrap(), with_far_shape);
    }

    #[test]
    fn elbow_to_a_free_end_also_avoids_shapes() {
        use crate::route::crosses;
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let wall = bx(&mut p, BoxGeom::Rect, 200.0, -100.0, 60.0, 300.0);
        let mut c = Connector::new(End::bound(a), End::free(Pt::new(500.0, 25.0)));
        c.route = Route::Elbow;
        let path = p.connector_path(&c).unwrap();
        assert!(!crosses(&path, &[p.get(&wall).unwrap().frame().unwrap().aabb()]), "{path:?}");
    }
}
