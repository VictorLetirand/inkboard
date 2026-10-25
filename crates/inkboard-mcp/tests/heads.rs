//! Arrowheads of connectors through the MCP tools: `head_start` / `head_end` on `add_connector` and `update_object`, what `describe_board` reports,
//! and what the saved file and the exports hold. With the real `inkboard-mcp` binary.

// Test code names a client, a path and two shapes with one letter each.
#![allow(clippy::many_single_char_names)]

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Arrowhead, HeadExt, Kind, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A board with two shapes; returns the client, the path and the two ids.
fn setup(tag: &str) -> (Client, PathBuf, String, String) {
    let dir = temp_dir(tag);
    let mut c = Client::start(&dir);
    let board = dir.join("h.inkboard");
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
fn add_connector_takes_an_original_or_an_added_head_for_each_end() {
    let (mut c, board, a, b) = setup("heads-add");
    let p = board.display().to_string();
    c.tool("add_connector", json!({"path": p, "from": a, "to": b, "head_start": "diamond", "head_end": "crow_foot_zero"})).unwrap();
    let conn = connector_of(&board);
    assert_eq!((conn.head_start_ext, conn.head_end_ext), (Some(HeadExt::Diamond), Some(HeadExt::CrowFootZero)));
    assert_eq!((conn.head_start, conn.head_end), (Arrowhead::Dot, Arrowhead::Arrow), "the original fields hold what older versions draw");
    let d = described(&mut c, &board);
    assert_eq!(d["heads"], json!({"start": "diamond", "end": "crow_foot_zero"}));
    assert_eq!(d["arrow"], "both");
}

#[test]
fn a_head_named_for_one_end_overrides_arrow_for_that_end_only() {
    let (mut c, board, a, b) = setup("heads-override");
    let p = board.display().to_string();
    c.tool("add_connector", json!({"path": p, "from": a, "to": b, "arrow": "both", "head_end": "triangle_open"})).unwrap();
    let conn = connector_of(&board);
    assert_eq!((conn.head_start, conn.head_start_ext), (Arrowhead::Arrow, None), "the start keeps what arrow gave it");
    assert_eq!(conn.head_end_ext, Some(HeadExt::TriangleOpen));
    let d = described(&mut c, &board);
    assert_eq!(d["heads"], json!({"start": "arrow", "end": "triangle_open"}));
    // original names work too, and clear an added head
    c.tool("add_connector", json!({"path": p, "from": b, "to": a, "head_end": "bar", "head_start": "none"})).unwrap();
    let doc = serial::load(&board).unwrap();
    let second = doc.pages[0].objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).nth(1).unwrap();
    assert_eq!((second.head_start, second.head_end, second.head_start_ext, second.head_end_ext), (Arrowhead::None, Arrowhead::Bar, None, None));
}

#[test]
fn update_object_changes_heads_and_arrow_resets_added_ones() {
    let (mut c, board, a, b) = setup("heads-update");
    let p = board.display().to_string();
    let id = c.tool("add_connector", json!({"path": p, "from": a, "to": b})).unwrap()["id"].as_str().unwrap().to_owned();
    c.tool("update_object", json!({"path": p, "id": id, "head_start": "one_only", "head_end": "crow_foot_one"})).unwrap();
    let conn = connector_of(&board);
    assert_eq!((conn.head_start_ext, conn.head_end_ext), (Some(HeadExt::OneOnly), Some(HeadExt::CrowFootOne)));
    c.tool("update_object", json!({"path": p, "id": id, "arrow": "end"})).unwrap();
    let conn = connector_of(&board);
    assert_eq!(
        (conn.head_start, conn.head_end, conn.head_start_ext, conn.head_end_ext),
        (Arrowhead::None, Arrowhead::Arrow, None, None),
        "arrow gives back plain heads"
    );
    // a head named together with arrow wins for its end
    c.tool("update_object", json!({"path": p, "id": id, "arrow": "none", "head_end": "diamond_open"})).unwrap();
    let conn = connector_of(&board);
    assert_eq!((conn.head_start, conn.head_end_ext), (Arrowhead::None, Some(HeadExt::DiamondOpen)));
    // a head on something that is not a connector is reported as ignored, not applied silently
    let r = c.tool("update_object", json!({"path": p, "id": a, "head_end": "diamond"})).unwrap();
    assert!(r.to_string().contains("head_end"), "{r}");
}

#[test]
fn unknown_head_names_are_refused_with_the_list_and_change_nothing() {
    let (mut c, board, a, b) = setup("heads-bad");
    let p = board.display().to_string();
    let before = std::fs::read(&board).unwrap();
    for bad in [json!("hexagon"), json!("Diamond"), json!(3), json!(["diamond"])] {
        let e = c.tool("add_connector", json!({"path": p, "from": a, "to": b, "head_end": bad})).unwrap_err();
        assert!(e.contains("head_end") && e.contains("crow_foot_zero") && e.contains("triangle"), "{bad}: {e}");
    }
    assert_eq!(std::fs::read(&board).unwrap(), before);
    let id = c.tool("add_connector", json!({"path": p, "from": a, "to": b})).unwrap()["id"].as_str().unwrap().to_owned();
    let e = c.tool("update_object", json!({"path": p, "id": id, "head_start": "nope"})).unwrap_err();
    assert!(e.contains("head_start") && e.contains("one_only"), "{e}");
}

#[test]
fn tool_schemas_list_every_head_and_the_exports_carry_them() {
    let (mut c, board, a, b) = setup("heads-export");
    let tools = c.request("tools/list", json!({}))["result"]["tools"].as_array().unwrap().clone();
    for tool in ["add_connector", "update_object"] {
        let schema = tools.iter().find(|t| t["name"] == tool).unwrap();
        for prop in ["head_start", "head_end"] {
            let names: Vec<&str> = schema["inputSchema"]["properties"][prop]["enum"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
            assert_eq!(names.len(), 5 + HeadExt::ALL.len(), "{tool} {prop}");
            for e in HeadExt::ALL {
                assert!(names.contains(&e.name()), "{tool} {prop} {}", e.name());
            }
        }
    }
    let p = board.display().to_string();
    c.tool("add_connector", json!({"path": p, "from": a, "to": b, "head_start": "diamond_open", "head_end": "crow_foot_one"})).unwrap();
    let dir = board.parent().unwrap();
    let svg = dir.join("h.svg");
    c.tool("export", json!({"path": p, "output": svg.display().to_string()})).unwrap();
    let svg = std::fs::read_to_string(svg).unwrap();
    assert!(svg.contains("<polygon"), "the hollow diamond is drawn as a polygon filled with the background");
    let drawio = dir.join("h.drawio");
    c.tool("export", json!({"path": p, "output": drawio.display().to_string()})).unwrap();
    let drawio = std::fs::read_to_string(drawio).unwrap();
    assert!(drawio.contains("endArrow=ERoneToMany") && drawio.contains("startArrow=diamond;") && drawio.contains("startFill=0"), "{drawio}");
    let png = dir.join("h.png");
    c.tool("export", json!({"path": p, "output": png.display().to_string()})).unwrap();
    assert!(std::fs::metadata(png).unwrap().len() > 100);
}
