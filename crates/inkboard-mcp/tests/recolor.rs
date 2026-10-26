//! `update_object` with `recolor` on an imported SVG image, with the real `inkboard-mcp` binary.

#![allow(clippy::assert_is_empty)]

mod common;

use common::{Client, temp_dir};
use inkboard_core::model::ImageObj;
use inkboard_core::{Color, Document, Frame, Id, Kind, Object, Style, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="5" height="10" fill="#ff0000"/><rect x="5" width="5" height="10" fill="#00ff00"/></svg>"##;

fn setup(tag: &str) -> (Client, PathBuf) {
    let dir = temp_dir(tag);
    let c = Client::start(&dir);
    let board = dir.join("b.inkboard");
    let mut doc = Document::new("Recolor");
    let svg = doc.add_asset("image/svg+xml", SVG.to_vec());
    let png = doc.add_asset("image/png", vec![1, 2, 3]);
    let img = |id: &str, asset, y| {
        let mut o = Object::new(
            Kind::Image(ImageObj { frame: Frame::new(0.0, y, 50.0, 50.0), asset, latex: None, mono: false, recolor: Vec::new() }),
            Style::default(),
        );
        o.id = Id::from(id);
        o
    };
    let objs = vec![img("im", svg, 0.0), img("bm", png, 100.0)];
    doc.pages[0].objects = objs;
    serial::save(&doc, &board).unwrap();
    (c, board)
}

fn call(c: &mut Client, board: &Path, args: Value) -> Result<Value, String> {
    let mut args = args;
    args["path"] = json!(board.display().to_string());
    c.tool("update_object", args)
}

fn recolor_of(board: &Path, id: &str) -> Vec<(Color, Color)> {
    let doc = serial::load(board).unwrap();
    match &doc.pages[0].get(&Id::from(id)).unwrap().kind {
        Kind::Image(i) => i.recolor.clone(),
        _ => panic!("not an image"),
    }
}

#[test]
fn recolor_is_set_from_an_object_or_an_array_and_cleared_by_an_empty_one() {
    let (mut c, board) = setup("recolor-set");
    call(&mut c, &board, json!({"id": "im", "recolor": {"#ff0000": "#0000ff"}})).unwrap();
    assert_eq!(recolor_of(&board, "im"), vec![(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255))]);
    call(&mut c, &board, json!({"id": "im", "recolor": [["#ff0000", "#112233"], ["#00ff00", "ink"]]})).unwrap();
    assert_eq!(recolor_of(&board, "im"), vec![(Color::rgb(255, 0, 0), Color::rgb(0x11, 0x22, 0x33)), (Color::rgb(0, 255, 0), Color::Ink)]);

    let d = c.tool("describe_board", json!({"path": board.display().to_string(), "detail": "full"})).unwrap();
    let im = d["page"]["objects"].as_array().unwrap().iter().find(|o| o["id"] == "im").unwrap();
    assert_eq!(im["colors"], json!(["#ff0000", "#00ff00"]));
    assert_eq!(im["recolor"], json!({"#ff0000": "#112233", "#00ff00": "ink"}));

    call(&mut c, &board, json!({"id": "im", "recolor": {}})).unwrap();
    assert!(recolor_of(&board, "im").is_empty());
}

#[test]
fn invalid_recolor_is_an_error_and_changes_nothing() {
    let (mut c, board) = setup("recolor-bad");
    call(&mut c, &board, json!({"id": "im", "recolor": {"#ff0000": "#0000ff"}})).unwrap();
    for bad in [
        json!({"#ff0000": "not-a-color"}),
        json!({"nope": "#000000"}),
        json!({"ink": "#000000"}),
        json!([["#ff0000", "#000000"], ["#ff0000", "#111111"]]),
        json!([["#ff0000"]]),
        json!("red"),
    ] {
        let before = std::fs::read(&board).unwrap();
        let err = call(&mut c, &board, json!({"id": "im", "recolor": bad, "x": 99})).unwrap_err();
        assert!(err.contains("recolor"), "{err}");
        assert_eq!(std::fs::read(&board).unwrap(), before, "the file changed despite the error for {bad}");
    }
}

#[test]
fn recolor_of_a_bitmap_is_ignored_with_a_warning() {
    let (mut c, board) = setup("recolor-bitmap");
    let r = call(&mut c, &board, json!({"id": "bm", "recolor": {"#ff0000": "#0000ff"}})).unwrap();
    assert!(r["avertissements"].to_string().contains("recolor"), "{r}");
    assert!(recolor_of(&board, "bm").is_empty());
}

#[test]
fn exported_svg_carries_the_new_colors() {
    let (mut c, board) = setup("recolor-export");
    call(&mut c, &board, json!({"id": "im", "recolor": {"#ff0000": "#0000ff", "#00ff00": "#ff0000"}})).unwrap();
    let out = board.with_file_name("o.svg");
    c.tool("export", json!({"path": board.display().to_string(), "output": out.display().to_string()})).unwrap();
    let svg = std::fs::read_to_string(&out).unwrap();
    let data = svg.split("data:image/svg+xml;base64,").nth(1).and_then(|s| s.split('"').next()).expect("embedded svg");
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).unwrap();
    let inner = String::from_utf8(bytes).unwrap();
    assert!(inner.contains("fill=\"#0000ff\"") && inner.contains("fill=\"#ff0000\"") && !inner.contains("#00ff00"), "{inner}");
}
