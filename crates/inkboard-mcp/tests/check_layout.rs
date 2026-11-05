//! `check_layout` with the real `inkboard-mcp` binary: a board with known defects gives the expected kinds and ids, a clean board gives
//! `ok: true`, and the tool changes nothing. The rules themselves are tested in `inkboard-core` (`layout_check`).

#![allow(clippy::many_single_char_names, clippy::needless_pass_by_value, clippy::assert_is_empty)]

mod common;

use common::{Client, temp_dir};
use inkboard_core::connector::{Connector, End};
use inkboard_core::{Align, BoxGeom, Document, Frame, Id, Kind, Label, Object, Pt, Style, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn shape(id: &str, geom: BoxGeom, text: &str, (x, y, w, h): (f64, f64, f64, f64)) -> Object {
    let mut o = Object::new_box(geom, Frame::new(x, y, w, h), Style::default(), Label::new(text, 16.0, Align::Center));
    o.id = Id::from(id);
    o
}

fn link(id: &str, a: &str, b: &str) -> Object {
    let mut o = Object::new(Kind::Connector(Connector::new(End::bound(Id::from(a)), End::bound(Id::from(b)))), Style::default());
    o.id = Id::from(id);
    o
}

fn setup(tag: &str, objects: Vec<Object>) -> (Client, PathBuf) {
    let dir = temp_dir(tag);
    let board = dir.join("c.inkboard");
    let mut doc = Document::new("c");
    doc.pages[0].objects = objects;
    serial::save(&doc, &board).unwrap();
    (Client::start(&dir), board)
}

fn check(c: &mut Client, board: &Path, extra: Value) -> Value {
    let mut a = json!({"path": board.display().to_string()});
    for (k, v) in extra.as_object().unwrap() {
        a[k] = v.clone();
    }
    c.tool("check_layout", a).unwrap()
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// Ids of the issues of one kind, each as a sorted list.
fn ids_of(r: &Value, kind: &str) -> Vec<Vec<String>> {
    r["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == kind)
        .map(|i| {
            let mut v: Vec<String> = i["ids"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_owned()).collect();
            v.sort();
            v
        })
        .collect()
}

#[test]
fn a_clean_board_is_ok() {
    let (mut c, board) = setup(
        "cl-clean",
        vec![shape("a", BoxGeom::Rect, "Start", (0.0, 0.0, 200.0, 100.0)), shape("b", BoxGeom::Rect, "End", (0.0, 200.0, 200.0, 100.0)), link("k", "a", "b")],
    );
    let r = check(&mut c, &board, json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["issues"], json!([]));
    assert_eq!(r["metrics"]["objects"], json!(2));
    assert_eq!(r["metrics"]["connectors"], json!(1));
    assert!(r["metrics"]["content_box"]["w"].as_f64().unwrap() > 200.0, "{r}");
    assert!(r["metrics"]["min_text_px"].as_f64().unwrap() > 10.0, "{r}");
    assert_eq!(r["metrics"]["empty_cells"], json!(0));
}

#[test]
fn a_board_with_defects_reports_each_kind_with_its_ids() {
    let mut free = Object::new(Kind::Connector(Connector::new(End::bound(Id::from("dst")), End::free(Pt::new(900.0, 700.0)))), Style::default());
    free.id = Id::from("kfree");
    let (mut c, board) = setup(
        "cl-defects",
        vec![
            shape("a", BoxGeom::Rect, "A", (0.0, 0.0, 120.0, 80.0)),
            shape("b", BoxGeom::Rect, "B", (60.0, 40.0, 120.0, 80.0)),
            shape("tall", BoxGeom::Rect, "one two three four five six seven eight nine ten", (400.0, 0.0, 120.0, 30.0)),
            shape("long", BoxGeom::Rect, "Supercalifragilistic", (700.0, 0.0, 90.0, 60.0)),
            shape("src", BoxGeom::Rect, "S", (0.0, 300.0, 100.0, 80.0)),
            shape("wall", BoxGeom::Rect, "W", (300.0, 300.0, 100.0, 80.0)),
            shape("dst", BoxGeom::Rect, "D", (600.0, 300.0, 100.0, 80.0)),
            link("kthrough", "src", "dst"),
            link("kdup1", "src", "wall"),
            link("kdup2", "wall", "src"),
            free,
        ],
    );
    let r = check(&mut c, &board, json!({}));
    assert_eq!(r["ok"], json!(false));
    assert_eq!(ids_of(&r, "overlap"), vec![strings(&["a", "b"])], "{r}");
    assert_eq!(ids_of(&r, "text_overflow"), vec![strings(&["tall"])], "{r}");
    assert_eq!(ids_of(&r, "word_split"), vec![strings(&["long"])], "{r}");
    assert_eq!(ids_of(&r, "connector_through_object"), vec![strings(&["kthrough", "wall"])], "{r}");
    assert_eq!(ids_of(&r, "duplicate_connector"), vec![strings(&["kdup1", "kdup2"])], "{r}");
    assert_eq!(ids_of(&r, "unbound_connector"), vec![strings(&["kfree"])], "{r}");
    let overlap = r["issues"].as_array().unwrap().iter().find(|i| i["kind"] == "overlap").unwrap();
    assert_eq!(overlap["severity"], json!("error"));
    assert!(overlap["message"].as_str().unwrap().contains("overlap"));
    assert!(overlap["overlap_w"].as_f64().unwrap() > 50.0, "{overlap}");
    // `ids` keeps what touches the given objects only
    let r = check(&mut c, &board, json!({"ids": ["long"]}));
    assert_eq!(r["issues"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["issues"][0]["kind"], json!("word_split"));
    assert_eq!(r["metrics"]["objects"], json!(7), "the metrics describe the whole page");
}

#[test]
fn small_text_depends_on_the_export_width() {
    let (mut c, board) = setup(
        "cl-small",
        vec![shape("a", BoxGeom::Rect, "A", (0.0, 0.0, 200.0, 100.0)), shape("b", BoxGeom::Rect, "B", (0.0, 5000.0, 200.0, 100.0)), link("k", "a", "b")],
    );
    // 16 units on a 205 wide page: fine at 1600 px, tiny at 100 px
    assert!(ids_of(&check(&mut c, &board, json!({})), "small_text").is_empty());
    let r = check(&mut c, &board, json!({"export_width": 100}));
    assert_eq!(ids_of(&r, "small_text"), vec![strings(&["a", "b"])], "{r}");
    let issue = r["issues"].as_array().unwrap().iter().find(|i| i["kind"] == "small_text").unwrap();
    assert_eq!(issue["severity"], json!("warning"));
    assert!(issue["min_px"].as_f64().unwrap() < 10.0 && issue["min_font_size"].as_f64().unwrap() > 16.0, "{issue}");
}

#[test]
fn the_tool_is_read_only_validated_and_allowed_in_a_batch() {
    let (mut c, board) =
        setup("cl-misc", vec![shape("a", BoxGeom::Rect, "A", (0.0, 0.0, 200.0, 100.0)), shape("b", BoxGeom::Rect, "B", (100.0, 50.0, 200.0, 100.0))]);
    let before = std::fs::read(&board).unwrap();
    let p = board.display().to_string();
    check(&mut c, &board, json!({}));
    assert!(c.tool("check_layout", json!({"path": p, "ids": ["nope"]})).unwrap_err().contains("not found"));
    assert!(c.tool("check_layout", json!({"path": p, "export_width": 5})).unwrap_err().contains("export_width"));
    assert!(c.tool("check_layout", json!({"path": p, "export_width": "wide"})).unwrap_err().contains("export_width"));
    let r = c.tool("batch", json!({"path": p, "operations": [{"tool": "check_layout", "args": {}}]})).unwrap();
    assert_eq!(r["results"][0]["issues"][0]["kind"], json!("overlap"), "{r}");
    assert_eq!(std::fs::read(&board).unwrap(), before, "the file is untouched");
}

#[test]
fn update_object_returns_warnings_and_the_deprecated_avertissements() {
    let (mut c, board) = setup("cl-warn", vec![shape("a", BoxGeom::Rect, "A", (0.0, 0.0, 200.0, 100.0))]);
    let p = board.display().to_string();
    let r = c.tool("update_object", json!({"path": p, "id": "a", "route": "elbow"})).unwrap();
    assert_eq!(r["warnings"], r["avertissements"]);
    assert_eq!(r["warnings"].as_array().unwrap().len(), 1, "{r}");
    let r = c.tool("update_object", json!({"path": p, "id": "a", "fill": "#ff0000"})).unwrap();
    assert_eq!(r["warnings"], json!([]));
    assert_eq!(r["avertissements"], json!([]));
}
