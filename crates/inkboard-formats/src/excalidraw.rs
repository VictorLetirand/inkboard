//! Excalidraw (`.excalidraw`, JSON).
//!
//! Export: shapes + bound text ("containerId"), arrows with bindings, freehand strokes, images.
//! Import: same elements; a polygon or a polyline becomes a connector with waypoints.

use crate::common::{FormatError, Imported, Report, data_url, parse_color, parse_data_url};
use inkboard_core::geom::Rect;
use inkboard_core::num;
use inkboard_core::text::{Measurer, layout_rich};
use inkboard_core::{
    Align, Arrowhead, Asset, BgPreset, BoxGeom, BoxObj, Color, Connector, Dash, Document, End, FontFamily, Frame, HeadExt, Id, ImageObj, Kind, Label, Object,
    Page, PenObj, Pt, RichText, Route, Style, Table,
};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;

fn seed_of(id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h % 2_147_483_647
}

fn xfont(f: FontFamily) -> u8 {
    match f {
        FontFamily::Hand => 1,
        FontFamily::Mono => 3,
        _ => 2,
    }
}

fn xfont_back(n: i64) -> FontFamily {
    match n {
        1 | 5 | 7 => FontFamily::Hand,
        3 | 8 => FontFamily::Mono,
        9 => FontFamily::Serif,
        _ => FontFamily::Sans,
    }
}

fn xhead(h: Arrowhead, ext: Option<HeadExt>) -> Value {
    match (ext, h) {
        (Some(HeadExt::Diamond), _) => json!("diamond"),
        (Some(HeadExt::DiamondOpen), _) => json!("diamond_outline"),
        (Some(HeadExt::TriangleOpen), _) => json!("triangle_outline"),
        (Some(HeadExt::CrowFoot | HeadExt::CrowFootZero), _) => json!("crowfoot_many"),
        (Some(HeadExt::CrowFootOne), _) => json!("crowfoot_one_or_many"),
        (Some(HeadExt::OneOnly), _) => json!("crowfoot_one"),
        (Some(HeadExt::ZeroOrOne), _) => json!("circle_outline"),
        (None, h) => xhead_original(h),
    }
}

fn xhead_original(h: Arrowhead) -> Value {
    match h {
        Arrowhead::None => Value::Null,
        Arrowhead::Arrow => json!("arrow"),
        Arrowhead::Triangle => json!("triangle"),
        Arrowhead::Dot => json!("dot"),
        Arrowhead::Bar => json!("bar"),
    }
}

/// An arrowhead read back: an original one, or an extended one (set on the connector by `set`) when Excalidraw has the symbol.
fn head_back(v: Option<&Value>, c: &mut Connector, at_end: bool) {
    let ext = match v.and_then(Value::as_str) {
        Some("diamond") => Some(HeadExt::Diamond),
        Some("diamond_outline") => Some(HeadExt::DiamondOpen),
        Some("triangle_outline") => Some(HeadExt::TriangleOpen),
        Some("crowfoot_many") => Some(HeadExt::CrowFoot),
        Some("crowfoot_one_or_many") => Some(HeadExt::CrowFootOne),
        Some("crowfoot_one") => Some(HeadExt::OneOnly),
        _ => None,
    };
    let original = match v.and_then(Value::as_str) {
        None => Arrowhead::None,
        Some("arrow") => Arrowhead::Arrow,
        Some("dot" | "circle" | "circle_outline") => Arrowhead::Dot,
        Some("bar") => Arrowhead::Bar,
        Some(_) => Arrowhead::Triangle,
    };
    match (ext, at_end) {
        (Some(e), true) => c.set_ext_end(e),
        (Some(e), false) => c.set_ext_start(e),
        (None, true) => c.set_head_end(original),
        (None, false) => c.set_head_start(original),
    }
}

/* ===================================================================== Export */

/// Where the text element of a label goes.
#[derive(Clone, Copy)]
enum Place<'s> {
    /// Bound to the container of this id (centered, wrapped to the area).
    In(&'s str),
    /// Free text whose top left corner is here.
    At(Pt),
    /// Free text centered on the area.
    Center,
}

/// Elements of the scene being built.
struct Exporter<'a> {
    doc: &'a Document,
    page: &'a Page,
    m: &'a dyn Measurer,
    dark: bool,
    els: Vec<Map<String, Value>>,
    files: Map<String, Value>,
    /// Arrows bound to an element, by element id (added to its `boundElements` at the end).
    bound_to: HashMap<String, Vec<Value>>,
    /// Objects that Excalidraw cannot bind an arrow to.
    skip_bind: Vec<&'a Id>,
}

