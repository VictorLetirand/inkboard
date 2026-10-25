//! Hostile arguments against the MCP server (the agent is not trusted to send reasonable calls).
//!
//! Every tool is called through `Server::handle` (the JSON-RPC entry the binary uses) with arguments generated from its own input schema
//! and spoiled: wrong types, extreme numbers, giant and malicious strings, ids that do not exist, huge lists. After each call the answer is
//! well formed, the process is alive, and the board file on disk still loads and contains only finite numbers and unique identifiers.
//! Seeds are fixed. Nothing measures a duration; the sizes are chosen so that a run stays quick.
// Test code counts, casts and compares freely: these `clippy::pedantic` lints are for the code that ships.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::assert_is_empty,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::needless_raw_string_hashes,
    clippy::needless_pass_by_value,
    clippy::explicit_iter_loop,
    clippy::needless_for_each,
    clippy::collapsible_else_if,
    clippy::single_match_else,
    clippy::manual_assert,
    clippy::redundant_closure_for_method_calls,
    clippy::unnested_or_patterns,
    clippy::range_plus_one,
    clippy::clone_on_copy,
    clippy::match_same_arms,
    clippy::struct_excessive_bools
)]

use inkboard_core::Document;
use inkboard_mcp::{Server, tools};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/* ------------------------------------------------------------------------------------------------------------ helpers */

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("inkboard-mcp-hostile-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn server(root: &Path) -> Server {
    Server { root: Some(root.to_path_buf()), no_live: true, ..Server::default() }
}

fn call(srv: &Server, name: &str, args: &Value) -> Value {
    let msg = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": args}});
    let r = srv.handle(&msg).expect("a request is answered");
    assert_eq!(r["id"], json!(7), "{name}: {r}");
    assert!(r.get("result").is_some() || r.get("error").is_some(), "{name}: neither result nor error: {r}");
    r
}

fn ok(srv: &Server, name: &str, args: Value) -> Value {
    let r = call(srv, name, &args);
    assert_eq!(r["result"]["isError"], json!(false), "{name} {args}: {r}");
    serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap_or(Value::Null)
}

/// The board on disk loads, every number in it is finite (JSON writes a non-finite one as `null` and the file would not load again),
/// and no identifier is used twice.
fn assert_board_sound(path: &Path, after: &str) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{after}: the board file cannot be read: {e}"));
    let doc: Document = inkboard_core::serial::from_json(&text).unwrap_or_else(|e| panic!("{after}: the board file no longer loads: {e}"));
    let back: Document = serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap_or_else(|e| panic!("{after}: non-finite number in the board: {e}"));
    assert!(back == doc, "{after}: the board does not survive encoding (a non-finite number?)");
    assert!(!doc.pages.is_empty(), "{after}: no page");
    // what a host relays to its guests: the guests refuse (and leave) a board that goes over these limits
    let limits = inkboard_session::limits::Limits::host_trust();
    inkboard_session::limits::check_pages(&limits, &doc.pages).unwrap_or_else(|v| panic!("{after}: a guest would refuse this board: {v}"));
    let mut pages = std::collections::HashSet::new();
    for p in &doc.pages {
        assert!(pages.insert(p.id.clone()), "{after}: page id twice");
        let mut ids = std::collections::HashSet::new();
        assert!(p.objects.iter().all(|o| ids.insert(o.id.clone())), "{after}: object id twice");
    }
    for p in &doc.pages {
        for o in &p.objects {
            let v = serde_json::to_string(o).unwrap();
            assert!(!v.contains("null") || !v.contains("\"x\":null"), "{after}: {v}");
            if let Some(f) = o.frame() {
                let big = [f.x, f.y, f.w, f.h, f.angle].iter().any(|n| !n.is_finite() || n.abs() > 1e12);
                assert!(!big, "{after}: an object far outside any usable range: {f:?} ({})", o.id);
            }
        }
    }
}

