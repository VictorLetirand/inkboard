//! Traps removed for agents: label names, connector style at creation, icon labels, group moves, width fitted to text, creation warnings
//! and icon search hints. With the real `inkboard-mcp` binary.

#![allow(clippy::many_single_char_names)]

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Dash, Kind, Object, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn start(tag: &str) -> (Client, PathBuf, String) {
    let dir = temp_dir(tag);
    let mut c = Client::start(&dir);
    let board = dir.join("q.inkboard");
    let p = board.display().to_string();
    c.tool("create_board", json!({"path": p})).unwrap();
    (c, board, p)
}

fn objects(board: &Path) -> Vec<Object> {
    serial::load(board).unwrap().pages[0].objects.clone()
}

fn object(board: &Path, id: &str) -> Object {
    objects(board).into_iter().find(|o| o.id.0 == id).expect("object")
}

fn id_of(r: &Value) -> String {
    r["id"].as_str().unwrap().to_owned()
}

fn rect(c: &mut Client, p: &str, x: f64) -> String {
    id_of(&c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "A", "x": x, "y": 0, "w": 100, "h": 60})).unwrap())
}

fn label_text(board: &Path, id: &str) -> Option<String> {
    match object(board, id).kind {
        Kind::Connector(c) => c.label.map(|l| l.text.to_plain()),
        _ => None,
    }
}

fn warnings(r: &Value) -> String {
    r["warnings"].to_string()
}

/* ------------------------------------------------------------------ 6 */

