//! Attachment sides (`from_side`, `to_side`), label position (`label_pos`, `label_offset`) and vertical text alignment (`valign`) through the
//! MCP tools, with the real `inkboard-mcp` binary: accepted, validated, stored, reported by `describe_board` and seen by `check_layout`.

// Test code names a client, a path and two shapes with one letter each.
#![allow(clippy::many_single_char_names)]

mod common;

use common::{Client, temp_dir};
use inkboard_core::{AttachSide, Kind, VAlign, serial};
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
        c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "B", "x": 300, "y": 200, "w": 100, "h": 60})).unwrap()["id"].as_str().unwrap().to_owned();
    (c, board, a, b)
}

fn connector_of(board: &Path) -> inkboard_core::Connector {
    let doc = serial::load(board).unwrap();
    doc.pages[0].objects.iter().find_map(|o| if let Kind::Connector(c) = &o.kind { Some(c.clone()) } else { None }).expect("a connector")
}

fn described(c: &mut Client, board: &Path, kind: &str) -> Value {
    let d = c.tool("describe_board", json!({"path": board.display().to_string(), "detail": "full"})).unwrap();
    d["page"]["objects"].as_array().unwrap().iter().find(|o| o["type"] == kind).cloned().unwrap_or_else(|| panic!("a {kind}"))
}

#[test]
fn sides_are_set_validated_reported_and_cleared_with_auto() {
    let (mut c, board, a, b) = setup("attach-sides");
    let p = board.display().to_string();
    let id = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "route": "elbow", "from_side": "bottom", "to_side": "left"})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let k = connector_of(&board);
    assert_eq!((k.start.side(), k.end.side()), (Some(AttachSide::Bottom), Some(AttachSide::Left)));
    let d = described(&mut c, &board, "connector");
    assert_eq!((d["from_side"].as_str(), d["to_side"].as_str()), (Some("bottom"), Some("left")));
    // one end changed, the other kept
    c.tool("update_object", json!({"path": p, "id": id, "to_side": "top"})).unwrap();
    let k = connector_of(&board);
    assert_eq!((k.start.side(), k.end.side()), (Some(AttachSide::Bottom), Some(AttachSide::Top)));
    // auto clears, and a connector without sides reports none
    c.tool("update_object", json!({"path": p, "id": id, "from_side": "auto", "to_side": "auto"})).unwrap();
    let k = connector_of(&board);
    assert_eq!((k.start.side(), k.end.side()), (None, None));
    let d = described(&mut c, &board, "connector");
    assert!(d.get("from_side").is_none() && d.get("to_side").is_none(), "{d}");
    // reconnecting an end in the same call: the side applies to the new target
    c.tool("update_object", json!({"path": p, "id": id, "from": b, "to": a, "from_side": "right"})).unwrap();
    let k = connector_of(&board);
    assert_eq!((k.start.side(), k.end.side()), (Some(AttachSide::Right), None));
    // reconnecting alone drops the side of that end (it was chosen for the old target)
    c.tool("update_object", json!({"path": p, "id": id, "from": a})).unwrap();
    assert_eq!(connector_of(&board).start.side(), None);
}

#[test]
fn a_bad_side_is_an_error_that_changes_nothing_and_a_shape_ignores_it() {
    let (mut c, board, a, b) = setup("attach-sides-bad");
    let p = board.display().to_string();
    let e = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "from_side": "middle"})).unwrap_err();
    assert!(e.contains("from_side") && e.contains("top, bottom, left, right, auto"), "{e}");
    assert!(c.tool("add_connector", json!({"path": p, "from": a, "to": b, "to_side": 3})).is_err());
    let id = c.tool("add_connector", json!({"path": p, "from": a, "to": b})).unwrap()["id"].as_str().unwrap().to_owned();
    let e = c.tool("update_object", json!({"path": p, "id": id, "to_side": "up", "route": "elbow"})).unwrap_err();
    assert!(e.contains("to_side"), "{e}");
    assert_eq!(connector_of(&board).route, inkboard_core::Route::Straight, "nothing was applied");
    let r = c.tool("update_object", json!({"path": p, "id": a, "from_side": "top"})).unwrap();
    assert!(r.to_string().contains("from_side"), "reported as ignored: {r}");
}

