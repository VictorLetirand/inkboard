//! JSON Canvas (`.canvas`, Obsidian's open format): text nodes, groups and links.

use crate::common::{FormatError, Imported, Report, parse_color};
use inkboard_core::geom::Rect;
use inkboard_core::text::Measurer;
use inkboard_core::{Align, Anchor, Arrowhead, BoxGeom, BoxObj, Color, Connector, Dash, Document, End, Frame, Id, Kind, Label, Object, Page, Pt, Style};
use serde_json::{Value, json};
use std::collections::HashMap;

const PRESETS: [(&str, Color); 6] = [
    ("1", Color::Rgb(0xe0, 0x31, 0x31)),
    ("2", Color::Rgb(0xf0, 0x8c, 0x00)),
    ("3", Color::Rgb(0xf2, 0xc6, 0x00)),
    ("4", Color::Rgb(0x2f, 0x9e, 0x44)),
    ("5", Color::Rgb(0x0c, 0x85, 0x99)),
    ("6", Color::Rgb(0x70, 0x48, 0xe8)),
];

fn color_of(v: Option<&str>) -> Option<Color> {
    let v = v?;
    PRESETS.iter().find(|(k, _)| *k == v).map(|(_, c)| *c).or_else(|| parse_color(v))
}

fn side_anchor(side: &str) -> Option<Anchor> {
    Some(match side {
        "top" => Anchor::Frac { fx: 0.5, fy: 0.0 },
        "bottom" => Anchor::Frac { fx: 0.5, fy: 1.0 },
        "left" => Anchor::Frac { fx: 0.0, fy: 0.5 },
        "right" => Anchor::Frac { fx: 1.0, fy: 0.5 },
        _ => return None,
    })
}

fn side_of(frame: &Frame, p: Pt) -> &'static str {
    let l = frame.to_local(p);
    let (dx, dy) = (l.x / frame.w.max(1.0), l.y / frame.h.max(1.0));
    if dx.abs() > dy.abs() {
        if dx > 0.0 { "right" } else { "left" }
    } else if dy > 0.0 {
        "bottom"
    } else {
        "top"
    }
}

