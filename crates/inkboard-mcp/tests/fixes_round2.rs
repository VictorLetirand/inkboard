//! Second round of agent-quality fixes: `add_frame` title color and inset, `create_board` on an existing file, unknown-argument warnings,
//! and the new `check_layout` findings (`sparse_frame`, staircase), with the real `inkboard-mcp` binary.

#![allow(clippy::many_single_char_names)]

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Color, Kind, Object, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn start(tag: &str) -> (Client, PathBuf, String) {
    let dir = temp_dir(tag);
    let mut c = Client::start(&dir);
    let board = dir.join("f.inkboard");
    let p = board.display().to_string();
    c.tool("create_board", json!({"path": p})).unwrap();
    (c, board, p)
}

fn object(board: &Path, id: &str) -> Object {
    serial::load(board).unwrap().pages[0].objects.iter().find(|o| o.id.0 == id).cloned().expect("object")
}

fn id_of(r: &Value) -> String {
    r["id"].as_str().unwrap().to_owned()
}

fn rect(c: &mut Client, p: &str, (x, y, w, h): (f64, f64, f64, f64)) -> String {
    id_of(&c.tool("add_shape", json!({"path": p, "geom": "rect", "x": x, "y": y, "w": w, "h": h})).unwrap())
}

fn title_colors(board: &Path, id: &str) -> Vec<Option<Color>> {
    let Kind::Box(b) = object(board, id).kind else { panic!("not a box") };
    b.label.text.0.iter().map(|s| s.style.color).collect()
}

#[test]
fn the_frame_title_is_dark_grey_by_default_and_follows_title_color() {
    let (mut c, board, p) = start("r2-title-color");
    let a = rect(&mut c, &p, (0.0, 0.0, 100.0, 60.0));
    let f = id_of(&c.tool("add_frame", json!({"path": p, "ids": [a], "title": "Zone"})).unwrap());
    assert_eq!(title_colors(&board, &f), [Some(Color::rgb(0x34, 0x3a, 0x40))]);
    // the border keeps its own color
    assert_eq!(object(&board, &f).style.stroke, Color::rgb(0xad, 0xb5, 0xbd));
    // a re-fit without title or title_color keeps the color; title_color changes it; a new title keeps it
    c.tool("add_frame", json!({"path": p, "frame": f, "ids": [a], "title_color": "#ff0000"})).unwrap();
    assert_eq!(title_colors(&board, &f), [Some(Color::rgb(255, 0, 0))]);
    c.tool("add_frame", json!({"path": p, "frame": f, "ids": [a]})).unwrap();
    assert_eq!(title_colors(&board, &f), [Some(Color::rgb(255, 0, 0))]);
    c.tool("add_frame", json!({"path": p, "frame": f, "ids": [a], "title": "Other"})).unwrap();
    assert_eq!(title_colors(&board, &f), [Some(Color::rgb(255, 0, 0))]);
    let Kind::Box(b) = object(&board, &f).kind else { panic!() };
    assert!(b.label.text.0[0].style.bold);
    let r = c.tool("add_frame", json!({"path": p, "frame": f, "ids": [a], "title_color": "ink"})).unwrap();
    assert!(r.get("warnings").is_none(), "{r}");
    assert_eq!(title_colors(&board, &f), [Some(Color::Ink)]);
}

#[test]
fn the_frame_title_clears_the_rounded_corner_and_the_content() {
    let (mut c, board, p) = start("r2-title-inset");
    let a = rect(&mut c, &p, (0.0, 0.0, 100.0, 60.0));
    let r = c.tool("add_frame", json!({"path": p, "ids": [a], "title": "Zone", "padding": 0})).unwrap();
    let f = object(&board, &id_of(&r));
    let Kind::Box(b) = &f.kind else { panic!() };
    let area = b.label_rect();
    let (left, top) = (area.x + b.frame.w / 2.0, area.y + b.frame.h / 2.0);
    assert!(left >= 16.0 && top >= 16.0, "title inset {left} x {top}");
    // even with no padding, the title text (laid out from `top`) ends above the content
    let lay = b.layout(&inkboard_core::ApproxMeasurer);
    let content_top = r["y"].as_f64().unwrap() + r["h"].as_f64().unwrap() - 60.0;
    assert!(r["y"].as_f64().unwrap() + top + lay.height <= content_top + 1e-6, "{r}");
    // check_layout agrees: no text_overflow / word_split on the frame
    let issues = c.tool("check_layout", json!({"path": p})).unwrap();
    assert!(issues["issues"].as_array().unwrap().iter().all(|i| i["kind"] != "text_overflow" && i["kind"] != "word_split"), "{issues}");
}

#[test]
fn create_board_refuses_an_existing_file_unless_overwrite() {
    let (mut c, board, p) = start("r2-create");
    rect(&mut c, &p, (0.0, 0.0, 100.0, 60.0));
    let e = c.tool("create_board", json!({"path": p})).unwrap_err();
    assert!(e.contains("already exists") && e.contains("overwrite"), "{e}");
    assert_eq!(serial::load(&board).unwrap().pages[0].objects.len(), 1, "the old board is untouched");
    let e = c.tool("create_board", json!({"path": p, "overwrite": false})).unwrap_err();
    assert!(e.contains("overwrite"), "{e}");
    c.tool("create_board", json!({"path": p, "overwrite": true, "name": "New"})).unwrap();
    let d = serial::load(&board).unwrap();
    assert_eq!(d.pages[0].objects.len(), 0);
    assert_eq!(d.pages.len(), 1);
}