#[test]
fn label_position_and_offset_are_set_validated_and_reported() {
    let (mut c, board, a, b) = setup("attach-label");
    let p = board.display().to_string();
    let id = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label": "flow", "label_pos": 0.25, "label_offset": -12})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let k = connector_of(&board);
    assert_eq!((k.label_pos, k.label_offset), (Some(0.25), -12.0));
    let d = described(&mut c, &board, "connector");
    assert_eq!((d["label_pos"].as_f64(), d["label_offset"].as_f64()), (Some(0.25), Some(-12.0)));
    c.tool("update_object", json!({"path": p, "id": id, "label_pos": 0.8})).unwrap();
    let k = connector_of(&board);
    assert_eq!((k.label_pos, k.label_offset), (Some(0.8), -12.0), "only what is given changes");
    c.tool("update_object", json!({"path": p, "id": id, "label_offset": 0})).unwrap();
    assert!(described(&mut c, &board, "connector").get("label_offset").is_none(), "a zero offset is not repeated");
    // out of range: refused, nothing changed
    for bad in [json!({"label_pos": 1.5}), json!({"label_pos": -0.1}), json!({"label_offset": 1001}), json!({"label_offset": "left"})] {
        let mut args = json!({"path": p, "id": id});
        args.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(c.tool("update_object", args).is_err(), "{bad}");
    }
    assert!(c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label": "x", "label_pos": 2})).is_err());
    assert_eq!(connector_of(&board).label_pos, Some(0.8));
}

#[test]
fn a_label_position_without_a_label_is_reported_and_a_shape_ignores_it() {
    let (mut c, board, a, b) = setup("attach-label-none");
    let p = board.display().to_string();
    let r = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label_pos": 0.3})).unwrap();
    assert!(r.to_string().contains("label_pos"), "{r}");
    assert_eq!(connector_of(&board).label_pos, None);
    let r = c.tool("update_object", json!({"path": p, "id": a, "label_pos": 0.3, "label_offset": 4})).unwrap();
    assert!(r.to_string().contains("label_pos") && r.to_string().contains("not applicable"), "{r}");
    // text and position in one call
    let id = described(&mut c, &board, "connector")["id"].as_str().unwrap().to_owned();
    c.tool("update_object", json!({"path": p, "id": id, "text": "yes", "label_pos": 0.7})).unwrap();
    assert_eq!(connector_of(&board).label_pos, Some(0.7));
}

#[test]
fn a_shape_carries_its_title_at_the_top_left_and_reports_it() {
    let (mut c, board, a, _) = setup("attach-valign");
    let p = board.display().to_string();
    let id = c
        .tool(
            "add_shape",
            json!({"path": p, "geom": "rect", "text": "Zone", "x": 0, "y": 400, "w": 300, "h": 200, "align": "left", "valign": "top", "fill": "#eeeeee"}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let doc = serial::load(&board).unwrap();
    let Kind::Box(shape) = &doc.pages[0].objects.iter().find(|o| o.id.0 == id).unwrap().kind else { panic!("shape") };
    assert_eq!((shape.label.align, shape.label.valign), (inkboard_core::Align::Left, VAlign::Top));
    assert!((shape.frame.h - 200.0).abs() < 1e-6, "the title does not make the frame grow");
    let d = c.tool("describe_board", json!({"path": p, "detail": "full"})).unwrap();
    let objs = d["page"]["objects"].as_array().unwrap();
    let zone = objs.iter().find(|o| o["id"] == id.as_str()).unwrap();
    assert_eq!((zone["font"]["valign"].as_str(), zone["font"]["align"].as_str()), (Some("top"), Some("left")));
    let other = objs.iter().find(|o| o["id"] == a.as_str()).unwrap();
    assert!(other["font"].get("valign").is_none(), "the usual middle is not repeated");
    // update
    c.tool("update_object", json!({"path": p, "id": id, "valign": "bottom"})).unwrap();
    let doc = serial::load(&board).unwrap();
    let Kind::Box(shape) = &doc.pages[0].objects.iter().find(|o| o.id.0 == id).unwrap().kind else { panic!("shape") };
    assert_eq!(shape.label.valign, VAlign::Bottom);
    // the file only has the key for a shape that asked for it
    let json = std::fs::read_to_string(&board).unwrap();
    assert_eq!(json.matches("\"valign\"").count(), 1, "{json}");
    // invalid, and ignored on a connector
    assert!(c.tool("update_object", json!({"path": p, "id": id, "valign": "center"})).is_err());
    assert!(c.tool("add_shape", json!({"path": p, "geom": "rect", "valign": "up"})).is_err());
    assert!(c.tool("add_shape", json!({"path": p, "geom": "rect", "align": "justify"})).is_err());
}

#[test]
fn elbows_fanning_out_do_not_share_a_segment_for_check_layout() {
    let dir = temp_dir("attach-fan");
    let mut c = Client::start(&dir);
    let board = dir.join("f.inkboard");
    let p = board.display().to_string();
    c.tool("create_board", json!({"path": p})).unwrap();
    let id_of = |r: Value| r["id"].as_str().unwrap().to_owned();
    let src = id_of(c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "S", "x": 0, "y": 200, "w": 100, "h": 60})).unwrap());
    for y in [0, 100, 300, 400] {
        let t = id_of(c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "T", "x": 400, "y": y, "w": 100, "h": 60})).unwrap());
        c.tool("add_connector", json!({"path": p, "from": src, "to": t, "route": "elbow"})).unwrap();
    }
    let r = c.tool("check_layout", json!({"path": p})).unwrap();
    let kinds: Vec<&str> = r["issues"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert!(!kinds.contains(&"shared_segment") && !kinds.contains(&"connector_crossing"), "{kinds:?}");
}
