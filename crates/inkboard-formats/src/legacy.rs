//! Import of the boards saved by the web prototype (`.tableau.json` version 1).

use crate::common::{FormatError, Imported, Report, parse_color, parse_data_url};
use inkboard_core::num;
use inkboard_core::{
    Align, Arrowhead, Asset, AssetId, Background, BgPreset, BoxGeom, Color, Connector, Dash, Document, FontFamily, Frame, Id, ImageObj, Kind, Label, Object,
    Pattern, Pt, RichText, Style, View,
};
use serde_json::Value;
use std::sync::Arc;

fn f(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}
fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn color(v: &Value, k: &str) -> Option<Color> {
    s(v, k).and_then(|c| if c == "ink" { Some(Color::Ink) } else { parse_color(c) })
}

/// Imports a board of the web prototype (`.tableau.json`).
///
/// # Errors
///
/// [`FormatError`] when the content is not a board of the prototype.
pub fn import(content: &str) -> Result<Imported, FormatError> {
    let v: Value = serde_json::from_str(content)?;
    let objs = v.get("objects").and_then(Value::as_array).ok_or_else(|| FormatError::Invalid("no list of objects".into()))?;
    let mut doc = Document::new(s(&v, "name").unwrap_or("Imported"));
    doc.background = Background {
        preset: match s(&v, "bg").unwrap_or("white") {
            "cream" => BgPreset::Cream,
            "gray" => BgPreset::Gray,
            "slate" => BgPreset::Slate,
            "black" => BgPreset::Black,
            "green" => BgPreset::Green,
            _ => BgPreset::White,
        },
        pattern: match s(&v, "pattern").unwrap_or("dots") {
            "none" => Pattern::None,
            "grid" => Pattern::Grid,
            "lines" => Pattern::Lines,
            _ => Pattern::Dots,
        },
    };
    let mut report = Report::default();
    if let Some(assets) = v.get("assets").and_then(Value::as_object) {
        for (id, url) in assets {
            if let Some((mime, data)) = url.as_str().and_then(parse_data_url) {
                doc.assets.insert(Id::from(id.as_str()), Asset { mime, data: Arc::new(data) });
            }
        }
    }
    let page = &mut doc.pages[0];
    if let Some(cam) = v.get("cam") {
        page.view = View { x: f(cam, "x"), y: f(cam, "y"), zoom: f(cam, "z").max(0.05) };
    }
    for o in objs {
        match convert(o, &doc.assets) {
            Some(obj) => doc.pages[0].objects.push(obj),
            None => report.skipped += 1,
        }
    }
    inkboard_core::serial::sanitize(&mut doc);
    Ok(Imported { doc, report })
}

