//! Layout check: the measurable defects of a page (overlaps, text that does not fit, arrows through objects, tiny text, unbalanced content…).
//!
//! Everything is computed from the model with approximate text metrics ([`ApproxMeasurer`]), the way `export` and `render` do, so the result
//! is cheap and deterministic. The checks are deliberately simple geometric rules (axis-aligned boxes, polylines); each one documents what it
//! can miss or report by mistake. Used by the MCP tool `check_layout`.

// Segment geometry reads best with the usual one-letter names.
#![allow(clippy::many_single_char_names)]

use crate::connector::{Connector, End};
use crate::geom::{Pt, Rect};
use crate::model::{BoxGeom, Id, Kind, Object, Page};
use crate::num;
use crate::text::{ApproxMeasurer, Measurer, layout_rich};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// Export width (pixels) assumed by the readability metrics when none is given.
pub const DEFAULT_EXPORT_WIDTH: f64 = 1600.0;
/// Two boxes overlap when they intersect by more than this, in both directions (board units).
pub const OVERLAP_TOL: f64 = 2.0;
/// Two connectors share a segment when they run along the same line (within 1.5 units) for more than this length.
pub const SHARED_MIN: f64 = 20.0;
/// Text smaller than this once exported (pixels) is reported.
pub const SMALL_TEXT_PX: f64 = 10.0;
/// Acceptable width / height of the content.
pub const ASPECT_RANGE: (f64, f64) = (0.6, 2.2);
/// `unbalanced` needs at least this many objects (connectors excluded) before the empty-region heuristic applies.
pub const BALANCE_MIN_OBJECTS: usize = 6;
/// ... and at least this many empty cells of the 3×3 grid to be reported.
pub const EMPTY_CELLS_MIN: usize = 3;
/// `sparse_frame`: a frame whose content covers less than this share of its area is reported ...
pub const SPARSE_COVERAGE: f64 = 0.30;
/// ... and so is one with an empty band wider than this (units) between its content and one of its borders.
pub const SPARSE_BAND: f64 = 200.0;
/// `unbalanced` (`staircase`): two top-level frames side by side whose tops differ by more than this (units).
pub const STAIRCASE_TOP_DELTA: f64 = 60.0;
/// `unbalanced` (`coverage`): the top-level objects cover less than this share of their bounding box (with at least [`COVERAGE_MIN_OBJECTS`] of them).
pub const COVERAGE_MIN: f64 = 0.35;
pub const COVERAGE_MIN_OBJECTS: usize = 3;
/// At most this many issues of one kind are kept (the report says `truncated`).
pub const MAX_PER_KIND: usize = 50;
/// Distance (units) under which two parallel segments are on the same line.
const SAME_LINE: f64 = 1.5;

/// How serious an issue is: `ok` is false as soon as there is one of either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One defect, with the ids of the objects involved.
#[derive(Clone, Debug)]
pub struct Issue {
    pub kind: &'static str,
    pub severity: Severity,
    pub ids: Vec<Id>,
    pub message: String,
    /// Measured values specific to the kind.
    pub details: Vec<(&'static str, Value)>,
}

/// Figures about the page (always about the whole page, whatever `Options::ids`).
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    /// Visible objects that are not connectors.
    pub objects: usize,
    pub connectors: usize,
    /// Bounding box of the visible content (as `export` frames it).
    pub content_box: Option<Rect>,
    /// Width / height of the content box.
    pub aspect: Option<f64>,
    /// Smallest text, in pixels once the content is exported at the given width.
    pub min_text_px: Option<f64>,
    /// Cells of the 3×3 grid of the content box that hold no object (0 when there are fewer than [`BALANCE_MIN_OBJECTS`] objects).
    pub empty_cells: usize,
    /// Share (0 to 1) of the bounding box of the top-level objects (not inside a frame) that their boxes cover (overlaps counted twice).
    pub coverage: Option<f64>,
    /// Largest difference between the tops of two top-level frames side by side (0 without such a pair).
    pub frame_top_offset: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub issues: Vec<Issue>,
    pub metrics: Metrics,
    /// Some issues were dropped: [`MAX_PER_KIND`] was reached for a kind.
    pub truncated: bool,
}

impl Report {
    /// No issue of severity error or warning.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    /// Width in pixels the page would be exported at (for the readability metrics).
    pub export_width: f64,
    /// Keep only the issues that involve one of these objects.
    pub ids: Option<&'a [Id]>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self { export_width: DEFAULT_EXPORT_WIDTH, ids: None }
    }
}

/* ------------------------------------------------------------- helpers */

/// A shape, image, text or table: something with a box.
struct Item<'a> {
    obj: &'a Object,
    rect: Rect,
    /// Plain text object.
    text: bool,
    /// Rectangle that contains at least one other item (a grouping frame).
    frame: bool,
}

struct Conn<'a> {
    obj: &'a Object,
    c: &'a Connector,
    /// Routed path, as render and export draw it; `None` when an end is free or its target is gone.
    path: Option<Vec<Pt>>,
    /// Start and end: free, or bound to an object that is gone.
    dangling: [bool; 2],
}

impl Conn<'_> {
    fn ends(&self) -> [Option<&Id>; 2] {
        [self.c.start.target(), self.c.end.target()]
    }
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn contains(outer: &Rect, inner: &Rect) -> bool {
    outer.expand(OVERLAP_TOL).contains_rect(inner)
}

/// Size of the intersection of two boxes, when it exceeds the tolerance in both directions.
fn overlap(a: &Rect, b: &Rect) -> Option<(f64, f64)> {
    let w = a.right().min(b.right()) - a.x.max(b.x);
    let h = a.bottom().min(b.bottom()) - a.y.max(b.y);
    (w > OVERLAP_TOL && h > OVERLAP_TOL).then_some((w, h))
}

fn cross(a: Pt, b: Pt) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Point where the segments `p` and `q` cross strictly inside both (touching ends and parallel segments do not count).
fn crossing(p: (Pt, Pt), q: (Pt, Pt)) -> Option<Pt> {
    let (d1, d2) = (p.1 - p.0, q.1 - q.0);
    let den = cross(d1, d2);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = cross(q.0 - p.0, d2) / den;
    let u = cross(q.0 - p.0, d1) / den;
    let inside = |v: f64| v > 1e-6 && v < 1.0 - 1e-6;
    (inside(t) && inside(u)).then(|| p.0 + d1 * t)
}

/// Length over which two segments run along the same line (0 when they are not parallel and close).
fn shared_length(p: (Pt, Pt), q: (Pt, Pt)) -> f64 {
    let d = p.1 - p.0;
    let len = d.len();
    let lq = (q.1 - q.0).len();
    if len < 1e-9 || lq < 1e-9 || cross(d, q.1 - q.0).abs() > 0.02 * len * lq {
        return 0.0;
    }
    let u = d * (1.0 / len);
    let off = |pt: Pt| cross(u, pt - p.0).abs();
    if off(q.0) > SAME_LINE || off(q.1) > SAME_LINE {
        return 0.0;
    }
    let (a, b) = ((q.0 - p.0).dot(u), (q.1 - p.0).dot(u));
    (len.min(a.max(b)) - a.min(b).max(0.0)).max(0.0)
}

