//! Our own exports must be read back identically: export, import, compare.
//!
//! Left out on purpose (documented losses, see `docs/ROADMAP.md`): pen strokes (exported as images to draw.io, no editable
//! equivalent) and, for JSON Canvas, everything that is not a text shape or an arrow between two shapes (tables, images).

use inkboard_core::highlight::Lang;
use inkboard_core::*;
use inkboard_formats::{drawio, excalidraw, import_any, jsoncanvas};

const PNG: [u8; 11] = [137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3];
const CODE: &str = "fn main() {\n    println!(\"hi\");\n}";
/// Positions and sizes must match within 1 px.
const TOL: f64 = 1.0;

fn shape(geom: BoxGeom, frame: Frame, text: &str) -> Object {
    Object::new_box(geom, frame, Style::default(), Label::new(text, 20.0, Align::Center))
}

fn table_object(origin: Pt) -> Object {
    let mut t = Table::new(origin, 2, 2);
    for (cell, text) in t.cells.iter_mut().zip(["Name", "Qty", "Apple", "3"]) {
        *cell = RichText::plain(text);
    }
    Object::new(Kind::Table(t), Style::default())
}

/// One object of each type: rectangle, ellipse (rotated), free text, code block, table, image, and two bound connectors
/// (the first one with a label if `connector_label`).
fn reference(connector_label: bool) -> Document {
    let mut d = Document::new("Reference");
    let process = shape(BoxGeom::Rect, Frame::new(100.0, 100.0, 160.0, 90.0), "Process");
    let decision = shape(BoxGeom::Ellipse, Frame { x: 400.0, y: 100.0, w: 140.0, h: 90.0, angle: 0.3 }, "Decision");
    let mut title = Object::new_box(
        BoxGeom::Text,
        Frame::new(600.0, 40.0, 10.0, 10.0),
        Style { width: 0.0, ..Default::default() },
        Label::new("Free title", 28.0, Align::Left),
    );
    if let Kind::Box(b) = &mut title.kind {
        b.fit(&ApproxMeasurer);
    }
    let mut code = Object::new_box(
        BoxGeom::Rect,
        Frame::new(100.0, 300.0, 260.0, 110.0),
        Style::default(),
        Label { family: FontFamily::Mono, ..Label::new(CODE, 16.0, Align::Left) },
    );
    if let Kind::Box(b) = &mut code.kind {
        b.lang = Some(Lang::Rust);
    }
    let aid = d.add_asset("image/png", PNG.to_vec());
    let image = Object::new(
        Kind::Image(ImageObj { frame: Frame::new(650.0, 300.0, 80.0, 60.0), asset: aid, latex: None, mono: false }),
        Style { width: 0.0, ..Default::default() },
    );
    let mut yes = Connector::new(End::bound(process.id.clone()), End::bound(decision.id.clone()));
    yes.label = connector_label.then(|| Label::new("yes", 14.0, Align::Center));
    let yes = Object::new(Kind::Connector(yes), Style::default());
    let to_code = Object::new(Kind::Connector(Connector::new(End::bound(decision.id.clone()), End::bound(code.id.clone()))), Style::default());
    d.pages[0].objects.extend([process, decision, title, code, table_object(Pt::new(450.0, 300.0)), image, yes, to_code]);
    d
}

#[derive(Clone, Debug)]
struct Item {
    kind: &'static str,
    text: String,
    frame: Frame,
}

fn kind_name(geom: BoxGeom) -> &'static str {
    match geom {
        BoxGeom::Rect => "rect",
        BoxGeom::Ellipse => "ellipse",
        BoxGeom::Diamond => "diamond",
        BoxGeom::Triangle => "triangle",
        BoxGeom::Sticky => "sticky",
        BoxGeom::Text => "text",
    }
}

