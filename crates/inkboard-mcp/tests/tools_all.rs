//! Every documented tool, with the real `inkboard-mcp` binary: one valid call (the file on disk reflects it) and one invalid call
//! (an error, then the server still answers). Also: malformed input does not stop the server.

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Document, Kind, Object, connector::End, serial};
use serde_json::{Value, json};
use std::path::Path;

/// The 16 tools of `docs/MCP.md`.
const TOOLS: [&str; 16] = [
    "create_board",
    "save_board",
    "board_changes",
    "add_image",
    "describe_board",
    "add_shape",
    "add_connector",
    "add_table",
    "add_code_block",
    "add_formula",
    "import_mermaid",
    "update_object",
    "delete_object",
    "auto_layout",
    "add_page",
    "export",
];

/// 1×1 PNG (red).
const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

fn load(path: &Path) -> Document {
    serial::load(path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()))
}

fn objects(path: &Path) -> Vec<Object> {
    load(path).pages[0].objects.clone()
}

fn object(path: &Path, id: &str) -> Object {
    objects(path).into_iter().find(|o| o.id.0 == id).unwrap_or_else(|| panic!("object {id} not in the file"))
}

fn id_of(r: &Value) -> String {
    r["id"].as_str().unwrap_or_else(|| panic!("no id in {r}")).to_owned()
}

fn text_of(o: &Object) -> String {
    match &o.kind {
        Kind::Box(b) => b.label.text.to_plain(),
        k => panic!("not a box: {k:?}"),
    }
}

/// An invalid call must fail (JSON-RPC error, or tool result with `isError`), leave the file untouched, and the server must answer the next call.
fn invalid(c: &mut Client, board: &Path, tool: &str, args: Value) {
    let before = std::fs::read(board).unwrap();
    let r = c.request("tools/call", json!({"name": tool, "arguments": args}));
    assert!(r["error"].is_object() || r["result"]["isError"] == json!(true), "{tool} with {args}: error expected, got {r}");
    assert_eq!(std::fs::read(board).unwrap(), before, "{tool}: the file changed despite the error");
    assert_eq!(c.request("ping", json!({}))["result"], json!({}), "{tool}: the server no longer answers");
}

#[test]
fn tools_list_returns_the_16_documented_tools() {
    let dir = temp_dir("tl");
    let mut c = Client::start(&dir);
    let list = c.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    let mut expected = TOOLS.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    assert!(tools.iter().all(|t| t["description"].as_str().is_some_and(|d| !d.is_empty()) && t["inputSchema"]["type"] == "object"));
}

#[test]
fn every_tool_changes_the_file_when_called_correctly() {
    let dir = temp_dir("valid");
    let mut c = Client::start(&dir);
    let board = dir.join("b.inkboard.json");
    let p = board.to_str().unwrap();

    c.tool("create_board", json!({"path": p, "name": "Tools"})).unwrap();
    let doc = load(&board);
    assert_eq!((doc.name.as_str(), doc.pages.len(), doc.pages[0].objects.len()), ("Tools", 1, 0));

    let a = id_of(&c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "A", "x": 10, "y": 20, "w": 100, "h": 60})).unwrap());
    let o = object(&board, &a);
    assert_eq!(text_of(&o), "A");
    let f = o.frame().unwrap();
    assert_eq!((f.x, f.y, f.w, f.h), (10.0, 20.0, 100.0, 60.0));
    let b = id_of(&c.tool("add_shape", json!({"path": p, "geom": "ellipse", "text": "B"})).unwrap());
    let gone = id_of(&c.tool("add_shape", json!({"path": p, "geom": "sticky", "text": "to delete"})).unwrap());
    assert_eq!(objects(&board).len(), 3);

    c.tool("add_connector", json!({"path": p, "from": a, "to": b, "label": "link"})).unwrap();
    let conn = objects(&board).into_iter().find_map(|o| if let Kind::Connector(k) = o.kind { Some(k) } else { None }).expect("connector in the file");
    assert!(matches!(&conn.start, End::Bound { target, .. } if target.0 == a), "start bound to A");
    assert!(matches!(&conn.end, End::Bound { target, .. } if target.0 == b), "end bound to B");

    c.tool("add_table", json!({"path": p, "rows": [["h1", "h2"], ["c1", "c2"]]})).unwrap();
    let cells = objects(&board).into_iter().find_map(|o| if let Kind::Table(t) = o.kind { Some(t.cells.len()) } else { None });
    assert_eq!(cells, Some(4));

    c.tool("add_code_block", json!({"path": p, "code": "fn main() {}"})).unwrap();
    assert!(objects(&board).iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.label.text.to_plain().contains("fn main"))), "code block in the file");

    c.tool("add_formula", json!({"path": p, "latex": r"\frac{a}{b}"})).unwrap();
    assert!(objects(&board).iter().any(|o| matches!(&o.kind, Kind::Image(i) if i.latex.as_deref() == Some(r"\frac{a}{b}"))), "formula in the file");

    let before = objects(&board).len();
    c.tool("add_image", json!({"path": p, "data_base64": PNG_1X1})).unwrap();
    assert_eq!(objects(&board).len(), before + 1);
    assert!(objects(&board).iter().any(|o| matches!(&o.kind, Kind::Image(i) if i.latex.is_none())), "image in the file");
    assert!(!load(&board).assets.is_empty(), "image data stored");

    let before = objects(&board).len();
    let mm = c.tool("import_mermaid", json!({"path": p, "mermaid": "flowchart LR\n  X[One] --> Y[Two]"})).unwrap();
    assert_eq!(mm["created"], 3, "{mm}");
    assert_eq!(objects(&board).len(), before + 3);

    c.tool("update_object", json!({"path": p, "id": a, "text": "Renamed", "x": 300, "y": 400})).unwrap();
    let o = object(&board, &a);
    assert_eq!(text_of(&o), "Renamed");
    let f = o.frame().unwrap();
    assert_eq!((f.x, f.y), (300.0, 400.0));

    let before = load(&board).pages[0].objects.iter().map(|o| (o.id.0.clone(), o.frame().map(|f| (f.x, f.y)))).collect::<Vec<_>>();
    c.tool("auto_layout", json!({"path": p, "direction": "LR"})).unwrap();
    let after = load(&board).pages[0].objects.iter().map(|o| (o.id.0.clone(), o.frame().map(|f| (f.x, f.y)))).collect::<Vec<_>>();
    assert_eq!(before.len(), after.len());
    assert_ne!(before, after, "auto_layout moved at least one shape");

    c.tool("delete_object", json!({"path": p, "id": gone})).unwrap();
    assert!(objects(&board).iter().all(|o| o.id.0 != gone));

    c.tool("add_page", json!({"path": p, "name": "Second"})).unwrap();
    let doc = load(&board);
    assert_eq!((doc.pages.len(), doc.pages[1].name.as_str()), (2, "Second"));

    // read-only tools and `save_board` (no effect in file mode): the file stays loadable and identical
    let bytes = std::fs::read(&board).unwrap();
    let d = c.tool("describe_board", json!({"path": p})).unwrap();
    assert_eq!(d["pages"].as_array().unwrap().len(), 2);
    assert_eq!(c.tool("board_changes", json!({"path": p})).unwrap()["unchanged"], true, "describe_board just took the baseline");
    c.tool("save_board", json!({"path": p})).unwrap();
    assert_eq!(std::fs::read(&board).unwrap(), bytes, "read-only tools do not touch the file");

    let svg = dir.join("out.svg");
    c.tool("export", json!({"path": p, "output": svg.to_str().unwrap()})).unwrap();
    let svg = std::fs::read_to_string(svg).unwrap();
    assert!(svg.contains("<svg") && svg.contains("Renamed"), "export reflects the board");
}