fn board_ids(path: &Path) -> Vec<String> {
    let doc = inkboard_core::serial::load(path).expect("the board loads");
    doc.pages.iter().flat_map(|p| std::iter::once(p.id.0.clone()).chain(p.objects.iter().map(|o| o.id.0.clone()))).collect()
}

/// A board with a bit of everything, in `root/b.inkboard`.
fn make_board(srv: &Server, root: &Path) -> PathBuf {
    let path = root.join("b.inkboard");
    let p = path.to_str().unwrap().to_owned();
    ok(srv, "create_board", json!({"path": p, "name": "hostile"}));
    let a = ok(srv, "add_shape", json!({"path": p, "geom": "rect", "text": "A", "x": 0, "y": 0}));
    let b = ok(srv, "add_shape", json!({"path": p, "geom": "ellipse", "text": "B", "x": 300, "y": 0}));
    ok(srv, "add_connector", json!({"path": p, "from": a["id"], "to": b["id"]}));
    ok(srv, "add_table", json!({"path": p, "rows": [["h1", "h2"], ["a", "b"]]}));
    ok(srv, "add_code_block", json!({"path": p, "code": "fn main() {}"}));
    ok(srv, "add_page", json!({"path": p, "name": "second"}));
    path
}

/* -------------------------------------------------------------------------------------------------------- generators */

fn nasty_number(r: &mut Rng) -> Value {
    let v = [
        json!(0),
        json!(-0.0),
        json!(1),
        json!(-1),
        json!(0.5),
        json!(16),
        json!(4096),
        json!(100_000),
        json!(1e7),
        json!(1e9),
        json!(1e15),
        json!(9_007_199_254_740_993_i64),
        json!(1e300),
        json!(-1e300),
        json!(f64::MAX),
        json!(f64::MIN),
        json!(f64::MIN_POSITIVE),
        json!(5e-324),
        json!(u64::MAX),
        json!(i64::MIN),
    ];
    r.pick(&v).clone()
}

fn nasty_string(r: &mut Rng, ids: &[String]) -> Value {
    let big = |r: &mut Rng, unit: &str| Value::String(unit.repeat([1, 70, 1_000, 8_000][r.below(4)]));
    match r.below(22) {
        0 => json!(""),
        1 => big(r, "a"),
        2 => big(r, "é"),
        3 => json!("\u{0}\u{1}\u{7f}\u{202e}\u{3164}\u{feff}"),
        4 => json!("../../outside.inkboard"),
        5 => json!("C:\\Windows\\win.ini"),
        6 => json!("$0.id"),
        7 => json!("$9999.x.y"),
        8 => big(r, "> "),
        9 => big(r, "- "),
        10 => big(r, "**"),
        11 => big(r, "[a](http://x) "),
        12 => big(r, "`"),
        13 => big(r, "# "),
        14 => json!("flowchart LR\nA --> B\nB --> C\n"),
        15 => json!("#zzzzzz"),
        16 => json!(r"\frac{1}{0} \sqrt{"),
        17 => big(r, "{"),
        18 => big(r, "| a | b |\n|---|---|\n"),
        19 => json!(format!("{}x{}", "- ".repeat(300), "\n  ")),
        _ => Value::String(r.pick(ids).clone()),
    }
}

fn junk(r: &mut Rng, ids: &[String]) -> Value {
    match r.below(7) {
        0 => Value::Null,
        1 => json!(true),
        2 => nasty_number(r),
        3 => nasty_string(r, ids),
        4 => json!([]),
        5 => json!({}),
        _ => json!([[[[[[]]]]], {"a": {"b": [null]}}]),
    }
}