/// Exports a page. Only "box" objects and connectors linking two boxes can be represented;
/// the second result is the number of skipped elements.
#[must_use]
pub fn export(page: &Page) -> (String, usize) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut dropped = 0;
    let boxes: Vec<(&Object, &BoxObj)> =
        page.objects.iter().filter(|o| page.is_visible(o)).filter_map(|o| if let Kind::Box(b) = &o.kind { Some((o, b)) } else { None }).collect();
    for (o, b) in &boxes {
        let r = b.frame.aabb();
        let mut n = json!({"id": o.id.as_str(), "type": "text", "text": b.label.text.to_plain(), "x": r.x.round(), "y": r.y.round(), "width": r.w.round().max(60.0), "height": r.h.round().max(40.0)});
        let color = match b.geom {
            BoxGeom::Sticky => o.style.fill,
            _ => o.style.fill.or((o.style.stroke != Color::Ink).then_some(o.style.stroke)),
        };
        if let Some(c) = color {
            n["color"] = json!(c.resolve_hex(false));
        }
        nodes.push(n);
    }
    let near = |p: Pt| -> Option<&Object> {
        boxes
            .iter()
            .filter_map(|(o, b)| {
                let r = b.frame.aabb();
                let (dx, dy) = ((r.x - p.x).max(0.0).max(p.x - r.right()), (r.y - p.y).max(0.0).max(p.y - r.bottom()));
                let d = dx.hypot(dy);
                (d <= 18.0).then_some((*o, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    };
    for o in &page.objects {
        match &o.kind {
            Kind::Connector(c) => {
                let Some(path) = page.connector_path(c) else {
                    dropped += 1;
                    continue;
                };
                let (a, z) = (path[0], path[path.len() - 1]);
                let ends = |e: &End, p: Pt| -> Option<&Object> { e.target().and_then(|t| page.get(t)).or_else(|| near(p)) };
                let (Some(oa), Some(oz)) = (ends(&c.start, a), ends(&c.end, z)) else {
                    dropped += 1;
                    continue;
                };
                let (Some(fa), Some(fz)) = (oa.frame(), oz.frame()) else {
                    dropped += 1;
                    continue;
                };
                if oa.id == oz.id || !matches!(oa.kind, Kind::Box(_)) || !matches!(oz.kind, Kind::Box(_)) {
                    dropped += 1;
                    continue;
                }
                let mut e = json!({"id": o.id.as_str(), "fromNode": oa.id.as_str(), "toNode": oz.id.as_str(), "fromSide": side_of(&fa, a), "toSide": side_of(&fz, z),
                    "fromEnd": if c.has_head_start() { "arrow" } else { "none" }, "toEnd": if c.has_head_end() { "arrow" } else { "none" }});
                if let Some(l) = c.label.as_ref().filter(|l| !l.text.is_empty()) {
                    e["label"] = json!(l.text.to_plain());
                }
                edges.push(e);
            }
            Kind::Pen(_) | Kind::Image(_) | Kind::Table(_) => dropped += 1,
            Kind::Box(_) => {}
        }
    }
    (format!("{:#}", json!({"nodes": nodes, "edges": edges})), dropped)
}

/// Imports a JSON Canvas file.
///
/// # Errors
///
/// [`FormatError`] when the content is not a JSON Canvas document.
pub fn import(content: &str, name: &str, measurer: &dyn Measurer) -> Result<Imported, FormatError> {
    let root: Value = serde_json::from_str(content)?;
    let nodes = root.get("nodes").and_then(Value::as_array).ok_or_else(|| FormatError::Invalid("no list of nodes".into()))?;
    let mut doc = Document::new(name);
    let mut report = Report::default();
    let mut out: Vec<Object> = Vec::new();
    let mut ids: HashMap<String, (Id, Rect)> = HashMap::new();
    for n in nodes {
        let field = |k: &str| n.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let (x, y, w, h) = (field("x"), field("y"), field("width").max(10.0), field("height").max(10.0));
        let color = color_of(n.get("color").and_then(Value::as_str));
        let kind = n.get("type").and_then(Value::as_str).unwrap_or("text");
        let style = Style {
            stroke: color.unwrap_or(Color::Ink),
            width: if kind == "group" { 2.0 } else { 3.0 },
            dash: if kind == "group" { Dash::Dashed } else { Dash::Solid },
            roundness: 0.3,
            ..Style::default()
        };
        let text = match kind {
            "text" => n.get("text").and_then(Value::as_str).unwrap_or("").to_owned(),
            "file" => format!("📄 {}", n.get("file").and_then(Value::as_str).unwrap_or("")),
            "link" => n.get("url").and_then(Value::as_str).unwrap_or("").to_owned(),
            "group" => String::new(),
            _ => {
                report.skipped += 1;
                continue;
            }
        };
        if matches!(kind, "file" | "link") {
            report.approximated += 1;
        }
        let mut obj =
            Object::new_box(BoxGeom::Rect, Frame::new(x, y, w, h), style, Label::new(&text, 16.0, if kind == "text" { Align::Left } else { Align::Center }));
        if let (Some(id), true) = (n.get("id").and_then(Value::as_str), kind != "group") {
            ids.insert(id.to_owned(), (obj.id.clone(), Rect::new(x, y, w, h)));
        }
        out.push(obj.clone());
        if kind == "group"
            && let Some(l) = n.get("label").and_then(Value::as_str).filter(|l| !l.is_empty())
        {
            obj = Object::new_box(
                BoxGeom::Text,
                Frame::new(x + 8.0, y - 26.0, 10.0, 10.0),
                Style { stroke: color.unwrap_or(Color::Ink), width: 0.0, ..Style::default() },
                Label::new(l, 16.0, Align::Left),
            );
            if let Kind::Box(b) = &mut obj.kind {
                b.fit(measurer);
            }
            out.push(obj);
        }
    }
    for e in root.get("edges").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
        let get = |k: &str| e.get(k).and_then(Value::as_str);
        let (Some((a, _)), Some((z, _))) = (get("fromNode").and_then(|i| ids.get(i)), get("toNode").and_then(|i| ids.get(i))) else {
            report.skipped += 1;
            continue;
        };
        let bound = |id: &Id, side: Option<&str>| End::Bound { target: id.clone(), anchor: side.and_then(side_anchor).unwrap_or(Anchor::Auto) };
        let mut c = Connector::new(bound(a, get("fromSide")), bound(z, get("toSide")));
        c.head_start = if get("fromEnd").is_some_and(|v| v != "none") { Arrowhead::Triangle } else { Arrowhead::None };
        c.head_end = if get("toEnd").is_none_or(|v| v != "none") { Arrowhead::Triangle } else { Arrowhead::None };
        if let Some(l) = get("label").filter(|l| !l.is_empty()) {
            c.label = Some(Label::new(l, 14.0, Align::Center));
        }
        out.push(Object::new(Kind::Connector(c), Style { stroke: color_of(get("color")).unwrap_or(Color::Ink), width: 2.5, ..Style::default() }));
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

    #[test]
    fn export_keeps_text_nodes_and_linked_edges_and_counts_dropped() {
        let d = sample_document();
        let (json, dropped) = export(&d.pages[0]);
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["nodes"].as_array().unwrap().len(), 6);
        let edges = v["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 1, "only the arrow bound to two boxes can be represented");
        assert_eq!(edges[0]["label"], "yes");
        assert_eq!(edges[0]["toEnd"], "arrow");
        assert_eq!(edges[0]["fromEnd"], "none");
        assert_eq!(dropped, 3, "free arrow + pen + image");
    }

    #[test]
    fn import_builds_bound_edges_groups_and_colors() {
        let src = r##"{"nodes":[
          {"id":"a","type":"text","text":"Un","x":0,"y":0,"width":200,"height":80,"color":"4"},
          {"id":"b","type":"text","text":"Deux","x":400,"y":0,"width":200,"height":80,"color":"#ff0000"},
          {"id":"f","type":"file","file":"notes/a.md","x":0,"y":200,"width":200,"height":80},
          {"id":"g","type":"group","label":"Groupe","x":-20,"y":-40,"width":700,"height":400},
          {"id":"x","type":"unknown","x":0,"y":0,"width":1,"height":1}
        ],"edges":[
          {"id":"e1","fromNode":"a","toNode":"b","fromSide":"right","toSide":"left","label":"lien"},
          {"id":"e2","fromNode":"a","toNode":"missing"},
          {"id":"e3","fromNode":"b","toNode":"f","toEnd":"none","fromEnd":"arrow"}
        ]}"##;
        let r = import(src, "ob", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        assert_eq!(r.report.skipped, 2, "unknown type + broken edge");
        assert_eq!(r.report.approximated, 1);
        let conns: Vec<&Connector> = p.objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!(conns.len(), 2);
        assert!(matches!(conns[0].start, End::Bound { anchor: Anchor::Frac { fx, .. }, .. } if (fx - 1.0).abs() < 1e-9));
        assert_eq!(conns[0].head_end, Arrowhead::Triangle, "toEnd by default = arrow");
        assert_eq!(conns[1].head_end, Arrowhead::None);
        assert_eq!(conns[1].head_start, Arrowhead::Triangle);
        assert!(p.connector_path(conns[0]).is_some());
        let a = p.objects.iter().find(|o| matches!(&o.kind, Kind::Box(b) if b.label.text.to_plain() == "Un")).unwrap();
        assert_eq!(a.style.stroke, Color::Rgb(0x2f, 0x9e, 0x44));
        assert!(p.objects.iter().any(|o| o.style.dash == Dash::Dashed), "dotted group");
        assert!(p.objects.iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.label.text.to_plain() == "Groupe")));
    }

    #[test]
    fn roundtrip_through_canvas() {
        let d = sample_document();
        let (json, _) = export(&d.pages[0]);
        let r = import(&json, "x", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        assert_eq!(p.objects.iter().filter(|o| matches!(o.kind, Kind::Box(_))).count(), 6);
        assert_eq!(p.objects.iter().filter(|o| matches!(o.kind, Kind::Connector(_))).count(), 1);
    }

    #[test]
    fn rejects_garbage() {
        assert!(import("{}", "x", &ApproxMeasurer).is_err());
    }
}