impl Exporter<'_> {
    fn col(&self, c: Color) -> String {
        c.resolve_hex(self.dark)
    }

    /// Properties shared by every element.
    fn base(&self, o: &Object, ty: &str, r: Rect, angle: f64) -> Map<String, Value> {
        let st = &o.style;
        let sw = (st.width / 1.6).round().max(1.0);
        let mut map = Map::new();
        for (k, v) in [
            ("id", json!(o.id.as_str())),
            ("type", json!(ty)),
            ("x", json!(r.x)),
            ("y", json!(r.y)),
            ("width", json!(r.w)),
            ("height", json!(r.h)),
            ("angle", json!(angle)),
            ("strokeColor", json!(if st.width > 0.0 || ty == "text" { self.col(st.stroke) } else { "transparent".to_owned() })),
            ("backgroundColor", json!(st.fill.map_or("transparent".to_owned(), |c| self.col(c)))),
            ("fillStyle", json!("solid")),
            ("strokeWidth", json!(sw)),
            (
                "strokeStyle",
                json!(match st.dash {
                    Dash::Solid => "solid",
                    Dash::Dashed => "dashed",
                    Dash::Dotted => "dotted",
                }),
            ),
            ("roughness", json!(0)),
            ("opacity", json!((st.opacity * 100.0).round())),
            ("groupIds", json!(o.group.as_ref().map(|g| vec![g.as_str().to_owned()]).unwrap_or_default())),
            ("frameId", Value::Null),
            ("roundness", Value::Null),
            ("seed", json!(seed_of(o.id.as_str()))),
            ("version", json!(1)),
            ("versionNonce", json!(seed_of(&format!("n{}", o.id)))),
            ("isDeleted", json!(false)),
            ("boundElements", Value::Null),
            ("updated", json!(1)),
            ("link", Value::Null),
            ("locked", json!(o.locked)),
        ] {
            map.insert(k.into(), v);
        }
        map
    }

    /// Text element of a label (`None` when it is empty); `area`: where the text is laid out.
    fn text_el(&self, o: &Object, label: &Label, area: Rect, place: Place, angle: f64, color: Color) -> Option<Map<String, Value>> {
        if label.text.is_empty() {
            return None;
        }
        let container = if let Place::In(id) = place { Some(id) } else { None };
        let wrap = container.map(|_| area.w);
        let lay = layout_rich(&label.text, wrap, self.m, label.family, label.size);
        let text = lay.lines.iter().map(|l| l.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n");
        let (x, y) = match place {
            Place::At(p) => (p.x, p.y),
            Place::In(_) | Place::Center => (area.center().x - lay.width / 2.0, area.center().y - lay.height / 2.0),
        };
        let id = Id::new();
        let mut t = self.base(
            &Object { id: id.clone(), style: Style { width: 1.0, fill: None, stroke: color, ..o.style.clone() }, ..o.clone() },
            "text",
            Rect::new(x, y, lay.width.max(4.0), lay.height),
            angle,
        );
        t.insert("text".into(), json!(text));
        t.insert("originalText".into(), json!(label.text.to_plain()));
        t.insert("fontSize".into(), json!(label.size));
        t.insert("fontFamily".into(), json!(xfont(label.family)));
        t.insert(
            "textAlign".into(),
            json!(match label.align {
                Align::Left => "left",
                Align::Center => "center",
                Align::Right => "right",
            }),
        );
        t.insert("verticalAlign".into(), json!(if container.is_some() { "middle" } else { "top" }));
        t.insert("containerId".into(), container.map_or(Value::Null, |c| json!(c)));
        t.insert("autoResize".into(), json!(true));
        t.insert("lineHeight".into(), json!(1.3));
        Some(t)
    }

    /// Pushes `e` followed by its bound text `label` (referenced in its `boundElements`).
    fn push_with_label(&mut self, mut e: Map<String, Value>, label: Option<Map<String, Value>>) {
        if let Some(t) = &label {
            e.insert("boundElements".into(), json!([{"id": t["id"], "type": "text"}]));
        }
        self.els.push(e);
        self.els.extend(label);
    }

    fn add_box(&mut self, o: &Object, b: &BoxObj) {
        let f = b.frame;
        let lr = b.label_rect();
        let area_world = {
            let c = f.to_world(Pt::new(lr.center().x, lr.center().y));
            Rect::new(c.x - lr.w / 2.0, c.y - lr.h / 2.0, lr.w, lr.h)
        };
        let rect = Rect::new(f.x, f.y, f.w, f.h);
        match b.geom {
            BoxGeom::Text => {
                if let Some(mut t) = self.text_el(o, &b.label, area_world, Place::At(Pt::new(f.x, f.y)), f.angle, o.style.stroke) {
                    t.insert("id".into(), json!(o.id.as_str()));
                    self.els.push(t);
                }
            }
            BoxGeom::Triangle => {
                let mut e = self.base(o, "line", rect, f.angle);
                e.insert("points".into(), json!([[f.w / 2.0, 0.0], [f.w, f.h], [0.0, f.h], [f.w / 2.0, 0.0]]));
                e.insert("polygon".into(), json!(true));
                for k in ["startBinding", "endBinding", "lastCommittedPoint"] {
                    e.insert(k.into(), Value::Null);
                }
                e.insert("startArrowhead".into(), Value::Null);
                e.insert("endArrowhead".into(), Value::Null);
                self.els.push(e);
                if let Some(t) = self.text_el(o, &b.label, area_world, Place::Center, f.angle, o.style.stroke) {
                    self.els.push(t);
                }
            }
            geom => {
                let ty = match geom {
                    BoxGeom::Ellipse => "ellipse",
                    BoxGeom::Diamond => "diamond",
                    _ => "rectangle",
                };
                let mut e = self.base(o, ty, rect, f.angle);
                if geom == BoxGeom::Rect && o.style.roundness > 0.0 {
                    e.insert("roundness".into(), json!({"type": 3}));
                }
                if geom == BoxGeom::Sticky {
                    e.insert("strokeColor".into(), json!("transparent"));
                    e.insert("backgroundColor".into(), json!(self.col(o.style.fill.unwrap_or(inkboard_core::style::NOTE_YELLOW))));
                }
                let color = if geom == BoxGeom::Sticky { Color::Rgb(0x1e, 0x1e, 0x1e) } else { o.style.stroke };
                let label = self.text_el(o, &b.label, area_world, Place::In(o.id.as_str()), f.angle, color);
                self.push_with_label(e, label);
            }
        }
    }

    fn add_pen(&mut self, o: &Object, p: &PenObj) {
        let Some(first) = p.pts.first() else { return };
        let bb = Rect::from_points(p.pts.iter().copied()).unwrap_or_default();
        let mut e = self.base(o, "freedraw", Rect::new(first.x, first.y, bb.w, bb.h), 0.0);
        e.insert("points".into(), json!(p.pts.iter().map(|q| [q.x - first.x, q.y - first.y]).collect::<Vec<_>>()));
        e.insert("pressures".into(), json!(p.pressures.as_deref().map_or(&[][..], Vec::as_slice)));
        e.insert("simulatePressure".into(), json!(p.pressures.is_none()));
        e.insert("lastCommittedPoint".into(), Value::Null);
        self.els.push(e);
    }

    fn add_image(&mut self, o: &Object, i: &ImageObj) {
        let Some(a) = self.doc.assets.get(&i.asset) else { return };
        let fid = i.asset.as_str().to_owned();
        let bytes = i.export_bytes(&a.mime, &a.data, o.style.stroke.resolve(self.doc.background.preset.is_dark()));
        self.files.insert(fid.clone(), json!({"mimeType": a.mime, "id": fid, "dataURL": data_url(&a.mime, &bytes), "created": 1, "lastRetrieved": 1}));
        let mut e = self.base(o, "image", Rect::new(i.frame.x, i.frame.y, i.frame.w, i.frame.h), i.frame.angle);
        e.insert("fileId".into(), json!(fid));
        e.insert("status".into(), json!("saved"));
        e.insert("scale".into(), json!([1, 1]));
        e.insert("crop".into(), Value::Null);
        self.els.push(e);
    }

    fn add_table(&mut self, o: &Object, t: &Table) {
        // invisible outline carrying the table id: the element a bound connector points to
        if self.page.objects.iter().any(|p| matches!(&p.kind, Kind::Connector(c) if c.targets().any(|t| *t == o.id))) {
            let bb = self.page.bounds(o);
            let outline = Object { style: Style { width: 0.0, fill: None, ..o.style.clone() }, ..o.clone() };
            let e = self.base(&outline, "rectangle", bb, 0.0);
            self.els.push(e);
        }
        for r in 0..t.rows() {
            for c in 0..t.cols() {
                let cr = t.cell_rect(r, c);
                let cell_id = Id::new();
                let cell_obj = Object {
                    id: cell_id.clone(),
                    style: Style { fill: (t.header && r == 0).then_some(Color::rgb(0xde, 0xe2, 0xe6)), ..o.style.clone() },
                    ..o.clone()
                };
                let e = self.base(&cell_obj, "rectangle", cr, 0.0);
                let label = Label { text: t.cell(r, c).cloned().unwrap_or_default(), family: t.family, size: t.size, align: Align::Left };
                let area = Rect::new(cr.x + 8.0, cr.y + 4.0, cr.w - 16.0, cr.h - 8.0);
                let t_el = self.text_el(&cell_obj, &label, area, Place::In(cell_id.as_str()), 0.0, o.style.stroke);
                self.push_with_label(e, t_el);
            }
        }
    }

    fn add_connector(&mut self, o: &Object, c: &Connector) {
        let Some(path) = self.page.connector_path(c) else { return };
        let (x0, y0) = (path[0].x, path[0].y);
        let bb = Rect::from_points(path.iter().copied()).unwrap_or_default();
        let arrow = c.has_head_start() || c.has_head_end() || c.start.target().is_some() || c.end.target().is_some();
        let mut e = self.base(o, if arrow { "arrow" } else { "line" }, Rect::new(x0, y0, bb.w, bb.h), 0.0);
        e.insert("points".into(), json!(path.iter().map(|q| [q.x - x0, q.y - y0]).collect::<Vec<_>>()));
        e.insert("lastCommittedPoint".into(), Value::Null);
        e.insert("startArrowhead".into(), xhead(c.head_start, c.head_start_ext));
        e.insert("endArrowhead".into(), xhead(c.head_end, c.head_end_ext));
        e.insert("elbowed".into(), json!(c.route == Route::Elbow));
        if c.route != Route::Straight {
            e.insert("roundness".into(), json!({"type": 2}));
        }
        for (end, key) in [(&c.start, "startBinding"), (&c.end, "endBinding")] {
            let bind = end.target().filter(|t| !self.skip_bind.contains(t)).filter(|t| self.page.get(t).is_some());
            match bind {
                Some(t) => {
                    e.insert(key.into(), json!({"elementId": t.as_str(), "focus": 0, "gap": 4}));
                    self.bound_to.entry(t.as_str().to_owned()).or_default().push(json!({"id": o.id.as_str(), "type": "arrow"}));
                }
                None => {
                    e.insert(key.into(), Value::Null);
                }
            }
        }
        let label = c.label.as_ref().and_then(|l| {
            let (mid, _) = inkboard_core::connector::path_midpoint(&path);
            self.text_el(o, l, Rect::new(mid.x - 40.0, mid.y - 10.0, 80.0, 20.0), Place::In(o.id.as_str()), 0.0, o.style.stroke)
        });
        self.push_with_label(e, label);
    }
}