#[test]
fn every_tool_rejects_invalid_arguments_and_the_server_keeps_answering() {
    let dir = temp_dir("invalid");
    let mut c = Client::start(&dir);
    let board = dir.join("b.inkboard.json");
    let p = board.to_str().unwrap();
    c.tool("create_board", json!({"path": p})).unwrap();
    let a = id_of(&c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "A"})).unwrap());

    invalid(&mut c, &board, "create_board", json!({}));
    invalid(&mut c, &board, "create_board", json!({"path": 42}));
    invalid(&mut c, &board, "save_board", json!({}));
    invalid(&mut c, &board, "describe_board", json!({"path": p, "page": true}));
    invalid(&mut c, &board, "board_changes", json!({}));
    invalid(&mut c, &board, "add_image", json!({"path": p}));
    invalid(&mut c, &board, "add_image", json!({"path": p, "data_base64": 42}));
    invalid(&mut c, &board, "add_shape", json!({"path": p}));
    invalid(&mut c, &board, "add_shape", json!({"path": p, "geom": 5}));
    invalid(&mut c, &board, "add_connector", json!({"path": p, "from": a}));
    invalid(&mut c, &board, "add_connector", json!({"path": p, "from": a, "to": 7}));
    invalid(&mut c, &board, "add_table", json!({"path": p}));
    invalid(&mut c, &board, "add_table", json!({"path": p, "rows": "not a list"}));
    invalid(&mut c, &board, "add_code_block", json!({"path": p}));
    invalid(&mut c, &board, "add_code_block", json!({"path": p, "code": 42}));
    invalid(&mut c, &board, "add_formula", json!({"path": p}));
    invalid(&mut c, &board, "add_formula", json!({"path": p, "latex": 42}));
    invalid(&mut c, &board, "import_mermaid", json!({"path": p}));
    invalid(&mut c, &board, "import_mermaid", json!({"path": p, "mermaid": ["flowchart LR"]}));
    invalid(&mut c, &board, "update_object", json!({"path": p, "text": "x"}));
    invalid(&mut c, &board, "delete_object", json!({"path": p}));
    invalid(&mut c, &board, "delete_object", json!({"path": p, "id": 7}));
    invalid(&mut c, &board, "auto_layout", json!({"path": p, "direction": "sideways"}));
    invalid(&mut c, &board, "add_page", json!({}));
    invalid(&mut c, &board, "export", json!({"path": p}));
    invalid(&mut c, &board, "export", json!({"path": p, "output": 42}));
    // unknown tool: JSON-RPC error
    let r = c.request("tools/call", json!({"name": "nope", "arguments": {}}));
    assert_eq!(r["error"]["code"], -32602);
    // arguments absent altogether
    let r = c.request("tools/call", json!({"name": "add_shape"}));
    assert_eq!(r["result"]["isError"], true, "{r}");
}

#[test]
fn malformed_json_on_stdin_does_not_stop_the_server() {
    let dir = temp_dir("junk");
    let mut c = Client::start(&dir);
    for junk in ["{not json", "{\"jsonrpc\": \"2.0\", \"id\": 1, \"method\": ", "[]", "42", "\"text\"", "null", "{}", "\u{0}\u{1}\u{2}", "}{"] {
        c.send_raw(junk);
        assert_eq!(c.request("ping", json!({}))["result"], json!({}), "no answer after {junk:?}");
    }
    // a truncated tool call, then a good one
    c.send_raw(r#"{"jsonrpc":"2.0","id":99,"method":"tools/call","params":{"name":"create_board","arguments":{"pa"#);
    let board = dir.join("after.inkboard.json");
    c.tool("create_board", json!({"path": board.to_str().unwrap()})).unwrap();
    assert!(board.exists());
}