fn warnings(r: &Value) -> Vec<String> {
    r["warnings"].as_array().map(|w| w.iter().map(|x| x.as_str().unwrap().to_owned()).collect()).unwrap_or_default()
}

#[test]
fn an_unknown_argument_is_ignored_with_a_warning_and_a_suggestion() {
    let (mut c, _board, p) = start("r2-unknown");
    let r = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "A", "bold": true, "strok": "#ff0000"})).unwrap();
    let w = warnings(&r);
    assert!(w.iter().any(|m| m.contains("unknown argument \"bold\" ignored")), "{w:?}");
    assert!(w.iter().any(|m| m.contains("\"strok\"") && m.contains("did you mean \"stroke\"")), "{w:?}");
    assert!(r["id"].is_string(), "the call ran: {r}");
    // known arguments, `path` and `page` do not warn
    let r = c.tool("add_shape", json!({"path": p, "page": 1, "geom": "rect", "text": "B"})).unwrap();
    assert!(r.get("warnings").is_none(), "{r}");
    // merges with a warning of the tool itself
    let r =
        c.tool("add_frame", json!({"path": p, "x": 0, "y": 0, "w": 10, "h": 10, "title": "A very long title that widens the frame", "colour": "x"})).unwrap();
    let w = warnings(&r);
    assert!(w.iter().any(|m| m.contains("widened")) && w.iter().any(|m| m.contains("\"colour\"")), "{w:?}");
}

#[test]
fn unknown_arguments_inside_batch_operations_warn_per_operation() {
    let (mut c, _board, p) = start("r2-unknown-batch");
    let r = c
        .tool(
            "batch",
            json!({"path": p, "operations": [
                {"tool": "add_shape", "args": {"geom": "rect", "text": "A", "italic": true}},
                {"tool": "add_shape", "args": {"geom": "rect", "text": "B"}}]}),
        )
        .unwrap();
    let res = r["results"].as_array().unwrap();
    assert!(warnings(&res[0]).iter().any(|m| m.contains("\"italic\"")), "{r}");
    assert!(res[1].get("warnings").is_none(), "{r}");
    assert!(r.get("warnings").is_none());
}

#[test]
fn check_layout_reports_a_sparse_frame_and_a_staircase() {
    let (mut c, _board, p) = start("r2-check");
    let a = rect(&mut c, &p, (0.0, 0.0, 200.0, 100.0));
    let f = id_of(&c.tool("add_frame", json!({"path": p, "ids": [a], "title": "A"})).unwrap());
    let kinds = |c: &mut Client| -> Vec<(String, Value)> {
        let r = c.tool("check_layout", json!({"path": p})).unwrap();
        r["issues"].as_array().unwrap().iter().map(|i| (i["kind"].as_str().unwrap().to_owned(), i["reason"].clone())).collect()
    };
    assert!(!kinds(&mut c).iter().any(|k| k.0 == "sparse_frame"));
    // the content moves away: the frame is left mostly empty
    c.tool("update_object", json!({"path": p, "id": a, "x": 40, "y": 600})).unwrap();
    let r = c.tool("check_layout", json!({"path": p})).unwrap();
    let s = r["issues"].as_array().unwrap().iter().find(|i| i["kind"] == "sparse_frame");
    // the content left the frame: it no longer contains anything, so it is not a frame for the check
    assert!(s.is_none(), "{r}");
    // a frame stretched around its content
    let g = rect(&mut c, &p, (1000.0, 0.0, 100.0, 60.0));
    c.tool("add_frame", json!({"path": p, "ids": [g], "x": 900, "y": -40, "w": 700, "h": 500})).unwrap();
    let r = c.tool("check_layout", json!({"path": p})).unwrap();
    let s = r["issues"].as_array().unwrap().iter().find(|i| i["kind"] == "sparse_frame").expect("sparse frame");
    assert!(s["message"].as_str().unwrap().contains("add_frame with frame="), "{s}");
    assert_ne!(s["ids"][0].as_str().unwrap(), f);
    // staircase
    let (mut c, _board, p) = start("r2-stair");
    for (i, y) in [0.0, 120.0].into_iter().enumerate() {
        let x = f64::from(u8::try_from(i).unwrap()) * 400.0;
        let n = rect(&mut c, &p, (x + 40.0, y + 40.0, 200.0, 200.0));
        c.tool("add_frame", json!({"path": p, "ids": [n], "padding": 40})).unwrap();
    }
    let r = c.tool("check_layout", json!({"path": p})).unwrap();
    let u = r["issues"].as_array().unwrap().iter().find(|i| i["reason"] == "staircase").expect("staircase");
    assert_eq!(u["kind"], "unbalanced");
    assert_eq!(u["ids"].as_array().unwrap().len(), 2);
    assert!(r["metrics"]["frame_top_offset"].as_f64().unwrap() >= 120.0 - 1e-6);
    assert!(r["metrics"]["coverage"].is_number());
}
