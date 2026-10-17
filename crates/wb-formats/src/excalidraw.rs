//! Excalidraw (`.excalidraw`, JSON).
//!
//! Export : formes + texte lié (« containerId »), flèches avec liaisons, traits libres, images.
//! Import : mêmes éléments ; un polygone ou une polyligne devient un connecteur à points de passage.

use crate::common::{FormatError, Imported, Report, data_url, parse_color, parse_data_url};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use wb_core::geom::Rect;
use wb_core::text::{Measurer, layout_rich};
use wb_core::*;

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

fn xhead(h: Arrowhead) -> Value {
    match h {
        Arrowhead::None => Value::Null,
        Arrowhead::Arrow => json!("arrow"),
        Arrowhead::Triangle => json!("triangle"),
        Arrowhead::Dot => json!("dot"),
        Arrowhead::Bar => json!("bar"),
    }
}

fn head_back(v: Option<&Value>) -> Arrowhead {
    match v.and_then(Value::as_str) {
        None => Arrowhead::None,
        Some("arrow") => Arrowhead::Arrow,
        Some("dot" | "circle" | "circle_outline") => Arrowhead::Dot,
        Some("bar") => Arrowhead::Bar,
        Some(_) => Arrowhead::Triangle,
    }
}

/* ===================================================================== Export */