/// Does the segment touch the box (Liang-Barsky clipping)?
fn segment_hits(a: Pt, b: Pt, r: &Rect) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-d.x, a.x - r.x), (d.x, r.right() - a.x), (-d.y, a.y - r.y), (d.y, r.bottom() - a.y)] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn segments(path: &[Pt]) -> impl Iterator<Item = (Pt, Pt)> + '_ {
    path.windows(2).map(|w| (w[0], w[1]))
}

fn bbox(path: &[Pt]) -> Rect {
    Rect::from_points(path.iter().copied()).unwrap_or_default()
}

/// Box of the label of a connector: its text laid out centered on its position along the path (`label_pos`, `label_offset`), as the SVG export does.
fn label_box(c: &Connector, path: &[Pt]) -> Option<Rect> {
    let l = c.label.as_ref().filter(|l| !l.text.is_empty())?;
    let mid = c.label_point(path);
    let lay = layout_rich(&l.text, None, &ApproxMeasurer, l.family, l.size);
    Some(Rect::new(mid.x - lay.width / 2.0, mid.y - lay.height / 2.0, lay.width, lay.height))
}

fn issue(kind: &'static str, severity: Severity, ids: Vec<Id>, message: String, details: Vec<(&'static str, Value)>) -> Issue {
    Issue { kind, severity, ids, message, details }
}

/* ------------------------------------------------------------ the checks */

/// Runs every check on a page.
///
/// Only visible objects count. Without `opts.ids` the whole page is checked; with them, only the issues that involve one of those objects
/// are returned (the page-level `unbalanced` issues are then left out). The metrics always describe the whole page.
#[must_use]
pub fn check(page: &Page, opts: &Options) -> Report {
    let visible: Vec<&Object> = page.objects.iter().filter(|o| page.is_visible(o)).collect();
    let mut items: Vec<Item> = visible
        .iter()
        .copied()
        .filter_map(|o| {
            let rect = o.frame()?.aabb();
            let text = matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Text);
            Some(Item { obj: o, rect, text, frame: false })
        })
        .collect();
    for i in 0..items.len() {
        let is_rect = matches!(&items[i].obj.kind, Kind::Box(b) if b.geom == BoxGeom::Rect);
        items[i].frame = is_rect && items.iter().enumerate().any(|(j, o)| j != i && contains(&items[i].rect, &o.rect) && !contains(&o.rect, &items[i].rect));
    }
    let index = page.index();
    let conns: Vec<Conn> = visible
        .iter()
        .copied()
        .filter_map(|o| match &o.kind {
            Kind::Connector(c) => {
                let path = page.connector_path_with(c, &|id| index.get(id).copied()).filter(|p| p.len() >= 2);
                let gone = |e: &End| e.target().is_none_or(|t| !index.contains_key(t));
                Some(Conn { obj: o, c, path, dangling: [gone(&c.start), gone(&c.end)] })
            }
            _ => None,
        })
        .collect();
    let content = page.content_bounds();

    let mut issues = Vec::new();
    overlaps(&items, &mut issues);
    text_fit(&items, &mut issues);
    text_outside_frame(&items, &mut issues);
    connector_issues(&items, &conns, &mut issues);
    label_collisions(&items, &conns, &mut issues);
    duplicates_and_unbound(&conns, &mut issues);
    let min_text_px = small_text(&items, &conns, content, opts.export_width, &mut issues);
    let objects = visible.iter().filter(|o| !matches!(o.kind, Kind::Connector(_))).count();
    let empty_cells = balance(page, &visible, &items, content, objects, &mut issues);
    sparse_frames(&items, &mut issues);
    let frame_top_offset = staircase(&items, &mut issues);
    let coverage = coverage(&items, &mut issues);

    let mut report = Report {
        issues: Vec::new(),
        metrics: Metrics {
            objects,
            connectors: conns.len(),
            content_box: content,
            aspect: content.filter(|r| r.h > 0.0).map(|r| r.w / r.h),
            min_text_px,
            empty_cells,
            coverage,
            frame_top_offset,
        },
        truncated: false,
    };
    let wanted: Option<HashSet<&Id>> = opts.ids.map(|l| l.iter().collect());
    let mut per_kind: HashMap<&str, usize> = HashMap::new();
    for i in issues {
        if wanted.as_ref().is_some_and(|w| !i.ids.iter().any(|id| w.contains(id))) {
            continue;
        }
        let n = per_kind.entry(i.kind).or_insert(0);
        *n += 1;
        if *n > MAX_PER_KIND {
            report.truncated = true;
        } else {
            report.issues.push(i);
        }
    }
    report
}

/// `overlap` (error): two shapes, images, texts or tables whose boxes intersect by more than [`OVERLAP_TOL`] in both directions.
/// Not reported when one contains the other (a frame and its content, a label in its shape), when both are in the same group (an icon and
/// its label), or for a text and a frame it only partly covers (that is `text_outside_frame`). Boxes are axis-aligned bounding boxes: an
/// ellipse or a diamond can be reported when only the corners of their boxes meet (false positive).
fn overlaps(items: &[Item], out: &mut Vec<Issue>) {
    for (i, a) in items.iter().enumerate() {
        for b in &items[i + 1..] {
            let same_group = a.obj.group.is_some() && a.obj.group == b.obj.group;
            let text_vs_frame = (a.text && b.frame) || (b.text && a.frame);
            if same_group || text_vs_frame || contains(&a.rect, &b.rect) || contains(&b.rect, &a.rect) {
                continue;
            }
            if let Some((w, h)) = overlap(&a.rect, &b.rect) {
                out.push(issue(
                    "overlap",
                    Severity::Error,
                    vec![a.obj.id.clone(), b.obj.id.clone()],
                    format!("{} and {} overlap by {:.0} x {:.0}: move or resize one of them", a.obj.id.0, b.obj.id.0, w, h),
                    vec![("overlap_w", json!(r1(w))), ("overlap_h", json!(r1(h)))],
                ));
            }
        }
    }
}