#[test]
fn a_connector_label_is_label_or_text_in_both_tools() {
    let (mut c, board, p) = start("aq-label");
    let (a, b) = (rect(&mut c, &p, 0.0), rect(&mut c, &p, 300.0));
    let k1 = id_of(&c.tool("add_connector", json!({"path": p, "from": a, "to": b, "text": "via text"})).unwrap());
    assert_eq!(label_text(&board, &k1).as_deref(), Some("via text"));
    let k2 = id_of(&c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label": "x", "text": "x"})).unwrap());
    assert_eq!(label_text(&board, &k2).as_deref(), Some("x"));
    c.tool("update_object", json!({"path": p, "id": k1, "label": "renamed"})).unwrap();
    assert_eq!(label_text(&board, &k1).as_deref(), Some("renamed"));
    c.tool("update_object", json!({"path": p, "id": k1, "text": "again"})).unwrap();
    assert_eq!(label_text(&board, &k1).as_deref(), Some("again"));
}

#[test]
fn different_label_and_text_are_refused_and_change_nothing() {
    let (mut c, board, p) = start("aq-label-bad");
    let (a, b) = (rect(&mut c, &p, 0.0), rect(&mut c, &p, 300.0));
    let k = id_of(&c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label": "keep"})).unwrap());
    let before = std::fs::read(&board).unwrap();
    let e = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label": "one", "text": "two"})).unwrap_err();
    assert!(e.contains("label") && e.contains("text") && e.contains("differ"), "{e}");
    let e = c.tool("update_object", json!({"path": p, "id": k, "label": "one", "text": "two"})).unwrap_err();
    assert!(e.contains("differ"), "{e}");
    assert_eq!(std::fs::read(&board).unwrap(), before);
}

/* ----------------------------------------------------------------- 12 */

#[test]
fn a_connector_gets_dash_font_size_and_stroke_width_at_creation() {
    let (mut c, board, p) = start("aq-conn-style");
    let (a, b) = (rect(&mut c, &p, 0.0), rect(&mut c, &p, 300.0));
    let k =
        id_of(&c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label": "vpn", "dash": "dashed", "font_size": 24, "stroke_width": 5})).unwrap());
    let o = object(&board, &k);
    assert_eq!(o.style.dash, Dash::Dashed);
    assert!((o.style.width - 5.0).abs() < 1e-9);
    let Kind::Connector(conn) = o.kind else { panic!() };
    assert!((conn.label.unwrap().size - 24.0).abs() < 1e-9);
    // defaults unchanged
    let k = id_of(&c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label": "x"})).unwrap());
    let o = object(&board, &k);
    assert_eq!(o.style.dash, Dash::Solid);
    assert!((o.style.width - 2.0).abs() < 1e-9);
    // bad values and a font size without label
    let before = std::fs::read(&board).unwrap();
    assert!(c.tool("add_connector", json!({"path": p, "from": a, "to": b, "dash": "wavy"})).unwrap_err().contains("dash"));
    assert!(c.tool("add_connector", json!({"path": p, "from": a, "to": b, "font_size": 1})).unwrap_err().contains("font_size"));
    assert_eq!(std::fs::read(&board).unwrap(), before);
    let r = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "font_size": 20})).unwrap();
    assert!(warnings(&r).contains("font_size"), "{r}");
}

/* ------------------------------------------------------------- 8 + 15 */

#[allow(clippy::needless_pass_by_value)]
fn add_icon(c: &mut Client, p: &str, extra: Value) -> Value {
    let mut a = json!({"path": p, "icon": icon_id(c, p), "x": 200, "y": 200});
    for (k, v) in extra.as_object().unwrap() {
        a[k] = v.clone();
    }
    c.tool("add_icon", a).unwrap()
}

fn icon_id(c: &mut Client, p: &str) -> String {
    c.tool("search_icons", json!({"path": p, "query": "router", "limit": 1})).unwrap()["icons"][0]["id"].as_str().unwrap().to_owned()
}

fn frame(board: &Path, id: &str) -> (f64, f64, f64, f64) {
    let f = object(board, id).frame().unwrap();
    (f.x, f.y, f.w, f.h)
}

#[test]
fn icon_label_side_places_the_label_with_a_6_unit_gap() {
    let (mut c, board, p) = start("aq-icon-side");
    for side in ["bottom", "top", "right", "left"] {
        let r = add_icon(&mut c, &p, json!({"label": "Box", "label_side": side, "size": 64}));
        let (ix, iy, iw, ih) = frame(&board, &id_of(&r));
        let (lx, ly, lw, lh) = frame(&board, r["label_id"].as_str().unwrap());
        match side {
            "bottom" => assert!((ly - (iy + ih + 6.0)).abs() < 1e-6 && ((lx + lw / 2.0) - (ix + iw / 2.0)).abs() < 1e-6),
            "top" => assert!((ly + lh + 6.0 - iy).abs() < 1e-6),
            "right" => assert!((lx - (ix + iw + 6.0)).abs() < 1e-6 && ((ly + lh / 2.0) - (iy + ih / 2.0)).abs() < 1e-6),
            _ => assert!((lx + lw + 6.0 - ix).abs() < 1e-6),
        }
    }
    let before = std::fs::read(&board).unwrap();
    let id = icon_id(&mut c, &p);
    assert!(c.tool("add_icon", json!({"path": p, "icon": id, "label": "x", "label_side": "middle"})).unwrap_err().contains("label_side"));
    assert!(c.tool("add_icon", json!({"path": p, "icon": id, "label": "x", "label_font_size": 2})).unwrap_err().contains("label_font_size"));
    assert_eq!(std::fs::read(&board).unwrap(), before);
}

#[test]
fn icon_label_font_size_and_wrapping() {
    let (mut c, board, p) = start("aq-icon-wrap");
    // default: size 16, a short label is untouched (one line, same as before)
    let r = add_icon(&mut c, &p, json!({"label": "Router"}));
    let Kind::Box(b) = object(&board, r["label_id"].as_str().unwrap()).kind else { panic!() };
    assert!((b.label.size - 16.0).abs() < 1e-9 && b.label.text.to_plain() == "Router");
    let r = add_icon(&mut c, &p, json!({"label": "Router", "label_font_size": 24}));
    let Kind::Box(b) = object(&board, r["label_id"].as_str().unwrap()).kind else { panic!() };
    assert!((b.label.size - 24.0).abs() < 1e-9);
    // a long label wraps by default (140 for a 64 icon) and never inside a word
    let long = "Internet gateway of the production network";
    let r = add_icon(&mut c, &p, json!({"label": long}));
    let Kind::Box(b) = object(&board, r["label_id"].as_str().unwrap()).kind else { panic!() };
    let text = b.label.text.to_plain();
    assert!(text.contains('\n') && text.replace('\n', " ") == long, "{text:?}");
    assert!(b.frame.w <= 160.0, "{}", b.frame.w);
    // a wider limit keeps one line; a limit smaller than a word never splits it
    let r = add_icon(&mut c, &p, json!({"label": long, "label_max_width": 1000}));
    let Kind::Box(b) = object(&board, r["label_id"].as_str().unwrap()).kind else { panic!() };
    assert_eq!(b.label.text.to_plain(), long);
    let r = add_icon(&mut c, &p, json!({"label": "Internet gateway", "label_max_width": 20}));
    let Kind::Box(b) = object(&board, r["label_id"].as_str().unwrap()).kind else { panic!() };
    assert_eq!(b.label.text.to_plain(), "Internet\ngateway");
    // the return value is unchanged
    assert!(r["id"].is_string() && r["label_id"].is_string());
}

/* ----------------------------------------------------------------- 18 */

#[test]
fn move_objects_moves_whole_groups_unless_told_otherwise() {
    let (mut c, board, p) = start("aq-move-group");
    let r = add_icon(&mut c, &p, json!({"label": "Box"}));
    let (icon, label) = (id_of(&r), r["label_id"].as_str().unwrap().to_owned());
    let other = rect(&mut c, &p, 600.0);
    let k = id_of(&c.tool("add_connector", json!({"path": p, "from": icon, "to": other})).unwrap());
    let (ix, iy, ..) = frame(&board, &icon);
    let (lx, ly, ..) = frame(&board, &label);
    let m = c.tool("move_objects", json!({"path": p, "ids": [icon], "dx": 50, "dy": 10})).unwrap();
    assert_eq!(m["moved"], json!(2));
    assert_eq!(m["group_members_added"], json!(1));
    let (nx, ny, ..) = frame(&board, &icon);
    let (mx, my, ..) = frame(&board, &label);
    assert!((nx - ix - 50.0).abs() < 1e-6 && (ny - iy - 10.0).abs() < 1e-6);
    assert!((mx - lx - 50.0).abs() < 1e-6 && (my - ly - 10.0).abs() < 1e-6);
    assert!(matches!(object(&board, &k).kind, Kind::Connector(_)));
    // group: false moves only the listed ids
    let m = c.tool("move_objects", json!({"path": p, "ids": [icon], "dx": 100, "dy": 0, "group": false})).unwrap();
    assert_eq!(m["moved"], json!(1));
    assert!((frame(&board, &icon).0 - nx - 100.0).abs() < 1e-6);
    assert!((frame(&board, &label).0 - mx).abs() < 1e-6);
    // an ungrouped object is unaffected
    assert_eq!(c.tool("move_objects", json!({"path": p, "ids": [other], "dx": 1, "dy": 1})).unwrap()["moved"], json!(1));
    assert!(c.tool("move_objects", json!({"path": p, "ids": [other], "dx": 1, "group": "no"})).unwrap_err().contains("group"));
}

/* -------------------------------------------------------- 7 + 2 (shapes) */

fn shape_w(board: &Path, id: &str) -> f64 {
    frame(board, id).2
}

#[test]
fn fit_width_computes_the_smallest_width_without_splitting_a_word() {
    let (mut c, board, p) = start("aq-fit");
    let text = "Load balancer for the public API";
    let r = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": text, "fit": "width"})).unwrap();
    let w = shape_w(&board, &id_of(&r));
    assert!(w > 16.0 && w < 400.0, "{w}");
    assert!(!warnings(&r).contains("wider"), "{r}");
    // two lines at that width; 4 units narrower needs more (or splits)
    let mut lines = |w: f64| {
        let t = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": text, "w": w, "h": 200})).unwrap();
        let Kind::Box(b) = object(&board, &id_of(&t)).kind else { panic!() };
        inkboard_core::text::layout_rich(&b.label.text, Some(b.label_rect().w), &inkboard_core::ApproxMeasurer, b.label.family, b.label.size).lines.len()
    };
    assert!(lines(w) <= 2);
    assert!(lines(w - 8.0) > 2);
    // max_lines 1: one line, wider
    let one = id_of(&c.tool("add_shape", json!({"path": p, "geom": "rect", "text": text, "fit": "width", "max_lines": 1})).unwrap());
    assert!(shape_w(&board, &one) > w);
    // never smaller than the given w
    let big = id_of(&c.tool("add_shape", json!({"path": p, "geom": "rect", "text": text, "fit": "width", "w": 900})).unwrap());
    assert!((shape_w(&board, &big) - 900.0).abs() < 1e-6);
    // each geometry uses its own usable area: the ellipse is wider than the rect for the same text
    let ell = id_of(&c.tool("add_shape", json!({"path": p, "geom": "ellipse", "text": text, "fit": "width"})).unwrap());
    assert!(shape_w(&board, &ell) > w);
    // a long word is never split: the width is at least that of the word
    let r = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "Infrastructure-as-code", "fit": "width"})).unwrap();
    assert!(!warnings(&r).contains("wider"), "{r}");
}