/// Exports a page in Excalidraw format (one scene = one page).
pub fn export(doc: &Document, page: &Page, m: &dyn Measurer) -> String {
    let dark = doc.background.preset.is_dark();
    let skip_bind: Vec<&Id> = page
        .objects
        .iter()
        .filter(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Triangle) || matches!(o.kind, Kind::Pen(_) | Kind::Connector(_)))
        .map(|o| &o.id)
        .collect();
    let mut x = Exporter { doc, page, m, dark, els: Vec::new(), files: Map::new(), bound_to: HashMap::new(), skip_bind };
    for o in page.objects.iter().filter(|o| page.is_visible(o)) {
        match &o.kind {
            Kind::Box(b) => x.add_box(o, b),
            Kind::Pen(p) => x.add_pen(o, p),
            Kind::Image(i) => x.add_image(o, i),
            Kind::Table(t) => x.add_table(o, t),
            Kind::Connector(c) => x.add_connector(o, c),
        }
    }
    let Exporter { mut els, files, mut bound_to, .. } = x;
    for e in &mut els {
        let Some(id) = e.get("id").and_then(Value::as_str).map(str::to_owned) else { continue };
        if let Some(extra) = bound_to.remove(&id) {
            let mut list = e.get("boundElements").and_then(Value::as_array).cloned().unwrap_or_default();
            list.extend(extra);
            e.insert("boundElements".into(), Value::Array(list));
        }
    }
    let scene = json!({
        "type": "excalidraw", "version": 2, "source": "inkboard",
        "elements": els,
        "appState": {"gridSize": null, "viewBackgroundColor": doc.background.preset.hex(), "theme": if dark { "dark" } else { "light" }},
        "files": files,
    });
    scene.to_string()
}

