//! Style additions through the MCP tools: `head_scale` of connectors on `add_connector` and `update_object`, and what `describe_board` reports.
//! With the real `inkboard-mcp` binary.

// Test code names a client, a path and two shapes with one letter each.
#![allow(clippy::many_single_char_names)]

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Kind, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A board with two shapes; returns the client, the path and the two ids.
fn setup(tag: &str) -> (Client, PathBuf, String, String) {
    let dir = temp_dir(tag);
    let mut c = Client::start(&dir);
    let board = dir.join("s.inkboard");
    let p = board.display().to_string();
    c.tool("create_board", json!({"path": p})).unwrap();
    let a = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "A", "x": 0, "y": 0, "w": 100, "h": 60})).unwrap()["id"].as_str().unwrap().to_owned();
    let b =
        c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "B", "x": 300, "y": 0, "w": 100, "h": 60})).unwrap()["id"].as_str().unwrap().to_owned();
    (c, board, a, b)
}

fn connector_of(board: &Path) -> inkboard_core::Connector {
    let doc = serial::load(board).unwrap();
    doc.pages[0].objects.iter().find_map(|o| if let Kind::Connector(c) = &o.kind { Some(c.clone()) } else { None }).expect("a connector")
}

fn described(c: &mut Client, board: &Path) -> Value {
    let d = c.tool("describe_board", json!({"path": board.display().to_string(), "detail": "full"})).unwrap();
    d["page"]["objects"].as_array().unwrap().iter().find(|o| o["type"] == "connector").cloned().expect("a connector")
}

#[test]
fn head_scale_is_set_when_adding_and_when_updating_a_connector_and_reported() {
    let (mut c, board, a, b) = setup("style-head-scale");
    let p = board.display().to_string();
    let id = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "head_scale": 2})).unwrap()["id"].as_str().unwrap().to_owned();
    assert!((connector_of(&board).head_scale - 2.0).abs() < 1e-6);
    assert_eq!(described(&mut c, &board)["head_scale"], json!(2.0));
    c.tool("update_object", json!({"path": p, "id": id, "head_scale": 0.5})).unwrap();
    assert!((connector_of(&board).head_scale - 0.5).abs() < 1e-6);
    c.tool("update_object", json!({"path": p, "id": id, "head_scale": 1})).unwrap();
    assert!(described(&mut c, &board).get("head_scale").is_none(), "the usual size is not repeated in the summary");
    // on something that is not a connector it is reported as ignored
    let r = c.tool("update_object", json!({"path": p, "id": a, "head_scale": 2})).unwrap();
    assert!(r.to_string().contains("head_scale"), "{r}");
}

#[test]
fn an_out_of_range_head_scale_is_refused_with_the_range_and_changes_nothing() {
    let (mut c, board, a, b) = setup("style-head-scale-bad");
    let p = board.display().to_string();
    let before = std::fs::read(&board).unwrap();
    for bad in [json!(0.1), json!(5), json!(-1), json!("big")] {
        let e = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "head_scale": bad})).unwrap_err();
        assert!(e.contains("head_scale") && e.contains("0.5") && e.contains('4'), "{bad}: {e}");
    }
    assert_eq!(std::fs::read(&board).unwrap(), before);
    let id = c.tool("add_connector", json!({"path": p, "from": a, "to": b})).unwrap()["id"].as_str().unwrap().to_owned();
    let before = std::fs::read(&board).unwrap();
    let e = c.tool("update_object", json!({"path": p, "id": id, "head_scale": 9, "stroke_width": 5})).unwrap_err();
    assert!(e.contains("head_scale"), "{e}");
    assert_eq!(std::fs::read(&board).unwrap(), before, "nothing of the call is applied");
}
