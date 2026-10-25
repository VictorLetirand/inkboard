//! Engine invariants checked on random documents (0 to 200 objects): undo/redo, saving, bound connectors, elbow routing,
//! spatial index, handle transformations.

use inkboard_core::connector::{Anchor, Connector, End, Route};
use inkboard_core::editor::{AlignMode, Axis, Editor, Order};
use inkboard_core::geom::{Frame, Pt, Rect, seg_dist};
use inkboard_core::model::{Background, BoxObj, Document, Id, ImageObj, Kind, Label, Layer, Object, Outline, Page, Table};
use inkboard_core::num;
use inkboard_core::spatial::{SpatialIndex, id_index};
use inkboard_core::style::{Align, Color, Style};
use inkboard_core::transform::{Handle, resize_frame, scale_objects};
use inkboard_core::{BgPreset, BoxGeom, Pattern, serial};
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::RngSeed;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn config() -> ProptestConfig {
    // no regression file written into the repository: the failing case is printed
    let mut config = ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() };
    // fixed seed, so that the CI run is reproducible; set PROPTEST_RNG_SEED=<number> to explore other inputs
    if matches!(config.rng_seed, RngSeed::Random) {
        config.rng_seed = RngSeed::Fixed(2026);
    }
    config
}

/* ------------------------------------------------------ Random documents */

const BOX_GEOMS: [BoxGeom; 6] = [BoxGeom::Rect, BoxGeom::Ellipse, BoxGeom::Diamond, BoxGeom::Triangle, BoxGeom::Sticky, BoxGeom::Text];
const PATTERNS: [Pattern; 4] = [Pattern::None, Pattern::Dots, Pattern::Grid, Pattern::Lines];
const ROUTES: [Route; 3] = [Route::Straight, Route::Elbow, Route::Curve];

#[derive(Clone, Debug)]
struct EndSpec {
    target: Index,
    free: bool,
    frac: Option<(f64, f64)>,
}

#[derive(Clone, Debug)]
enum Shape {
    Box(BoxGeom),
    Image { latex: Option<String>, mono: bool, data: Vec<u8> },
    Table { rows: usize, cols: usize },
    Pen { pts: Vec<(f64, f64)>, pressures: bool, highlighter: bool },
    Connector { start: EndSpec, end: EndSpec, via: Vec<(f64, f64)>, route: Route, label: Option<String> },
}

#[derive(Clone, Debug)]
struct Spec {
    shape: Shape,
    frame: Frame,
    width: f64,
    opacity: f32,
    fill: Option<(u8, u8, u8)>,
    text: String,
    group: Option<u8>,
    locked: bool,
    layer2: bool,
}

impl Spec {
    fn attachable(&self) -> bool {
        matches!(self.shape, Shape::Box(_) | Shape::Image { .. } | Shape::Table { .. })
    }
}

#[derive(Clone, Debug)]
struct DocSpec {
    name: String,
    preset: BgPreset,
    pattern: Pattern,
    pages: usize,
    layer2_visible: bool,
    layer2_locked: bool,
    objs: Vec<Spec>,
}

fn end_spec() -> impl Strategy<Value = EndSpec> {
    (any::<Index>(), prop::bool::weighted(0.15), prop::option::weighted(0.3, (0.0..=1.0f64, 0.0..=1.0f64))).prop_map(|(target, free, frac)| EndSpec {
        target,
        free,
        frac,
    })
}

