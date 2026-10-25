//! The `batch` tool (several operations in one call, references, all-or-nothing), with the real `inkboard-mcp` binary.
//! The undo-step and rollback guarantees of the engine are tested in `inkboard-core` (`Editor::try_transact`); security cases are in `security.rs`.

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Document, Kind, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn setup(tag: &str) -> (Client, PathBuf) {
    let dir = temp_dir(tag);
    let mut c = Client::start(&dir);
    let board = dir.join("b.inkboard");
    c.tool("create_board", json!({"path": board.display().to_string()})).unwrap();
    (c, board)
}

fn batch(c: &mut Client, board: &Path, mut args: Value) -> Result<Value, String> {
    args["path"] = json!(board.display().to_string());
    c.tool("batch", args)
}

fn shape(text: &str) -> Value {
    json!({"tool": "add_shape", "args": {"geom": "rect", "text": text}})
}

fn load(board: &Path) -> Document {
    serial::load(board).unwrap_or_else(|e| panic!("{} unreadable: {e}", board.display()))
}

fn texts(doc: &Document, page: usize) -> Vec<String> {
    doc.pages[page].objects.iter().filter_map(|o| if let Kind::Box(b) = &o.kind { Some(b.label.text.to_plain()) } else { None }).collect()
}

fn alive(c: &mut Client) {
    assert_eq!(c.request("ping", json!({}))["result"], json!({}), "the server no longer answers");
}

#[test]
fn twelve_operations_with_references_build_shapes_arrows_and_labels() {
    let (mut c, board) = setup("batch-12");
    let ops = json!([
        shape("Client"),
        shape("Server"),
        shape("Database"),
        {"tool": "add_connector", "args": {"from": "$0.id", "to": "$1.id", "label": "request"}},
        {"tool": "add_connector", "args": {"from": "$1.id", "to": "$2.id", "label": "query", "route": "elbow"}},
        {"tool": "update_object", "args": {"id": "$0.id", "fill": "#ffd8a8"}},
        {"tool": "update_object", "args": {"id": "$2.id", "text": "Database (primary)"}},
        {"tool": "add_shape", "args": {"geom": "sticky", "text": "Linked to $0.id stays literal"}},
        {"tool": "add_table", "args": {"rows": [["a", "b"], ["1", "2"]]}},
        {"tool": "add_code_block", "args": {"code": "fn main() {}"}},
        {"tool": "describe_board"},
        {"tool": "delete_object", "args": {"id": "$7.id"}}
    ]);
    let r = batch(&mut c, &board, json!({"operations": ops})).unwrap();
    assert_eq!(r["applied"], 12, "{r}");
    let results = r["results"].as_array().unwrap();
    assert_eq!(results.len(), 12);
    let id = |i: usize| results[i]["id"].as_str().unwrap_or_else(|| panic!("no id in result {i}: {}", results[i])).to_owned();
    assert_eq!(results[11]["deleted"], json!(id(7)));

    let doc = load(&board);
    let page = &doc.pages[0];
    assert!(page.objects.iter().all(|o| o.id.0 != id(7)), "the sticky note was deleted by the last operation");
    let texts = texts(&doc, 0);
    assert!(texts.contains(&"Database (primary)".to_owned()), "{texts:?}");
    let mut links: Vec<(String, String)> = page
        .objects
        .iter()
        .filter_map(|o| if let Kind::Connector(cn) = &o.kind { Some(cn) } else { None })
        .map(|cn| {
            let t: Vec<String> = cn.targets().map(|t| t.0.clone()).collect();
            (t[0].clone(), t[1].clone())
        })
        .collect();
    links.sort();
    let mut expected = vec![(id(0), id(1)), (id(1), id(2))];
    expected.sort();
    assert_eq!(links, expected, "the arrows join the shapes named by the references");
    alive(&mut c);
}

#[test]
fn text_with_a_reference_inside_a_longer_string_stays_literal() {
    let (mut c, board) = setup("batch-literal");
    batch(&mut c, &board, json!({"operations": [shape("first"), shape("see $0.id for details")]})).unwrap();
    assert!(texts(&load(&board), 0).contains(&"see $0.id for details".to_owned()));
}