#[test]
fn update_object_fits_the_width_of_a_shape_and_validates_fit() {
    let (mut c, board, p) = start("aq-fit-update");
    let id = id_of(&c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "short", "w": 100, "h": 60})).unwrap());
    c.tool("update_object", json!({"path": p, "id": id, "text": "A rather long caption for this box", "fit": "width", "max_lines": 3})).unwrap();
    let w = shape_w(&board, &id);
    assert!(w > 60.0 && w < 400.0, "{w}");
    let before = std::fs::read(&board).unwrap();
    assert!(c.tool("update_object", json!({"path": p, "id": id, "fit": "height"})).unwrap_err().contains("fit"));
    assert!(c.tool("update_object", json!({"path": p, "id": id, "max_lines": 3})).unwrap_err().contains("max_lines"));
    assert!(c.tool("add_shape", json!({"path": p, "geom": "rect", "fit": "width", "max_lines": 0})).unwrap_err().contains("max_lines"));
    assert_eq!(std::fs::read(&board).unwrap(), before);
}

#[test]
fn creation_warns_when_the_shape_grew_or_a_word_is_too_wide() {
    let (mut c, _board, p) = start("aq-warn");
    let r = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "one two three four five six seven eight nine ten", "w": 100, "h": 40})).unwrap();
    assert!(warnings(&r).contains("text did not fit: height grew from 40 to"), "{r}");
    let r = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "Infrastructure", "w": 60, "h": 60})).unwrap();
    let w = warnings(&r);
    assert!(w.contains("word \\\"Infrastructure\\\" is wider than the text area: use w ≥ "), "{r}");
    // the advised width really holds the word
    let n: f64 = w.split("w ≥ ").nth(1).unwrap().trim_end_matches(['"', ']']).parse().unwrap();
    let r2 = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "Infrastructure", "w": n, "h": 60})).unwrap();
    assert!(!warnings(&r2).contains("wider"), "{r2}");
    // no warning for a text that fits
    let r = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "ok", "w": 100, "h": 60})).unwrap();
    assert!(r.get("warnings").is_none(), "{r}");
    // update_object reports the same
    let id = id_of(&r);
    let r = c.tool("update_object", json!({"path": p, "id": id, "text": "one two three four five six seven eight nine ten"})).unwrap();
    assert!(warnings(&r).contains("height grew from 60 to"), "{r}");
    let r = c.tool("update_object", json!({"path": p, "id": id, "text": "Infrastructure", "w": 60})).unwrap();
    assert!(warnings(&r).contains("wider than the text area"), "{r}");
}