/// Exporte une page au format Excalidraw (une scène = une page).
pub fn export(doc: &Document, page: &Page, m: &dyn Measurer) -> String {
    let dark = doc.background.preset.is_dark();
    let col = |c: Color| c.resolve_hex(dark);
    let mut els: Vec<Map<String, Value>> = Vec::new();
    let mut files = Map::new();
    let mut bound_to: HashMap<String, Vec<Value>> = HashMap::new();
    let skip_bind: Vec<&Id> = page
        .objects
        .iter()
        .filter(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Triangle) || matches!(o.kind, Kind::Pen(_) | Kind::Connector(_)))
        .map(|o| &o.id)
        .collect();

    let base = |o: &Object, ty: &str, x: f64, y: f64, w: f64, h: f64, angle: f64| -> Map<String, Value> {
        let st = &o.style;
        let sw = (st.width / 1.6).round().max(1.0);
        let v = json!({
            "id": o.id.as_str(), "type": ty, "x": x, "y": y, "width": w, "height": h, "angle": angle,
            "strokeColor": if st.width > 0.0 || ty == "text" { col(st.stroke) } else { "transparent".to_owned() },
            "backgroundColor": st.fill.map_or("transparent".to_owned(), col),
            "fillStyle": "solid", "strokeWidth": sw,
            "strokeStyle": match st.dash { Dash::Solid => "solid", Dash::Dashed => "dashed", Dash::Dotted => "dotted" },
            "roughness": 0, "opacity": (st.opacity * 100.0).round(),
            "groupIds": o.group.as_ref().map(|g| vec![g.as_str().to_owned()]).unwrap_or_default(),
            "frameId": null, "roundness": null, "seed": seed_of(o.id.as_str()), "version": 1, "versionNonce": seed_of(&format!("n{}", o.id)),
            "isDeleted": false, "boundElements": null, "updated": 1, "link": null, "locked": o.locked,
        });
        let Value::Object(map) = v else { unreachable!() };
        map
    };
    let text_el = |o: &Object,
                   label: &Label,
                   area: Rect,
                   container: Option<&str>,
                   frame_angle: f64,
                   standalone_pos: Option<Pt>,
                   color: Color|
     -> Option<Map<String, Value>> {
        if label.text.is_empty() {
            return None;
        }
        let wrap = container.map(|_| area.w);
        let lay = layout_rich(&label.text, wrap, m, label.family, label.size);
        let text = lay.lines.iter().map(|l| l.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n");
        let (x, y) = match standalone_pos {
            Some(p) => (p.x, p.y),
            None => (area.center().x - lay.width / 2.0, area.center().y - lay.height / 2.0),
        };
        let id = Id::new();
        let mut t = base(
            &Object { id: id.clone(), style: Style { width: 1.0, fill: None, stroke: color, ..o.style.clone() }, ..o.clone() },
            "text",
            x,
            y,
            lay.width.max(4.0),
            lay.height,
            frame_angle,
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
    };
    for o in &page.objects {
        if !page.is_visible(o) {
            continue;
        }
        match &o.kind {
            Kind::Box(b) => {
                let f = b.frame;
                let lab = b.layout(m);
                let _ = lab;
                let lr = b.label_rect();
                let area_world = {
                    let c = f.to_world(Pt::new(lr.center().x, lr.center().y));
                    Rect::new(c.x - lr.w / 2.0, c.y - lr.h / 2.0, lr.w, lr.h)
                };
                match b.geom {
                    BoxGeom::Text => {
                        if let Some(mut t) = text_el(o, &b.label, area_world, None, f.angle, Some(Pt::new(f.x, f.y)), o.style.stroke) {
                            t.insert("id".into(), json!(o.id.as_str()));
                            els.push(t);
                        }
                    }
                    BoxGeom::Triangle => {
                        let mut e = base(o, "line", f.x, f.y, f.w, f.h, f.angle);
                        e.insert("points".into(), json!([[f.w / 2.0, 0.0], [f.w, f.h], [0.0, f.h], [f.w / 2.0, 0.0]]));
                        e.insert("polygon".into(), json!(true));
                        for k in ["startBinding", "endBinding", "lastCommittedPoint"] {
                            e.insert(k.into(), Value::Null);
                        }
                        e.insert("startArrowhead".into(), Value::Null);
                        e.insert("endArrowhead".into(), Value::Null);
                        els.push(e);
                        if let Some(t) = text_el(o, &b.label, area_world, None, f.angle, None, o.style.stroke) {
                            els.push(t);
                        }
                    }
                    geom => {
                        let ty = match geom {
                            BoxGeom::Ellipse => "ellipse",
                            BoxGeom::Diamond => "diamond",
                            _ => "rectangle",
                        };
                        let mut e = base(o, ty, f.x, f.y, f.w, f.h, f.angle);
                        if geom == BoxGeom::Rect && o.style.roundness > 0.0 {
                            e.insert("roundness".into(), json!({"type": 3}));
                        }
                        if geom == BoxGeom::Sticky {
                            e.insert("strokeColor".into(), json!("transparent"));
                            e.insert("backgroundColor".into(), json!(col(o.style.fill.unwrap_or(wb_core::style::NOTE_YELLOW))));
                        }
                        let color = if geom == BoxGeom::Sticky { Color::Rgb(0x1e, 0x1e, 0x1e) } else { o.style.stroke };
                        let t = text_el(o, &b.label, area_world, Some(o.id.as_str()), f.angle, None, color);
                        if let Some(t) = &t {
                            e.insert("boundElements".into(), json!([{"id": t["id"], "type": "text"}]));
                        }
                        els.push(e);
                        els.extend(t);
                    }
                }
            }
            Kind::Pen(p) => {
                let Some(first) = p.pts.first() else { continue };
                let bb = Rect::from_points(p.pts.iter().copied()).unwrap_or_default();
                let mut e = base(o, "freedraw", first.x, first.y, bb.w, bb.h, 0.0);
                e.insert("points".into(), json!(p.pts.iter().map(|q| [q.x - first.x, q.y - first.y]).collect::<Vec<_>>()));
                e.insert("pressures".into(), json!(p.pressures.as_ref().map(|v| v.as_slice()).unwrap_or(&[])));
                e.insert("simulatePressure".into(), json!(p.pressures.is_none()));
                e.insert("lastCommittedPoint".into(), Value::Null);
                els.push(e);
            }
            Kind::Image(i) => {
                let Some(a) = doc.assets.get(&i.asset) else { continue };
                let fid = i.asset.as_str().to_owned();
                files.insert(fid.clone(), json!({"mimeType": a.mime, "id": fid, "dataURL": data_url(&a.mime, &a.data), "created": 1, "lastRetrieved": 1}));
                let mut e = base(o, "image", i.frame.x, i.frame.y, i.frame.w, i.frame.h, i.frame.angle);
                e.insert("fileId".into(), json!(fid));
                e.insert("status".into(), json!("saved"));
                e.insert("scale".into(), json!([1, 1]));
                e.insert("crop".into(), Value::Null);
                els.push(e);
            }
            Kind::Table(t) => {
                for r in 0..t.rows() {
                    for c in 0..t.cols() {
                        let cr = t.cell_rect(r, c);
                        let cell_id = Id::new();
                        let cell_obj = Object {
                            id: cell_id.clone(),
                            style: Style { fill: (t.header && r == 0).then_some(Color::rgb(0xde, 0xe2, 0xe6)), ..o.style.clone() },
                            ..o.clone()
                        };
                        let mut e = base(&cell_obj, "rectangle", cr.x, cr.y, cr.w, cr.h, 0.0);
                        let label = Label { text: t.cell(r, c).cloned().unwrap_or_default(), family: t.family, size: t.size, align: Align::Left };
                        let t_el = text_el(
                            &cell_obj,
                            &label,
                            Rect::new(cr.x + 8.0, cr.y + 4.0, cr.w - 16.0, cr.h - 8.0),
                            Some(cell_id.as_str()),
                            0.0,
                            None,
                            o.style.stroke,
                        );
                        if let Some(t_el) = &t_el {
                            e.insert("boundElements".into(), json!([{"id": t_el["id"], "type": "text"}]));
                        }
                        els.push(e);
                        els.extend(t_el);
                    }
                }
            }
            Kind::Connector(c) => {
                let Some(path) = page.connector_path(c) else { continue };
                let (x0, y0) = (path[0].x, path[0].y);
                let bb = Rect::from_points(path.iter().copied()).unwrap_or_default();
                let arrow = c.head_start != Arrowhead::None || c.head_end != Arrowhead::None || c.start.target().is_some() || c.end.target().is_some();
                let mut e = base(o, if arrow { "arrow" } else { "line" }, x0, y0, bb.w, bb.h, 0.0);
                e.insert("points".into(), json!(path.iter().map(|q| [q.x - x0, q.y - y0]).collect::<Vec<_>>()));
                e.insert("lastCommittedPoint".into(), Value::Null);
                e.insert("startArrowhead".into(), xhead(c.head_start));
                e.insert("endArrowhead".into(), xhead(c.head_end));
                e.insert("elbowed".into(), json!(c.route == Route::Elbow));
                if c.route != Route::Straight {
                    e.insert("roundness".into(), json!({"type": 2}));
                }
                for (end, key) in [(&c.start, "startBinding"), (&c.end, "endBinding")] {
                    let bind = end.target().filter(|t| !skip_bind.contains(t)).filter(|t| page.get(t).is_some());
                    match bind {
                        Some(t) => {
                            e.insert(key.into(), json!({"elementId": t.as_str(), "focus": 0, "gap": 4}));
                            bound_to.entry(t.as_str().to_owned()).or_default().push(json!({"id": o.id.as_str(), "type": "arrow"}));
                        }
                        None => {
                            e.insert(key.into(), Value::Null);
                        }
                    }
                }
                let label_el = c.label.as_ref().and_then(|l| {
                    let (mid, _) = wb_core::connector::path_midpoint(&path);
                    text_el(o, l, Rect::new(mid.x - 40.0, mid.y - 10.0, 80.0, 20.0), Some(o.id.as_str()), 0.0, None, o.style.stroke)
                });
                if let Some(t) = &label_el {
                    e.insert("boundElements".into(), json!([{"id": t["id"], "type": "text"}]));
                }
                els.push(e);
                els.extend(label_el);
            }
        }
    }
    for e in &mut els {
        let Some(id) = e.get("id").and_then(Value::as_str).map(str::to_owned) else { continue };
        if let Some(extra) = bound_to.remove(&id) {
            let mut list = e.get("boundElements").and_then(Value::as_array).cloned().unwrap_or_default();
            list.extend(extra);
            e.insert("boundElements".into(), Value::Array(list));
        }
    }
    let scene = json!({
        "type": "excalidraw", "version": 2, "source": "tableau-blanc",
        "elements": els,
        "appState": {"gridSize": null, "viewBackgroundColor": doc.background.preset.hex(), "theme": if dark { "dark" } else { "light" }},
        "files": files,
    });
    serde_json::to_string(&scene).expect("scène sérialisable")
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
        opacity: (e.get("opacity").and_then(Value::as_f64).unwrap_or(100.0) / 100.0).clamp(0.05, 1.0) as f32,
        roundness: 0.0,
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

pub fn import(content: &str, name: &str, m: &dyn Measurer) -> Result<Imported, FormatError> {
    let v: Value = serde_json::from_str(content)?;
    if text(&v, "type") != Some("excalidraw") {
        return Err(FormatError::Invalid("pas une scène Excalidraw".into()));
    }
    let els: Vec<&Value> = v
        .get("elements")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|e| !e.get("isDeleted").and_then(Value::as_bool).unwrap_or(false)).collect())
        .unwrap_or_default();
    let files = v.get("files").and_then(Value::as_object);
    let mut report = Report::default();
    let mut doc = Document::new(name);
    if let Some(bg) = v.pointer("/appState/viewBackgroundColor").and_then(Value::as_str).and_then(parse_color) {
        doc.background.preset = BgPreset::nearest(bg.resolve(false));
    }
    // identifiants pré-alloués : permet de résoudre les liaisons quel que soit l'ordre
    let ids: HashMap<&str, Id> = els.iter().filter_map(|e| text(e, "id").map(|i| (i, Id::new()))).collect();
    let kind_of: HashMap<&str, &str> = els.iter().filter_map(|e| Some((text(e, "id")?, text(e, "type")?))).collect();
    let mut labels: HashMap<&str, &Value> = HashMap::new();
    for e in &els {
        if text(e, "type") == Some("text")
            && let Some(c) = text(e, "containerId")
        {
            labels.insert(c, e);
        }
    }
    let mut out: Vec<Object> = Vec::new();
    for e in &els {
        let id = text(e, "id").unwrap_or("");
        let ty = text(e, "type").unwrap_or("");
        let (x, y, w, h) = (num(e, "x"), num(e, "y"), num(e, "width"), num(e, "height"));
        let angle = num(e, "angle");
        match ty {
            "rectangle" | "ellipse" | "diamond" => {
                let geom = match ty {
                    "ellipse" => BoxGeom::Ellipse,
                    "diamond" => BoxGeom::Diamond,
                    _ => BoxGeom::Rect,
                };
                let mut st = style_of(e, false);
                if geom == BoxGeom::Rect && e.get("roundness").is_some_and(|r| !r.is_null()) {
                    st.roundness = 0.45;
                }
                let mut label = labels.get(id).map(|t| label_of(t)).unwrap_or_default();
                if !labels.contains_key(id) {
                    label.align = Align::Center;
                } else if st.width == 0.0
                    && let Some(c) = labels.get(id).and_then(|t| text(t, "strokeColor")).and_then(parse_color)
                {
                    st.stroke = c;
                }
                let mut o = Object::new_box(geom, Frame { x, y, w, h, angle }, st, label);
                o.id = ids[id].clone();
                out.push(finish(o, e));
            }
            "text" => {
                let container = text(e, "containerId");
                if container.is_some_and(|c| {
                    labels.get(c).is_some_and(|t| std::ptr::eq(*t, *e)) && matches!(kind_of.get(c), Some(&("rectangle" | "ellipse" | "diamond")))
                }) {
                    continue;
                }
                let mut o = Object::new_box(BoxGeom::Text, Frame::new(x, y, w, h), style_of(e, true), label_of(e));
                if let Kind::Box(b) = &mut o.kind {
                    b.fit(m);
                    b.frame.angle = angle;
                }
                o.style.stroke = text(e, "strokeColor").and_then(parse_color).unwrap_or(Color::Ink);
                o.id = ids[id].clone();
                out.push(finish(o, e));
            }
            "line" | "arrow" => {
                let rel: Vec<Pt> = e
                    .get("points")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|p| Some(Pt::new(p.get(0)?.as_f64()?, p.get(1)?.as_f64()?))).collect())
                    .unwrap_or_default();
                if rel.len() < 2 {
                    report.skipped += 1;
                    continue;
                }
                let mut pts: Vec<Pt> = rel.iter().map(|p| Pt::new(x + p.x, y + p.y)).collect();
                if angle != 0.0 {
                    let c = Rect::from_points(pts.iter().copied()).map(|r| r.center()).unwrap_or_default();
                    pts.iter_mut().for_each(|p| *p = p.rotate(c, angle));
                }
                let closed = pts.len() == 4 && pts[0].dist(pts[3]) < 2.0 && ty == "line";
                if closed && angle == 0.0 {
                    let bb = Rect::from_points(pts.iter().copied()).unwrap_or_default();
                    let apex_up = (pts[0].x - bb.center().x).abs() < bb.w * 0.1 && (pts[0].y - bb.y).abs() < 1.0;
                    if apex_up {
                        let mut o = Object::new_box(BoxGeom::Triangle, Frame::from_rect(bb), style_of(e, false), Label::default());
                        o.id = ids[id].clone();
                        out.push(finish(o, e));
                        continue;
                    }
                }
                let bind = |key: &str| -> Option<End> {
                    let t = e.get(key)?.get("elementId")?.as_str()?;
                    (matches!(kind_of.get(t), Some(&("rectangle" | "ellipse" | "diamond" | "image" | "text")))).then(|| End::bound(ids[t].clone()))
                };
                let mut c = Connector::new(
                    bind("startBinding").unwrap_or_else(|| End::free(pts[0])),
                    bind("endBinding").unwrap_or_else(|| End::free(pts[pts.len() - 1])),
                );
                c.via = pts[1..pts.len() - 1].to_vec();
                c.head_end = Arrowhead::None;
                if ty == "arrow" {
                    c.head_end = if e.get("endArrowhead").is_none() { Arrowhead::Arrow } else { head_back(e.get("endArrowhead")) };
                    c.head_start = head_back(e.get("startArrowhead"));
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
                if let Some(t) = labels.get(id) {
                    c.label = Some(Label { align: Align::Center, ..label_of(t) });
                }
                let mut o = Object::new(Kind::Connector(c), Style { fill: None, ..style_of(e, false) });
                o.id = ids[id].clone();
                out.push(finish(o, e));
            }
            "freedraw" => {
                let mut pts: Vec<Pt> = e
                    .get("points")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|p| Some(Pt::new(x + p.get(0)?.as_f64()?, y + p.get(1)?.as_f64()?))).collect())
                    .unwrap_or_default();
                if pts.is_empty() {
                    report.skipped += 1;
                    continue;
                }
                let mut pressures: Vec<f32> =
                    e.get("pressures").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect()).unwrap_or_default();
                if pts.len() == 1 {
                    pts.push(pts[0]);
                    pressures.extend(pressures.first().copied());
                }
                if angle != 0.0 {
                    let c = Rect::from_points(pts.iter().copied()).map(|r| r.center()).unwrap_or_default();
                    pts.iter_mut().for_each(|p| *p = p.rotate(c, angle));
                }
                let pen_style = Style { dash: Dash::Solid, fill: None, ..style_of(e, false) };
                let mut o = if pressures.len() == pts.len() {
                    Object::new_pen_with_pressure(pts, pressures, false, pen_style)
                } else {
                    Object::new_pen(pts, false, pen_style)
                };
                o.style.width = (num(e, "strokeWidth").max(1.0) * 1.5).max(1.5);
                o.id = ids[id].clone();
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
                    Kind::Image(ImageObj { frame: Frame { x, y, w, h, angle }, asset: aid, latex: None }),
                    Style { width: 0.0, opacity: style_of(e, false).opacity, ..Style::default() },
                );
                o.id = ids[id].clone();
                out.push(finish(o, e));
            }
            "frame" | "magicframe" => {
                report.approximated += 1;
                let mut o =
                    Object::new_box(BoxGeom::Rect, Frame::new(x, y, w, h), Style { dash: Dash::Dashed, width: 2.0, ..Style::default() }, Label::default());
                o.id = ids[id].clone();
                out.push(o);
                if let Some(n) = text(e, "name").filter(|n| !n.is_empty()) {
                    let mut t = Object::new_box(
                        BoxGeom::Text,
                        Frame::new(x, y - 26.0, 10.0, 10.0),
                        Style { width: 0.0, ..Style::default() },
                        Label::new(n, 16.0, Align::Left),
                    );
                    if let Kind::Box(b) = &mut t.kind {
                        b.fit(m);
                    }
                    out.push(t);
                }
            }
            "text_container_placeholder" => {}
            _ => report.skipped += 1,
        }
    }
    doc.pages[0].objects = out;
    wb_core::serial::sanitize(&mut doc);
    Ok(Imported { doc, report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_document;
    use wb_core::text::ApproxMeasurer;

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
            assert!(types.contains(&t), "{t} absent de {types:?}");
        }
        assert_eq!(v["files"].as_object().unwrap().len(), 1);
        // le texte d'un rectangle est lié à son conteneur
        let rect = els.iter().find(|e| e["type"] == "rectangle").unwrap();
        let tid = rect["boundElements"][0]["id"].as_str().unwrap();
        let t = els.iter().find(|e| e["id"] == tid).unwrap();
        assert_eq!(t["containerId"], rect["id"]);
        // le rectangle référence aussi la flèche qui s'y accroche
        assert!(rect["boundElements"].as_array().unwrap().iter().any(|b| b["type"] == "arrow"));
    }

    #[test]
    fn roundtrip_keeps_shapes_text_and_bindings() {
        let d = sample_document();
        let r = export_import(&d);
        let p = &r.doc.pages[0];
        let boxes: Vec<&BoxObj> = p.objects.iter().filter_map(|o| if let Kind::Box(b) = &o.kind { Some(b) } else { None }).collect();
        assert!(boxes.iter().any(|b| b.geom == BoxGeom::Rect && b.label.text.to_plain() == "Début\nligne 2"));
        assert!(boxes.iter().any(|b| b.geom == BoxGeom::Ellipse && b.label.text.to_plain() == "Fin"));
        assert!(boxes.iter().any(|b| b.geom == BoxGeom::Triangle), "triangle reconnu");
        assert!(boxes.iter().any(|b| b.geom == BoxGeom::Text && b.label.text.to_plain() == "Titre libre"));
        let conns: Vec<&Connector> = p.objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!(conns.len(), 2);
        assert!(conns[0].targets().count() == 2, "flèche liée aux deux formes");
        assert!(p.connector_path(conns[0]).is_some());
        assert!(conns[1].targets().count() == 1 && matches!(conns[1].end, End::Free { .. }));
        assert_eq!(conns[0].label.as_ref().map(|l| l.text.to_plain()), Some("oui".into()));
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
        assert_eq!(r.report.skipped, 1, "embeddable ignoré");
        assert_eq!(r.report.approximated, 1, "cadre approximé");
        let rect = p
            .objects
            .iter()
            .find_map(|o| if let Kind::Box(b) = &o.kind { (b.geom == BoxGeom::Rect && b.label.text.to_plain() == "Hello").then_some((o, b)) } else { None })
            .expect("rectangle avec son texte");
        assert!(rect.0.style.roundness > 0.0);
        assert_eq!(rect.1.label.family, FontFamily::Hand);
        assert_eq!(rect.1.label.align, Align::Center);
        assert!(
            !p.objects.iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Text && b.label.text.to_plain() == "Hello")),
            "le texte lié ne reste pas en double"
        );
        let conns: Vec<&Connector> = p.objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!(conns.len(), 2);
        assert_eq!(conns[0].route, Route::Elbow);
        assert_eq!(conns[0].head_end, Arrowhead::Triangle);
        assert_eq!(conns[0].targets().count(), 2);
        assert_eq!(conns[1].via.len(), 1, "polyligne = connecteur à point de passage");
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
        let mut d = wb_core::Document::new("p");
        let pts = vec![Pt::new(0.0, 0.0), Pt::new(10.0, 5.0), Pt::new(20.0, 0.0)];
        d.pages[0].objects.push(Object::new_pen_with_pressure(pts, vec![0.2, 0.9, 0.4], false, Style::default()));
        d.pages[0].objects.push(Object::new_pen(vec![Pt::new(0.0, 50.0), Pt::new(10.0, 60.0)], false, Style::default()));
        let json = export(&d, &d.pages[0], &ApproxMeasurer);
        let v: Value = serde_json::from_str(&json).unwrap();
        let els = v["elements"].as_array().unwrap();
        assert_eq!(els[0]["pressures"].as_array().unwrap().len(), 3);
        assert_eq!(els[0]["simulatePressure"], false);
        assert_eq!(els[1]["simulatePressure"], true, "sans pression : simulée par Excalidraw");
        let back = import(&json, "x", &ApproxMeasurer).unwrap();
        let pens: Vec<&PenObj> = back.doc.pages[0].objects.iter().filter_map(|o| if let Kind::Pen(p) = &o.kind { Some(p) } else { None }).collect();
        let first = pens[0].pressures.as_ref().expect("pression conservée");
        assert!((first[1] - 0.9).abs() < 1e-6);
        assert!(pens[1].pressures.is_none());
    }
}