/* ===================================================================== Import */

fn num(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}
fn text<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn style_of(e: &Value, is_text: bool) -> Style {
    let transparent = text(e, "strokeColor") == Some("transparent");
    let bg = text(e, "backgroundColor").and_then(parse_color);
    Style {
        stroke: text(e, "strokeColor").and_then(parse_color).unwrap_or(Color::Ink),
        fill: if is_text { None } else { bg },
        width: if is_text || transparent { 0.0 } else { (num(e, "strokeWidth").max(1.0) * 1.5).max(1.5) },
        dash: match text(e, "strokeStyle") {
            Some("dashed") => Dash::Dashed,
            Some("dotted") => Dash::Dotted,
            _ => Dash::Solid,
        },
        opacity: num::f32((e.get("opacity").and_then(Value::as_f64).unwrap_or(100.0) / 100.0).clamp(0.05, 1.0)),
        roundness: 0.0,
        shadow: false,
    }
}

fn label_of(e: &Value) -> Label {
    Label {
        text: RichText::plain(text(e, "originalText").or_else(|| text(e, "text")).unwrap_or("")),
        family: xfont_back(e.get("fontFamily").and_then(Value::as_i64).unwrap_or(2)),
        size: num(e, "fontSize").max(6.0).max(if e.get("fontSize").is_some() { 0.0 } else { 20.0 }),
        align: match text(e, "textAlign") {
            Some("center") => Align::Center,
            Some("right") => Align::Right,
            _ => Align::Left,
        },
    }
}

fn finish(mut o: Object, e: &Value) -> Object {
    if let Some(g) = e.get("groupIds").and_then(Value::as_array).and_then(|a| a.last()).and_then(Value::as_str) {
        o.group = Some(Id::from(g));
    }
    o.locked = e.get("locked").and_then(Value::as_bool).unwrap_or(false);
    o
}

/// Lookup tables of the scene being imported.
struct Scene<'a> {
    /// Pre-allocated ids: bindings resolve whatever the order of the elements.
    ids: HashMap<&'a str, Id>,
    kind_of: HashMap<&'a str, &'a str>,
    /// Text element bound to each container.
    labels: HashMap<&'a str, &'a Value>,
}