fn shape() -> impl Strategy<Value = Shape> {
    let offsets = prop::collection::vec((-200.0..200.0f64, -200.0..200.0f64), 0..3);
    prop_oneof![
        6 => prop::sample::select(BOX_GEOMS.to_vec()).prop_map(Shape::Box),
        1 => (prop::option::of("[a-z]{1,6}"), any::<bool>(), prop::collection::vec(any::<u8>(), 0..32))
            .prop_map(|(latex, mono, data)| Shape::Image { latex, mono, data }),
        1 => (1usize..4, 1usize..4).prop_map(|(rows, cols)| Shape::Table { rows, cols }),
        1 => (prop::collection::vec((-200.0..200.0f64, -200.0..200.0f64), 1..8), any::<bool>(), any::<bool>())
            .prop_map(|(pts, pressures, highlighter)| Shape::Pen { pts, pressures, highlighter }),
        4 => (end_spec(), end_spec(), offsets, prop::sample::select(ROUTES.to_vec()), prop::option::of("(?s).{0,6}"))
            .prop_map(|(start, end, via, route, label)| Shape::Connector { start, end, via, route, label }),
    ]
}

fn spec() -> impl Strategy<Value = Spec> {
    // sizes are mostly small; a few objects are huge (they end up in the "large" list of the spatial index)
    let size = prop_oneof![20 => 10.0..400.0f64, 1 => 400.0..100_000.0f64];
    let frame = (-3000.0..3000.0f64, -3000.0..3000.0f64, size.clone(), size, -3.2..3.2f64).prop_map(|(x, y, w, h, angle)| Frame { x, y, w, h, angle });
    let look = (0.0..6.0f64, 0.0..=1.0f32, prop::option::of((any::<u8>(), any::<u8>(), any::<u8>())), "(?s).{0,12}");
    let flags = (prop::option::of(0u8..4), prop::bool::weighted(0.1), prop::bool::weighted(0.2));
    (shape(), frame, look, flags).prop_map(|(shape, frame, (width, opacity, fill, text), (group, locked, layer2))| Spec {
        shape,
        frame,
        width,
        opacity,
        fill,
        text,
        group,
        locked,
        layer2,
    })
}

fn doc_spec(max_objects: usize) -> impl Strategy<Value = DocSpec> {
    (
        "(?s).{0,12}",
        prop::sample::select(BgPreset::ALL.to_vec()),
        prop::sample::select(PATTERNS.to_vec()),
        1usize..=3,
        (any::<bool>(), prop::bool::weighted(0.3)),
        prop::collection::vec(spec(), 0..=max_objects),
    )
        .prop_map(|(name, preset, pattern, pages, (layer2_visible, layer2_locked), objs)| DocSpec {
            name,
            preset,
            pattern,
            pages,
            layer2_visible,
            layer2_locked,
            objs,
        })
}

fn end_of(e: &EndSpec, attach: &[Id], at: Pt) -> End {
    if e.free || attach.is_empty() {
        return End::free(at);
    }
    let target = attach[e.target.index(attach.len())].clone();
    End::Bound { target, anchor: e.frac.map_or(Anchor::Auto, |(fx, fy)| Anchor::Frac { fx, fy }) }
}

/// Builds an object; connectors are bound to the ids of `attach`.
fn object(s: &Spec, id: Id, attach: &[Id], doc: &mut Document) -> Object {
    let style = Style { fill: s.fill.map(|(r, g, b)| Color::rgb(r, g, b)), width: s.width, opacity: s.opacity, ..Style::default() };
    let f = s.frame;
    let kind = match &s.shape {
        Shape::Box(geom) => Kind::Box(BoxObj { geom: *geom, frame: f, label: Label::new(&s.text, 20.0, Align::Center), lang: None }),
        Shape::Image { latex, mono, data } => {
            Kind::Image(ImageObj { frame: f, asset: doc.add_asset("image/png", data.clone()), latex: latex.clone(), mono: *mono })
        }
        Shape::Table { rows, cols } => {
            let mut t = Table::new(Pt::new(f.x, f.y), *rows, *cols);
            t.col_w[0] = f.w.clamp(40.0, 400.0);
            if let Some(c) = t.cell_mut(0, 0) {
                c.set_plain(&s.text);
            }
            Kind::Table(t)
        }
        Shape::Pen { pts, pressures, highlighter } => {
            let pts: Vec<Pt> = pts.iter().map(|(dx, dy)| Pt::new(f.x + dx, f.y + dy)).collect();
            if *pressures {
                let p = (0..pts.len()).map(|i| num::f32_of(i) / 8.0).collect();
                return Object { id, ..Object::new_pen_with_pressure(pts, p, *highlighter, style) };
            }
            return Object { id, ..Object::new_pen(pts, *highlighter, style) };
        }
        Shape::Connector { start, end, via, route, label } => {
            let mut c = Connector::new(end_of(start, attach, Pt::new(f.x, f.y)), end_of(end, attach, Pt::new(f.x + f.w, f.y + f.h)));
            c.via = via.iter().map(|(dx, dy)| Pt::new(f.x + dx, f.y + dy)).collect();
            c.route = *route;
            c.label = label.as_deref().map(|t| Label::new(t, 16.0, Align::Center));
            Kind::Connector(c)
        }
    };
    let layer = Id::from(if s.layer2 { "l2" } else { "main" });
    Object { id, kind, style, group: s.group.map(|g| Id(format!("g{g}"))), locked: s.locked, layer }
}

