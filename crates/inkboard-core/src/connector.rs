//! Connectors (arrows and lines) bound to objects.
//!
//! A connector only stores its ends: free (a point) or bound to
//! an object (id + anchor). The path is **recomputed on demand** from
//! the page's current state: moving, resizing or rotating a shape
//! therefore makes its arrows follow with no update at all.

use crate::geom::{Frame, Pt, Rect, ray_poly_hit};
use crate::model::{Id, Kind, Label, Object, Outline, Page};
use crate::num;
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

/// Side of the target's box where a bound end attaches: at the middle of that side, the route leaving (or arriving) perpendicular to it.
/// Without one, the side is chosen automatically from the position of the other end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachSide {
    Top,
    Bottom,
    Left,
    Right,
}

impl AttachSide {
    /// Name used by the MCP server and JSON Canvas (`top`, `bottom`, `left`, `right`).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            AttachSide::Top => "top",
            AttachSide::Bottom => "bottom",
            AttachSide::Left => "left",
            AttachSide::Right => "right",
        }
    }
    #[must_use]
    pub fn from_name(name: &str) -> Option<AttachSide> {
        [AttachSide::Top, AttachSide::Bottom, AttachSide::Left, AttachSide::Right].into_iter().find(|s| s.name() == name)
    }
    fn side(self) -> Side {
        match self {
            AttachSide::Top => Side::N,
            AttachSide::Bottom => Side::S,
            AttachSide::Left => Side::W,
            AttachSide::Right => Side::E,
        }
    }
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
        /// Side of the target's box the end is attached to; absent: automatic (see [`AttachSide`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side: Option<AttachSide>,
    },
}