#[test]
fn import_mermaid_result_can_be_reused() {
    let (mut c, board) = setup("batch-mermaid");
    let r = batch(
        &mut c,
        &board,
        json!({"operations": [
            {"tool": "import_mermaid", "args": {"mermaid": "graph TD\n A[Start] --> B[End]"}},
            {"tool": "update_object", "args": {"id": "$0.ids.0", "fill": "#d3f9d8"}},
            {"tool": "add_connector", "args": {"from": "$0.ids.0", "to": "$0.ids.1", "label": "extra"}}
        ]}),
    )
    .unwrap();
    assert_eq!(r["applied"], 3, "{r}");
    let first = r["results"][0]["ids"][0].as_str().unwrap();
    assert_eq!(r["results"][1]["id"], json!(first));
    // an out-of-range index in the list is an error that names the operation
    let before = std::fs::read(&board).unwrap();
    let e = batch(
        &mut c,
        &board,
        json!({"operations": [{"tool": "import_mermaid", "args": {"mermaid": "graph TD\n A --> B"}}, {"tool": "delete_object", "args": {"id": "$0.ids.9"}}]}),
    )
    .unwrap_err();
    assert!(e.contains("operation 1") && e.contains("$0.ids.9"), "{e}");
    assert_eq!(std::fs::read(&board).unwrap(), before);
}

#[test]
fn invalid_references_are_errors_naming_the_operation_and_change_nothing() {
    let (mut c, board) = setup("batch-badref");
    let before = std::fs::read(&board).unwrap();
    for (bad, needle) in
        [("$1.id", "has not run yet"), ("$5.id", "has not run yet"), ("$0.nope", "not found in the result of operation 0"), ("$0.id.x", "not found")]
    {
        let e = batch(&mut c, &board, json!({"operations": [shape("ok"), {"tool": "delete_object", "args": {"id": bad}}]})).unwrap_err();
        assert!(e.contains("operation 1") && e.contains(needle) && e.contains(bad), "{bad}: {e}");
        assert_eq!(std::fs::read(&board).unwrap(), before, "{bad}: the file changed");
    }
    alive(&mut c);
}

#[test]
fn forbidden_tools_are_refused_before_anything_runs() {
    let (mut c, board) = setup("batch-forbidden");
    let before = std::fs::read(&board).unwrap();
    for tool in ["batch", "create_board", "save_board", "board_changes", "export", "render"] {
        let e = batch(&mut c, &board, json!({"operations": [shape("must not appear"), {"tool": tool, "args": {}}]})).unwrap_err();
        assert!(e.contains(&format!("tool {tool} cannot be used inside a batch")), "{tool}: {e}");
        assert!(e.contains("operation 1"), "{e}");
        assert_eq!(std::fs::read(&board).unwrap(), before, "{tool}: the first operation ran");
    }
    // an unknown tool fails at its turn, and what came before is rolled back
    let e = batch(&mut c, &board, json!({"operations": [shape("must not appear"), {"tool": "no_such_tool"}]})).unwrap_err();
    assert!(e.contains("operation 1") && e.contains("unknown tool"), "{e}");
    assert_eq!(std::fs::read(&board).unwrap(), before);
    alive(&mut c);
}

#[test]
fn malformed_requests_are_refused() {
    let (mut c, board) = setup("batch-malformed");
    let before = std::fs::read(&board).unwrap();
    let too_many: Vec<Value> = (0..201).map(|i| shape(&format!("s{i}"))).collect();
    let cases = [
        (json!({}), "operations"),
        (json!({"operations": []}), "1 to 200"),
        (json!({"operations": too_many}), "1 to 200"),
        (json!({"operations": "add_shape"}), "operations"),
        (json!({"operations": [42]}), "operation 0"),
        (json!({"operations": [{"args": {}}]}), "missing \"tool\""),
        (json!({"operations": [{"tool": "add_shape", "args": "x"}]}), "\"args\" must be an object"),
        (json!({"operations": [{"tool": "add_shape", "extra": 1}]}), "unknown key"),
        (json!({"operations": [shape("x")], "atomic": "yes"}), "atomic"),
        (json!({"operations": [shape("x")], "page": 9}), "page not found"),
    ];
    for (args, needle) in cases {
        let e = batch(&mut c, &board, args.clone()).unwrap_err();
        assert!(e.contains(needle), "{needle}: {e}");
        assert_eq!(std::fs::read(&board).unwrap(), before, "{needle}: the file changed");
    }
    alive(&mut c);
}