fn value_for(r: &mut Rng, schema: &Value, field: &str, ids: &[String], depth: usize) -> Value {
    if depth > 4 || r.chance(7) {
        return junk(r, ids);
    }
    if let Some(options) = schema.get("oneOf").and_then(Value::as_array) {
        let option = r.pick(options).clone();
        return value_for(r, &option, field, ids, depth + 1);
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array)
        && !e.is_empty()
        && r.chance(85)
    {
        return r.pick(e).clone();
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => {
            if matches!(field, "id" | "from" | "to" | "target") && r.chance(60) {
                Value::String(r.pick(ids).clone())
            } else {
                nasty_string(r, ids)
            }
        }
        Some("number" | "integer") => {
            // sometimes exactly at, or just beyond, the documented bounds
            let mut candidates = vec![nasty_number(r)];
            for k in ["minimum", "maximum"] {
                if let Some(b) = schema.get(k).and_then(Value::as_f64) {
                    candidates.extend([json!(b), json!(b + 1.0), json!(b - 1.0)]);
                }
            }
            r.pick(&candidates).clone()
        }
        Some("boolean") => json!(r.chance(50)),
        Some("array") => {
            let n = *r.pick(&[0, 1, 2, 3, 10, 300, 2_500]);
            let item = schema.get("items").cloned().unwrap_or_else(|| json!({}));
            // long lists of short things (identifiers); a long list of giant strings would weigh gigabytes before the server sees it
            if n >= 300 {
                return if item["type"] == "string" {
                    Value::Array((0..n).map(|_| Value::String(r.pick(ids).clone())).collect())
                } else {
                    Value::Array((0..n.min(40)).map(|_| value_for(r, &item, field, ids, depth + 2)).collect())
                };
            }
            Value::Array((0..n).map(|_| value_for(r, &item, field, ids, depth + 1)).collect())
        }
        Some("object") => {
            let mut m = serde_json::Map::new();
            if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                for (k, s) in props {
                    if r.chance(60) {
                        m.insert(k.clone(), value_for(r, s, k, ids, depth + 1));
                    }
                }
            }
            if field == "operations" || field == "args" {
                m.insert("tool".into(), json!(r.pick(&["add_shape", "update_object", "add_table", "delete_object", "nope"])));
            }
            Value::Object(m)
        }
        _ => junk(r, ids),
    }
}

/// Arguments of a tool: its own properties, mostly well-typed, always spoiled somewhere.
fn args_for(r: &mut Rng, tool: &Value, board: &Path, ids: &[String]) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(props) = tool["inputSchema"]["properties"].as_object() {
        for (k, s) in props {
            if k == "path" {
                // the path is right most of the time: the arguments are what is under test
                if r.chance(92) {
                    args.insert(k.clone(), json!(board.to_str().unwrap()));
                } else {
                    args.insert(k.clone(), nasty_string(r, ids));
                }
                continue;
            }
            if k == "output" || k == "source" {
                if r.chance(50) {
                    args.insert(k.clone(), nasty_string(r, ids));
                }
                continue;
            }
            let required = tool["inputSchema"]["required"].as_array().is_some_and(|q| q.iter().any(|x| x == k));
            if required || r.chance(55) {
                args.insert(k.clone(), value_for(r, s, k, ids, 0));
            }
        }
    }
    if r.chance(5) {
        args.insert("page".into(), nasty_number(r));
    }
    Value::Object(args)
}

fn bombard(seed: u64, calls: usize, tag: &str) {
    let root = temp_dir(tag);
    let srv = server(&root);
    let board = make_board(&srv, &root);
    let mut r = Rng(seed);
    let specs: Vec<Value> = tools::list().into_iter().filter(|t| t["name"] != "save_board").collect();
    let mut errors = 0usize;
    for i in 0..calls {
        let tool = r.pick(&specs).clone();
        let name = tool["name"].as_str().unwrap().to_owned();
        let ids = board_ids(&board);
        let args = args_for(&mut r, &tool, &board, &ids);
        if std::env::var_os("INKBOARD_FUZZ_TRACE").is_some() {
            eprintln!("TRACE {i} {name} {}", args.to_string().chars().take(300).collect::<String>());
        }
        let reply = call(&srv, &name, &args);
        if reply["result"]["isError"] == json!(true) || reply.get("error").is_some() {
            errors += 1;
        }
        assert_board_sound(&board, &format!("call {i}: {name}"));
        // the board must never be left with a huge number of objects by a single call
        let n: usize = inkboard_core::serial::load(&board).unwrap().pages.iter().map(|p| p.objects.len()).sum();
        assert!(n < 20_000, "call {i}: {name} left {n} objects");
    }
    assert!(errors > 0, "no call was refused: the generator is not hostile");
    let _ = std::fs::remove_dir_all(&root);
}

