//! The `render` tool, with the real `inkboard-mcp` binary: the picture comes back as an MCP image block, the board is never touched.

mod common;

use base64::{Engine, engine::general_purpose::STANDARD};
use common::{Client, temp_dir};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fn png_size(png: &[u8]) -> (u32, u32) {
    assert_eq!(&png[..8], PNG_SIGNATURE, "not a PNG");
    (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()))
}

/// Calls `render`; the picture (decoded) and the JSON text block, or the error message.
fn render(c: &mut Client, args: Value) -> Result<(Vec<u8>, Value), String> {
    let r = c.request("tools/call", json!({"name": "render", "arguments": args}));
    let content = r["result"]["content"].as_array().unwrap_or_else(|| panic!("no content in {}", &r.to_string()[..r.to_string().len().min(400)]));
    if r["result"]["isError"] == json!(true) {
        return Err(content[0]["text"].as_str().unwrap().to_owned());
    }
    assert_eq!(content.len(), 2, "an image block then a text block");
    assert_eq!((content[0]["type"].as_str(), content[0]["mimeType"].as_str()), (Some("image"), Some("image/png")), "the image is the first block");
    assert_eq!(content[1]["type"], "text");
    let png = STANDARD.decode(content[0]["data"].as_str().expect("base64 data")).expect("valid base64");
    let meta: Value = serde_json::from_str(content[1]["text"].as_str().unwrap()).expect("the text block is JSON");
    assert!(meta.get("_image").is_none(), "the reserved key is not repeated in the text");
    Ok((png, meta))
}

/// Board with A (rect, 0,0 200x100), B (ellipse, 1000,600 200x100) and a connector between them; returns the path and the ids.
fn fixture(c: &mut Client, dir: &Path) -> (PathBuf, String, String) {
    let board = dir.join("r.inkboard");
    let p = board.to_str().unwrap();
    c.tool("create_board", json!({"path": p, "name": "Render"})).unwrap();
    let a = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "Alpha", "x": 0, "y": 0, "w": 200, "h": 100})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let b = c.tool("add_shape", json!({"path": p, "geom": "ellipse", "text": "Beta", "x": 1000, "y": 600, "w": 200, "h": 100})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    c.tool("add_connector", json!({"path": p, "from": a, "to": b})).unwrap();
    (board, a, b)
}

/// The call must fail with a message, leave the board file untouched, and the server must still answer.
fn fails(c: &mut Client, board: &Path, mut args: Value) -> String {
    let before = std::fs::read(board).unwrap();
    args["path"] = json!(board.to_str().unwrap());
    let err = render(c, args.clone()).expect_err(&format!("an error was expected for {args}"));
    assert_eq!(std::fs::read(board).unwrap(), before, "the board changed despite the error");
    assert_eq!(c.request("ping", json!({}))["result"], json!({}), "the server no longer answers");
    err
}

