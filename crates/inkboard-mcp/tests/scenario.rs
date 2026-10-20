//! The whole agent workflow of `context/PLAN_MCP_V2.md` (§8) with the real binary: one `batch` builds an architecture diagram with icons,
//! formatted captions and bound arrows; `render` shows it with the object ids; the agent fixes an object and arranges the result.

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Kind, serial};
use serde_json::{Value, json};

#[test]
fn an_agent_builds_checks_and_fixes_an_architecture_diagram() {
    let dir = temp_dir("scenario");
    let mut c = Client::start(&dir);
    let board = dir.join("arch.inkboard");
    let p = board.to_str().unwrap();
    c.tool("create_board", json!({"path": p, "name": "Architecture"})).unwrap();

    // two icons (found by search), one box, three bound arrows, formatted captions, all in one call
    let found = |c: &mut Client, query: &str, group: &str| -> String {
        let r = c.tool("search_icons", json!({"path": p, "query": query, "group": group, "limit": 1})).unwrap();
        serde_json::from_str::<Value>(&r.to_string()).map(|v| v["icons"][0]["id"].as_str().expect("an icon").to_owned()).unwrap()
    };
    let lambda = found(&mut c, "lambda", "AWS");
    let bucket = found(&mut c, "s3", "AWS");
    let ops = json!([
        {"tool": "add_icon", "args": {"icon": lambda, "x": 0, "y": 0, "size": 80, "label": "**Lambda**"}},
        {"tool": "add_icon", "args": {"icon": bucket, "x": 300, "y": 0, "size": 80, "label": "{color=#e03131}Bucket{/}"}},
        {"tool": "add_shape", "args": {"geom": "rect", "text": "**API** gateway", "markdown": true, "x": 150, "y": 200}},
        {"tool": "add_connector", "args": {"from": "$0.id", "to": "$1.id", "label": "writes", "route": "elbow"}},
        {"tool": "add_connector", "args": {"from": "$2.id", "to": "$0.id"}},
    ]);
    let r = c.tool("batch", json!({"path": p, "operations": ops})).unwrap();
    assert_eq!(r["applied"], 5, "{r}");
    let objects = serial::load(&board).unwrap().pages[0].objects.len();
    assert!(objects >= 7, "icons, captions, box and arrows are there ({objects})");

    // the agent looks at the result, with the ids drawn on it
    let shot = c.request("tools/call", json!({"name": "render", "arguments": {"path": p, "show_ids": true, "max_px": 800}}));
    assert_eq!(shot["result"]["content"][0]["type"], "image", "{shot}");
    let info: Value = serde_json::from_str(shot["result"]["content"][1]["text"].as_str().unwrap()).unwrap();
    assert!(info["ids_shown"].as_u64().unwrap() >= 5, "{info}");

    // it reads the styles, then fixes and aligns
    let full = c.tool("describe_board", json!({"path": p, "detail": "full"})).unwrap();
    let rect = full["page"]["objects"].as_array().unwrap().iter().find(|o| o["type"] == "rect").expect("the box").clone();
    assert!(rect["style"]["stroke"].is_string() && rect["font"]["size"].is_number(), "{rect}");
    let id = rect["id"].as_str().unwrap();
    c.tool("update_object", json!({"path": p, "id": id, "fill": "#ffec99", "stroke_width": 4})).unwrap();
    c.tool("format_text", json!({"path": p, "id": id, "match": "gateway", "italic": true})).unwrap();
    let found_boxes = c.tool("find_objects", json!({"path": p, "type": "rect"})).unwrap();
    assert_eq!(found_boxes["count"], 1, "{found_boxes}");

    let doc = serial::load(&board).unwrap();
    let Kind::Box(b) = &doc.pages[0].get(&inkboard_core::Id(id.to_owned())).unwrap().kind else { panic!("not a box") };
    assert_eq!(b.label.text.to_plain(), "API gateway");
    assert!(b.label.text.0.iter().any(|s| s.style.bold) && b.label.text.0.iter().any(|s| s.style.italic));
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}