impl Scene<'_> {
    /// Id of an element (an element without "id" gets a synthetic one).
    fn own_id(&self, id: &str) -> Id {
        self.ids.get(id).cloned().unwrap_or_else(Id::new)
    }

    /// Shape of a `rectangle` / `ellipse` / `diamond` element, with its bound text as label.
    fn shape(&self, e: &Value, ty: &str) -> Object {
        let id = text(e, "id").unwrap_or("");
        let geom = match ty {
            "ellipse" => BoxGeom::Ellipse,
            "diamond" => BoxGeom::Diamond,
            _ => BoxGeom::Rect,
        };
        let mut st = style_of(e, false);
        if geom == BoxGeom::Rect && e.get("roundness").is_some_and(|r| !r.is_null()) {
            st.roundness = 0.45;
        }
        let mut label = self.labels.get(id).map(|t| label_of(t)).unwrap_or_default();
        if !self.labels.contains_key(id) {
            label.align = Align::Center;
        } else if st.width == 0.0
            && let Some(c) = self.labels.get(id).and_then(|t| text(t, "strokeColor")).and_then(parse_color)
        {
            st.stroke = c;
        }
        let frame = Frame { x: num(e, "x"), y: num(e, "y"), w: num(e, "width"), h: num(e, "height"), angle: num(e, "angle") };
        let mut obj = Object::new_box(geom, frame, st, label);
        obj.id = self.own_id(id);
        obj
    }

    /// Object of a `line` / `arrow` element: a connector, or a triangle drawn as a closed line. `None`: fewer than 2 points.
    fn linear(&self, e: &Value, ty: &str, angle: f64) -> Option<Object> {
        let id = text(e, "id").unwrap_or("");
        let pts = element_points(e, angle);
        if pts.len() < 2 {
            return None;
        }
        let closed = pts.len() == 4 && pts[0].dist(pts[3]) < 2.0 && ty == "line";
        if closed && angle == 0.0 {
            let bb = Rect::from_points(pts.iter().copied()).unwrap_or_default();
            let apex_up = (pts[0].x - bb.center().x).abs() < bb.w * 0.1 && (pts[0].y - bb.y).abs() < 1.0;
            if apex_up {
                let mut triangle = Object::new_box(BoxGeom::Triangle, Frame::from_rect(bb), style_of(e, false), Label::default());
                triangle.id = self.own_id(id);
                return Some(triangle);
            }
        }
        let bind = |key: &str| -> Option<End> {
            let t = e.get(key)?.get("elementId")?.as_str()?;
            (matches!(self.kind_of.get(t), Some(&("rectangle" | "ellipse" | "diamond" | "image" | "text")))).then(|| End::bound(self.own_id(t)))
        };
        let mut c =
            Connector::new(bind("startBinding").unwrap_or_else(|| End::free(pts[0])), bind("endBinding").unwrap_or_else(|| End::free(pts[pts.len() - 1])));
        c.via = pts[1..pts.len() - 1].to_vec();
        c.head_end = Arrowhead::None;
        if ty == "arrow" {
            if e.get("endArrowhead").is_none() {
                c.set_head_end(Arrowhead::Arrow);
            } else {
                head_back(e.get("endArrowhead"), &mut c, true);
            }
            head_back(e.get("startArrowhead"), &mut c, false);
        }
        let elbowed = e.get("elbowed").and_then(Value::as_bool).unwrap_or(false);
        c.route = if elbowed {
            c.via.clear();
            Route::Elbow
        } else if e.get("roundness").is_some_and(|r| !r.is_null()) && pts.len() > 2 {
            Route::Curve
        } else {
            Route::Straight
        };
        if let Some(t) = self.labels.get(id) {
            c.label = Some(Label { align: Align::Center, ..label_of(t) });
        }
        let mut o = Object::new(Kind::Connector(c), Style { fill: None, ..style_of(e, false) });
        o.id = self.own_id(id);
        Some(o)
    }
}

/// Points of a `line` / `arrow` / `freedraw` element in world coordinates, its rotation applied.
fn element_points(e: &Value, angle: f64) -> Vec<Pt> {
    let (x, y) = (num(e, "x"), num(e, "y"));
    let mut pts: Vec<Pt> = e
        .get("points")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|p| Some(Pt::new(x + p.get(0)?.as_f64()?, y + p.get(1)?.as_f64()?))).collect())
        .unwrap_or_default();
    if angle != 0.0 {
        let center = Rect::from_points(pts.iter().copied()).map(|r| r.center()).unwrap_or_default();
        for p in &mut pts {
            *p = p.rotate(center, angle);
        }
    }
    pts
}

/// Pen stroke of a `freedraw` element (`None`: no point).
fn freedraw(e: &Value, angle: f64) -> Option<Object> {
    let mut pts = element_points(e, angle);
    if pts.is_empty() {
        return None;
    }
    let mut pressures: Vec<f32> =
        e.get("pressures").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_f64().map(num::f32)).collect()).unwrap_or_default();
    if pts.len() == 1 {
        pts.push(pts[0]);
        pressures.extend(pressures.first().copied());
    }
    let pen_style = Style { dash: Dash::Solid, fill: None, ..style_of(e, false) };
    let mut o =
        if pressures.len() == pts.len() { Object::new_pen_with_pressure(pts, pressures, false, pen_style) } else { Object::new_pen(pts, false, pen_style) };
    o.style.width = (num(e, "strokeWidth").max(1.0) * 1.5).max(1.5);
    Some(o)
}