#[test]
fn render_is_listed_with_a_schema() {
    let dir = temp_dir("render-list");
    let mut c = Client::start(&dir);
    let list = c.request("tools/list", json!({}));
    let tool = list["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "render").expect("render is listed").clone();
    assert!(tool["description"].as_str().unwrap().len() > 100);
    for k in ["region", "ids", "padding", "max_px", "show_ids", "output", "path", "page"] {
        assert!(tool["inputSchema"]["properties"].get(k).is_some(), "schema lacks {k}");
    }
}

#[test]
fn render_returns_a_png_image_block_then_json_text() {
    let dir = temp_dir("render-basic");
    let mut c = Client::start(&dir);
    let (board, _, _) = fixture(&mut c, &dir);
    let (png, meta) = render(&mut c, json!({"path": board.to_str().unwrap()})).unwrap();
    let (w, h) = png_size(&png);
    assert!(w.max(h) <= 1400 && w.max(h) > 1000, "{w}x{h}");
    assert_eq!((meta["width_px"].as_u64(), meta["height_px"].as_u64()), (Some(u64::from(w)), Some(u64::from(h))));
    assert_eq!(meta["ids_shown"], 0);
    assert!(meta["scale"].as_f64().unwrap() > 0.0);
    // default region: all the content, with the padding (24) around it
    let r = &meta["region"];
    assert!(r["x"].as_f64().unwrap() < 0.0 && r["y"].as_f64().unwrap() < 0.0 && r["w"].as_f64().unwrap() > 1200.0 && r["h"].as_f64().unwrap() > 700.0, "{r}");
    assert!(meta.get("output").is_none() && meta.get("note").is_none());
}

#[test]
fn other_tools_still_answer_with_a_single_text_block() {
    let dir = temp_dir("render-others");
    let mut c = Client::start(&dir);
    let (board, _, _) = fixture(&mut c, &dir);
    let r = c.request("tools/call", json!({"name": "describe_board", "arguments": {"path": board.to_str().unwrap()}}));
    let content = r["result"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
    assert!(serde_json::from_str::<Value>(content[0]["text"].as_str().unwrap()).is_ok());
}

#[test]
fn max_px_bounds_the_longest_side() {
    let dir = temp_dir("render-maxpx");
    let mut c = Client::start(&dir);
    let (board, _, _) = fixture(&mut c, &dir);
    let (png, meta) = render(&mut c, json!({"path": board.to_str().unwrap(), "max_px": 300})).unwrap();
    let (w, h) = png_size(&png);
    assert_eq!(w.max(h), 300, "{w}x{h}");
    assert!(meta["scale"].as_f64().unwrap() < 0.3);
}

#[test]
fn region_crops_the_picture() {
    let dir = temp_dir("render-region");
    let mut c = Client::start(&dir);
    let (board, _, _) = fixture(&mut c, &dir);
    let p = board.to_str().unwrap();
    let (full, _) = render(&mut c, json!({"path": p})).unwrap();
    let (crop, meta) = render(&mut c, json!({"path": p, "region": {"x": -20, "y": -20, "w": 300, "h": 200}})).unwrap();
    let (fw, fh) = png_size(&full);
    let (w, h) = png_size(&crop);
    assert_ne!((w, h), (fw, fh), "dimensions differ");
    assert_ne!(full, crop, "pixels differ");
    assert_eq!(w.max(h), 1200, "a small region is enlarged 4 times at most (300 units give 1200 px)");
    assert!((f64::from(w) / f64::from(h) - 1.5).abs() < 0.02, "aspect ratio of the region: {w}x{h}");
    assert_eq!(meta["region"], json!({"x": -20.0, "y": -20.0, "w": 300.0, "h": 200.0}));
    // a region far from any object is a valid (blank) picture
    let (blank, _) = render(&mut c, json!({"path": p, "region": {"x": 5000, "y": 5000, "w": 100, "h": 100}})).unwrap();
    assert_eq!(png_size(&blank), (400, 400));
    assert_ne!(blank, crop);
}

#[test]
fn ids_frame_the_objects_and_padding_enlarges() {
    let dir = temp_dir("render-ids");
    let mut c = Client::start(&dir);
    let (board, a, _) = fixture(&mut c, &dir);
    let p = board.to_str().unwrap();
    let region = |meta: &Value| (meta["region"]["x"].as_f64().unwrap(), meta["region"]["w"].as_f64().unwrap());
    let (_, tight) = render(&mut c, json!({"path": p, "ids": [a], "padding": 0})).unwrap();
    let (_, loose) = render(&mut c, json!({"path": p, "ids": [a], "padding": 100})).unwrap();
    let ((tx, tw), (lx, lw)) = (region(&tight), region(&loose));
    assert!(tx <= 0.0 && tx > -10.0 && (200.0..215.0).contains(&tw), "the frame hugs the object: {tight}");
    assert!((lw - tw - 200.0).abs() < 0.01 && (tx - lx - 100.0).abs() < 0.01, "padding adds 100 on each side: {tight} / {loose}");
    let (full_png, full) = render(&mut c, json!({"path": p})).unwrap();
    assert!(lw < full["region"]["w"].as_f64().unwrap());
    let (png, _) = render(&mut c, json!({"path": p, "ids": [a]})).unwrap();
    assert_ne!(png, full_png);
}

#[test]
fn show_ids_changes_the_picture_but_never_the_board() {
    let dir = temp_dir("render-showids");
    let mut c = Client::start(&dir);
    let (board, _, _) = fixture(&mut c, &dir);
    let p = board.to_str().unwrap();
    let before = std::fs::read(&board).unwrap();
    let (plain, _) = render(&mut c, json!({"path": p})).unwrap();
    let (badged, meta) = render(&mut c, json!({"path": p, "show_ids": true})).unwrap();
    assert_eq!(meta["ids_shown"], 3, "two shapes and the connector");
    assert_ne!(plain, badged, "badges are drawn");
    assert_eq!(png_size(&plain), png_size(&badged));
    assert_eq!(std::fs::read(&board).unwrap(), before, "the .inkboard file is byte for byte the same");
    // and the same call without show_ids afterwards still gives the original picture
    let (again, meta) = render(&mut c, json!({"path": p, "show_ids": false})).unwrap();
    assert_eq!((again, meta["ids_shown"].as_u64()), (plain, Some(0)));
    // only the badges inside the region are counted
    let (_, meta) = render(&mut c, json!({"path": p, "show_ids": true, "region": {"x": -30, "y": -30, "w": 300, "h": 200}})).unwrap();
    assert_eq!(meta["ids_shown"], 1);
}

#[test]
fn invalid_arguments_are_refused_and_the_board_is_untouched() {
    let dir = temp_dir("render-errors");
    let mut c = Client::start(&dir);
    let (board, a, _) = fixture(&mut c, &dir);
    let region = json!({"x": 0, "y": 0, "w": 10, "h": 10});

    assert!(fails(&mut c, &board, json!({"region": region, "ids": [a]})).contains("give either region or ids"));
    let err = fails(&mut c, &board, json!({"ids": [a, "nope1", "nope2"]}));
    assert!(err.contains("nope1") && err.contains("nope2") && !err.contains(&a), "{err}");
    assert!(fails(&mut c, &board, json!({"ids": []})).contains("ids"));
    assert!(fails(&mut c, &board, json!({"ids": [1]})).contains("strings"));
    assert!(fails(&mut c, &board, json!({"region": {"x": 0, "y": 0, "w": 0, "h": 10}})).contains("greater than 0"));
    assert!(fails(&mut c, &board, json!({"region": {"x": 0, "y": 0, "w": 10}})).contains("region.h"));
    assert!(fails(&mut c, &board, json!({"max_px": 3001})).contains("max_px"));
    assert!(fails(&mut c, &board, json!({"max_px": 5})).contains("max_px"));
    assert!(fails(&mut c, &board, json!({"max_px": "big"})).contains("max_px"));
    assert!(fails(&mut c, &board, json!({"padding": -1})).contains("padding"));
    assert!(fails(&mut c, &board, json!({"padding": "wide"})).contains("padding"));
    assert!(fails(&mut c, &board, json!({"show_ids": "yes"})).contains("show_ids"));
    assert!(fails(&mut c, &board, json!({"page": 9})).contains("page not found"));
}

#[test]
fn an_empty_page_is_an_error() {
    let dir = temp_dir("render-empty");
    let mut c = Client::start(&dir);
    let board = dir.join("empty.inkboard");
    let p = board.to_str().unwrap();
    c.tool("create_board", json!({"path": p})).unwrap();
    assert!(fails(&mut c, &board, json!({})).contains("the page is empty: nothing to render"));
    assert!(fails(&mut c, &board, json!({"region": {"x": 0, "y": 0, "w": 100, "h": 100}})).contains("the page is empty"));
    // the second page of a board that has content on its first page only
    let (full, _, _) = fixture(&mut c, &dir);
    c.tool("add_page", json!({"path": full.to_str().unwrap()})).unwrap();
    assert!(fails(&mut c, &full, json!({"page": 2})).contains("the page is empty"));
    assert!(render(&mut c, json!({"path": full.to_str().unwrap(), "page": 1})).is_ok());
}

#[test]
fn output_writes_a_valid_png_under_the_root() {
    let dir = temp_dir("render-output");
    let mut c = Client::start(&dir);
    let (board, _, _) = fixture(&mut c, &dir);
    let (png, meta) = render(&mut c, json!({"path": board.to_str().unwrap(), "output": "shot.png", "max_px": 400})).unwrap();
    let written = dir.join("shot.png");
    assert_eq!(Path::new(meta["output"].as_str().unwrap()), written);
    let bytes = std::fs::read(&written).unwrap();
    assert_eq!(bytes, png, "the file holds the same picture as the image block");
    assert!(image::load_from_memory(&bytes).is_ok());
    assert_eq!(png_size(&bytes).0.max(png_size(&bytes).1), 400);
}

#[test]
fn output_outside_the_root_or_not_a_png_is_refused() {
    let dir = temp_dir("render-confine");
    let outside = temp_dir("render-confine-out");
    let mut c = Client::start(&dir);
    let (board, _, _) = fixture(&mut c, &dir);
    let p = board.to_str().unwrap();

    let target = outside.join("shot.png");
    let err = fails(&mut c, &board, json!({"output": target.to_str().unwrap()}));
    assert!(err.contains("outside the allowed root"), "{err}");
    assert!(!target.exists());
    let err = fails(&mut c, &board, json!({"output": "../shot.png"}));
    assert!(err.contains("outside the allowed root"), "{err}");
    assert!(!dir.parent().unwrap().join("shot.png").exists());

    // never overwrite a board (or anything else) with picture bytes
    let before = std::fs::read(&board).unwrap();
    for name in ["r.inkboard", "shot.svg", "shot"] {
        let err = fails(&mut c, &board, json!({"output": name}));
        assert!(err.contains(".png"), "{err}");
    }
    assert_eq!(std::fs::read(&board).unwrap(), before);
    assert!(!dir.join("shot.svg").exists() && !dir.join("shot").exists());
    assert!(render(&mut c, json!({"path": p})).is_ok(), "the board is still readable");
}

#[test]
fn a_png_over_4_mib_is_produced_smaller_automatically() {
    // incompressible content: a noise image shown large
    let (side, mut x) = (1300u32, 0x2545_F491_4F6C_DD1Du64);
    let noise = image::RgbImage::from_fn(side, side, |_, _| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        image::Rgb([x as u8, (x >> 8) as u8, (x >> 16) as u8])
    });
    let mut data = Vec::new();
    noise.write_to(&mut std::io::Cursor::new(&mut data), image::ImageFormat::Png).unwrap();

    let dir = temp_dir("render-big");
    let mut c = Client::start(&dir);
    let board = dir.join("big.inkboard");
    let p = board.to_str().unwrap();
    c.tool("create_board", json!({"path": p})).unwrap();
    c.tool("add_image", json!({"path": p, "data_base64": STANDARD.encode(&data), "x": 0, "y": 0, "w": 1300, "h": 1300})).unwrap();
    let (png, meta) = render(&mut c, json!({"path": p, "max_px": 1300, "padding": 0, "output": "big.png"})).unwrap();
    assert!(png.len() <= 4 << 20, "{} bytes", png.len());
    assert!(meta["max_px_used"].as_u64().unwrap() < 1300 && meta["note"].as_str().unwrap().contains("4 MiB"), "{meta}");
    assert!(png_size(&png).0 < 1300);
    assert_eq!(std::fs::read(dir.join("big.png")).unwrap(), png, "output holds the reduced picture");
}

#[test]
fn an_agent_renders_with_ids_fixes_an_object_and_renders_again() {
    let dir = temp_dir("render-session");
    let mut c = Client::start(&dir);
    let init = c.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}));
    assert_eq!(init["result"]["serverInfo"]["name"], "inkboard-mcp");
    c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let board = dir.join("flow.inkboard");
    let p = board.to_str().unwrap();
    c.tool("create_board", json!({"path": p, "name": "Flow"})).unwrap();
    c.tool("import_mermaid", json!({"path": p, "mermaid": "flowchart LR\n  A[Client] --> B[Server]\n  B --> C[(Database)]"})).unwrap();
    let described = c.tool("describe_board", json!({"path": p})).unwrap();
    let first = described["page"]["objects"].as_array().unwrap().iter().find(|o| o["type"] != "connector").unwrap()["id"].as_str().unwrap().to_owned();

    let (before, meta) = render(&mut c, json!({"path": p, "show_ids": true, "max_px": 800})).unwrap();
    assert!(meta["ids_shown"].as_u64().unwrap() >= 3);
    c.tool("update_object", json!({"path": p, "id": first, "text": "Browser"})).unwrap();
    let (after, _) = render(&mut c, json!({"path": p, "show_ids": true, "max_px": 800, "ids": [first]})).unwrap();
    assert_ne!(before, after);
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}