/* ------------------------------------------------------------ 16 + 19 */

#[test]
fn search_results_carry_the_aspect_and_flag_wide_logos() {
    let (mut c, _board, p) = start("aq-aspect");
    let r = c.tool("search_icons", json!({"path": p, "query": "router", "limit": 5})).unwrap();
    for i in r["icons"].as_array().unwrap() {
        let a = i["aspect"].as_f64().unwrap_or_else(|| panic!("no aspect in {i}"));
        assert!(a > 0.0);
        assert_eq!(i.get("wide").is_some(), a > 2.0, "{i}");
        assert!(((a * 100.0).round() - a * 100.0).abs() < 1e-6, "2 decimals: {a}");
    }
    // a wide icon of the pack (found from the data) is flagged
    let lib = inkboard_icons::shared().unwrap();
    let wide = |si: usize, ii: usize| {
        inkboard_icons::svg_dimensions(&lib.svg(inkboard_icons::IconRef { set: si, icon: ii }).unwrap()).is_some_and(|(w, h)| w / h > 2.5)
    };
    let (set, icon) = lib
        .sets
        .iter()
        .enumerate()
        .find_map(|(si, s)| (0..s.names.len().min(300)).find(|&ii| wide(si, ii)).map(|ii| (si, ii)))
        .expect("a wide icon in the pack");
    let r = c.tool("search_icons", json!({"path": p, "query": lib.sets[set].names[icon], "set": lib.sets[set].id, "limit": 100})).unwrap();
    assert!(r["icons"].as_array().unwrap().iter().any(|i| i["wide"] == json!(true)), "{r}");
}