/// Deterministic ids (`p<page>`, `o<index>`), so a failing case is reproducible from the printed spec.
fn build(spec: &DocSpec) -> Document {
    let mut doc = Document::new(&spec.name);
    doc.background = Background { preset: spec.preset, pattern: spec.pattern };
    doc.pages[0].id = Id::from("p0");
    for p in 1..spec.pages {
        doc.pages.push(Page { id: Id(format!("p{p}")), ..Page::new(&format!("Page {}", p + 1)) });
    }
    for page in &mut doc.pages {
        page.layers.push(Layer { id: Id::from("l2"), name: "Layer 2".into(), visible: spec.layer2_visible, locked: spec.layer2_locked });
    }
    let attachable: Vec<Vec<Id>> = (0..spec.pages)
        .map(|p| spec.objs.iter().enumerate().filter(|(i, s)| i % spec.pages == p && s.attachable()).map(|(i, _)| Id(format!("o{i}"))).collect())
        .collect();
    for (i, s) in spec.objs.iter().enumerate() {
        let p = i % spec.pages;
        let obj = object(s, Id(format!("o{i}")), &attachable[p], &mut doc);
        doc.pages[p].objects.push(obj);
    }
    doc
}

/* ------------------------------------------------------------ 1. History */

#[derive(Clone, Debug)]
enum EditOp {
    Insert(Spec),
    Remove(Vec<Index>),
    Translate(Vec<Index>, f64, f64),
    Duplicate(Vec<Index>, f64, f64),
    Connect(Index, Index),
    Reorder(Vec<Index>, Order),
    Group(Vec<Index>),
    Ungroup(Vec<Index>),
    Lock(Vec<Index>, bool),
    Align(Vec<Index>, AlignMode),
    Distribute(Vec<Index>, Axis),
    Restyle(Index, f64),
    AddPage(String),
    RemovePage(Index),
    RenamePage(Index, String),
    SetBackground(BgPreset, Pattern),
    SetName(String),
    SetLayers(bool),
    /// Several modifications in one undo step.
    Transact(Vec<Index>, f64),
}