/// Shapes, texts and images of a page, sorted. A table counts as one rectangle per cell: that is how
/// draw.io and Excalidraw represent it.
fn items(page: &Page) -> Vec<Item> {
    let mut out = Vec::new();
    for o in &page.objects {
        match &o.kind {
            Kind::Box(b) => out.push(Item { kind: kind_name(b.geom), text: b.label.text.to_plain(), frame: b.frame }),
            Kind::Image(i) => out.push(Item { kind: "image", text: String::new(), frame: i.frame }),
            Kind::Table(t) => {
                for r in 0..t.rows() {
                    for c in 0..t.cols() {
                        out.push(Item {
                            kind: "rect",
                            text: t.cell(r, c).map(RichText::to_plain).unwrap_or_default(),
                            frame: Frame::from_rect(t.cell_rect(r, c)),
                        });
                    }
                }
            }
            Kind::Pen(_) | Kind::Connector(_) => {}
        }
    }
    out.sort_by(|a, b| (a.kind, &a.text).cmp(&(b.kind, &b.text)));
    out
}

/// JSON Canvas keeps text shapes only, as rectangles whose rounded bounding box is at least 60 x 40.
fn canvas_items(page: &Page) -> Vec<Item> {
    let mut out: Vec<Item> = page
        .objects
        .iter()
        .filter_map(|o| if let Kind::Box(b) = &o.kind { Some(b) } else { None })
        .map(|b| {
            let r = b.frame.aabb();
            Item { kind: "rect", text: b.label.text.to_plain(), frame: Frame::new(r.x.round(), r.y.round(), r.w.round().max(60.0), r.h.round().max(40.0)) }
        })
        .collect();
    out.sort_by(|a, b| (a.kind, &a.text).cmp(&(b.kind, &b.text)));
    out
}

/// `(text of the source, text of the target, label)` of every connector.
fn links(page: &Page) -> Vec<(String, String, String)> {
    let text_of = |e: &End| match e.target().and_then(|t| page.get(t)).map(|o| &o.kind) {
        Some(Kind::Box(b)) => b.label.text.to_plain(),
        Some(_) => "<not a text shape>".to_owned(),
        None => "<free end>".to_owned(),
    };
    let mut out: Vec<_> = page
        .objects
        .iter()
        .filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None })
        .map(|c| (text_of(&c.start), text_of(&c.end), c.label.as_ref().map(|l| l.text.to_plain()).unwrap_or_default()))
        .collect();
    out.sort();
    out
}

/// Same objects (type, text) and same frames within 1 px, after moving the expected ones by `shift`.
fn assert_same_items(expected: &[Item], got: &[Item], shift: (f64, f64)) {
    let key = |v: &[Item]| v.iter().map(|i| (i.kind, i.text.clone())).collect::<Vec<_>>();
    assert_eq!(key(got), key(expected), "objects per type and texts");
    for (e, g) in expected.iter().zip(got) {
        let (a, b) = (e.frame, g.frame);
        for (name, want, have) in [("x", a.x + shift.0, b.x), ("y", a.y + shift.1, b.y), ("width", a.w, b.w), ("height", a.h, b.h)] {
            assert!((want - have).abs() <= TOL, "{} {:?}: {name} expected {want}, got {have}", e.kind, e.text);
        }
        assert!((a.angle - b.angle).abs() <= 0.01, "{} {:?}: angle expected {}, got {}", e.kind, e.text, a.angle, b.angle);
    }
}

fn expected_links(connector_label: bool) -> Vec<(String, String, String)> {
    let label = if connector_label { "yes" } else { "" };
    vec![("Decision".into(), CODE.into(), String::new()), ("Process".into(), "Decision".into(), label.into())]
}

#[test]
fn drawio_export_is_read_back_identically() {
    let d = reference(true);
    let imp = import_any("ref.drawio", &drawio::export(&d, &ApproxMeasurer), &ApproxMeasurer).expect("import");
    assert_eq!(imp.report.skipped, 0);
    let (exp, got) = (items(&d.pages[0]), items(&imp.doc.pages[0]));
    // the export moves the whole content to a fixed origin: one common shift is allowed
    let process = |v: &[Item]| v.iter().find(|i| i.text == "Process").expect("Process").frame;
    let shift = (process(&got).x - process(&exp).x, process(&got).y - process(&exp).y);
    assert_same_items(&exp, &got, shift);
    assert_eq!(links(&imp.doc.pages[0]), expected_links(true));
}