#[test]
fn an_empty_search_retries_with_synonyms_and_says_so() {
    let (mut c, _board, p) = start("aq-synonyms");
    for (q, must_find) in [
        ("box internet", "router"),
        ("user team", "user"),
        ("registry", "registry"),
        ("phone", "phone"),
        ("t\u{e9}l\u{e9}phone", "phone"),
        ("nas", ""),
        ("ubuntu", ""),
        ("laptop", ""),
    ] {
        let r = c.tool("search_icons", json!({"path": p, "query": q, "limit": 5})).unwrap();
        // a query that already has results is left alone
        if r.get("note").is_none() {
            assert!(r["count"].as_u64().unwrap() > 0, "{q}: {r}");
            continue;
        }
        assert!(r["count"].as_u64().unwrap() > 0, "{q}: {r}");
        assert!(r["note"].as_str().unwrap().contains(q), "{q}: {r}");
        if !must_find.is_empty() {
            let names = r["icons"].to_string().to_lowercase();
            assert!(names.contains(must_find), "{q}: {r}");
        }
    }
    // nothing and no synonym: empty, no note
    let r = c.tool("search_icons", json!({"path": p, "query": "zzzqqq", "limit": 5})).unwrap();
    assert_eq!((r["count"].as_u64(), r.get("note")), (Some(0), None));
}

/// Every synonym target exists in the pack, so a retry is never wasted.
#[test]
fn every_synonym_table_entry_leads_somewhere() {
    let lib = inkboard_icons::shared().unwrap();
    for (from, to) in [
        ("modem", "router"),
        ("team", "users"),
        ("registry", "container registry"),
        ("phone", "smartphone"),
        ("laptop", "notebook"),
        ("nas", "storage"),
        ("ubuntu", "linux"),
    ] {
        assert!(!lib.search(to, 1).is_empty(), "{from} -> {to}");
    }
}