#[test]
fn the_client_prefix_of_a_tool_name_is_accepted_and_the_rules_still_apply() {
    let (mut c, board) = setup("batch-prefix");
    let r = batch(&mut c, &board, json!({"operations": [{"tool": "inkboard_add_shape", "args": {"geom": "rect", "text": "prefixed"}}]})).unwrap();
    assert_eq!(r["applied"], 1);
    assert_eq!(texts(&load(&board), 0), ["prefixed"]);
    let e = batch(&mut c, &board, json!({"operations": [{"tool": "inkboard_export", "args": {}}]})).unwrap_err();
    assert!(e.contains("cannot be used inside a batch"), "{e}");
    let e = batch(&mut c, &board, json!({"operations": [{"tool": "inkboard_nope", "args": {}}]})).unwrap_err();
    assert!(e.contains("unknown tool"), "{e}");
}

#[test]
fn a_page_number_written_as_text_selects_the_page() {
    let (mut c, board) = setup("batch-page-text");
    c.tool("add_page", json!({"path": board.display().to_string(), "name": "second"})).unwrap();
    batch(&mut c, &board, json!({"page": "2", "operations": [shape("on page two")]})).unwrap();
    let doc = load(&board);
    assert_eq!(texts(&doc, 1), ["on page two"]);
    assert_eq!(texts(&doc, 0).len(), 0, "nothing on page 1");
}

#[test]
fn two_hundred_operations_are_accepted() {
    let (mut c, board) = setup("batch-200");
    let ops: Vec<Value> = (0..200).map(|i| json!({"tool": "add_shape", "args": {"geom": "rect", "text": format!("s{i}")}})).collect();
    let r = batch(&mut c, &board, json!({"operations": ops})).unwrap();
    assert_eq!(r["applied"], 200);
    assert_eq!(load(&board).pages[0].objects.len(), 200);
    alive(&mut c);
}

#[test]
fn atomic_failure_leaves_the_file_identical_byte_for_byte() {
    let (mut c, board) = setup("batch-atomic");
    c.tool("add_shape", json!({"path": board.display().to_string(), "geom": "ellipse", "text": "already there"})).unwrap();
    let before = std::fs::read(&board).unwrap();
    // 8 operations; the sixth one (index 5) fails
    let mut ops: Vec<Value> = (0..5).map(|i| shape(&format!("s{i}"))).collect();
    ops.push(json!({"tool": "update_object", "args": {"id": "does-not-exist", "text": "x"}}));
    ops.extend((6..8).map(|i| shape(&format!("s{i}"))));
    let e = batch(&mut c, &board, json!({"operations": ops})).unwrap_err();
    assert!(e.contains("operation 5") && e.contains("update_object") && e.contains("object not found"), "{e}");
    assert!(e.contains("rolled back"), "{e}");
    assert!(e.contains("\"id\""), "the results obtained before the rollback are quoted: {e}");
    assert_eq!(std::fs::read(&board).unwrap(), before);
    // `atomic: true` spelled out behaves the same
    let e = batch(&mut c, &board, json!({"atomic": true, "operations": [shape("a"), {"tool": "delete_object", "args": {"id": "nope"}}]})).unwrap_err();
    assert!(e.contains("operation 1"), "{e}");
    assert_eq!(std::fs::read(&board).unwrap(), before);
    alive(&mut c);
}

#[test]
fn atomic_failure_also_undoes_page_operations() {
    let (mut c, board) = setup("batch-atomic-pages");
    let before = std::fs::read(&board).unwrap();
    let e = batch(
        &mut c,
        &board,
        json!({"operations": [{"tool": "add_page", "args": {"name": "Extra"}}, shape("on the first page"), {"tool": "delete_object", "args": {"id": "nope"}}]}),
    )
    .unwrap_err();
    assert!(e.contains("operation 2"), "{e}");
    assert_eq!(std::fs::read(&board).unwrap(), before);
    assert_eq!(load(&board).pages.len(), 1);
}