fn edit_op() -> impl Strategy<Value = EditOp> {
    let ids = || prop::collection::vec(any::<Index>(), 0..6);
    // no image or connector inserted from scratch: an asset added to the document is not part of the history
    let plain = spec().prop_map(|mut s| {
        if matches!(s.shape, Shape::Image { .. } | Shape::Connector { .. }) {
            s.shape = Shape::Box(BoxGeom::Rect);
        }
        s
    });
    let orders = vec![Order::Front, Order::Forward, Order::Backward, Order::Back];
    let aligns = vec![AlignMode::Left, AlignMode::CenterX, AlignMode::Right, AlignMode::Top, AlignMode::CenterY, AlignMode::Bottom];
    let delta = || -500.0..500.0f64;
    prop_oneof![
        4 => plain.prop_map(EditOp::Insert),
        3 => ids().prop_map(EditOp::Remove),
        4 => (ids(), delta(), delta()).prop_map(|(i, x, y)| EditOp::Translate(i, x, y)),
        2 => (ids(), delta(), delta()).prop_map(|(i, x, y)| EditOp::Duplicate(i, x, y)),
        2 => (any::<Index>(), any::<Index>()).prop_map(|(a, b)| EditOp::Connect(a, b)),
        2 => (ids(), prop::sample::select(orders)).prop_map(|(i, o)| EditOp::Reorder(i, o)),
        1 => ids().prop_map(EditOp::Group),
        1 => ids().prop_map(EditOp::Ungroup),
        1 => (ids(), any::<bool>()).prop_map(|(i, l)| EditOp::Lock(i, l)),
        2 => (ids(), prop::sample::select(aligns)).prop_map(|(i, m)| EditOp::Align(i, m)),
        1 => (ids(), prop::sample::select(vec![Axis::Horizontal, Axis::Vertical])).prop_map(|(i, a)| EditOp::Distribute(i, a)),
        2 => (any::<Index>(), 0.0..8.0f64).prop_map(|(i, w)| EditOp::Restyle(i, w)),
        1 => "(?s).{0,8}".prop_map(EditOp::AddPage),
        1 => any::<Index>().prop_map(EditOp::RemovePage),
        1 => (any::<Index>(), "(?s).{0,8}").prop_map(|(i, n)| EditOp::RenamePage(i, n)),
        1 => (prop::sample::select(BgPreset::ALL.to_vec()), prop::sample::select(PATTERNS.to_vec())).prop_map(|(b, p)| EditOp::SetBackground(b, p)),
        1 => "(?s).{0,8}".prop_map(EditOp::SetName),
        1 => any::<bool>().prop_map(EditOp::SetLayers),
        2 => (ids(), delta()).prop_map(|(i, d)| EditOp::Transact(i, d)),
    ]
}

fn pick(ed: &Editor, ixs: &[Index]) -> Vec<Id> {
    let objs = &ed.page().objects;
    if objs.is_empty() {
        return Vec::new();
    }
    ixs.iter().map(|ix| objs[ix.index(objs.len())].id.clone()).collect()
}