/// `text_overflow` (error): the text of a shape (not a free text) is taller than its text area ([`crate::model::BoxObj::label_rect`]).
/// `word_split` (error): a word is wider than the usable text width, so it is cut in two (bold metrics when any part of the text is bold).
fn text_fit(items: &[Item], out: &mut Vec<Issue>) {
    for it in items {
        let Kind::Box(b) = &it.obj.kind else { continue };
        if b.geom == BoxGeom::Text || b.label.text.is_empty() {
            continue;
        }
        let area = b.label_rect();
        let lay = b.layout(&ApproxMeasurer);
        let id = &it.obj.id;
        if lay.height > area.h + 0.5 {
            out.push(issue(
                "text_overflow",
                Severity::Error,
                vec![id.clone()],
                format!(
                    "the text of {} needs {:.0} high but the text area is {:.0}: increase h, shorten the text or lower font_size",
                    id.0, lay.height, area.h
                ),
                vec![("text_h", json!(r1(lay.height))), ("area_h", json!(r1(area.h)))],
            ));
        }
        let bold = b.label.text.0.iter().any(|s| s.style.bold);
        let widest = b
            .label
            .text
            .to_plain()
            .split_whitespace()
            .map(|w| (w.to_owned(), ApproxMeasurer.width(w, b.label.family, b.label.size, bold)))
            .max_by(|p, q| p.1.total_cmp(&q.1));
        if let Some((word, w)) = widest.filter(|(_, w)| *w > area.w) {
            out.push(issue(
                "word_split",
                Severity::Error,
                vec![id.clone()],
                format!("the word \"{word}\" ({w:.0} wide) does not fit the {:.0} wide text area of {}: widen the shape", area.w, id.0),
                vec![("word", json!(word)), ("word_w", json!(r1(w))), ("area_w", json!(r1(area.w)))],
            ));
        }
    }
}

/// `text_outside_frame` (warning): a text object that partly overlaps a frame (intersects it by more than [`OVERLAP_TOL`] in both
/// directions without being contained in it, and without containing it): typically a frame title straddling the border.
fn text_outside_frame(items: &[Item], out: &mut Vec<Issue>) {
    for t in items.iter().filter(|i| i.text) {
        for f in items.iter().filter(|i| i.frame) {
            if overlap(&t.rect, &f.rect).is_some() && !contains(&f.rect, &t.rect) && !contains(&t.rect, &f.rect) {
                out.push(issue(
                    "text_outside_frame",
                    Severity::Warning,
                    vec![t.obj.id.clone(), f.obj.id.clone()],
                    format!("text {} straddles the border of frame {}: move it fully inside or outside", t.obj.id.0, f.obj.id.0),
                    vec![],
                ));
            }
        }
    }
}

/// Checks of the connector paths against the objects and against each other:
/// - `connector_through_object` (error): a segment of the path touches the box (shrunk by [`OVERLAP_TOL`]) of an item that is neither the
///   source, nor the target, nor a frame containing the source or the target. Boxes are bounding boxes, so a path that passes near the corner of
///   a diamond or an ellipse can be reported by mistake.
/// - `connector_crossing` (warning): two paths cross strictly inside a segment each; connectors sharing an end object are skipped.
/// - `shared_segment` (warning): two paths run along the same line (within 1.5 units, angle under about 1 degree) for more than [`SHARED_MIN`]
///   units, unless they end on the same object (arrows merging into one target are fine) or only share the first segment of two arrows leaving the same point.
fn connector_issues(items: &[Item], conns: &[Conn], out: &mut Vec<Issue>) {
    let by_id: HashMap<&Id, &Item> = items.iter().map(|i| (&i.obj.id, i)).collect();
    for k in conns {
        let Some(path) = &k.path else { continue };
        let ends: Vec<&Item> = k.ends().into_iter().flatten().filter_map(|id| by_id.get(id).copied()).collect();
        for it in items {
            let is_end = k.ends().contains(&Some(&it.obj.id));
            let frame_of_end = it.frame && ends.iter().any(|e| contains(&it.rect, &e.rect));
            if is_end || frame_of_end {
                continue;
            }
            let r = it.rect.expand(-OVERLAP_TOL);
            if r.w > 0.0 && r.h > 0.0 && segments(path).any(|(a, b)| segment_hits(a, b, &r)) {
                out.push(issue(
                    "connector_through_object",
                    Severity::Error,
                    vec![k.obj.id.clone(), it.obj.id.clone()],
                    format!("connector {} passes through {}: reroute it (via points, other route) or move the object", k.obj.id.0, it.obj.id.0),
                    vec![],
                ));
            }
        }
    }
    for (i, a) in conns.iter().enumerate() {
        let Some(pa) = &a.path else { continue };
        let ba = bbox(pa).expand(SAME_LINE);
        for b in &conns[i + 1..] {
            let Some(pb) = &b.path else { continue };
            if !ba.intersects(&bbox(pb)) {
                continue;
            }
            let shared_end = a.ends().iter().flatten().any(|id| b.ends().contains(&Some(*id)));
            let same_target = a.c.end.target().is_some() && a.c.end.target() == b.c.end.target();
            // arrows leaving the same point share their first segment, then branch: a common stem is not a shared segment
            let stem = pa[0].dist(pb[0]) < 1e-6;
            let mut x = None;
            let mut shared = 0.0_f64;
            for (ia, sa) in segments(pa).enumerate() {
                for (ib, sb) in segments(pb).enumerate() {
                    if !shared_end && x.is_none() {
                        x = crossing(sa, sb);
                    }
                    if !(same_target || stem && ia == 0 && ib == 0) {
                        shared = shared.max(shared_length(sa, sb));
                    }
                }
            }
            let ids = vec![a.obj.id.clone(), b.obj.id.clone()];
            if let Some(p) = x {
                out.push(issue(
                    "connector_crossing",
                    Severity::Warning,
                    ids.clone(),
                    format!("connectors {} and {} cross near ({:.0}, {:.0})", a.obj.id.0, b.obj.id.0, p.x, p.y),
                    vec![("x", json!(r1(p.x))), ("y", json!(r1(p.y)))],
                ));
            }
            if shared > SHARED_MIN {
                out.push(issue(
                    "shared_segment",
                    Severity::Warning,
                    ids,
                    format!("connectors {} and {} run along the same line for {:.0} units: separate them", a.obj.id.0, b.obj.id.0, shared),
                    vec![("length", json!(r1(shared)))],
                ));
            }
        }
    }
}

/// `label_collision` (warning): the label of a connector (its text laid out at `label_pos` along the path, the middle by default, as the export draws it) intersects,
/// by more than [`OVERLAP_TOL`] in both directions, a shape, an image, a text, a table or another connector's label; or it crosses the border
/// of a frame (intersects it without being inside).
fn label_collisions(items: &[Item], conns: &[Conn], out: &mut Vec<Issue>) {
    let labels: Vec<(&Conn, Rect)> = conns.iter().filter_map(|k| Some((k, label_box(k.c, k.path.as_deref()?)?))).collect();
    for (i, (k, lb)) in labels.iter().enumerate() {
        for it in items {
            let hit = if it.frame { lb.intersects(&it.rect) && !contains(&it.rect, lb) && !contains(lb, &it.rect) } else { overlap(lb, &it.rect).is_some() };
            if hit {
                let what = if it.frame { "the border of frame" } else { "object" };
                out.push(issue(
                    "label_collision",
                    Severity::Warning,
                    vec![k.obj.id.clone(), it.obj.id.clone()],
                    format!("the label of connector {} collides with {what} {}: shorten it, or move the connector or the object", k.obj.id.0, it.obj.id.0),
                    vec![],
                ));
            }
        }
        for (other, ob) in &labels[i + 1..] {
            if overlap(lb, ob).is_some() {
                out.push(issue(
                    "label_collision",
                    Severity::Warning,
                    vec![k.obj.id.clone(), other.obj.id.clone()],
                    format!("the labels of connectors {} and {} overlap", k.obj.id.0, other.obj.id.0),
                    vec![],
                ));
            }
        }
    }
}