#[test]
fn non_atomic_keeps_what_succeeded_and_reports_the_failure() {
    let (mut c, board) = setup("batch-nonatomic");
    let mut ops: Vec<Value> = (0..4).map(|i| shape(&format!("kept{i}"))).collect();
    ops.push(json!({"tool": "update_object", "args": {"id": "does-not-exist", "text": "x"}}));
    ops.push(shape("never run"));
    let r = batch(&mut c, &board, json!({"atomic": false, "operations": ops})).unwrap();
    assert_eq!(r["applied"], 4, "{r}");
    assert_eq!(r["failed"]["index"], 4, "{r}");
    assert!(r["failed"]["error"].as_str().unwrap().contains("object not found"), "{r}");
    assert_eq!(r["results"].as_array().unwrap().len(), 4);
    assert_eq!(texts(&load(&board), 0), ["kept0", "kept1", "kept2", "kept3"]);
    alive(&mut c);
}

#[test]
fn page_is_inherited_and_can_be_overridden_per_operation() {
    let (mut c, board) = setup("batch-pages");
    c.tool("add_page", json!({"path": board.display().to_string(), "name": "Second"})).unwrap();
    // batch-level page 2; one operation goes to page 1; the next one is back on page 2
    let ops = json!([shape("two-a"), {"tool": "add_shape", "args": {"geom": "rect", "text": "one", "page": 1}}, shape("two-b")]);
    batch(&mut c, &board, json!({"page": 2, "operations": ops})).unwrap();
    let doc = load(&board);
    assert_eq!(texts(&doc, 0), ["one"]);
    assert_eq!(texts(&doc, 1), ["two-a", "two-b"]);
    // without a batch-level page: the first page, even after an operation that named another one
    let ops = json!([{"tool": "add_shape", "args": {"geom": "rect", "text": "named-second", "page": "Second"}}, shape("default")]);
    batch(&mut c, &board, json!({"operations": ops})).unwrap();
    let doc = load(&board);
    assert_eq!(texts(&doc, 0), ["one", "default"]);
    assert_eq!(texts(&doc, 1), ["two-a", "two-b", "named-second"]);
    // a page that does not exist is an error naming the operation
    let before = std::fs::read(&board).unwrap();
    let e = batch(&mut c, &board, json!({"operations": [shape("x"), {"tool": "add_shape", "args": {"geom": "rect", "page": 7}}]})).unwrap_err();
    assert!(e.contains("operation 1") && e.contains("page not found"), "{e}");
    assert_eq!(std::fs::read(&board).unwrap(), before);
}

#[test]
fn board_changes_does_not_report_what_a_batch_did() {
    let (mut c, board) = setup("batch-changes");
    let p = board.display().to_string();
    assert_eq!(c.tool("board_changes", json!({"path": p})).unwrap()["baseline"], true);
    let ops = json!([shape("a"), shape("b"), {"tool": "add_connector", "args": {"from": "$0.id", "to": "$1.id"}}, {"tool": "update_object", "args": {"id": "$0.id", "text": "a2"}}]);
    batch(&mut c, &board, json!({"operations": ops})).unwrap();
    let none = c.tool("board_changes", json!({"path": p})).unwrap();
    assert_eq!(none["unchanged"], true, "the agent's batch is not a user change: {none}");
    // a failed atomic batch leaves nothing to report either
    let _ = batch(&mut c, &board, json!({"operations": [shape("c"), {"tool": "delete_object", "args": {"id": "nope"}}]})).unwrap_err();
    assert_eq!(c.tool("board_changes", json!({"path": p})).unwrap()["unchanged"], true);
}

/// A control measurement only: the elapsed time is printed, never asserted (timing assertions are flaky on CI).
#[test]
fn a_big_batch_of_additions_runs_in_one_call() {
    let (mut c, board) = setup("batch-big");
    let ops: Vec<Value> = (0..200)
        .map(|i| json!({"tool": "add_shape", "args": {"geom": "rect", "text": format!("shape {i}"), "x": (i % 20) * 120, "y": (i / 20) * 80}}))
        .collect();
    let start = std::time::Instant::now();
    let r = batch(&mut c, &board, json!({"operations": ops})).unwrap();
    eprintln!("batch of 200 additions: {:?}", start.elapsed());
    assert_eq!(r["applied"], 200);
    assert_eq!(load(&board).pages[0].objects.len(), 200);
}