fn apply(ed: &mut Editor, op: &EditOp, step: usize) {
    match op {
        EditOp::Insert(s) => {
            let o = object(s, Id(format!("n{step}")), &[], &mut Document::default());
            ed.insert(o);
        }
        EditOp::Remove(i) => ed.remove(&pick(ed, i)),
        EditOp::Translate(i, x, y) => ed.translate(&pick(ed, i), Pt::new(*x, *y)),
        EditOp::Duplicate(i, x, y) => {
            ed.duplicate(&pick(ed, i), Pt::new(*x, *y));
        }
        EditOp::Connect(a, b) => {
            let ids: Vec<Id> = ed.page().objects.iter().filter(|o| o.is_attachable()).map(|o| o.id.clone()).collect();
            if !ids.is_empty() {
                ed.connect(&ids[a.index(ids.len())], &ids[b.index(ids.len())]);
            }
        }
        EditOp::Reorder(i, m) => ed.reorder(&pick(ed, i), *m),
        EditOp::Group(i) => ed.group(&pick(ed, i)),
        EditOp::Ungroup(i) => ed.ungroup(&pick(ed, i)),
        EditOp::Lock(i, l) => ed.set_locked(&pick(ed, i), *l),
        EditOp::Align(i, m) => ed.align(&pick(ed, i), *m),
        EditOp::Distribute(i, a) => ed.distribute(&pick(ed, i), *a),
        EditOp::Restyle(i, w) => {
            if let Some(id) = pick(ed, std::slice::from_ref(i)).first() {
                ed.update(id, |o| o.style.width = *w);
            }
        }
        EditOp::AddPage(n) => {
            ed.add_page(n);
        }
        EditOp::RemovePage(i) => {
            let id = ed.doc.pages[i.index(ed.doc.pages.len())].id.clone();
            ed.remove_page(&id);
        }
        EditOp::RenamePage(i, n) => {
            let id = ed.doc.pages[i.index(ed.doc.pages.len())].id.clone();
            ed.rename_page(&id, n);
        }
        EditOp::SetBackground(preset, pattern) => ed.set_background(Background { preset: *preset, pattern: *pattern }),
        EditOp::SetName(n) => ed.set_name(n),
        EditOp::SetLayers(visible) => {
            ed.set_layers(vec![Layer::main(), Layer { id: Id::from("l2"), name: "Layer 2".into(), visible: *visible, locked: false }]);
        }
        EditOp::Transact(i, d) => {
            let ids = pick(ed, i);
            ed.transact(|e| {
                e.translate(&ids, Pt::new(*d, *d));
                e.remove(&ids[..ids.len().min(1)]);
            });
        }
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn undoing_everything_restores_the_initial_document_and_redoing_the_final_one(doc in doc_spec(200), ops in prop::collection::vec(edit_op(), 0..25)) {
        let initial = build(&doc);
        let mut ed = Editor::new(initial.clone());
        for (step, op) in ops.iter().enumerate() {
            apply(&mut ed, op, step);
        }
        let last = ed.doc.clone();
        let mut undone = 0;
        while ed.undo() {
            undone += 1;
        }
        prop_assert!(!ed.can_undo());
        prop_assert_eq!(&ed.doc, &initial, "after {} undo steps", undone);
        let mut redone = 0;
        while ed.redo() {
            redone += 1;
        }
        prop_assert_eq!(redone, undone);
        prop_assert_eq!(&ed.doc, &last);
    }
}

/* ------------------------------------------------------- 2. Serialization */

/// Folder under the test target directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("properties-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).expect("create the temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assert_round_trip(doc: &Document) -> Result<(), TestCaseError> {
    let dir = TempDir::new();
    let path = dir.0.join("board.inkboard.json");
    serial::save(doc, &path).expect("save");
    prop_assert!(!path.with_extension("tmp-save").exists(), "the temporary file is renamed");
    let back = serial::load(&path).expect("load");
    prop_assert_eq!(&back, doc);
    // the in-memory path (used by the MCP server and the exports) too, compact and pretty
    prop_assert_eq!(&serial::from_json(&serial::to_json(doc, true)).expect("pretty"), doc);
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn saving_then_loading_gives_an_equal_document(doc in doc_spec(200)) {
        assert_round_trip(&build(&doc))?;
    }
}

/* ------------------------------------------------------ 3. Bound connectors */