/// `duplicate_connector` (error): several connectors join the same two objects (in either direction).
/// `unbound_connector` (error): an end is free, or bound to an object that no longer exists.
fn duplicates_and_unbound(conns: &[Conn], out: &mut Vec<Issue>) {
    let mut pairs: Vec<((&Id, &Id), Vec<Id>)> = Vec::new();
    for k in conns {
        let [s, e] = k.dangling;
        if s || e {
            let which = match (s, e) {
                (true, true) => "start and end",
                (true, false) => "start",
                _ => "end",
            };
            out.push(issue(
                "unbound_connector",
                Severity::Error,
                vec![k.obj.id.clone()],
                format!("the {which} of connector {} is not attached to an object: give from/to", k.obj.id.0),
                vec![("ends", json!(which))],
            ));
        }
        if let [Some(a), Some(b)] = k.ends() {
            let key = if a.0 <= b.0 { (a, b) } else { (b, a) };
            match pairs.iter_mut().find(|(p, _)| *p == key) {
                Some((_, ids)) => ids.push(k.obj.id.clone()),
                None => pairs.push((key, vec![k.obj.id.clone()])),
            }
        }
    }
    for ((a, b), ids) in pairs.into_iter().filter(|(_, ids)| ids.len() > 1) {
        out.push(issue(
            "duplicate_connector",
            Severity::Error,
            ids.clone(),
            format!("{} connectors join {} and {}: delete the extra ones", ids.len(), a.0, b.0),
            vec![("objects", json!([a.0, b.0]))],
        ));
    }
}

/// `small_text` (warning): text whose size, scaled to the export, is under [`SMALL_TEXT_PX`] pixels.
///
/// Exporting the page at `export_width` pixels scales the content box (width `content_w`) by `export_width / content_w` pixels per unit, so a
/// font of `size` units is `size * export_width / content_w` pixels tall. The texts concerned are those of shapes, tables and connector
/// labels; one issue lists them all, with the smallest. Returns the smallest size in pixels (the `min_text_px` metric).
fn small_text(items: &[Item], conns: &[Conn], content: Option<Rect>, export_width: f64, out: &mut Vec<Issue>) -> Option<f64> {
    let w = content.map(|r| r.w).filter(|w| *w > 0.0)?;
    let scale = export_width / w;
    let mut sizes: Vec<(&Id, f64)> = Vec::new();
    for it in items {
        match &it.obj.kind {
            Kind::Box(b) if !b.label.text.is_empty() => sizes.push((&it.obj.id, b.label.size)),
            Kind::Table(t) if t.cells.iter().any(|c| !c.is_empty()) => sizes.push((&it.obj.id, t.size)),
            _ => {}
        }
    }
    for k in conns {
        if let Some(l) = k.c.label.as_ref().filter(|l| !l.text.is_empty()) {
            sizes.push((&k.obj.id, l.size));
        }
    }
    let (smallest, size) = sizes.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1))?;
    let px = size * scale;
    let small: Vec<Id> = sizes.iter().filter(|(_, s)| s * scale < SMALL_TEXT_PX).map(|(id, _)| (*id).clone()).collect();
    if !small.is_empty() {
        let needed = SMALL_TEXT_PX / scale;
        out.push(issue(
            "small_text",
            Severity::Warning,
            small.clone(),
            format!(
                "{} text(s) will be under {SMALL_TEXT_PX:.0} px at {export_width:.0} px wide (smallest {px:.1} px, {}): use font_size >= {:.0}, or reduce the size of the diagram",
                small.len(),
                smallest.0,
                needed.ceil()
            ),
            vec![("min_px", json!(r1(px))), ("smallest", json!(smallest.0)), ("min_font_size", json!(needed.ceil()))],
        ));
    }
    Some(px)
}

/// `unbalanced` (warning), two reasons (`reason` in the details):
/// - `aspect`: width / height of the content box outside [`ASPECT_RANGE`] (0.6 to 2.2);
/// - `empty_region`: with at least [`BALANCE_MIN_OBJECTS`] objects (connectors excluded), the content box is cut in a 3 x 3 grid and the
///   cells that no object touches are counted; [`EMPTY_CELLS_MIN`] or more of them are reported. Frames do not fill a cell (they would
///   cover everything), connectors neither. It is a cheap heuristic: it catches a diagram squeezed in a corner or split in two far apart
///   groups, not subtle gaps, and an L-shaped diagram legitimately leaves a corner empty.
///
/// Returns the number of empty cells.
fn balance(page: &Page, visible: &[&Object], items: &[Item], content: Option<Rect>, objects: usize, out: &mut Vec<Issue>) -> usize {
    let Some(r) = content.filter(|r| r.w > 0.0 && r.h > 0.0) else { return 0 };
    let aspect = r.w / r.h;
    if aspect < ASPECT_RANGE.0 || aspect > ASPECT_RANGE.1 {
        out.push(issue(
            "unbalanced",
            Severity::Warning,
            vec![],
            format!(
                "the content is {:.0} x {:.0} (width / height = {aspect:.2}, expected {} to {}): rearrange it into a more compact shape",
                r.w, r.h, ASPECT_RANGE.0, ASPECT_RANGE.1
            ),
            vec![("reason", json!("aspect")), ("aspect", json!((aspect * 100.0).round() / 100.0))],
        ));
    }
    if objects < BALANCE_MIN_OBJECTS {
        return 0;
    }
    let mut boxes: Vec<Rect> = items.iter().filter(|i| !i.frame).map(|i| i.rect).collect();
    boxes.extend(visible.iter().filter(|o| matches!(o.kind, Kind::Pen(_))).map(|o| page.bounds(o)));
    let mut empty = Vec::new();
    for row in 0..3 {
        for col in 0..3 {
            let cell = Rect::new(r.x + r.w * num::f64_of(col) / 3.0, r.y + r.h * num::f64_of(row) / 3.0, r.w / 3.0, r.h / 3.0);
            let filled = boxes.iter().any(|b| b.w > 0.0 && b.h > 0.0 && b.right() > cell.x && b.x < cell.right() && b.bottom() > cell.y && b.y < cell.bottom());
            if !filled {
                empty.push(json!({"col": col, "row": row}));
            }
        }
    }
    if empty.len() >= EMPTY_CELLS_MIN {
        out.push(issue(
            "unbalanced",
            Severity::Warning,
            vec![],
            format!("{} of the 9 cells (3 x 3 grid) of the content hold no object: the layout leaves large empty areas, spread it out evenly", empty.len()),
            vec![("reason", json!("empty_region")), ("empty_cells", json!(empty))],
        ));
    }
    empty.len()
}