impl End {
    #[must_use]
    pub fn free(at: Pt) -> End {
        End::Free { at }
    }
    #[must_use]
    pub fn bound(target: Id) -> End {
        End::Bound { target, anchor: Anchor::Auto, side: None }
    }
    #[must_use]
    pub fn target(&self) -> Option<&Id> {
        match self {
            End::Bound { target, .. } => Some(target),
            End::Free { .. } => None,
        }
    }
    /// Side the end is attached to (`None`: automatic, or a free end).
    #[must_use]
    pub fn side(&self) -> Option<AttachSide> {
        match self {
            End::Bound { side, .. } => *side,
            End::Free { .. } => None,
        }
    }
    /// Sets the side of a bound end (`None` returns it to automatic); a free end is left alone.
    pub fn set_side(&mut self, new: Option<AttachSide>) {
        if let End::Bound { side, .. } = self {
            *side = new;
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

/// Arrowheads added after the first version of the file format. They are stored in separate, optional fields of the connector
/// (`head_start_ext`, `head_end_ext`) next to the original `head_start` / `head_end`, which keep the closest original head ([`HeadExt::fallback`]):
/// a version that does not know these fields ignores them and draws that fallback, so nothing is lost or refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadExt {
    /// Filled diamond (UML composition).
    Diamond,
    /// Hollow diamond (UML aggregation).
    DiamondOpen,
    /// Hollow triangle (UML inheritance, realization).
    TriangleOpen,
    /// Crow's foot: many.
    CrowFoot,
    /// Crow's foot and a bar: one or many.
    CrowFootOne,
    /// Crow's foot and a circle: zero or many.
    CrowFootZero,
    /// Two bars: exactly one.
    OneOnly,
    /// A circle and a bar: zero or one.
    ZeroOrOne,
}

impl HeadExt {
    /// Every extended head, in the order the interface lists them.
    pub const ALL: [HeadExt; 8] = [
        HeadExt::Diamond,
        HeadExt::DiamondOpen,
        HeadExt::TriangleOpen,
        HeadExt::CrowFoot,
        HeadExt::CrowFootOne,
        HeadExt::CrowFootZero,
        HeadExt::OneOnly,
        HeadExt::ZeroOrOne,
    ];

    /// The original arrowhead that stands for this one in a version that does not know it.
    #[must_use]
    pub fn fallback(self) -> Arrowhead {
        match self {
            HeadExt::Diamond | HeadExt::DiamondOpen | HeadExt::ZeroOrOne => Arrowhead::Dot,
            HeadExt::TriangleOpen => Arrowhead::Triangle,
            HeadExt::CrowFoot | HeadExt::CrowFootOne | HeadExt::CrowFootZero => Arrowhead::Arrow,
            HeadExt::OneOnly => Arrowhead::Bar,
        }
    }

    /// Name used by the MCP server and in text (`diamond_open`, `crow_foot`…).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            HeadExt::Diamond => "diamond",
            HeadExt::DiamondOpen => "diamond_open",
            HeadExt::TriangleOpen => "triangle_open",
            HeadExt::CrowFoot => "crow_foot",
            HeadExt::CrowFootOne => "crow_foot_one",
            HeadExt::CrowFootZero => "crow_foot_zero",
            HeadExt::OneOnly => "one_only",
            HeadExt::ZeroOrOne => "zero_or_one",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<HeadExt> {
        HeadExt::ALL.into_iter().find(|h| h.name() == name)
    }
}

/// One piece of an arrowhead, in page coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum HeadPart {
    /// Open path drawn with the stroke of the connector (round ends).
    Line(Vec<Pt>),
    /// Closed polygon; `solid`: filled with the color of the connector, otherwise with the background (a hollow head hides the line behind it).
    Polygon { pts: Vec<Pt>, solid: bool },
    /// Circle; same meaning of `solid`.
    Circle { center: Pt, radius: f64, solid: bool },
}

/// The pieces of the arrowhead at `tip` of a path whose previous point is `from`, for an original head, or for the extended one when there is one.
#[must_use]
pub fn head_glyph(tip: Pt, from: Pt, size: f64, head: Arrowhead, ext: Option<HeadExt>) -> Vec<HeadPart> {
    let d = (tip - from).normalized();
    let n = Pt::new(-d.y, d.x);
    let at = |along: f64, across: f64| tip - d * (size * along) + n * (size * across);
    let bar = |along: f64| HeadPart::Line(vec![at(along, 0.5), at(along, -0.5)]);
    let crow = |extra: Vec<HeadPart>| {
        let mut parts = vec![HeadPart::Line(vec![at(0.0, 0.5), at(1.0, 0.0), at(0.0, -0.5)]), HeadPart::Line(vec![at(1.0, 0.0), at(0.0, 0.0)])];
        parts.extend(extra);
        parts
    };
    let diamond = |solid: bool| vec![HeadPart::Polygon { pts: vec![at(0.0, 0.0), at(0.5, 0.35), at(1.0, 0.0), at(0.5, -0.35)], solid }];
    match ext {
        Some(HeadExt::Diamond) => diamond(true),
        Some(HeadExt::DiamondOpen) => diamond(false),
        Some(HeadExt::TriangleOpen) => vec![HeadPart::Polygon { pts: vec![at(1.0, 0.5), at(0.0, 0.0), at(1.0, -0.5)], solid: false }],
        Some(HeadExt::CrowFoot) => crow(Vec::new()),
        Some(HeadExt::CrowFootOne) => crow(vec![bar(1.3)]),
        Some(HeadExt::CrowFootZero) => crow(vec![HeadPart::Circle { center: at(1.55, 0.0), radius: size * 0.28, solid: false }]),
        Some(HeadExt::OneOnly) => vec![bar(0.25), bar(0.6)],
        Some(HeadExt::ZeroOrOne) => vec![bar(0.3), HeadPart::Circle { center: at(0.95, 0.0), radius: size * 0.28, solid: false }],
        None => {
            let pts = head_points(tip, from, size, head);
            match head {
                Arrowhead::None => Vec::new(),
                Arrowhead::Arrow | Arrowhead::Bar => vec![HeadPart::Line(pts)],
                Arrowhead::Triangle => vec![HeadPart::Polygon { pts, solid: true }],
                Arrowhead::Dot => vec![HeadPart::Circle { center: pts[0], radius: size * 0.4, solid: true }],
            }
        }
    }
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
    /// Head at the start that was added after the first format (see [`HeadExt`]); `head_start` then holds its fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_start_ext: Option<HeadExt>,
    /// Same at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_end_ext: Option<HeadExt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,
    /// Size factor of both heads (1.0 = the usual size), kept within [`Connector::HEAD_SCALE_RANGE`] by [`Connector::set_head_scale`].
    #[serde(default = "default_head_scale", skip_serializing_if = "is_default_head_scale")]
    pub head_scale: f32,
    /// Position of the label along the routed path, 0 (start) to 1 (end), kept within that range by [`Connector::set_label_pos`]; absent: the middle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_pos: Option<f64>,
    /// Distance (units) from the path to the label, measured perpendicular to it; positive: to the right of the direction of travel
    /// (below a path going right), negative: to the left. Kept within [`Connector::LABEL_OFFSET_MAX`] by [`Connector::set_label_offset`].
    #[serde(default, skip_serializing_if = "is_zero_offset")]
    pub label_offset: f64,
}

// Exact comparison on purpose: only the untouched default is left out of the file.
#[allow(clippy::float_cmp, clippy::trivially_copy_pass_by_ref, reason = "the signature serde asks of `skip_serializing_if`")]
fn is_zero_offset(v: &f64) -> bool {
    *v == 0.0
}

fn default_head_scale() -> f32 {
    1.0
}

/// Usual size of an arrowhead for a stroke of the given width (connectors and freehand strokes).
#[must_use]
pub fn base_head_size(width: f64) -> f64 {
    (width * 4.5).max(12.0)
}

// Exact comparison on purpose: only the untouched default is left out of the file.
#[allow(clippy::float_cmp, clippy::trivially_copy_pass_by_ref, reason = "the signature serde asks of `skip_serializing_if`")]
fn is_default_head_scale(s: &f32) -> bool {
    *s == 1.0
}

impl Connector {
    /// Allowed range of the head size factor.
    pub const HEAD_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=4.0;
    /// Sets the head size factor; a value out of range is brought back into it, a non-finite one is ignored.
    pub fn set_head_scale(&mut self, scale: f32) {
        if scale.is_finite() {
            self.head_scale = scale.clamp(*Self::HEAD_SCALE_RANGE.start(), *Self::HEAD_SCALE_RANGE.end());
        }
    }
    /// Largest distance (units) between the path and its label.
    pub const LABEL_OFFSET_MAX: f64 = 1000.0;
    /// Sets the position of the label along the path (`None`: the middle); brought into 0..1, a non-finite value is ignored.
    pub fn set_label_pos(&mut self, pos: Option<f64>) {
        match pos {
            None => self.label_pos = None,
            Some(p) if p.is_finite() => self.label_pos = Some(p.clamp(0.0, 1.0)),
            Some(_) => {}
        }
    }
    /// Sets the distance between the path and its label; brought into [`Connector::LABEL_OFFSET_MAX`], a non-finite value is ignored.
    pub fn set_label_offset(&mut self, offset: f64) {
        if offset.is_finite() {
            self.label_offset = offset.clamp(-Self::LABEL_OFFSET_MAX, Self::LABEL_OFFSET_MAX);
        }
    }
    /// Where the center of the label goes on the routed `path`: at `label_pos` along it (the middle by default), moved by `label_offset`.
    #[must_use]
    pub fn label_point(&self, path: &[Pt]) -> Pt {
        let pos = self.label_pos.filter(|v| v.is_finite()).map_or(0.5, |v| v.clamp(0.0, 1.0));
        let (p, angle) = path_point_at(path, pos);
        let off = if self.label_offset.is_finite() { self.label_offset.clamp(-Self::LABEL_OFFSET_MAX, Self::LABEL_OFFSET_MAX) } else { 0.0 };
        if off == 0.0 { p } else { p + Pt::new(-angle.sin(), angle.cos()) * off }
    }
    /// Whether the heads have a size other than the usual one.
    #[must_use]
    pub fn has_custom_head_scale(&self) -> bool {
        !is_default_head_scale(&self.head_scale)
    }
    /// Size of the heads for a stroke of the given width.
    #[must_use]
    pub fn head_size(&self, width: f64) -> f64 {
        let scale = if self.head_scale.is_finite() { self.head_scale } else { 1.0 };
        base_head_size(width) * f64::from(scale.clamp(*Self::HEAD_SCALE_RANGE.start(), *Self::HEAD_SCALE_RANGE.end()))
    }
    /// Size of the heads as drawn on the given routed path: `head_size`, but at most 40% of the path length (so that two heads never overlap
    /// nor overrun the other end of a short connector), with a floor of 4 so that a head stays visible. A usual connector is unchanged.
    #[must_use]
    pub fn head_size_on(&self, width: f64, path: &[Pt]) -> f64 {
        let len: f64 = path.windows(2).map(|w| w[0].dist(w[1])).sum();
        let size = self.head_size(width);
        size.min((len * 0.4).max(4.0))
    }
    #[must_use]
    pub fn new(start: End, end: End) -> Self {
        Self {
            start,
            end,
            via: Vec::new(),
            route: Route::Straight,
            head_start: Arrowhead::None,
            head_end: Arrowhead::Arrow,
            head_start_ext: None,
            head_end_ext: None,
            label: None,
            head_scale: 1.0,
            label_pos: None,
            label_offset: 0.0,
        }
    }
    /// Sets an original head at the start (an extended head set before is dropped).
    pub fn set_head_start(&mut self, head: Arrowhead) {
        (self.head_start, self.head_start_ext) = (head, None);
    }
    /// Sets an original head at the end (an extended head set before is dropped).
    pub fn set_head_end(&mut self, head: Arrowhead) {
        (self.head_end, self.head_end_ext) = (head, None);
    }
    /// Sets an extended head at the start; the original field takes its fallback, so that older versions draw something close.
    pub fn set_ext_start(&mut self, ext: HeadExt) {
        (self.head_start, self.head_start_ext) = (ext.fallback(), Some(ext));
    }
    /// Sets an extended head at the end.
    pub fn set_ext_end(&mut self, ext: HeadExt) {
        (self.head_end, self.head_end_ext) = (ext.fallback(), Some(ext));
    }
    /// Whether there is a head at the start (original or extended).
    #[must_use]
    pub fn has_head_start(&self) -> bool {
        self.head_start != Arrowhead::None || self.head_start_ext.is_some()
    }
    /// Whether there is a head at the end.
    #[must_use]
    pub fn has_head_end(&self) -> bool {
        self.head_end != Arrowhead::None || self.head_end_ext.is_some()
    }
    #[must_use]
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
#[must_use]
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
/// How far around the straight elbow path (px) the shapes taken into account when routing around obstacles may be.
const ROUTE_REGION: f64 = 400.0;
/// Above this many objects, elbow connectors no longer try to go around shapes (linear cost per connector).
const AVOID_MAX_OBJECTS: usize = 5000;
/// Distance (units) between the parallel trunks of elbow connectors that would otherwise run along the same line.
const CHANNEL_GAP: f64 = 10.0;
/// Trunks closer than this (units) across their direction are on the same line, as `check_layout` sees it, and compete for it.
const TRUNK_SAME_LINE: f64 = 3.0;
/// Trunks overlapping by less than this (units) along their direction only touch.
const TRUNK_MIN_OVERLAP: f64 = 1.0;
/// Room (units) a trunk needs between its two stubs to be moved.
const TRUNK_MIN_ROOM: f64 = 4.0;
/// Above this many elbow connectors on a page, their trunks are no longer separated (quadratic cost).
const SEPARATE_MAX_CONNECTORS: usize = 150;

/// Orthogonal path between two points, leaving perpendicular to the sides.
/// Points past the stubs, and whether the first and last segments are horizontal.
fn elbow_axes(a: Pt, sa: Option<Side>, b: Pt, sb: Option<Side>) -> (Pt, Pt, bool, bool) {
    let a1 = sa.map_or(a, |s| a + s.dir() * STUB);
    let b1 = sb.map_or(b, |s| b + s.dir() * STUB);
    let ha = sa.map_or_else(|| (b.x - a.x).abs() >= (b.y - a.y).abs(), Side::horizontal);
    let hb = sb.map_or(ha, Side::horizontal);
    (a1, b1, ha, hb)
}

/// `shift` moves the middle segment of a three-segment path (see [`Trunk`]).
fn orthogonal(a: Pt, sa: Option<Side>, b: Pt, sb: Option<Side>, shift: f64) -> Vec<Pt> {
    let (a1, b1, ha, hb) = elbow_axes(a, sa, b, sb);
    let mut v = vec![a, a1];
    if ha && hb {
        let mx = match (sa, sb) {
            (Some(Side::E), Some(Side::E)) => a1.x.max(b1.x),
            (Some(Side::W), Some(Side::W)) => a1.x.min(b1.x),
            _ => f64::midpoint(a1.x, b1.x) + shift,
        };
        v.extend([Pt::new(mx, a1.y), Pt::new(mx, b1.y)]);
    } else if !ha && !hb {
        let my = match (sa, sb) {
            (Some(Side::S), Some(Side::S)) => a1.y.max(b1.y),
            (Some(Side::N), Some(Side::N)) => a1.y.min(b1.y),
            _ => f64::midpoint(a1.y, b1.y) + shift,
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
            let t = num::f64_of(i) / num::f64_of(n);
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
#[must_use]
pub fn path_midpoint(path: &[Pt]) -> (Pt, f64) {
    path_point_at(path, 0.5)
}

/// Point at the fraction `t` (0..1) of the path length, and the tangent angle at that point.
#[must_use]
pub fn path_point_at(path: &[Pt], t: f64) -> (Pt, f64) {
    let total: f64 = path.windows(2).map(|w| w[0].dist(w[1])).sum();
    let target = total * t;
    let mut acc = 0.0;
    for w in path.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= target && l > 0.0 {
            let t = (target - acc) / l;
            let d = w[1] - w[0];
            return (w[0].lerp(w[1], t), d.y.atan2(d.x));
        }
        acc += l;
    }
    (path.first().copied().unwrap_or_default(), 0.0)
}

/// Vertices of the arrowhead: `tip` is the end, `from` a point upstream.
#[must_use]
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

/// Middle segment ("trunk") of a three-segment elbow path, before any separation: elbow connectors whose trunks run along the same line
/// and overlap are spread over parallel channels (see [`Page::trunk_shift`]).
#[derive(Clone, Copy, Debug)]
struct Trunk {
    /// The trunk is vertical (the path leaves and arrives horizontally).
    vertical: bool,
    /// Position of the trunk across its direction (x for a vertical one).
    at: f64,
    /// Coordinates of the trunk along its direction, where it starts and ends.
    from: f64,
    to: f64,
    /// Room between the two stubs across the trunk: the channels must stay inside.
    room: f64,
    /// Order key: connectors with a lower key take the channel with a lower position (the nesting that does not make them cross).
    key: f64,
}

impl Trunk {
    fn of(a: &Attach, b: &Attach) -> Option<Trunk> {
        if a.side.is_some() && a.side == b.side {
            return None;
        }
        let (a1, b1, ha, hb) = elbow_axes(a.pt, a.side, b.pt, b.side);
        if ha != hb {
            return None;
        }
        let (across_a, across_b, from, to) = if ha { (a1.x, b1.x, a1.y, b1.y) } else { (a1.y, b1.y, a1.x, b1.x) };
        let (room, length) = ((across_b - across_a).abs(), (to - from).abs());
        if room < TRUNK_MIN_ROOM || length < TRUNK_MIN_OVERLAP {
            return None;
        }
        let key = -(to - from).signum() * (across_b - across_a).signum() * (from + to);
        Some(Trunk { vertical: ha, at: f64::midpoint(across_a, across_b), from, to, room, key })
    }
    fn span(&self) -> (f64, f64) {
        (self.from.min(self.to), self.from.max(self.to))
    }
    fn overlaps(&self, other: &Trunk) -> bool {
        let ((a0, a1), (b0, b1)) = (self.span(), other.span());
        self.vertical == other.vertical && (self.at - other.at).abs() <= TRUNK_SAME_LINE && a1.min(b1) - a0.max(b0) > TRUNK_MIN_OVERLAP
    }
    fn order(&self, other: &Trunk) -> std::cmp::Ordering {
        self.key.total_cmp(&other.key).then(self.from.total_cmp(&other.from)).then(self.to.total_cmp(&other.to))
    }
}

impl Page {
    /// Offset to give the trunk of the elbow connector `c` so that it does not run along the trunk of another elbow connector: the trunks
    /// that share a line and overlap (typically one source fanning out to a column of targets, or two columns linked by crossing arrows)
    /// are spread over channels [`CHANNEL_GAP`] apart, centered on the common line and ordered so that the arrows nest instead of crossing.
    /// 0 when `c` shares its trunk with no one (every connector of a page without such overlaps is routed exactly as before).
    /// Only connectors without waypoints count, and none of this is done on a page with more than [`SEPARATE_MAX_CONNECTORS`] of them
    /// (the cost grows with the square of their number) or [`AVOID_MAX_OBJECTS`] objects.
    fn trunk_shift<'a>(&self, c: &Connector, mine: Trunk, get: &dyn Fn(&Id) -> Option<&'a Object>) -> f64 {
        if self.objects.len() > AVOID_MAX_OBJECTS {
            return 0.0;
        }
        let mut rest: Vec<Trunk> = Vec::new();
        let mut count = 0;
        for o in &self.objects {
            let Kind::Connector(k) = &o.kind else { continue };
            if k.route != Route::Elbow || !k.via.is_empty() || !self.is_visible(o) {
                continue;
            }
            count += 1;
            if count > SEPARATE_MAX_CONNECTORS {
                return 0.0;
            }
            // a connector identical to `c` (a duplicate, or `c` itself) is the same route, not a competitor
            if k.start == c.start && k.end == c.end {
                continue;
            }
            if let Some((a, b)) = Self::attach_pair(k, get)
                && let Some(t) = Trunk::of(&a, &b)
                && t.vertical == mine.vertical
                && (t.at - mine.at).abs() <= TRUNK_SAME_LINE
            {
                rest.push(t);
            }
        }
        let mut group = vec![mine];
        loop {
            let (hit, miss): (Vec<Trunk>, Vec<Trunk>) = rest.into_iter().partition(|t| group.iter().any(|g| g.overlaps(t)));
            rest = miss;
            if hit.is_empty() {
                break;
            }
            group.extend(hit);
        }
        if group.len() < 2 {
            return 0.0;
        }
        let n = num::f64_of(group.len());
        let center = group.iter().map(|t| t.at).sum::<f64>() / n;
        let gap = group.iter().map(|t| t.room / (n + 1.0)).fold(CHANNEL_GAP, f64::min);
        let rank = num::f64_of(group.iter().filter(|t| t.order(&mine) == std::cmp::Ordering::Less).count());
        center + (rank - (n - 1.0) / 2.0) * gap - mine.at
    }

    /// Both attachments of a connector, as the path computation does.
    fn attach_pair<'a>(c: &Connector, get: &dyn Fn(&Id) -> Option<&'a Object>) -> Option<(Attach, Attach)> {
        let (sref, sobj) = Self::end_ref(get, &c.start)?;
        let (eref, eobj) = Self::end_ref(get, &c.end)?;
        let elbow = c.route == Route::Elbow;
        let s_toward = c.via.first().copied().unwrap_or(eref);
        let e_toward = c.via.last().copied().unwrap_or(sref);
        Some((Self::attach(&c.start, sobj, s_toward, elbow)?, Self::attach(&c.end, eobj, e_toward, elbow)?))
    }

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
            End::Bound { anchor, side, .. } => {
                let o = obj?;
                let f = o.frame()?;
                let outline = outline_of(o);
                // an explicit side wins over the anchor: the middle of that side, whatever the route
                if let Some(s) = side.map(AttachSide::side) {
                    return Some(Attach { pt: boundary_hit(outline, &f, f.to_world(s.dir() * 1.0e6)), side: Some(s) });
                }
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
                        // an elbow, or an image (an icon: its box is not its shape), leaves from the middle of the side facing the other end
                        let aim = if elbow || matches!(o.kind, Kind::Image(_)) { f.to_world(side.dir() * 1.0e6) } else { toward };
                        Attach { pt: boundary_hit(outline, &f, aim), side: Some(side) }
                    }
                })
            }
        }
    }

    /// Index of objects by id: build it once per frame to draw many connectors
    /// (direct lookup is linear).
    #[must_use]
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
        // An end shape whose margin-grown box contains a stub tip (triangle, rotated shape) would make
        // the search start inside an obstacle and give up: it is left out, the others are still avoided.
        let holds = |r: &Rect, p: Pt| r.expand(CLEARANCE).contains(p);
        own.retain(|r| !holds(r, a1) && !holds(r, b1));
        // Only the shapes near the path matter to the search, whose cost grows with the square of their number: on a board with a
        // thousand shapes, routing around all of them took seconds for each elbow connector, at every modification of the board.
        let near = Rect::from_points(path.iter().copied()).map(|r| r.expand(ROUTE_REGION));
        let all: Vec<Rect> = others.into_iter().filter(|r| near.as_ref().is_none_or(|n| n.intersects(r))).chain(own).collect();
        let Some(mid) = crate::route::route_around(a1, b1, a.side.map(Side::horizontal), &all, CLEARANCE) else { return path };
        let mut out = vec![a.pt];
        out.extend(mid);
        out.push(b.pt);
        simplify(out)
    }

    /// Path (polyline) of the connector; `None` when a target has disappeared.
    #[must_use]
    pub fn connector_path(&self, c: &Connector) -> Option<Vec<Pt>> {
        self.connector_path_with(c, &|id| self.get(id))
    }

    /// Like [`Page::connector_path`], with a supplied object lookup (e.g. [`Page::index`]).
    pub fn connector_path_with<'a>(&self, c: &Connector, get: &dyn Fn(&Id) -> Option<&'a Object>) -> Option<Vec<Pt>> {
        let (a, b) = Self::attach_pair(c, get)?;
        Some(match c.route {
            Route::Straight => {
                let mut v = vec![a.pt];
                v.extend(&c.via);
                v.push(b.pt);
                v
            }
            Route::Elbow => {
                if c.via.is_empty() {
                    let shift = Trunk::of(&a, &b).map_or(0.0, |t| self.trunk_shift(c, t, get));
                    self.avoid_obstacles(orthogonal(a.pt, a.side, b.pt, b.side, shift), &a, &b, c)
                } else {
                    let mut pts = vec![a.pt];
                    pts.extend(&c.via);
                    pts.push(b.pt);
                    let mut out = vec![pts[0]];
                    for w in pts.windows(2) {
                        out.extend(orthogonal(w[0], None, w[1], None, 0.0).into_iter().skip(1));
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
    #[must_use]
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
        let obj = Object::new_box(geom, Frame::new(x, y, w, h), Style::default(), Label::default());
        let id = obj.id.clone();
        page.objects.push(obj);
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
        let mut page = Page::new("t");
        let a = bx(&mut page, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut page, BoxGeom::Ellipse, 300.0, 0.0, 100.0, 50.0);
        let c = Connector::new(End::bound(a), End::bound(b.clone()));
        let before = page.connector_path(&c).unwrap();
        let i = page.index_of(&b).unwrap();
        page.objects[i].translate(Pt::new(0.0, 200.0));
        let after = page.connector_path(&c).unwrap();
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
    fn elbow_around_a_shape_ignores_the_thousand_shapes_far_away() {
        use crate::route::crosses;
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 500.0, 0.0, 100.0, 50.0);
        let wall = bx(&mut p, BoxGeom::Rect, 250.0, -60.0, 100.0, 170.0);
        let mut c = Connector::new(End::bound(a), End::bound(b));
        c.route = Route::Elbow;
        let alone = p.connector_path(&c).unwrap();
        for i in 0..1000 {
            bx(&mut p, BoxGeom::Rect, 3000.0 + f64::from(i % 40) * 120.0, 3000.0 + f64::from(i / 40) * 90.0, 100.0, 60.0);
        }
        let crowded = p.connector_path(&c).unwrap();
        assert_eq!(crowded, alone, "same path with or without the shapes that are far away");
        assert!(!crosses(&crowded, &[p.get(&wall).unwrap().frame().unwrap().aabb()]));
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

    fn icon(page: &mut Page, x: f64, y: f64, w: f64, h: f64) -> Id {
        let obj = Object::new(
            Kind::Image(crate::model::ImageObj { frame: Frame::new(x, y, w, h), asset: Id::from("a"), latex: None, mono: false, recolor: Vec::new() }),
            Style::default(),
        );
        let id = obj.id.clone();
        page.objects.push(obj);
        id
    }

    fn sided(a: &Id, sa: Option<AttachSide>, b: &Id, sb: Option<AttachSide>, route: Route) -> Connector {
        let mut c = Connector::new(End::bound(a.clone()), End::bound(b.clone()));
        c.start.set_side(sa);
        c.end.set_side(sb);
        c.route = route;
        c
    }

    #[test]
    fn a_side_attaches_at_the_middle_of_that_side_whatever_the_other_end() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 300.0, 200.0, 100.0, 50.0);
        let route = |sa, sb, r| p.connector_path(&sided(&a, sa, &b, sb, r)).unwrap();
        let path = route(Some(AttachSide::Bottom), Some(AttachSide::Top), Route::Straight);
        assert!(path[0].dist(Pt::new(50.0, 50.0)) < 1e-6 && path[1].dist(Pt::new(350.0, 200.0)) < 1e-6, "{path:?}");
        let path = route(Some(AttachSide::Left), Some(AttachSide::Right), Route::Straight);
        assert!(path[0].dist(Pt::new(0.0, 25.0)) < 1e-6 && path[1].dist(Pt::new(400.0, 225.0)) < 1e-6, "{path:?}");
        // one end only: the other stays automatic
        let path = route(Some(AttachSide::Top), None, Route::Straight);
        assert!(path[0].dist(Pt::new(50.0, 0.0)) < 1e-6);
        // without a side the end is where it always was
        let auto = route(None, None, Route::Straight);
        assert_eq!(auto, p.connector_path(&Connector::new(End::bound(a.clone()), End::bound(b.clone()))).unwrap());
    }

    #[test]
    fn a_side_wins_over_a_fractional_anchor_and_follows_a_rotated_target() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 300.0, 0.0, 100.0, 50.0);
        let mut c = sided(&a, Some(AttachSide::Top), &b, None, Route::Straight);
        c.start = End::Bound { target: a.clone(), anchor: Anchor::Frac { fx: 1.0, fy: 1.0 }, side: Some(AttachSide::Top) };
        assert!(p.connector_path(&c).unwrap()[0].dist(Pt::new(50.0, 0.0)) < 1e-6);
        let i = p.index_of(&a).unwrap();
        if let Kind::Box(bo) = &mut p.objects[i].kind {
            bo.frame.angle = std::f64::consts::FRAC_PI_2;
        }
        // rotated by 90 degrees about its center (50, 25): the middle of the local top side is now on the right
        let start = p.connector_path(&c).unwrap()[0];
        assert!(start.dist(Pt::new(75.0, 25.0)) < 1e-6, "{start:?}");
    }

    #[test]
    fn an_elbow_leaves_and_arrives_perpendicular_to_the_sides() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 300.0, 200.0, 100.0, 50.0);
        // bottom of a to the left of b: down from a, then across, arriving horizontally
        let path = p.connector_path(&sided(&a, Some(AttachSide::Bottom), &b, Some(AttachSide::Left), Route::Elbow)).unwrap();
        assert!(path[0].dist(Pt::new(50.0, 50.0)) < 1e-6 && path.last().unwrap().dist(Pt::new(300.0, 225.0)) < 1e-6, "{path:?}");
        assert!((path[0].x - path[1].x).abs() < 1e-6 && path[1].y > path[0].y, "first segment goes down: {path:?}");
        let n = path.len();
        assert!((path[n - 1].y - path[n - 2].y).abs() < 1e-6 && path[n - 2].x < path[n - 1].x, "last segment goes right: {path:?}");
        for w in path.windows(2) {
            assert!((w[0].x - w[1].x).abs() < 1e-6 || (w[0].y - w[1].y).abs() < 1e-6, "oblique segment: {w:?}");
        }
        // top of a, whose target is below it: the path first goes up, away from b
        let path = p.connector_path(&sided(&a, Some(AttachSide::Top), &b, None, Route::Elbow)).unwrap();
        assert!(path[1].y < path[0].y, "{path:?}");
    }

    #[test]
    fn a_curve_leaves_along_the_normal_of_the_side() {
        let mut p = Page::new("t");
        let a = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 50.0);
        let b = bx(&mut p, BoxGeom::Rect, 300.0, 0.0, 100.0, 50.0);
        let path = p.connector_path(&sided(&a, Some(AttachSide::Top), &b, Some(AttachSide::Top), Route::Curve)).unwrap();
        assert!(path[0].dist(Pt::new(50.0, 0.0)) < 1e-6 && path.last().unwrap().dist(Pt::new(350.0, 0.0)) < 1e-6);
        let first = path[1] - path[0];
        assert!(first.y < 0.0 && first.x.abs() < first.y.abs() * 0.2, "tangent is vertical, upward: {first:?}");
    }

    #[test]
    fn an_icon_attaches_at_the_middle_of_the_side_facing_the_other_end() {
        let mut p = Page::new("t");
        let a = icon(&mut p, 0.0, 0.0, 64.0, 64.0);
        let b = icon(&mut p, 300.0, 120.0, 64.0, 64.0);
        let path = p.connector_path(&Connector::new(End::bound(a.clone()), End::bound(b.clone()))).unwrap();
        // the other end is mostly to the right: right side of a, left side of b, not a point of the box on the line between the centers
        assert!(path[0].dist(Pt::new(64.0, 32.0)) < 1e-6 && path[1].dist(Pt::new(300.0, 152.0)) < 1e-6, "{path:?}");
        let c = icon(&mut p, 0.0, 300.0, 64.0, 64.0);
        let path = p.connector_path(&Connector::new(End::bound(a), End::bound(c))).unwrap();
        assert!(path[0].dist(Pt::new(32.0, 64.0)) < 1e-6 && path[1].dist(Pt::new(32.0, 300.0)) < 1e-6, "{path:?}");
        // a shape keeps the point of its outline facing the other end
        let s = bx(&mut p, BoxGeom::Rect, 300.0, 120.0, 64.0, 64.0);
        let t = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 64.0, 64.0);
        let path = p.connector_path(&Connector::new(End::bound(t), End::bound(s))).unwrap();
        assert!((path[0].y - 32.0).abs() > 1.0, "{path:?}");
    }

    #[test]
    fn label_point_follows_pos_and_offset() {
        let path = [Pt::new(0.0, 0.0), Pt::new(100.0, 0.0), Pt::new(100.0, 100.0)];
        let mut c = Connector::straight(Pt::ZERO, Pt::new(1.0, 1.0));
        assert!(c.label_point(&path).dist(Pt::new(100.0, 0.0)) < 1e-6, "the middle by default");
        assert_eq!(c.label_point(&path), path_midpoint(&path).0);
        c.set_label_pos(Some(0.25));
        assert!(c.label_point(&path).dist(Pt::new(50.0, 0.0)) < 1e-6);
        c.set_label_pos(Some(1.0));
        assert!(c.label_point(&path).dist(Pt::new(100.0, 100.0)) < 1e-6);
        c.set_label_pos(Some(0.0));
        assert!(c.label_point(&path).dist(Pt::ZERO) < 1e-6);
        // the offset is perpendicular: to the right of the direction of travel (below a path going right)
        c.set_label_pos(Some(0.25));
        c.set_label_offset(12.0);
        assert!(c.label_point(&path).dist(Pt::new(50.0, 12.0)) < 1e-6, "{:?}", c.label_point(&path));
        c.set_label_offset(-12.0);
        assert!(c.label_point(&path).dist(Pt::new(50.0, -12.0)) < 1e-6);
        c.set_label_pos(Some(0.75));
        c.set_label_offset(10.0);
        assert!(c.label_point(&path).dist(Pt::new(90.0, 50.0)) < 1e-6, "going down, the right is the left of the screen");
    }

    #[test]
    fn label_pos_and_offset_are_kept_in_range_and_ignore_non_finite_values() {
        let mut c = Connector::straight(Pt::ZERO, Pt::new(1.0, 1.0));
        c.set_label_pos(Some(7.0));
        assert_eq!(c.label_pos, Some(1.0));
        c.set_label_pos(Some(-3.0));
        assert_eq!(c.label_pos, Some(0.0));
        c.set_label_pos(Some(f64::NAN));
        assert_eq!(c.label_pos, Some(0.0));
        c.set_label_pos(None);
        assert_eq!(c.label_pos, None);
        c.set_label_offset(1e9);
        assert!((c.label_offset - Connector::LABEL_OFFSET_MAX).abs() < 1e-9);
        c.set_label_offset(f64::INFINITY);
        assert!((c.label_offset - Connector::LABEL_OFFSET_MAX).abs() < 1e-9);
        // a hostile file cannot move the label out of range either
        c.label_pos = Some(f64::NAN);
        c.label_offset = f64::NAN;
        let path = [Pt::ZERO, Pt::new(100.0, 0.0)];
        assert!(c.label_point(&path).dist(Pt::new(50.0, 0.0)) < 1e-6);
    }

    /// Elbow connectors from one source to a column of targets.
    fn fan_out(targets: &[f64]) -> (Page, Vec<Id>, Id) {
        let mut p = Page::new("t");
        let src = bx(&mut p, BoxGeom::Rect, 0.0, 200.0, 100.0, 60.0);
        let mut ids = Vec::new();
        for y in targets {
            let t = bx(&mut p, BoxGeom::Rect, 400.0, *y, 100.0, 60.0);
            let mut c = Connector::new(End::bound(src.clone()), End::bound(t));
            c.route = Route::Elbow;
            let o = Object::new(Kind::Connector(c), Style::default());
            ids.push(o.id.clone());
            p.objects.push(o);
        }
        (p, ids, src)
    }

    fn path_of(p: &Page, id: &Id) -> Vec<Pt> {
        let Kind::Connector(c) = &p.get(id).unwrap().kind else { panic!("connector") };
        p.connector_path(c).unwrap()
    }

    /// x of the vertical segments of a path.
    fn verticals(path: &[Pt]) -> Vec<f64> {
        path.windows(2).filter(|w| (w[0].x - w[1].x).abs() < 1e-6 && (w[0].y - w[1].y).abs() > 1.0).map(|w| w[0].x).collect()
    }

    #[test]
    fn elbows_fanning_out_to_a_column_get_distinct_vertical_channels() {
        let (p, ids, _) = fan_out(&[0.0, 100.0, 300.0, 400.0]);
        let paths: Vec<Vec<Pt>> = ids.iter().map(|i| path_of(&p, i)).collect();
        let xs: Vec<f64> = paths.iter().flat_map(|q| verticals(q)).collect();
        assert_eq!(xs.len(), 4, "one vertical each: {paths:?}");
        // the two above the source would share a line if not separated, and so would the two below (above and below only touch at the source)
        for pair in xs.chunks(2) {
            assert!((pair[0] - pair[1]).abs() >= 4.0, "channels {} and {} are the same line", pair[0], pair[1]);
        }
        // channels stay between the two stubs and every path stays orthogonal and joins its ends
        for q in &paths {
            assert!(xs.iter().all(|x| *x > 124.0 && *x < 376.0));
            for w in q.windows(2) {
                assert!((w[0].x - w[1].x).abs() < 1e-6 || (w[0].y - w[1].y).abs() < 1e-6, "oblique segment: {w:?}");
            }
            assert!(q[0].dist(Pt::new(100.0, 230.0)) < 1e-6);
        }
        // the farther a target, the nearer to the source its channel (the arrows nest, none crosses another)
        let by_y = |i: usize| (paths[i].last().unwrap().y, verticals(&paths[i])[0]);
        let (far_up, near_up) = (by_y(0), by_y(1));
        assert!(far_up.0 < near_up.0 && far_up.1 < near_up.1, "{far_up:?} {near_up:?}");
        let (near_down, far_down) = (by_y(2), by_y(3));
        assert!(far_down.0 > near_down.0 && far_down.1 < near_down.1, "{near_down:?} {far_down:?}");
    }

    #[test]
    fn a_lone_elbow_and_elbows_that_do_not_overlap_are_routed_as_before() {
        let (p, ids, _) = fan_out(&[0.0]);
        let mut alone = Page::new("t");
        alone.objects = p.objects.clone();
        assert_eq!(path_of(&p, &ids[0]), path_of(&alone, &ids[0]));
        let x = verticals(&path_of(&p, &ids[0]))[0];
        assert!((x - 250.0).abs() < 1e-6, "midpoint of the two stubs: {x}");
        // one target above, one below: the trunks only touch at the stub level
        let (p, ids, _) = fan_out(&[0.0, 400.0]);
        let (a, b) = (verticals(&path_of(&p, &ids[0])), verticals(&path_of(&p, &ids[1])));
        assert!((a[0] - 250.0).abs() < 1e-6 && (b[0] - 250.0).abs() < 1e-6, "{a:?} {b:?}");
    }

    #[test]
    fn two_unrelated_elbows_on_the_same_trunk_are_separated_without_crossing() {
        let mut p = Page::new("t");
        let a1 = bx(&mut p, BoxGeom::Rect, 0.0, 0.0, 100.0, 60.0);
        let a2 = bx(&mut p, BoxGeom::Rect, 0.0, 100.0, 100.0, 60.0);
        let b1 = bx(&mut p, BoxGeom::Rect, 400.0, 150.0, 100.0, 60.0);
        let b2 = bx(&mut p, BoxGeom::Rect, 400.0, 250.0, 100.0, 60.0);
        let mut ids = Vec::new();
        for (s, t) in [(a1, b1), (a2, b2)] {
            let mut c = Connector::new(End::bound(s), End::bound(t));
            c.route = Route::Elbow;
            let o = Object::new(Kind::Connector(c), Style::default());
            ids.push(o.id.clone());
            p.objects.push(o);
        }
        let (c0, c1) = (path_of(&p, &ids[0]), path_of(&p, &ids[1]));
        let (x0, x1) = (verticals(&c0)[0], verticals(&c1)[0]);
        assert!((x0 - x1).abs() >= 4.0, "{x0} {x1}");
        assert_eq!(c0, path_of(&p, &ids[0]), "deterministic");
        // the connector that starts and ends lower takes the channel nearer to the sources
        assert!(x1 < x0, "{x0} {x1}");
        assert!(!c0.windows(2).any(|s| c1.windows(2).any(|t| segments_cross(s, t))), "{c0:?} {c1:?}");
    }

    /// Does an axis-aligned segment cross another strictly inside both?
    fn segments_cross(s: &[Pt], t: &[Pt]) -> bool {
        let (h, v) = if (s[0].y - s[1].y).abs() < 1e-6 { (s, t) } else { (t, s) };
        (h[0].y - h[1].y).abs() < 1e-6
            && (v[0].x - v[1].x).abs() < 1e-6
            && v[0].x > h[0].x.min(h[1].x) + 1e-6
            && v[0].x < h[0].x.max(h[1].x) - 1e-6
            && h[0].y > v[0].y.min(v[1].y) + 1e-6
            && h[0].y < v[0].y.max(v[1].y) - 1e-6
    }

    #[test]
    fn separation_is_skipped_on_a_page_with_too_many_elbows() {
        let (mut p, ids, src) = fan_out(&[0.0, 100.0]);
        assert!((verticals(&path_of(&p, &ids[0]))[0] - 250.0).abs() > 1.0, "separated");
        // many unrelated elbows far away: the quadratic work is not done, the paths are the plain ones
        for i in 0..=SEPARATE_MAX_CONNECTORS {
            let t = bx(&mut p, BoxGeom::Rect, 5000.0, 5000.0 + f64::from(u32::try_from(i).unwrap()) * 100.0, 10.0, 10.0);
            let mut c = Connector::new(End::bound(src.clone()), End::bound(t));
            c.route = Route::Elbow;
            p.objects.push(Object::new(Kind::Connector(c), Style::default()));
        }
        assert!((verticals(&path_of(&p, &ids[0]))[0] - 250.0).abs() < 1e-6);
    }
}