/// Distance from a world point to the outline of an attachable object.
fn contour_distance(o: &Object, p: Pt) -> f64 {
    let f = o.frame().expect("attachable object");
    let outline = match &o.kind {
        Kind::Box(b) => b.geom.outline(),
        _ => Outline::Rect,
    };
    let local = f.to_local(p);
    match outline {
        Outline::Ellipse => {
            // first-order distance to the implicit curve (x/a)^2 + (y/b)^2 = 1
            let (rx, ry) = (f.w / 2.0, f.h / 2.0);
            let value = (local.x / rx).powi(2) + (local.y / ry).powi(2) - 1.0;
            value.abs() / (2.0 * (local.x / (rx * rx)).hypot(local.y / (ry * ry))).max(1e-12)
        }
        other => {
            let poly = other.polygon(f.w, f.h);
            (0..poly.len()).map(|i| seg_dist(local, poly[i], poly[(i + 1) % poly.len()])).fold(f64::INFINITY, f64::min)
        }
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn bound_connector_ends_stay_on_their_shape(
        doc in doc_spec(200),
        // moves, rotations and resizing of some shapes: (shape, dx, dy, angle, scale)
        moves in prop::collection::vec((any::<Index>(), -800.0..800.0f64, -800.0..800.0f64, prop::option::of(-3.2..3.2f64), prop::option::of((0.3..3.0f64, 0.3..3.0f64))), 0..30),
    ) {
        let mut ed = Editor::new(build(&doc));
        let shapes: Vec<Id> = ed.page().objects.iter().filter(|o| o.is_attachable()).map(|o| o.id.clone()).collect();
        for (ix, dx, dy, angle, scale) in &moves {
            if shapes.is_empty() {
                break;
            }
            ed.update(&shapes[ix.index(shapes.len())], |o| {
                o.translate(Pt::new(*dx, *dy));
                let c = o.frame().expect("attachable").center();
                if let Some(a) = angle {
                    o.rotate_about(c, *a);
                }
                if let Some((sx, sy)) = scale {
                    o.scale_about(c, *sx, *sy);
                }
            });
        }
        let page = ed.page();
        for o in &page.objects {
            let Kind::Connector(c) = &o.kind else { continue };
            let path = page.connector_path(c);
            prop_assert!(path.as_ref().is_some_and(|p| p.len() >= 2), "no path for {:?}", c);
            let path = path.unwrap_or_default();
            for (end, at) in [(&c.start, path[0]), (&c.end, path[path.len() - 1])] {
                let End::Bound { target, anchor } = end else { continue };
                let t = page.get(target).expect("the target exists");
                match anchor {
                    Anchor::Auto => {
                        let d = contour_distance(t, at);
                        prop_assert!(d <= 0.5, "end {:?} is {} px from the outline of {:?}", at, d, t.kind);
                    }
                    // a fractional anchor is a position in the box (not necessarily on the outline of an ellipse, a diamond…)
                    Anchor::Frac { fx, fy } => {
                        let f = t.frame().expect("attachable");
                        let want = f.to_world(Pt::new((fx - 0.5) * f.w, (fy - 0.5) * f.h));
                        prop_assert!(want.dist(at) <= 0.5, "end {:?} should be at {:?}", at, want);
                    }
                }
            }
        }
    }
}

/* ------------------------------------------------------- 4. Elbow routing */

const CELL: f64 = 400.0;

/// One shape per cell of a grid, placed in the middle of its cell (+/- 20), so that a free corridor always exists.
#[derive(Clone, Debug)]
struct Cell {
    geom: Option<BoxGeom>, // `None`: a table
    rows: usize,
    size: (f64, f64),
    shift: (f64, f64),
    angle: f64,
}

fn cell() -> impl Strategy<Value = Cell> {
    let geoms = vec![Some(BoxGeom::Rect), Some(BoxGeom::Ellipse), Some(BoxGeom::Diamond), Some(BoxGeom::Sticky), Some(BoxGeom::Triangle), None];
    (prop::sample::select(geoms), 1usize..3, (20.0..120.0f64, 20.0..120.0f64), (-20.0..20.0f64, -20.0..20.0f64), -3.2..3.2f64)
        .prop_map(|(geom, rows, size, shift, angle)| Cell { geom, rows, size, shift, angle })
}

fn scene() -> impl Strategy<Value = (Vec<Cell>, Vec<(Index, Index)>)> {
    (prop::collection::vec(cell(), 2..=24), prop::collection::vec((any::<Index>(), any::<Index>()), 1..10))
}

/// Grid page with elbow connectors; the shapes are rotated by their `angle` only when `rotated`.
fn scene_page(cells: &[Cell], links: &[(Index, Index)], rotated: bool) -> Page {
    let mut page = Page::new("scene");
    for (i, c) in cells.iter().enumerate() {
        let (cx, cy) = (num::f64_of(i % 6) * CELL + CELL / 2.0 + c.shift.0, num::f64_of(i / 6) * CELL + CELL / 2.0 + c.shift.1);
        let kind = match c.geom {
            Some(geom) => {
                let frame = Frame { x: cx - c.size.0 / 2.0, y: cy - c.size.1 / 2.0, w: c.size.0, h: c.size.1, angle: if rotated { c.angle } else { 0.0 } };
                Kind::Box(BoxObj { geom, frame, label: Label::default(), lang: None })
            }
            None => Kind::Table(Table::new(Pt::new(cx - 70.0, cy - 22.0 * num::f64_of(c.rows)), c.rows, 1)),
        };
        page.objects.push(Object { id: Id(format!("c{i}")), ..Object::new(kind, Style::default()) });
    }
    for (a, b) in links {
        let (a, b) = (a.index(cells.len()), b.index(cells.len()));
        if a != b {
            let mut conn = Connector::new(End::bound(Id(format!("c{a}"))), End::bound(Id(format!("c{b}"))));
            conn.route = Route::Elbow;
            page.objects.push(Object::new(Kind::Connector(conn), Style::default()));
        }
    }
    page
}

/// Does the segment enter the inside of the rectangle? (a segment running along an edge does not)
fn enters(p: Pt, q: Pt, r: Rect) -> bool {
    const E: f64 = 1e-6;
    let (x0, x1, y0, y1) = (r.x + E, r.right() - E, r.y + E, r.bottom() - E);
    if x0 >= x1 || y0 >= y1 {
        return false;
    }
    // Liang-Barsky clipping of the segment against the shrunk rectangle
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let delta = q - p;
    for (dir, room) in [(-delta.x, p.x - x0), (delta.x, x1 - p.x), (-delta.y, p.y - y0), (delta.y, y1 - p.y)] {
        if dir == 0.0 {
            if room < 0.0 {
                return false;
            }
        } else {
            let t = room / dir;
            if dir < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    t0 <= t1
}

/// Returns a description of the first connector segment that goes through an unrelated shape.
fn first_crossing(page: &Page) -> Option<String> {
    for o in &page.objects {
        let Kind::Connector(c) = &o.kind else { continue };
        let path = page.connector_path(c)?;
        let ends: Vec<&Id> = c.targets().collect();
        for other in page.objects.iter().filter(|x| x.is_attachable() && !ends.contains(&&x.id)) {
            // the router treats a shape as its bounding box
            let r = other.frame()?.aabb();
            if let Some(w) = path.windows(2).find(|w| enters(w[0], w[1], r)) {
                return Some(format!("{:?} -> {:?} goes through {} {:?} (path {:?})", c.start, c.end, other.id, r, w));
            }
        }
    }
    None
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn elbow_connectors_go_around_unrelated_shapes((cells, links) in scene()) {
        // upright shapes, then the same scene with every shape rotated by its own angle
        for rotated in [false, true] {
            if let Some(why) = first_crossing(&scene_page(&cells, &links, rotated)) {
                prop_assert!(false, "rotated: {}: {}", rotated, why);
            }
        }
    }
}

/* ------------------------------------------------------- 5. Spatial index */

proptest! {
    #![proptest_config(config())]

    #[test]
    fn index_query_equals_exhaustive_search(
        doc in doc_spec(200),
        queries in prop::collection::vec((-4000.0..4000.0f64, -4000.0..4000.0f64, prop_oneof![3 => 0.0..1500.0f64, 1 => 1500.0..200_000.0f64], 0.0..1500.0f64), 1..12),
    ) {
        let doc = build(&doc);
        let page = &doc.pages[0];
        let ids = id_index(page);
        let idx = SpatialIndex::build(page, &ids);
        prop_assert_eq!(idx.len(), page.objects.len());
        for (x, y, w, h) in queries {
            let r = Rect::new(x, y, w, h);
            let want: Vec<usize> = (0..page.objects.len()).filter(|&i| page.bounds(&page.objects[i]).intersects(&r)).collect();
            prop_assert_eq!(idx.query(r), want, "{:?}", r);
            for contain in [false, true] {
                let mut want = page.objects_in_rect(r, contain);
                let mut got = page.objects_in_rect_idx(r, contain, &idx);
                want.sort();
                got.sort();
                prop_assert_eq!(got, want, "{:?} contain={}", r, contain);
            }
        }
    }
}

/* ------------------------------------------------------ 6. Transformations */

const TOL: f64 = 1e-6;

fn frame_numbers(f: &Frame) -> [f64; 5] {
    [f.x, f.y, f.w, f.h, f.angle]
}

/// Every coordinate of an object that a transformation moves.
fn numbers(o: &Object) -> Vec<f64> {
    match &o.kind {
        Kind::Box(b) => frame_numbers(&b.frame).to_vec(),
        Kind::Image(i) => frame_numbers(&i.frame).to_vec(),
        Kind::Pen(p) => p.pts.iter().flat_map(|q| [q.x, q.y]).collect(),
        Kind::Connector(c) => {
            let free = [&c.start, &c.end].into_iter().filter_map(|e| if let End::Free { at } = e { Some(*at) } else { None });
            free.chain(c.via.iter().copied()).flat_map(|q| [q.x, q.y]).collect()
        }
        Kind::Table(t) => [t.origin.x, t.origin.y].into_iter().chain(t.col_w.iter().copied()).chain(t.row_h.iter().copied()).collect(),
    }
}

fn same_numbers(a: &Object, b: &Object) -> bool {
    let (a, b) = (numbers(a), numbers(b));
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= TOL)
}

fn frame() -> impl Strategy<Value = Frame> {
    (-3000.0..3000.0f64, -3000.0..3000.0f64, 10.0..1500.0f64, 10.0..1500.0f64, -3.2..3.2f64).prop_map(|(x, y, w, h, angle)| Frame { x, y, w, h, angle })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn dragging_a_handle_then_dragging_it_back_restores_the_frame(
        f in frame(),
        h in prop::sample::select(Handle::ALL.to_vec()),
        (px, py) in (-4000.0..4000.0f64, -4000.0..4000.0f64),
        keep_ratio in any::<bool>(),
    ) {
        let min = 4.0;
        let moved = resize_frame(&f, h, Pt::new(px, py), keep_ratio, min);
        // a frame stopped by the minimum size forgets where the pointer was: not reversible
        prop_assume!(moved.w > min + 1e-6 && moved.h > min + 1e-6);
        let back = resize_frame(&moved, h, f.to_world(h.local(f.w, f.h)), keep_ratio, min);
        for (a, b) in frame_numbers(&back).into_iter().zip(frame_numbers(&f)) {
            prop_assert!((a - b).abs() <= TOL, "{:?} -> {:?} -> {:?}", f, moved, back);
        }
    }

    #[test]
    fn rotating_then_rotating_back_restores_every_object(doc in doc_spec(60), pivot in (-3000.0..3000.0f64, -3000.0..3000.0f64), angle in -6.3..6.3f64) {
        let page = build(&doc).pages.swap_remove(0);
        let c = Pt::new(pivot.0, pivot.1);
        for o in &page.objects {
            let mut r = o.clone();
            if r.rotate_about(c, angle) {
                prop_assert!(r.rotate_about(c, -angle));
            }
            prop_assert!(same_numbers(o, &r), "{:?} -> {:?}", o.kind, r.kind);
        }
    }

    #[test]
    fn scaling_to_a_box_then_back_restores_every_object(
        doc in doc_spec(60),
        from in (-3000.0..3000.0f64, -3000.0..3000.0f64, 10.0..2000.0f64, 10.0..2000.0f64),
        to_pos in (-3000.0..3000.0f64, -3000.0..3000.0f64),
        // tables never shrink below 20 x 16: the factors keep their cells above that
        (sx, sy) in (0.5..4.0f64, 0.5..4.0f64),
    ) {
        let from = Rect::new(from.0, from.1, from.2, from.3);
        let to = Rect::new(to_pos.0, to_pos.1, from.w * sx, from.h * sy);
        let original = build(&doc).pages.swap_remove(0).objects;
        let mut objs = original.clone();
        scale_objects(&mut objs, from, to);
        scale_objects(&mut objs, to, from);
        for (a, b) in original.iter().zip(&objs) {
            prop_assert!(same_numbers(a, b), "{:?} -> {:?}", a.kind, b.kind);
        }
    }
}