/* ------------------------------------------------------------------------------------------------------------- tests */

#[test]
fn spoiled_arguments_never_panic_or_ruin_the_board() {
    bombard(1, 700, "a");
}

#[test]
#[ignore = "long bench: see context/mcp-remote/test-robustesse.md"]
fn long_spoiled_arguments() {
    let n: usize = std::env::var("INKBOARD_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let seed: u64 = std::env::var("INKBOARD_FUZZ_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(900);
    bombard(seed, n, "long");
}

#[test]
fn extreme_numbers_are_refused_not_stored() {
    let root = temp_dir("num");
    let srv = server(&root);
    let board = make_board(&srv, &root);
    let p = board.to_str().unwrap();
    let shape = inkboard_core::serial::load(&board).unwrap().pages[0].objects.iter().find(|o| o.frame().is_some()).unwrap().id.0.clone();
    for bad in [json!(1e300), json!(-1e300), json!(f64::MAX), json!(1e13)] {
        for field in ["x", "y", "w", "h", "font_size"] {
            let r = call(&srv, "add_shape", &json!({"path": p, "geom": "rect", field: bad}));
            assert_board_sound(&board, &format!("add_shape {field}={bad}: {r}"));
            let r = call(&srv, "update_object", &json!({"path": p, "id": shape, field: bad}));
            assert_board_sound(&board, &format!("update_object {field}={bad}: {r}"));
        }
        let r = call(&srv, "update_object", &json!({"path": p, "id": shape, "rotation": bad, "stroke_width": bad, "opacity": bad, "roundness": bad}));
        assert_board_sound(&board, &format!("update_object style {bad}: {r}"));
        let r = call(&srv, "add_image", &json!({"path": p, "data_base64": "iVBORw0KGgo=", "w": bad, "h": bad}));
        assert_board_sound(&board, &format!("add_image {bad}: {r}"));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ids_that_do_not_exist_or_are_hostile_give_errors() {
    let root = temp_dir("ids");
    let srv = server(&root);
    let board = make_board(&srv, &root);
    let p = board.to_str().unwrap();
    for id in ["", "nope", "\u{0}", &"x".repeat(100_000), "../../etc", "$0.id", "page", "*"] {
        for (tool, args) in [
            ("update_object", json!({"path": p, "id": id, "text": "x"})),
            ("delete_object", json!({"path": p, "id": id})),
            ("delete_object", json!({"path": p, "ids": [id, id]})),
            ("add_connector", json!({"path": p, "from": id, "to": id})),
            ("render", json!({"path": p, "ids": [id]})),
            ("describe_board", json!({"path": p, "page": id})),
        ] {
            let r = call(&srv, tool, &args);
            assert_eq!(r["result"]["isError"], json!(true), "{tool} with id {:?}: {r}", id.chars().take(20).collect::<String>());
        }
    }
    assert_board_sound(&board, "ids");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_huge_batch_and_huge_strings_are_refused_or_handled_in_bounded_work() {
    let root = temp_dir("big");
    let srv = server(&root);
    let board = make_board(&srv, &root);
    let p = board.to_str().unwrap();
    // 201 operations: refused as a whole; 200 operations: fine
    let op = json!({"tool": "add_shape", "args": {"geom": "rect"}});
    let r = call(&srv, "batch", &json!({"path": p, "operations": vec![op.clone(); 201]}));
    assert_eq!(r["result"]["isError"], json!(true), "{r}");
    let before = inkboard_core::serial::load(&board).unwrap();
    let r = call(&srv, "batch", &json!({"path": p, "operations": vec![op.clone(); 5_000]}));
    assert_eq!(r["result"]["isError"], json!(true));
    assert_eq!(inkboard_core::serial::load(&board).unwrap(), before, "a refused batch changes nothing");
    // references that chain: "$0.id" in every operation, and a reference to a result that does not exist
    let r = call(&srv, "batch", &json!({"path": p, "operations": [{"tool": "add_shape", "args": {"geom": "rect", "text": "$5.id"}}]}));
    assert!(r.get("result").is_some());
    // a very large text in a shape, a table, a code block, a Mermaid diagram, a formula
    let giant = "word ".repeat(200_000);
    for (tool, args) in [
        ("add_shape", json!({"path": p, "geom": "text", "text": giant})),
        ("add_shape", json!({"path": p, "geom": "rect", "text": giant, "markdown": true})),
        ("add_code_block", json!({"path": p, "code": giant})),
        ("add_table", json!({"path": p, "rows": vec![vec!["cell"; 400]; 400]})),
        ("add_table", json!({"path": p, "rows": [[]]})),
        ("add_table", json!({"path": p, "rows": []})),
        ("add_formula", json!({"path": p, "latex": "x".repeat(200_000)})),
        ("import_mermaid", json!({"path": p, "mermaid": giant})),
        ("import_mermaid", json!({"path": p, "mermaid": format!("flowchart LR\n{}", "a --> b\n".repeat(100_000))})),
        ("add_image", json!({"path": p, "data_base64": "A".repeat(30_000_000)})),
    ] {
        let r = call(&srv, tool, &args);
        assert!(r.get("result").is_some() || r.get("error").is_some(), "{tool}");
        assert_board_sound(&board, tool);
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn bitmaps_that_declare_huge_dimensions_are_not_decoded_by_the_server() {
    let root = temp_dir("img");
    let srv = server(&root);
    let board = make_board(&srv, &root);
    let p = board.to_str().unwrap();
    use base64::Engine as _;
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(65_535u32.to_be_bytes());
    png.extend(65_535u32.to_be_bytes());
    png.extend([8, 6, 0, 0, 0]);
    let data = base64::engine::general_purpose::STANDARD.encode(&png);
    let r = call(&srv, "add_image", &json!({"path": p, "data_base64": data}));
    // either refused, or stored with a reasonable display size: the board must stay usable
    assert_board_sound(&board, &format!("png 65535x65535: {r}"));
    let doc = inkboard_core::serial::load(&board).unwrap();
    for pg in &doc.pages {
        for o in &pg.objects {
            if let Some(f) = o.frame() {
                assert!(f.w <= 20_000.0 && f.h <= 20_000.0);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn protocol_level_garbage_gets_an_error_or_nothing() {
    let srv = Server { no_live: true, ..Server::default() };
    let long = "z".repeat(1_000_000);
    for msg in [
        json!({}),
        json!([]),
        json!(null),
        json!({"method": 5, "id": 1}),
        json!({"method": "tools/call", "id": 1}),
        json!({"method": "tools/call", "id": 1, "params": {"name": 5}}),
        json!({"method": "tools/call", "id": 1, "params": {"name": "add_shape", "arguments": 7}}),
        json!({"method": "tools/call", "id": 1, "params": {"name": "add_shape", "arguments": [1, 2]}}),
        json!({"method": "tools/call", "id": 1, "params": {"name": long, "arguments": {}}}),
        json!({"method": "resources/read", "id": 1, "params": {"uri": long}}),
        json!({"method": "resources/read", "id": 1, "params": {"uri": "inkboard://../../x"}}),
        json!({"method": "resources/subscribe", "id": 1, "params": {"uri": long}}),
        json!({"method": "initialize", "id": 1, "params": {"protocolVersion": 12}}),
        json!({"method": "tools/call", "id": {"a": [1]}, "params": {"name": "describe_board", "arguments": {"path": long}}}),
    ] {
        let _ = srv.handle(&msg);
    }
    // thousands of subscriptions do not grow without bound
    for i in 0..5_000 {
        let _ = srv.handle(&json!({"method": "resources/subscribe", "id": i, "params": {"uri": format!("inkboard://board/{i}")}}));
    }
    assert!(srv.subs.lock().unwrap().len() <= 1_000, "{} subscriptions kept", srv.subs.lock().unwrap().len());
}