/// Items strictly inside the frame `f` (the notion of frame of [`Item::frame`]).
fn content_of<'a>(items: &'a [Item<'a>], f: &Item) -> impl Iterator<Item = &'a Item<'a>> {
    let (id, rect) = (f.obj.id.clone(), f.rect);
    items.iter().filter(move |o| o.obj.id != id && contains(&rect, &o.rect) && !contains(&o.rect, &rect))
}

fn union(rects: impl Iterator<Item = Rect>) -> Option<Rect> {
    rects.reduce(|a, b| {
        let (x, y) = (a.x.min(b.x), a.y.min(b.y));
        Rect::new(x, y, a.right().max(b.right()) - x, a.bottom().max(b.bottom()) - y)
    })
}

/// `sparse_frame` (warning): a frame whose content (the bounding box of the items inside it) covers less than [`SPARSE_COVERAGE`] of its area,
/// or leaves an empty band wider than [`SPARSE_BAND`] between the content and one of its borders (typically a frame left behind or stretched
/// after its content moved). The title room at the top is far below the band limit. Details: `coverage`, `band_side`, `band`.
fn sparse_frames(items: &[Item], out: &mut Vec<Issue>) {
    for f in items.iter().filter(|i| i.frame) {
        let Some(c) = union(content_of(items, f).map(|o| o.rect)) else { continue };
        let coverage = if f.rect.w * f.rect.h > 0.0 { c.w * c.h / (f.rect.w * f.rect.h) } else { 1.0 };
        let bands = [("left", c.x - f.rect.x), ("right", f.rect.right() - c.right()), ("top", c.y - f.rect.y), ("bottom", f.rect.bottom() - c.bottom())];
        let (side, band) = bands.into_iter().fold(("left", 0.0), |m, b| if b.1 > m.1 { b } else { m });
        if coverage < SPARSE_COVERAGE || band > SPARSE_BAND {
            let id = &f.obj.id.0;
            out.push(issue(
                "sparse_frame",
                Severity::Warning,
                vec![f.obj.id.clone()],
                format!(
                    "frame {id} is mostly empty (its content covers {:.0}% of it, widest empty band {band:.0} on the {side}): call add_frame with frame={id} and ids=[its content] to re-fit it",
                    coverage * 100.0
                ),
                vec![("coverage", json!((coverage * 100.0).round() / 100.0)), ("band_side", json!(side)), ("band", json!(r1(band)))],
            ));
        }
    }
}

/// Top-level items: those no frame contains.
fn top_level<'a>(items: &'a [Item<'a>]) -> Vec<&'a Item<'a>> {
    items.iter().filter(|i| !items.iter().any(|f| f.frame && f.obj.id != i.obj.id && contains(&f.rect, &i.rect) && !contains(&i.rect, &f.rect))).collect()
}

/// `unbalanced` (warning), reason `staircase`: two top-level frames side by side (their x ranges are disjoint, their y ranges overlap) whose
/// tops differ by more than [`STAIRCASE_TOP_DELTA`]; ids are the frames involved. Frames in different rows do not overlap vertically, so a
/// grid of frames is fine. Returns the largest top difference found.
fn staircase(items: &[Item], out: &mut Vec<Issue>) -> f64 {
    let frames: Vec<&Item> = top_level(items).into_iter().filter(|i| i.frame).collect();
    let (mut worst, mut ids) = (0.0_f64, Vec::new());
    for (i, a) in frames.iter().enumerate() {
        for b in &frames[i + 1..] {
            let side_by_side = a.rect.right() <= b.rect.x + OVERLAP_TOL || b.rect.right() <= a.rect.x + OVERLAP_TOL;
            let diff = (a.rect.y - b.rect.y).abs();
            if side_by_side && a.rect.y < b.rect.bottom() - OVERLAP_TOL && b.rect.y < a.rect.bottom() - OVERLAP_TOL && diff > STAIRCASE_TOP_DELTA {
                worst = worst.max(diff);
                for f in [a, b] {
                    if !ids.contains(&f.obj.id) {
                        ids.push(f.obj.id.clone());
                    }
                }
            }
        }
    }
    if !ids.is_empty() {
        out.push(issue(
            "unbalanced",
            Severity::Warning,
            ids,
            format!("frames side by side start at different heights (tops differ by up to {worst:.0}, more than {STAIRCASE_TOP_DELTA:.0}): align their tops (arrange_objects align top, or give them the same y)"),
            vec![("reason", json!("staircase")), ("top_offset", json!(r1(worst)))],
        ));
    }
    worst
}