fn check_excalidraw(connector_label: bool) {
    let d = reference(connector_label);
    let imp = import_any("ref.excalidraw", &excalidraw::export(&d, &d.pages[0], &ApproxMeasurer), &ApproxMeasurer).expect("import");
    assert_eq!(imp.report.skipped, 0);
    assert_same_items(&items(&d.pages[0]), &items(&imp.doc.pages[0]), (0.0, 0.0));
    assert_eq!(links(&imp.doc.pages[0]), expected_links(connector_label));
}

#[test]
fn excalidraw_export_is_read_back_identically() {
    check_excalidraw(false);
}

#[test]
fn excalidraw_connector_label_is_read_back_once() {
    check_excalidraw(true);
}

#[test]
fn json_canvas_export_is_read_back_identically() {
    let d = reference(true);
    let (json, dropped) = jsoncanvas::export(&d.pages[0]);
    assert_eq!(dropped, 2, "the table and the image are reported as dropped");
    let imp = import_any("ref.canvas", &json, &ApproxMeasurer).expect("import");
    assert_eq!(imp.report.skipped, 0);
    assert_same_items(&canvas_items(&d.pages[0]), &canvas_items(&imp.doc.pages[0]), (0.0, 0.0));
    assert_eq!(links(&imp.doc.pages[0]), expected_links(true));
}

/* ----------------------------- connectors bound to an image or to a table */

/// A document with rectangle "A" and `target` on its right, joined by a bound connector.
fn link_to(mut d: Document, target: Object) -> Document {
    let a = shape(BoxGeom::Rect, Frame::new(100.0, 100.0, 100.0, 60.0), "A");
    let c = Object::new(Kind::Connector(Connector::new(End::bound(a.id.clone()), End::bound(target.id.clone()))), Style::default());
    d.pages[0].objects.extend([a, target, c]);
    d
}

fn image_object(d: &mut Document) -> Object {
    let aid = d.add_asset("image/png", PNG.to_vec());
    Object::new(
        Kind::Image(ImageObj { frame: Frame::new(400.0, 100.0, 80.0, 60.0), asset: aid, latex: None, mono: false }),
        Style { width: 0.0, ..Default::default() },
    )
}

/// After a round trip the connector must still exist and be bound at both ends.
fn assert_link_survives(imp: &inkboard_formats::Imported) {
    let conns: Vec<&Connector> = imp.doc.pages[0].objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
    assert_eq!(conns.len(), 1, "the connector is kept (report: {:?})", imp.report);
    assert_eq!(conns[0].targets().count(), 2, "both ends stay bound");
}

fn link_to_image() -> Document {
    let mut d = Document::new("Link");
    let img = image_object(&mut d);
    link_to(d, img)
}

fn link_to_table() -> Document {
    link_to(Document::new("Link"), table_object(Pt::new(400.0, 100.0)))
}

#[test]
fn drawio_keeps_a_connector_bound_to_an_image() {
    let d = link_to_image();
    assert_link_survives(&import_any("l.drawio", &drawio::export(&d, &ApproxMeasurer), &ApproxMeasurer).unwrap());
}

#[test]
fn excalidraw_keeps_a_connector_bound_to_an_image() {
    let d = link_to_image();
    assert_link_survives(&import_any("l.excalidraw", &excalidraw::export(&d, &d.pages[0], &ApproxMeasurer), &ApproxMeasurer).unwrap());
}

#[test]
fn drawio_keeps_a_connector_bound_to_a_table() {
    let d = link_to_table();
    assert_link_survives(&import_any("l.drawio", &drawio::export(&d, &ApproxMeasurer), &ApproxMeasurer).unwrap());
}

#[test]
fn excalidraw_keeps_a_connector_bound_to_a_table() {
    let d = link_to_table();
    assert_link_survives(&import_any("l.excalidraw", &excalidraw::export(&d, &d.pages[0], &ApproxMeasurer), &ApproxMeasurer).unwrap());
}