/// A frame becomes a dashed rectangle, with its name above it.
fn frame_objects(e: &Value, id: Id, measurer: &dyn Measurer) -> Vec<Object> {
    let (x, y) = (num(e, "x"), num(e, "y"));
    let mut outline = Object::new_box(
        BoxGeom::Rect,
        Frame::new(x, y, num(e, "width"), num(e, "height")),
        Style { dash: Dash::Dashed, width: 2.0, ..Style::default() },
        Label::default(),
    );
    outline.id = id;
    let mut out = vec![outline];
    if let Some(n) = text(e, "name").filter(|n| !n.is_empty()) {
        let mut title =
            Object::new_box(BoxGeom::Text, Frame::new(x, y - 26.0, 10.0, 10.0), Style { width: 0.0, ..Style::default() }, Label::new(n, 16.0, Align::Left));
        if let Kind::Box(b) = &mut title.kind {
            b.fit(measurer);
        }
        out.push(title);
    }
    out
}

/// Imports an Excalidraw scene.
///
/// # Errors
///
/// [`FormatError`] when the content is not an Excalidraw scene.
pub fn import(content: &str, name: &str, measurer: &dyn Measurer) -> Result<Imported, FormatError> {
    let root: Value = serde_json::from_str(content)?;
    if text(&root, "type") != Some("excalidraw") {
        return Err(FormatError::Invalid("not an Excalidraw scene".into()));
    }
    let els: Vec<&Value> = root
        .get("elements")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|e| !e.get("isDeleted").and_then(Value::as_bool).unwrap_or(false)).collect())
        .unwrap_or_default();
    let files = root.get("files").and_then(Value::as_object);
    let mut report = Report::default();
    let mut doc = Document::new(name);
    if let Some(bg) = root.pointer("/appState/viewBackgroundColor").and_then(Value::as_str).and_then(parse_color) {
        doc.background.preset = BgPreset::nearest(bg.resolve(false));
    }
    let mut scene = Scene {
        ids: els.iter().filter_map(|e| text(e, "id").map(|i| (i, Id::new()))).collect(),
        kind_of: els.iter().filter_map(|e| Some((text(e, "id")?, text(e, "type")?))).collect(),
        labels: HashMap::new(),
    };
    for e in &els {
        if text(e, "type") == Some("text")
            && let Some(c) = text(e, "containerId")
        {
            scene.labels.insert(c, e);
        }
    }
    let mut out: Vec<Object> = Vec::new();
    for e in &els {
        let id = text(e, "id").unwrap_or("");
        let ty = text(e, "type").unwrap_or("");
        let (x, y, width, height) = (num(e, "x"), num(e, "y"), num(e, "width"), num(e, "height"));
        let angle = num(e, "angle");
        match ty {
            "rectangle" | "ellipse" | "diamond" => out.push(finish(scene.shape(e, ty), e)),
            "text" => {
                let container = text(e, "containerId");
                if container.is_some_and(|c| {
                    scene.labels.get(c).is_some_and(|t| std::ptr::eq(*t, *e))
                        && matches!(scene.kind_of.get(c), Some(&("rectangle" | "ellipse" | "diamond" | "arrow" | "line")))
                }) {
                    continue;
                }
                let mut text_obj = Object::new_box(BoxGeom::Text, Frame::new(x, y, width, height), style_of(e, true), label_of(e));
                if let Kind::Box(b) = &mut text_obj.kind {
                    b.fit(measurer);
                    b.frame.angle = angle;
                }
                text_obj.style.stroke = text(e, "strokeColor").and_then(parse_color).unwrap_or(Color::Ink);
                text_obj.id = scene.own_id(id);
                out.push(finish(text_obj, e));
            }
            "line" | "arrow" => {
                let Some(o) = scene.linear(e, ty, angle) else {
                    report.skipped += 1;
                    continue;
                };
                out.push(finish(o, e));
            }
            "freedraw" => {
                let Some(mut o) = freedraw(e, angle) else {
                    report.skipped += 1;
                    continue;
                };
                o.id = scene.own_id(id);
                out.push(finish(o, e));
            }
            "image" => {
                let data = text(e, "fileId").and_then(|f| files?.get(f)).and_then(|f| f.get("dataURL")).and_then(Value::as_str).and_then(parse_data_url);
                let Some((mime, bytes)) = data else {
                    report.skipped += 1;
                    continue;
                };
                let aid = Id::new();
                doc.assets.insert(aid.clone(), Asset { mime, data: Arc::new(bytes) });
                let mut o = Object::new(
                    Kind::Image(ImageObj { frame: Frame { x, y, w: width, h: height, angle }, asset: aid, latex: None, mono: false, recolor: Vec::new() }),
                    Style { width: 0.0, opacity: style_of(e, false).opacity, ..Style::default() },
                );
                o.id = scene.own_id(id);
                out.push(finish(o, e));
            }
            "frame" | "magicframe" => {
                report.approximated += 1;
                out.extend(frame_objects(e, scene.own_id(id), measurer));
            }
            "text_container_placeholder" => {}
            _ => report.skipped += 1,
        }
    }
    doc.pages[0].objects = out;
    inkboard_core::serial::sanitize(&mut doc);
    Ok(Imported { doc, report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_document;
    use inkboard_core::text::ApproxMeasurer;
    use inkboard_core::{BoxObj, PenObj};

    fn export_import(d: &Document) -> Imported {
        let json = export(d, &d.pages[0], &ApproxMeasurer);
        import(&json, "x", &ApproxMeasurer).unwrap()
    }

    #[test]
    fn export_has_expected_structure() {
        let d = sample_document();
        let v: Value = serde_json::from_str(&export(&d, &d.pages[0], &ApproxMeasurer)).unwrap();
        assert_eq!(v["type"], "excalidraw");
        let els = v["elements"].as_array().unwrap();
        let types: Vec<&str> = els.iter().map(|e| e["type"].as_str().unwrap()).collect();
        for t in ["rectangle", "ellipse", "diamond", "line", "arrow", "freedraw", "image", "text"] {
            assert!(types.contains(&t), "{t} missing from {types:?}");
        }
        assert_eq!(v["files"].as_object().unwrap().len(), 1);
        // a rectangle's text is bound to its container
        let rect = els.iter().find(|e| e["type"] == "rectangle").unwrap();
        let tid = rect["boundElements"][0]["id"].as_str().unwrap();
        let t = els.iter().find(|e| e["id"] == tid).unwrap();
        assert_eq!(t["containerId"], rect["id"]);
        // the rectangle also references the arrow that attaches to it
        assert!(rect["boundElements"].as_array().unwrap().iter().any(|b| b["type"] == "arrow"));
    }

    #[test]
    fn roundtrip_keeps_shapes_text_and_bindings() {
        let d = sample_document();
        let r = export_import(&d);
        let p = &r.doc.pages[0];
        let boxes: Vec<&BoxObj> = p.objects.iter().filter_map(|o| if let Kind::Box(b) = &o.kind { Some(b) } else { None }).collect();
        assert!(boxes.iter().any(|b| b.geom == BoxGeom::Rect && b.label.text.to_plain() == "Start\nline 2"));
        assert!(boxes.iter().any(|b| b.geom == BoxGeom::Ellipse && b.label.text.to_plain() == "End"));
        assert!(boxes.iter().any(|b| b.geom == BoxGeom::Triangle), "triangle reconnu");
        assert!(boxes.iter().any(|b| b.geom == BoxGeom::Text && b.label.text.to_plain() == "Free title"));
        let conns: Vec<&Connector> = p.objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!(conns.len(), 2);
        assert!(conns[0].targets().count() == 2, "arrow bound to both shapes");
        assert!(p.connector_path(conns[0]).is_some());
        assert!(conns[1].targets().count() == 1 && matches!(conns[1].end, End::Free { .. }));
        assert_eq!(conns[0].label.as_ref().map(|l| l.text.to_plain()), Some("yes".into()));
        assert!(p.objects.iter().any(|o| matches!(o.kind, Kind::Pen(_))));
        assert_eq!(r.doc.assets.len(), 1);
        assert_eq!(r.report.skipped, 0);
    }

    #[test]
    fn imports_foreign_scene_with_elbow_polyline_and_deleted_items() {
        let src = r##"{"type":"excalidraw","version":2,"elements":[
          {"id":"r1","type":"rectangle","x":0,"y":0,"width":100,"height":60,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"#a5d8ff","strokeWidth":2,"strokeStyle":"solid","opacity":100,"roundness":{"type":3},"boundElements":[{"id":"t1","type":"text"},{"id":"a1","type":"arrow"}]},
          {"id":"t1","type":"text","x":10,"y":20,"width":80,"height":20,"text":"Hello","originalText":"Hello","fontSize":20,"fontFamily":1,"textAlign":"center","containerId":"r1","strokeColor":"#1e1e1e"},
          {"id":"r2","type":"ellipse","x":300,"y":0,"width":100,"height":60,"angle":0,"strokeColor":"#e03131","backgroundColor":"transparent","strokeWidth":1,"strokeStyle":"dashed","opacity":50},
          {"id":"a1","type":"arrow","x":100,"y":30,"width":200,"height":0,"angle":0,"points":[[0,0],[100,0],[200,0]],"elbowed":true,"startBinding":{"elementId":"r1"},"endBinding":{"elementId":"r2"},"startArrowhead":null,"endArrowhead":"triangle","strokeColor":"#1e1e1e","backgroundColor":"transparent","strokeWidth":2},
          {"id":"l1","type":"line","x":0,"y":200,"width":100,"height":50,"angle":0,"points":[[0,0],[50,50],[100,0]],"strokeColor":"#1e1e1e","strokeWidth":2},
          {"id":"gone","type":"rectangle","x":0,"y":0,"width":5,"height":5,"isDeleted":true},
          {"id":"emb","type":"embeddable","x":0,"y":0,"width":5,"height":5},
          {"id":"fr","type":"frame","x":-20,"y":-20,"width":500,"height":400,"name":"Cadre"}
        ],"appState":{"viewBackgroundColor":"#1e1e1e"},"files":{}}"##;
        let r = import(src, "ext", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        assert_eq!(r.doc.background.preset, BgPreset::Black);
        assert_eq!(r.report.skipped, 1, "embeddable skipped");
        assert_eq!(r.report.approximated, 1, "frame approximated");
        let rect = p
            .objects
            .iter()
            .find_map(|o| if let Kind::Box(b) = &o.kind { (b.geom == BoxGeom::Rect && b.label.text.to_plain() == "Hello").then_some((o, b)) } else { None })
            .expect("rectangle with its text");
        assert!(rect.0.style.roundness > 0.0);
        assert_eq!(rect.1.label.family, FontFamily::Hand);
        assert_eq!(rect.1.label.align, Align::Center);
        assert!(
            !p.objects.iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Text && b.label.text.to_plain() == "Hello")),
            "the bound text is not left duplicated"
        );
        let conns: Vec<&Connector> = p.objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!(conns.len(), 2);
        assert_eq!(conns[0].route, Route::Elbow);
        assert_eq!(conns[0].head_end, Arrowhead::Triangle);
        assert_eq!(conns[0].targets().count(), 2);
        assert_eq!(conns[1].via.len(), 1, "polyline = connector with a waypoint");
        assert_eq!(conns[1].head_end, Arrowhead::None);
        let dashed = p.objects.iter().find(|o| o.style.dash == Dash::Dashed && matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Ellipse)).unwrap();
        assert!((dashed.style.opacity - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rejects_non_excalidraw() {
        assert!(import("{\"type\":\"autre\"}", "x", &ApproxMeasurer).is_err());
        assert!(import("nope", "x", &ApproxMeasurer).is_err());
    }

    #[test]
    fn pen_pressure_roundtrips_through_excalidraw() {
        let mut d = inkboard_core::Document::new("p");
        let pts = vec![Pt::new(0.0, 0.0), Pt::new(10.0, 5.0), Pt::new(20.0, 0.0)];
        d.pages[0].objects.push(Object::new_pen_with_pressure(pts, vec![0.2, 0.9, 0.4], false, Style::default()));
        d.pages[0].objects.push(Object::new_pen(vec![Pt::new(0.0, 50.0), Pt::new(10.0, 60.0)], false, Style::default()));
        let json = export(&d, &d.pages[0], &ApproxMeasurer);
        let v: Value = serde_json::from_str(&json).unwrap();
        let els = v["elements"].as_array().unwrap();
        assert_eq!(els[0]["pressures"].as_array().unwrap().len(), 3);
        assert_eq!(els[0]["simulatePressure"], false);
        assert_eq!(els[1]["simulatePressure"], true, "without pressure: simulated by Excalidraw");
        let back = import(&json, "x", &ApproxMeasurer).unwrap();
        let pens: Vec<&PenObj> = back.doc.pages[0].objects.iter().filter_map(|o| if let Kind::Pen(p) = &o.kind { Some(p) } else { None }).collect();
        let first = pens[0].pressures.as_ref().expect("pressure kept");
        assert!((first[1] - 0.9).abs() < 1e-6);
        assert!(pens[1].pressures.is_none());
    }

    #[test]
    fn extended_heads_use_excalidraw_arrowheads_and_come_back() {
        use inkboard_core::HeadExt;
        let mut d = inkboard_core::Document::new("heads");
        let boxes: Vec<Object> =
            (0..2).map(|i| Object::new_box(BoxGeom::Rect, Frame::new(f64::from(i) * 300.0, 0.0, 80.0, 40.0), Style::default(), Label::default())).collect();
        let ids: Vec<inkboard_core::Id> = boxes.iter().map(|b| b.id.clone()).collect();
        d.pages[0].objects.extend(boxes);
        for ext in [HeadExt::Diamond, HeadExt::DiamondOpen, HeadExt::TriangleOpen, HeadExt::CrowFoot, HeadExt::CrowFootOne, HeadExt::OneOnly] {
            let mut c = Connector::new(End::bound(ids[0].clone()), End::bound(ids[1].clone()));
            c.set_ext_end(ext);
            d.pages[0].objects.push(Object::new(Kind::Connector(c), Style::default()));
        }
        let json = export(&d, &d.pages[0], &ApproxMeasurer);
        for name in ["diamond", "diamond_outline", "triangle_outline", "crowfoot_many", "crowfoot_one_or_many", "crowfoot_one"] {
            assert!(json.contains(&format!("\"endArrowhead\": \"{name}\"")) || json.contains(&format!("\"endArrowhead\":\"{name}\"")), "{name}");
        }
        let r = import(&json, "x", &ApproxMeasurer).unwrap();
        let back: Vec<Option<HeadExt>> =
            r.doc.pages[0].objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c.head_end_ext) } else { None }).collect();
        assert_eq!(back, [HeadExt::Diamond, HeadExt::DiamondOpen, HeadExt::TriangleOpen, HeadExt::CrowFoot, HeadExt::CrowFootOne, HeadExt::OneOnly].map(Some));
        // the other half of the family has no Excalidraw symbol: it is exported as the closest one, and read back as that
        let mut c = Connector::new(End::bound(ids[0].clone()), End::bound(ids[1].clone()));
        c.set_ext_end(HeadExt::CrowFootZero);
        d.pages[0].objects.push(Object::new(Kind::Connector(c), Style::default()));
        assert!(export(&d, &d.pages[0], &ApproxMeasurer).contains("crowfoot_many"));
    }
}