/// `unbalanced` (warning), reason `coverage`: with at least [`COVERAGE_MIN_OBJECTS`] top-level objects, their boxes cover less than
/// [`COVERAGE_MIN`] of their bounding box (a diagonal or scattered layout). Areas are added up, so overlaps count twice (they are reported
/// on their own). Returns the coverage.
fn coverage(items: &[Item], out: &mut Vec<Issue>) -> Option<f64> {
    let top = top_level(items);
    let bounds = union(top.iter().map(|i| i.rect)).filter(|r| r.w > 0.0 && r.h > 0.0)?;
    let covered = (top.iter().map(|i| i.rect.w * i.rect.h).sum::<f64>() / (bounds.w * bounds.h)).min(1.0);
    if top.len() >= COVERAGE_MIN_OBJECTS && covered < COVERAGE_MIN {
        out.push(issue(
            "unbalanced",
            Severity::Warning,
            vec![],
            format!(
                "the objects cover only {:.0}% of the area they span (expected at least {:.0}%): bring them closer together or arrange them in rows and columns",
                covered * 100.0,
                COVERAGE_MIN * 100.0
            ),
            vec![("reason", json!("coverage")), ("coverage", json!((covered * 100.0).round() / 100.0))],
        ));
    }
    Some(covered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::Route;
    use crate::geom::Frame;
    use crate::model::Label;
    use crate::style::{Align, Style};

    fn shape(p: &mut Page, geom: BoxGeom, text: &str, x: f64, y: f64, w: f64, h: f64) -> Id {
        let o = Object::new_box(geom, Frame::new(x, y, w, h), Style::default(), Label::new(text, 16.0, Align::Center));
        let id = o.id.clone();
        p.objects.push(o);
        id
    }

    fn rect(p: &mut Page, x: f64, y: f64, w: f64, h: f64) -> Id {
        shape(p, BoxGeom::Rect, "", x, y, w, h)
    }

    fn link(p: &mut Page, a: &Id, b: &Id) -> Id {
        link_with(p, a, b, Route::Straight, None)
    }

    fn link_with(p: &mut Page, a: &Id, b: &Id, route: Route, label: Option<&str>) -> Id {
        let mut c = Connector::new(End::bound(a.clone()), End::bound(b.clone()));
        c.route = route;
        c.label = label.map(|t| Label::new(t, 16.0, Align::Center));
        let o = Object::new(Kind::Connector(c), Style::default());
        let id = o.id.clone();
        p.objects.push(o);
        id
    }

    fn issues_of(p: &Page, kind: &str) -> Vec<Issue> {
        check(p, &Options::default()).issues.into_iter().filter(|i| i.kind == kind).collect()
    }

    #[test]
    fn a_clean_page_has_no_issue() {
        let mut p = Page::new("t");
        let a = shape(&mut p, BoxGeom::Rect, "A", 0.0, 0.0, 200.0, 100.0);
        let b = shape(&mut p, BoxGeom::Rect, "B", 0.0, 200.0, 200.0, 100.0);
        link(&mut p, &a, &b);
        let r = check(&p, &Options::default());
        assert!(r.ok(), "{:?}", r.issues);
        assert_eq!((r.metrics.objects, r.metrics.connectors), (2, 1));
        assert!(r.metrics.min_text_px.unwrap() > 10.0);
    }

    #[test]
    fn overlap_needs_more_than_the_tolerance_and_skips_containment_and_groups() {
        let mut p = Page::new("t");
        let a = rect(&mut p, 0.0, 0.0, 100.0, 100.0);
        let b = rect(&mut p, 50.0, 50.0, 100.0, 100.0);
        let _touch = rect(&mut p, 100.5, 200.0, 50.0, 50.0);
        let _next = rect(&mut p, 150.0, 200.0, 50.0, 50.0); // 0.5 unit of overlap with the previous one
        let _inner = rect(&mut p, 10.0, 10.0, 20.0, 20.0); // inside a: containment, not an overlap
        let hit = issues_of(&p, "overlap");
        assert_eq!(hit.len(), 1, "{hit:?}");
        assert_eq!(hit[0].ids, vec![a.clone(), b.clone()]);
        assert_eq!(hit[0].severity, Severity::Error);
        // an icon and its label: same group
        for o in &mut p.objects {
            if o.id == a || o.id == b {
                o.group = Some(Id::from("g"));
            }
        }
        assert!(issues_of(&p, "overlap").is_empty());
    }

    #[test]
    fn text_overflow_and_word_split() {
        let mut p = Page::new("t");
        let tall = shape(&mut p, BoxGeom::Rect, "one two three four five six seven eight nine ten", 0.0, 0.0, 120.0, 30.0);
        let long = shape(&mut p, BoxGeom::Rect, "Supercalifragilistic", 300.0, 0.0, 90.0, 60.0);
        let fine = shape(&mut p, BoxGeom::Rect, "ok", 600.0, 0.0, 90.0, 60.0);
        let o = issues_of(&p, "text_overflow");
        assert_eq!(o.len(), 1, "{o:?}");
        assert_eq!(o[0].ids, vec![tall]);
        let w = issues_of(&p, "word_split");
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].ids, vec![long]);
        assert!(check(&p, &Options::default()).issues.iter().all(|i| !i.ids.contains(&fine)));
    }

    #[test]
    fn a_text_straddling_a_frame_border_is_reported_not_as_an_overlap() {
        let mut p = Page::new("t");
        let frame = rect(&mut p, 0.0, 0.0, 400.0, 300.0);
        let _inside = rect(&mut p, 50.0, 80.0, 100.0, 60.0);
        let title = shape(&mut p, BoxGeom::Text, "Title", 20.0, -10.0, 80.0, 24.0);
        let ok = shape(&mut p, BoxGeom::Text, "Inside", 20.0, 20.0, 80.0, 24.0);
        let r = issues_of(&p, "text_outside_frame");
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].ids, vec![title.clone(), frame]);
        assert!(issues_of(&p, "overlap").is_empty());
        assert!(!r[0].ids.contains(&ok));
    }

    #[test]
    fn a_connector_through_an_object_but_not_through_its_ends_or_frames() {
        let mut p = Page::new("t");
        let frame = rect(&mut p, -50.0, -50.0, 900.0, 300.0);
        let a = rect(&mut p, 0.0, 0.0, 100.0, 100.0);
        let mid = rect(&mut p, 300.0, 0.0, 100.0, 100.0);
        let b = rect(&mut p, 600.0, 0.0, 100.0, 100.0);
        let k = link(&mut p, &a, &b);
        let r = issues_of(&p, "connector_through_object");
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].ids, vec![k, mid]);
        assert!(!r[0].ids.contains(&frame));
        // a frame that contains neither end is crossed: reported
        let mut q = Page::new("t");
        let a = rect(&mut q, 0.0, 0.0, 100.0, 100.0);
        let b = rect(&mut q, 600.0, 0.0, 100.0, 100.0);
        let other = rect(&mut q, 250.0, -40.0, 200.0, 180.0);
        let _inner = rect(&mut q, 280.0, 300.0, 40.0, 40.0);
        let _in_other = rect(&mut q, 270.0, 120.0, 40.0, 10.0);
        link(&mut q, &a, &b);
        assert!(issues_of(&q, "connector_through_object").iter().any(|i| i.ids.contains(&other)));
    }

    #[test]
    fn crossing_connectors_are_found_unless_they_share_an_object() {
        let mut p = Page::new("t");
        let a = rect(&mut p, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut p, 400.0, 400.0, 60.0, 60.0);
        let c = rect(&mut p, 400.0, 0.0, 60.0, 60.0);
        let d = rect(&mut p, 0.0, 400.0, 60.0, 60.0);
        let k1 = link(&mut p, &a, &b);
        let k2 = link(&mut p, &c, &d);
        let r = issues_of(&p, "connector_crossing");
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].ids, vec![k1, k2]);
        // same crossing but the connectors share the object a: not reported
        let mut q = Page::new("t");
        let a = rect(&mut q, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut q, 400.0, 400.0, 60.0, 60.0);
        let c = rect(&mut q, 400.0, 0.0, 60.0, 60.0);
        link(&mut q, &a, &b);
        link(&mut q, &a, &c);
        assert!(issues_of(&q, "connector_crossing").is_empty());
    }

    #[test]
    fn connectors_on_one_line_share_a_segment_unless_they_merge_into_the_same_target() {
        let mut p = Page::new("t");
        let a = rect(&mut p, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut p, 200.0, 0.0, 60.0, 60.0);
        let c = rect(&mut p, 400.0, 0.0, 60.0, 60.0);
        let d = rect(&mut p, 600.0, 0.0, 60.0, 60.0);
        // a -> c passes through b's row, b -> d runs on the same line between b and c
        let k1 = link(&mut p, &a, &c);
        let k2 = link(&mut p, &b, &d);
        let r = issues_of(&p, "shared_segment");
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].ids, vec![k1, k2]);
        // two connectors into the same target along one line: allowed
        let mut q = Page::new("t");
        let a = rect(&mut q, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut q, 150.0, 0.0, 60.0, 60.0);
        let t = rect(&mut q, 500.0, 0.0, 60.0, 60.0);
        link(&mut q, &a, &t);
        link(&mut q, &b, &t);
        assert!(issues_of(&q, "shared_segment").is_empty());
    }

    #[test]
    fn a_label_on_a_shape_or_on_another_label_collides() {
        let mut p = Page::new("t");
        let a = rect(&mut p, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut p, 400.0, 0.0, 60.0, 60.0);
        let blocker = rect(&mut p, 200.0, 10.0, 60.0, 40.0);
        let k = link_with(&mut p, &a, &b, Route::Straight, Some("flow"));
        let r = issues_of(&p, "label_collision");
        assert!(r.iter().any(|i| i.ids == vec![k.clone(), blocker.clone()]), "{r:?}");
        // two labels on top of each other (parallel connectors)
        let mut q = Page::new("t");
        let a = rect(&mut q, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut q, 400.0, 0.0, 60.0, 60.0);
        let k1 = link_with(&mut q, &a, &b, Route::Straight, Some("one"));
        let k2 = link_with(&mut q, &b, &a, Route::Straight, Some("two"));
        assert!(issues_of(&q, "label_collision").iter().any(|i| i.ids == vec![k1.clone(), k2.clone()]));
        // a clean label
        let mut c = Page::new("t");
        let a = rect(&mut c, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut c, 400.0, 0.0, 60.0, 60.0);
        link_with(&mut c, &a, &b, Route::Straight, Some("flow"));
        assert!(issues_of(&c, "label_collision").is_empty());
    }

    fn tune(p: &mut Page, id: &Id, set: impl FnOnce(&mut Connector)) {
        let i = p.index_of(id).unwrap();
        let Kind::Connector(c) = &mut p.objects[i].kind else { panic!("connector") };
        set(c);
    }

    #[test]
    fn the_label_collision_uses_the_position_and_the_offset_of_the_label() {
        let mut p = Page::new("t");
        let a = rect(&mut p, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut p, 400.0, 0.0, 60.0, 60.0);
        let blocker = rect(&mut p, 200.0, 10.0, 60.0, 40.0);
        let k = link_with(&mut p, &a, &b, Route::Straight, Some("flow"));
        assert!(issues_of(&p, "label_collision").iter().any(|i| i.ids == vec![k.clone(), blocker.clone()]));
        // moved along the arrow, away from the blocker in the middle
        tune(&mut p, &k, |c| c.set_label_pos(Some(0.2)));
        assert!(issues_of(&p, "label_collision").is_empty(), "{:?}", issues_of(&p, "label_collision"));
        // back at the middle but beside the line, below the blocker
        tune(&mut p, &k, |c| {
            c.set_label_pos(None);
            c.set_label_offset(60.0);
        });
        assert!(issues_of(&p, "label_collision").is_empty(), "{:?}", issues_of(&p, "label_collision"));
        // and a label moved back onto the blocker collides again
        tune(&mut p, &k, |c| c.set_label_offset(0.0));
        assert_eq!(issues_of(&p, "label_collision").len(), 1);
    }

    #[test]
    fn elbows_fanning_out_from_one_source_are_not_a_shared_segment() {
        let mut p = Page::new("t");
        let src = rect(&mut p, 0.0, 200.0, 100.0, 60.0);
        for y in [0.0, 100.0, 300.0, 400.0] {
            let t = rect(&mut p, 400.0, y, 100.0, 60.0);
            link_with(&mut p, &src, &t, Route::Elbow, None);
        }
        assert!(issues_of(&p, "shared_segment").is_empty(), "{:?}", issues_of(&p, "shared_segment"));
        assert!(issues_of(&p, "connector_crossing").is_empty(), "{:?}", issues_of(&p, "connector_crossing"));
        // the same elbows from different sources do not share a segment either
        let mut q = Page::new("t");
        let a1 = rect(&mut q, 0.0, 0.0, 100.0, 60.0);
        let a2 = rect(&mut q, 0.0, 100.0, 100.0, 60.0);
        let b1 = rect(&mut q, 400.0, 150.0, 100.0, 60.0);
        let b2 = rect(&mut q, 400.0, 250.0, 100.0, 60.0);
        link_with(&mut q, &a1, &b1, Route::Elbow, None);
        link_with(&mut q, &a2, &b2, Route::Elbow, None);
        assert!(issues_of(&q, "shared_segment").is_empty(), "{:?}", issues_of(&q, "shared_segment"));
        assert!(issues_of(&q, "connector_crossing").is_empty(), "{:?}", issues_of(&q, "connector_crossing"));
    }

    #[test]
    fn duplicate_and_unbound_connectors() {
        let mut p = Page::new("t");
        let a = rect(&mut p, 0.0, 0.0, 60.0, 60.0);
        let b = rect(&mut p, 400.0, 0.0, 60.0, 60.0);
        let k1 = link(&mut p, &a, &b);
        let k2 = link(&mut p, &b, &a);
        let free = Connector::new(End::bound(a.clone()), End::free(Pt::new(200.0, 300.0)));
        let o = Object::new(Kind::Connector(free), Style::default());
        let kf = o.id.clone();
        p.objects.push(o);
        let d = issues_of(&p, "duplicate_connector");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].ids, vec![k1, k2]);
        let u = issues_of(&p, "unbound_connector");
        assert_eq!(u.len(), 1, "{u:?}");
        assert_eq!(u[0].ids, vec![kf]);
        assert_eq!(u[0].details[0].1, json!("end"));
    }

    #[test]
    fn small_text_follows_the_export_width() {
        let mut p = Page::new("t");
        let a = shape(&mut p, BoxGeom::Rect, "A", 0.0, 0.0, 200.0, 100.0);
        let b = shape(&mut p, BoxGeom::Rect, "B", 5800.0, 0.0, 200.0, 100.0);
        link(&mut p, &a, &b);
        // 16 units on a 6000 wide content exported at 1600 px: 4.3 px
        let r = check(&p, &Options::default());
        let s: Vec<&Issue> = r.issues.iter().filter(|i| i.kind == "small_text").collect();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].severity, Severity::Warning);
        assert!((r.metrics.min_text_px.unwrap() - 16.0 * 1600.0 / 6000.0).abs() < 0.5, "{:?}", r.metrics);
        assert!(s[0].ids.contains(&a) && s[0].ids.contains(&b));
        // at 6000 px the same text is 16 px
        let r = check(&p, &Options { export_width: 6000.0, ids: None });
        assert!(r.issues.iter().all(|i| i.kind != "small_text"));
    }

    #[test]
    fn unbalanced_aspect_and_empty_cells() {
        let mut p = Page::new("t");
        for i in 0..6 {
            shape(&mut p, BoxGeom::Rect, "n", f64::from(i) * 150.0, 0.0, 100.0, 60.0);
        }
        let u = issues_of(&p, "unbalanced");
        assert!(u.iter().any(|i| i.details[0].1 == json!("aspect")), "{u:?}");
        // a 2x2 block of nodes at two opposite corners of a large page: empty cells but a balanced aspect
        let mut q = Page::new("t");
        for (x, y) in [(0.0, 0.0), (120.0, 0.0), (0.0, 80.0), (120.0, 80.0), (880.0, 640.0), (1000.0, 640.0), (880.0, 720.0), (1000.0, 720.0)] {
            shape(&mut q, BoxGeom::Rect, "n", x, y, 100.0, 60.0);
        }
        let r = check(&q, &Options::default());
        let u: Vec<&Issue> = r.issues.iter().filter(|i| i.kind == "unbalanced").collect();
        // two far apart clusters also leave most of the span uncovered
        assert_eq!(u.len(), 2, "{:?}", r.issues);
        assert_eq!(u[0].details[0].1, json!("empty_region"));
        assert_eq!(u[1].details[0].1, json!("coverage"));
        assert_eq!(r.metrics.empty_cells, 7);
        // fewer than 6 objects: only the aspect is judged
        let mut s = Page::new("t");
        shape(&mut s, BoxGeom::Rect, "n", 0.0, 0.0, 100.0, 60.0);
        shape(&mut s, BoxGeom::Rect, "n", 800.0, 600.0, 100.0, 60.0);
        assert_eq!(check(&s, &Options::default()).metrics.empty_cells, 0);
    }

    fn frame_with(p: &mut Page, x: f64, y: f64, w: f64, h: f64, inner: (f64, f64, f64, f64)) -> Id {
        let f = rect(p, x, y, w, h);
        rect(p, inner.0, inner.1, inner.2, inner.3);
        f
    }

    #[test]
    fn a_mostly_empty_frame_is_sparse_and_a_snug_one_is_not() {
        let mut p = Page::new("t");
        let snug = frame_with(&mut p, 0.0, 0.0, 280.0, 180.0, (40.0, 40.0, 200.0, 100.0));
        let loose = frame_with(&mut p, 0.0, 400.0, 900.0, 600.0, (40.0, 440.0, 200.0, 100.0));
        let wide = frame_with(&mut p, 1200.0, 0.0, 800.0, 200.0, (1210.0, 10.0, 360.0, 180.0));
        let s = issues_of(&p, "sparse_frame");
        assert!(s.iter().all(|i| i.ids != vec![snug.clone()]), "{s:?}");
        let by = |id: &Id| s.iter().find(|i| &i.ids[0] == id);
        assert!(by(&loose).unwrap().message.contains("add_frame with frame="));
        // dense (coverage 0.45) but a 220 wide empty band on the right
        assert_eq!(by(&wide).unwrap().details.iter().find(|d| d.0 == "band_side").unwrap().1, json!("right"));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn frames_side_by_side_at_different_heights_are_a_staircase_but_a_grid_is_not() {
        let mut p = Page::new("t");
        let a = frame_with(&mut p, 0.0, 0.0, 300.0, 300.0, (40.0, 40.0, 200.0, 200.0));
        let b = frame_with(&mut p, 400.0, 100.0, 300.0, 300.0, (440.0, 140.0, 200.0, 200.0));
        let r = check(&p, &Options::default());
        let u: Vec<&Issue> = r.issues.iter().filter(|i| i.kind == "unbalanced" && i.details[0].1 == json!("staircase")).collect();
        assert_eq!(u.len(), 1, "{:?}", r.issues);
        assert_eq!(u[0].ids, vec![a, b]);
        assert!((r.metrics.frame_top_offset - 100.0).abs() < 1e-9);
        // same tops: fine; a second row (no vertical overlap): fine
        let mut q = Page::new("t");
        frame_with(&mut q, 0.0, 0.0, 300.0, 300.0, (40.0, 40.0, 200.0, 200.0));
        frame_with(&mut q, 400.0, 0.0, 300.0, 300.0, (440.0, 40.0, 200.0, 200.0));
        frame_with(&mut q, 400.0, 400.0, 300.0, 300.0, (440.0, 440.0, 200.0, 200.0));
        let r = check(&q, &Options::default());
        assert!(r.issues.iter().all(|i| i.details.first().is_none_or(|d| d.1 != json!("staircase"))), "{:?}", r.issues);
        assert_eq!(r.metrics.frame_top_offset, 0.0);
    }

    #[test]
    fn a_diagonal_scatter_covers_too_little_of_its_box() {
        let mut p = Page::new("t");
        for i in 0..4 {
            rect(&mut p, f64::from(i) * 300.0, f64::from(i) * 250.0, 100.0, 100.0);
        }
        let r = check(&p, &Options::default());
        assert!(r.issues.iter().any(|i| i.kind == "unbalanced" && i.details[0].1 == json!("coverage")), "{:?}", r.issues);
        assert!(r.metrics.coverage.unwrap() < COVERAGE_MIN);
    }

    #[test]
    fn ids_restrict_the_issues_to_what_touches_them() {
        let mut p = Page::new("t");
        let a = rect(&mut p, 0.0, 0.0, 100.0, 100.0);
        let b = rect(&mut p, 50.0, 50.0, 100.0, 100.0);
        rect(&mut p, 1000.0, 0.0, 100.0, 100.0);
        rect(&mut p, 1050.0, 50.0, 100.0, 100.0);
        let all = check(&p, &Options::default());
        assert_eq!(all.issues.iter().filter(|i| i.kind == "overlap").count(), 2);
        let only = check(&p, &Options { export_width: DEFAULT_EXPORT_WIDTH, ids: Some(std::slice::from_ref(&a)) });
        assert_eq!(only.issues.len(), 1);
        assert_eq!(only.issues[0].ids, vec![a, b]);
        assert_eq!(only.metrics.objects, 4);
    }

    #[test]
    fn issues_of_one_kind_are_capped() {
        let mut p = Page::new("t");
        for i in 0..80 {
            rect(&mut p, f64::from(i) * 3000.0, 0.0, 100.0, 100.0);
            rect(&mut p, f64::from(i) * 3000.0 + 50.0, 50.0, 100.0, 100.0);
        }
        let r = check(&p, &Options::default());
        assert_eq!(r.issues.iter().filter(|i| i.kind == "overlap").count(), MAX_PER_KIND);
        assert!(r.truncated);
    }
}