fn convert(raw: &Value, assets: &std::collections::BTreeMap<AssetId, Asset>) -> Option<Object> {
    let ty = s(raw, "type")?;
    let (x, y, w, h, angle) = (f(raw, "x"), f(raw, "y"), f(raw, "w"), f(raw, "h"), f(raw, "angle"));
    let stroke = color(raw, "stroke").unwrap_or(Color::Ink);
    let fill = color(raw, "fill");
    let width = f(raw, "sw");
    let opacity = num::f32(raw.get("opacity").and_then(Value::as_f64).unwrap_or(1.0));
    let style = Style {
        stroke,
        fill,
        width,
        dash: match s(raw, "dash") {
            Some("dashed") => Dash::Dashed,
            Some("dotted") => Dash::Dotted,
            _ => Dash::Solid,
        },
        opacity,
        roundness: if raw.get("round").and_then(Value::as_bool).unwrap_or(false) { 0.45 } else { 0.0 },
        shadow: false,
    };
    let label = || Label {
        text: RichText::plain(s(raw, "text").unwrap_or("")),
        family: match s(raw, "font") {
            Some("hand") => FontFamily::Hand,
            Some("serif") => FontFamily::Serif,
            Some("mono") => FontFamily::Mono,
            _ => FontFamily::Sans,
        },
        size: raw.get("fontSize").and_then(Value::as_f64).unwrap_or(20.0),
        align: match s(raw, "align") {
            Some("center") => Align::Center,
            Some("right") => Align::Right,
            _ => Align::Left,
        },
    };
    let frame = Frame { x, y, w, h, angle };
    let abs_pts = || -> Vec<Pt> {
        raw.get("pts")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        let coords = p.as_array()?;
                        let local = Pt::new((coords.first()?.as_f64()? - 0.5) * w, (coords.get(1)?.as_f64()? - 0.5) * h);
                        Some(frame.to_world(local))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut obj = match ty {
        "rect" | "ellipse" | "diamond" | "triangle" | "sticky" | "text" => {
            let geom = match ty {
                "rect" => BoxGeom::Rect,
                "ellipse" => BoxGeom::Ellipse,
                "diamond" => BoxGeom::Diamond,
                "triangle" => BoxGeom::Triangle,
                "sticky" => BoxGeom::Sticky,
                _ => BoxGeom::Text,
            };
            let mut st = style;
            if geom == BoxGeom::Text || geom == BoxGeom::Sticky {
                st.width = 0.0;
                if geom == BoxGeom::Text {
                    st.fill = None;
                }
            }
            Object::new_box(geom, frame, st, label())
        }
        "pen" => {
            let pts = abs_pts();
            if pts.is_empty() {
                return None;
            }
            Object::new_pen(pts, raw.get("hl").and_then(Value::as_bool).unwrap_or(false), Style { dash: Dash::Solid, fill: None, ..style })
        }
        "line" => {
            let pts = abs_pts();
            if pts.len() < 2 {
                return None;
            }
            let mut conn = Connector::straight(pts[0], pts[1]);
            conn.head_start = if raw.get("arrowStart").and_then(Value::as_bool).unwrap_or(false) { Arrowhead::Arrow } else { Arrowhead::None };
            conn.head_end = if raw.get("arrowEnd").and_then(Value::as_bool).unwrap_or(false) { Arrowhead::Arrow } else { Arrowhead::None };
            Object::new(Kind::Connector(conn), Style { fill: None, ..style })
        }
        "image" => {
            let asset = Id::from(s(raw, "asset")?);
            if !assets.contains_key(&asset) {
                return None;
            }
            Object::new(Kind::Image(ImageObj { frame, asset, latex: None, mono: false }), Style { width: 0.0, fill: None, ..style })
        }
        _ => return None,
    };
    if let Some(g) = s(raw, "gid") {
        obj.group = Some(Id::from(g));
    }
    obj.locked = raw.get("locked").and_then(Value::as_bool).unwrap_or(false);
    Some(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkboard_core::End;

    const SAMPLE: &str = r##"{
      "app":"inkboard","version":1,"name":"Ancien","bg":"slate","pattern":"grid","cam":{"x":10,"y":20,"z":1.5},
      "objects":[
        {"id":"a","type":"rect","x":100,"y":100,"w":200,"h":100,"angle":0,"stroke":"ink","fill":"#a5d8ff","sw":3,"dash":"dashed","opacity":0.8,"text":"Salut","font":"hand","fontSize":24,"align":"center","round":true},
        {"id":"b","type":"sticky","x":400,"y":100,"w":150,"h":150,"angle":0.2,"stroke":"ink","fill":"#ffec99","sw":0,"opacity":1,"text":"Note","font":"sans","fontSize":20,"align":"center"},
        {"id":"c","type":"line","x":0,"y":0,"w":100,"h":50,"angle":0,"stroke":"#e03131","fill":"transparent","sw":3,"opacity":1,"pts":[[0,0],[1,1]],"arrowEnd":true,"gid":"g1"},
        {"id":"d","type":"pen","x":0,"y":300,"w":100,"h":40,"angle":0,"stroke":"ink","fill":"transparent","sw":4,"opacity":0.4,"hl":true,"pts":[[0,0],[0.5,1],[1,0]]},
        {"id":"e","type":"text","x":50,"y":400,"w":80,"h":26,"angle":0,"stroke":"ink","fill":"transparent","sw":0,"opacity":1,"text":"Libre","font":"mono","fontSize":20,"align":"left","locked":true},
        {"id":"f","type":"image","x":600,"y":0,"w":40,"h":30,"angle":0,"stroke":"ink","fill":"transparent","sw":0,"opacity":1,"asset":"img1"},
        {"id":"g","type":"image","x":0,"y":0,"w":10,"h":10,"asset":"missing"},
        {"id":"h","type":"unknown"}
      ],
      "assets":{"img1":"data:image/png;base64,AQIDBA=="}
    }"##;

    #[test]
    fn imports_prototype_file() {
        let imported = import(SAMPLE).unwrap();
        let d = &imported.doc;
        assert_eq!(d.name, "Ancien");
        assert_eq!(d.background.preset, BgPreset::Slate);
        assert_eq!(d.background.pattern, Pattern::Grid);
        assert_eq!(imported.report.skipped, 2, "image without an asset + unknown type");
        let p = &d.pages[0];
        assert_eq!(p.objects.len(), 6);
        assert_eq!(p.view.zoom, 1.5);
        let Kind::Box(b) = &p.objects[0].kind else { panic!() };
        assert_eq!(b.geom, BoxGeom::Rect);
        assert_eq!(b.label.text.to_plain(), "Salut");
        assert_eq!(b.label.family, FontFamily::Hand);
        assert_eq!(p.objects[0].style.dash, Dash::Dashed);
        assert!(p.objects[0].style.roundness > 0.0);
        assert_eq!(p.objects[0].style.fill, Some(Color::Rgb(0xa5, 0xd8, 0xff)));
        // line: free ends converted to world coordinates, arrowhead kept
        let Kind::Connector(c) = &p.objects[2].kind else { panic!() };
        assert_eq!(c.head_end, Arrowhead::Arrow);
        assert_eq!(c.head_start, Arrowhead::None);
        assert!(matches!(c.start, End::Free { at } if at.dist(Pt::new(0.0, 0.0)) < 1e-9));
        assert!(matches!(c.end, End::Free { at } if at.dist(Pt::new(100.0, 50.0)) < 1e-9));
        assert!(p.objects[2].group.is_some());
        // surligneur
        let Kind::Pen(pen) = &p.objects[3].kind else { panic!() };
        assert!(pen.highlighter);
        assert_eq!(pen.pts[1], Pt::new(50.0, 340.0));
        assert!(p.objects[4].locked);
        assert_eq!(d.assets.len(), 1);
    }

    #[test]
    fn rotated_pen_points_use_object_center() {
        let j = r#"{"objects":[{"id":"p","type":"pen","x":0,"y":0,"w":100,"h":0,"angle":1.5707963267948966,"stroke":"ink","sw":3,"opacity":1,"pts":[[0,0],[1,0]]}]}"#;
        let r = import(j).unwrap();
        let Kind::Pen(p) = &r.doc.pages[0].objects[0].kind else { panic!() };
        // horizontal stroke of 100 centered at (50, 0), rotated by 90° → vertical
        assert!((p.pts[0].x - 50.0).abs() < 1e-6 && (p.pts[1].x - 50.0).abs() < 1e-6);
        assert!((p.pts[0].y - p.pts[1].y).abs() > 99.0);
    }

    #[test]
    fn rejects_non_board_json() {
        assert!(import("{}").is_err());
        assert!(import("not json").is_err());
    }
}
