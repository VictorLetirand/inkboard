//! Rich text (`markdown`, `format_text`, faithful reading), with the real `inkboard-mcp` binary.
//! The parser and the Markdown writer have their own round-trip tests in `src/tools/richtext.rs`.

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Document, Kind, Object, RichText, SpanStyle, serial};
use serde_json::{Value, json};
use std::fmt::Write;
use std::path::{Path, PathBuf};

const RED: &str = "#e03131";

fn setup(tag: &str) -> (Client, PathBuf) {
    let dir = temp_dir(tag);
    let mut c = Client::start(&dir);
    let board = dir.join("b.inkboard");
    c.tool("create_board", json!({"path": board.display().to_string()})).unwrap();
    (c, board)
}

fn call(c: &mut Client, board: &Path, tool: &str, mut args: Value) -> Result<Value, String> {
    args["path"] = json!(board.display().to_string());
    c.tool(tool, args)
}

fn ok(c: &mut Client, board: &Path, tool: &str, args: Value) -> Value {
    call(c, board, tool, args).unwrap_or_else(|e| panic!("{tool}: {e}"))
}

fn load(board: &Path) -> Document {
    serial::load(board).unwrap_or_else(|e| panic!("{} unreadable: {e}", board.display()))
}

fn object(board: &Path, id: &str) -> Object {
    load(board).pages[0].objects.iter().find(|o| o.id.0 == id).cloned().unwrap_or_else(|| panic!("object {id} not in the file"))
}

fn id_of(r: &Value) -> String {
    r["id"].as_str().unwrap_or_else(|| panic!("no id in {r}")).to_owned()
}

fn box_text(board: &Path, id: &str) -> RichText {
    match object(board, id).kind {
        Kind::Box(b) => b.label.text,
        k => panic!("not a box: {k:?}"),
    }
}

fn label_text(board: &Path, id: &str) -> RichText {
    match object(board, id).kind {
        Kind::Connector(c) => c.label.expect("label").text,
        k => panic!("not a connector: {k:?}"),
    }
}

fn cells(board: &Path, id: &str) -> Vec<RichText> {
    match object(board, id).kind {
        Kind::Table(t) => t.cells,
        k => panic!("not a table: {k:?}"),
    }
}

/// Compact flags of a style: b i u s, c=color, h=highlight.
fn flags(st: &SpanStyle) -> String {
    let mut f = String::new();
    for (on, c) in [(st.bold, 'b'), (st.italic, 'i'), (st.underline, 'u'), (st.strike, 's')] {
        if on {
            f.push(c);
        }
    }
    if let Some(c) = st.color {
        let _ = write!(f, "c={}", c.to_hex());
    }
    if let Some(c) = st.highlight {
        let _ = write!(f, "h={}", c.to_hex());
    }
    f
}

fn spans(rt: &RichText) -> Vec<(String, String)> {
    rt.0.iter().map(|s| (s.text.clone(), flags(&s.style))).collect()
}

fn sp(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(t, f)| ((*t).to_owned(), (*f).to_owned())).collect()
}

/// An invalid call must fail, leave the file untouched, and the server must answer the next call.
fn invalid(c: &mut Client, board: &Path, tool: &str, args: Value) -> String {
    let before = std::fs::read(board).unwrap();
    let mut args = args;
    args["path"] = json!(board.display().to_string());
    let r = c.request("tools/call", json!({"name": tool, "arguments": args}));
    assert!(r["error"].is_object() || r["result"]["isError"] == json!(true), "{tool} with {args}: error expected, got {r}");
    assert_eq!(std::fs::read(board).unwrap(), before, "{tool}: the file changed despite the error");
    assert_eq!(c.request("ping", json!({}))["result"], json!({}), "{tool}: the server no longer answers");
    r["result"]["content"][0]["text"].as_str().or(r["error"]["message"].as_str()).unwrap_or_default().to_owned()
}

fn describe(c: &mut Client, board: &Path, id: &str) -> Value {
    let d = ok(c, board, "describe_board", json!({}));
    d["page"]["objects"].as_array().unwrap().iter().find(|o| o["id"] == json!(id)).cloned().unwrap_or_else(|| panic!("{id} not described in {d}"))
}

fn shape(c: &mut Client, board: &Path, text: &str, markdown: bool) -> String {
    id_of(&ok(c, board, "add_shape", json!({"geom": "rect", "text": text, "markdown": markdown, "x": 0, "y": 0})))
}

/* ------------------------------------------------------------ Markdown in */

#[test]
fn markdown_gives_the_right_spans() {
    let (mut c, board) = setup("rt-spans");
    let md = "a **b** *i* _j_ __u__ ~~s~~ {color=#e03131}r{/} {bg=#ffec99}h{/} {color=ink bg=#ffec99}k{/}";
    let r = ok(&mut c, &board, "add_shape", json!({"geom": "rect", "text": md, "markdown": true}));
    assert!(r.get("warnings").is_none(), "{r}");
    let want = sp(&[
        ("a ", ""),
        ("b", "b"),
        (" ", ""),
        ("i", "i"),
        (" ", ""),
        ("j", "i"),
        (" ", ""),
        ("u", "u"),
        (" ", ""),
        ("s", "s"),
        (" ", ""),
        ("r", "c=#e03131"),
        (" ", ""),
        ("h", "h=#ffec99"),
        (" ", ""),
        ("k", "c=inkh=#ffec99"),
    ]);
    assert_eq!(spans(&box_text(&board, &id_of(&r))), want);
}

#[test]
fn nested_and_overlapping_markup() {
    let (mut c, board) = setup("rt-nest");
    let id = shape(&mut c, &board, "***x*** {color=#e03131}a **b** {bg=#ffec99}c{/} d{/} **e *f** g*", true);
    let want = sp(&[
        ("x", "bi"),
        (" ", ""),
        ("a ", "c=#e03131"),
        ("b", "bc=#e03131"),
        (" ", "c=#e03131"),
        ("c", "c=#e03131h=#ffec99"),
        (" d", "c=#e03131"),
        (" ", ""),
        ("e ", "b"),
        ("f", "bi"),
        (" g", "i"),
    ]);
    assert_eq!(spans(&box_text(&board, &id)), want);
}

#[test]
fn escapes_give_literal_characters() {
    let (mut c, board) = setup("rt-esc");
    let r = ok(&mut c, &board, "add_shape", json!({"geom": "rect", "text": r"\*x\* \_y\_ \~z\~ \{color=#e03131\} \\ 2 \* 3", "markdown": true}));
    assert!(r.get("warnings").is_none(), "{r}");
    assert_eq!(spans(&box_text(&board, &id_of(&r))), sp(&[(r"*x* _y_ ~z~ {color=#e03131} \ 2 * 3", "")]));
}

#[test]
fn invalid_markup_is_kept_as_text_and_reported() {
    let (mut c, board) = setup("rt-warn");
    for (src, shown) in
        [("**x", "**x"), ("a {color=#e03131}x", "a {color=#e03131}x"), ("{color=red}x{/}", "{color=red}x{/}"), ("x {/} y", "x {/} y"), ("**{/}**", "**{/}**")]
    {
        let r = ok(&mut c, &board, "add_shape", json!({"geom": "rect", "text": src, "markdown": true}));
        let w = r["warnings"].as_array().unwrap_or_else(|| panic!("{src}: no warnings in {r}"));
        assert!(!w.is_empty() && w.iter().all(|m| m.as_str().is_some_and(|m| m.contains("literal"))), "{src}: {w:?}");
        assert_eq!(spans(&box_text(&board, &id_of(&r))), sp(&[(shown, "")]), "{src}");
    }
    // valid and invalid markup together: the valid part applies
    let r = ok(&mut c, &board, "add_shape", json!({"geom": "rect", "text": "**ok** and *open", "markdown": true}));
    assert_eq!(r["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(spans(&box_text(&board, &id_of(&r))), sp(&[("ok", "b"), (" and *open", "")]));
}

#[test]
fn without_markdown_the_text_is_unchanged() {
    let (mut c, board) = setup("rt-plain");
    let text = r"**x** *y* __u__ ~~s~~ {color=#e03131}r{/} \* \\";
    for args in [json!({"geom": "rect", "text": text}), json!({"geom": "rect", "text": text, "markdown": false})] {
        let r = ok(&mut c, &board, "add_shape", args);
        assert!(r.get("warnings").is_none(), "{r}");
        assert_eq!(spans(&box_text(&board, &id_of(&r))), sp(&[(text, "")]));
    }
    // same for the other tools
    let a = shape(&mut c, &board, "a", false);
    let b = shape(&mut c, &board, "b", false);
    let con = id_of(&ok(&mut c, &board, "add_connector", json!({"from": a, "to": b, "label": "**x**"})));
    assert_eq!(spans(&label_text(&board, &con)), sp(&[("**x**", "")]));
    ok(&mut c, &board, "update_object", json!({"id": a, "text": "**y**"}));
    assert_eq!(spans(&box_text(&board, &a)), sp(&[("**y**", "")]));
    let t = id_of(&ok(&mut c, &board, "add_table", json!({"rows": [["**h**", "_i_"]]})));
    assert_eq!(spans(&cells(&board, &t)[0]), sp(&[("**h**", "")]));
    // a non-boolean flag is an error
    invalid(&mut c, &board, "add_shape", json!({"geom": "rect", "text": "x", "markdown": "yes"}));
}

#[test]
fn describe_board_is_unchanged_for_plain_text() {
    let (mut client, board) = setup("rt-describe");
    let a = id_of(&ok(&mut client, &board, "add_shape", json!({"geom": "rect", "text": "snake_case * 2", "x": 10, "y": 20, "w": 180, "h": 90})));
    let b = shape(&mut client, &board, "B", false);
    let con = id_of(&ok(&mut client, &board, "add_connector", json!({"from": a, "to": b, "label": "yes"})));
    let t = id_of(&ok(&mut client, &board, "add_table", json!({"rows": [["h1", "h2"], ["a", "b"]]})));
    let d = ok(&mut client, &board, "describe_board", json!({}));
    let objects = d["page"]["objects"].as_array().unwrap();
    let find = |id: &str| objects.iter().find(|o| o["id"] == json!(id)).unwrap();
    let keys = |o: &Value| {
        let mut names: Vec<&str> = o.as_object().unwrap().keys().map(String::as_str).collect();
        names.sort_unstable();
        names.join(",")
    };
    assert_eq!(*find(&a), json!({"id": a, "type": "rect", "x": 10.0, "y": 20.0, "w": 180.0, "h": 90.0, "text": "snake_case * 2"}));
    assert_eq!(keys(find(&b)), "h,id,text,type,w,x,y");
    assert_eq!(keys(find(&con)), "from,id,label,to,type");
    assert_eq!(find(&con)["label"], json!("yes"));
    assert_eq!(keys(find(&t)), "cols,h,id,rows,type,w,x,y");
    let out = d.to_string();
    assert!(!out.contains("markdown"), "{out}");
    // the other tools' answers keep their shape too
    let updated = ok(&mut client, &board, "update_object", json!({"id": a, "text": "new"}));
    assert_eq!(updated, json!({"id": a, "avertissements": []}));
    let reply = ok(&mut client, &board, "add_shape", json!({"geom": "rect", "text": "x"}));
    assert_eq!(keys(&reply), "h,id,w,x,y");
}

/* ----------------------------------------------------- tables and connectors */

#[test]
fn table_cells_can_be_formatted() {
    let (mut client, board) = setup("rt-table");
    let rows = json!([["**Name**", "{color=#e03131}Qty{/}"], ["plain", "*x* and **open"]]);
    let r = ok(&mut client, &board, "add_table", json!({"rows": rows, "markdown": true}));
    let w = r["warnings"].as_array().unwrap();
    assert_eq!(w.len(), 1, "{r}");
    assert!(w[0].as_str().unwrap().starts_with("cell [1, 1]:"), "{w:?}");
    let t = cells(&board, &id_of(&r));
    assert_eq!(spans(&t[0]), sp(&[("Name", "b")]));
    assert_eq!(spans(&t[1]), sp(&[("Qty", "c=#e03131")]));
    assert_eq!(spans(&t[2]), sp(&[("plain", "")]));
    assert_eq!(spans(&t[3]), sp(&[("x", "i"), (" and **open", "")]));
    let d = describe(&mut client, &board, &id_of(&r));
    assert_eq!(d["cells_markdown"], json!([["**Name**", "{color=#e03131}Qty{/}"], ["plain", "*x* and \\*\\*open"]]));
}

#[test]
fn the_columns_follow_the_text_not_the_markup() {
    let (mut c, board) = setup("rt-colw");
    let mut w = |md: bool, text: &str| {
        let r = ok(&mut c, &board, "add_table", json!({"rows": [[text]], "markdown": md}));
        describe(&mut c, &board, &id_of(&r))["w"].as_f64().unwrap()
    };
    let long = "abcdefghijklmnopqrstuvwxyz";
    assert_eq!(w(true, &format!("{{color=#e03131}}**{long}**{{/}}")), w(false, long));
}

#[test]
fn connector_labels_can_be_formatted() {
    let (mut client, board) = setup("rt-label");
    let (a, b) = (shape(&mut client, &board, "A", false), shape(&mut client, &board, "B", false));
    let r = ok(&mut client, &board, "add_connector", json!({"from": a, "to": b, "label": "{color=#e03131}**no**{/}", "markdown": true}));
    assert!(r.get("warnings").is_none());
    let id = id_of(&r);
    assert_eq!(spans(&label_text(&board, &id)), sp(&[("no", "bc=#e03131")]));
    let d = describe(&mut client, &board, &id);
    assert_eq!((d["label"].as_str(), d["markdown"].as_str()), (Some("no"), Some("{color=#e03131}**no**{/}")));
    // update_object
    ok(&mut client, &board, "update_object", json!({"id": id, "text": "*maybe* **", "markdown": true}));
    assert_eq!(spans(&label_text(&board, &id)), sp(&[("maybe", "i"), (" **", "")]));
    let r = ok(&mut client, &board, "update_object", json!({"id": id, "text": "x", "markdown": true}));
    assert_eq!(r, json!({"id": id, "avertissements": []}));
    // an empty text removes the label
    ok(&mut client, &board, "update_object", json!({"id": id, "text": "", "markdown": true}));
    assert!(describe(&mut client, &board, &id).get("label").is_none());
}

#[test]
fn update_object_text_can_be_formatted() {
    let (mut c, board) = setup("rt-update");
    let id = shape(&mut c, &board, "old", false);
    let r = ok(&mut c, &board, "update_object", json!({"id": id, "text": "new **bold** {bg=#ffec99}x{/} *open", "markdown": true}));
    assert_eq!(r["warnings"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(spans(&box_text(&board, &id)), sp(&[("new ", ""), ("bold", "b"), (" ", ""), ("x", "h=#ffec99"), (" *open", "")]));
    // text is replaced as a whole, plain again without the flag
    ok(&mut c, &board, "update_object", json!({"id": id, "text": "**plain**"}));
    assert_eq!(spans(&box_text(&board, &id)), sp(&[("**plain**", "")]));
    // a table has no text to set: the existing warning, nothing new
    let t = id_of(&ok(&mut c, &board, "add_table", json!({"rows": [["a"]]})));
    let r = ok(&mut c, &board, "update_object", json!({"id": t, "text": "**x**", "markdown": true}));
    assert_eq!(r, json!({"id": t, "avertissements": ["text can only be changed on shapes and connectors"]}));
}

/* --------------------------------------------------------------- reading */

#[test]
fn describe_board_reports_the_markdown_of_formatted_text_and_it_reads_back() {
    let (mut c, board) = setup("rt-read");
    let src = "plain **bold** {color=#e03131 bg=ink}*both*{/} 2 \\* 3 \\_ end";
    let id = shape(&mut c, &board, src, true);
    let d = describe(&mut c, &board, &id);
    assert_eq!(d["text"], json!("plain bold both 2 * 3 _ end"));
    let md = d["markdown"].as_str().unwrap_or_else(|| panic!("no markdown in {d}"));
    // what is read can be written back: same spans
    let again = shape(&mut c, &board, md, true);
    assert_eq!(spans(&box_text(&board, &again)), spans(&box_text(&board, &id)));
    assert_eq!(describe(&mut c, &board, &again)["markdown"], d["markdown"]);
}

#[test]
fn bold_counts_in_the_width() {
    let (mut c, board) = setup("rt-bold");
    let w = |c: &mut Client, text: &str| ok(c, &board, "add_shape", json!({"geom": "text", "text": text, "markdown": true}))["w"].as_f64().unwrap();
    assert!(w(&mut c, "**WWWWWWWWWW**") > w(&mut c, "WWWWWWWWWW"));
    // and `format_text` refits the shape
    let id = id_of(&ok(&mut c, &board, "add_shape", json!({"geom": "text", "text": "WWWWWWWWWW"})));
    let before = describe(&mut c, &board, &id)["w"].as_f64().unwrap();
    ok(&mut c, &board, "format_text", json!({"id": id, "bold": true}));
    assert!(describe(&mut c, &board, &id)["w"].as_f64().unwrap() > before);
}

#[test]
fn a_formatted_board_exports_with_its_colors() {
    let (mut c, board) = setup("rt-export");
    shape(&mut c, &board, "{color=#12ab34}green{/} {bg=#ab12cd}marked{/} **bold** ~~gone~~ __under__ *it*", true);
    let dir = board.parent().unwrap();
    let svg = dir.join("out.svg");
    ok(&mut c, &board, "export", json!({"output": svg.display().to_string()}));
    let text = std::fs::read_to_string(&svg).unwrap().to_ascii_lowercase();
    assert!(text.contains("#12ab34"), "the text color is not in the SVG");
    assert!(text.contains("#ab12cd"), "the highlight is not in the SVG");
    let png = dir.join("out.png");
    ok(&mut c, &board, "export", json!({"output": png.display().to_string()}));
    assert!(std::fs::read(&png).unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
}

#[test]
fn a_very_large_markdown_text_is_accepted() {
    let (mut c, board) = setup("rt-big");
    let src = "**a** ".repeat(20_000);
    let id = shape(&mut c, &board, &src, true);
    let t = box_text(&board, &id);
    assert_eq!(t.len_chars(), 40_000); // "a " × 20 000
    assert!(t.0.len() >= 20_000);
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}

/* ------------------------------------------------------------ format_text */

#[test]
fn format_text_by_match_all_range_and_whole_text() {
    let (mut c, board) = setup("rt-ft");
    let id = shape(&mut c, &board, "alpha beta alpha", false);
    let r = ok(&mut c, &board, "format_text", json!({"id": id, "match": "alpha", "bold": true}));
    assert_eq!(r, json!({"id": id, "changed": 5}));
    assert_eq!(spans(&box_text(&board, &id)), sp(&[("alpha", "b"), (" beta alpha", "")]));
    // already bold: nothing changes
    assert_eq!(ok(&mut c, &board, "format_text", json!({"id": id, "match": "alpha", "bold": true}))["changed"], json!(0));
    let r = ok(&mut c, &board, "format_text", json!({"id": id, "match": "alpha", "all": true, "color": RED, "italic": true}));
    assert_eq!(r["changed"], json!(10), "both occurrences gain color and italic");
    assert_eq!(spans(&box_text(&board, &id)), sp(&[("alpha", "bic=#e03131"), (" beta ", ""), ("alpha", "ic=#e03131")]));
    // range, end excluded (characters, not bytes)
    let u = shape(&mut c, &board, "é🙂 abc", false);
    ok(&mut c, &board, "format_text", json!({"id": u, "range": [1, 3], "underline": true, "strike": true}));
    assert_eq!(spans(&box_text(&board, &u)), sp(&[("é", ""), ("🙂 ", "us"), ("abc", "")]));
    // no selector: the whole text
    let w = shape(&mut c, &board, "all of it", false);
    assert_eq!(ok(&mut c, &board, "format_text", json!({"id": w, "highlight": "#ffec99"}))["changed"], json!(9));
    assert_eq!(spans(&box_text(&board, &w)), sp(&[("all of it", "h=#ffec99")]));
}

#[test]
fn format_text_removes_and_clears() {
    let (mut c, board) = setup("rt-ft-rm");
    let id = shape(&mut c, &board, "**a** {color=#e03131}b{/} *c* {bg=ink}**d**{/}", true);
    let all = |a: Value| {
        let mut a = a;
        a["id"] = json!(id);
        a
    };
    ok(&mut c, &board, "format_text", all(json!({"match": "a", "bold": false})));
    ok(&mut c, &board, "format_text", all(json!({"match": "b", "color": null})));
    ok(&mut c, &board, "format_text", all(json!({"match": "d", "highlight": null})));
    assert_eq!(spans(&box_text(&board, &id)), sp(&[("a b ", ""), ("c", "i"), (" ", ""), ("d", "b")]));
    // clear on a part, then everything
    ok(&mut c, &board, "format_text", all(json!({"range": [4, 7], "clear": true})));
    assert_eq!(spans(&box_text(&board, &id)), sp(&[("a b c d", "")]));
    ok(&mut c, &board, "format_text", all(json!({"match": "c", "bold": true})));
    ok(&mut c, &board, "format_text", all(json!({"clear": true})));
    assert_eq!(spans(&box_text(&board, &id)), sp(&[("a b c d", "")]));
    // clear then set in one call
    ok(&mut c, &board, "format_text", all(json!({"match": "a", "bold": true})));
    ok(&mut c, &board, "format_text", all(json!({"clear": true, "italic": true})));
    assert_eq!(spans(&box_text(&board, &id)), sp(&[("a b c d", "i")]));
}

#[test]
fn format_text_on_table_cells_and_connector_labels() {
    let (mut client, board) = setup("rt-ft-cell");
    let t = id_of(&ok(&mut client, &board, "add_table", json!({"rows": [["x", "y z"], ["z", "w"]]})));
    let r = ok(&mut client, &board, "format_text", json!({"id": t, "cell": [0, 1], "match": "z", "bold": true}));
    assert_eq!(r["changed"], json!(1));
    ok(&mut client, &board, "format_text", json!({"id": t, "cell": [1, 0], "color": RED}));
    let cells = cells(&board, &t);
    assert_eq!(spans(&cells[1]), sp(&[("y ", ""), ("z", "b")]));
    assert_eq!(spans(&cells[2]), sp(&[("z", "c=#e03131")]));
    assert_eq!(spans(&cells[0]), sp(&[("x", "")]));
    assert_eq!(spans(&cells[3]), sp(&[("w", "")]));
    let described = describe(&mut client, &board, &t);
    assert_eq!(described["cells_markdown"], json!([["x", "y **z**"], ["{color=#e03131}z{/}", "w"]]));
    let (a, b) = (shape(&mut client, &board, "A", false), shape(&mut client, &board, "B", false));
    let con = id_of(&ok(&mut client, &board, "add_connector", json!({"from": a, "to": b, "label": "go"})));
    ok(&mut client, &board, "format_text", json!({"id": con, "italic": true}));
    assert_eq!(spans(&label_text(&board, &con)), sp(&[("go", "i")]));
}

#[test]
fn format_text_errors_leave_the_file_untouched() {
    let (mut client, board) = setup("rt-ft-err");
    let id = shape(&mut client, &board, "alpha beta", false);
    let (a, b) = (shape(&mut client, &board, "A", false), shape(&mut client, &board, "B", false));
    let bare = id_of(&ok(&mut client, &board, "add_connector", json!({"from": a, "to": b})));
    let t = id_of(&ok(&mut client, &board, "add_table", json!({"rows": [["x", "y"]]})));
    let empty = shape(&mut client, &board, "", false);
    let e = invalid(&mut client, &board, "format_text", json!({"id": id, "match": "gamma", "bold": true}));
    assert!(e.contains("text not found") && e.contains("alpha beta"), "{e}");
    for args in [
        json!({"id": id, "bold": true, "match": "alpha", "range": [0, 2]}),
        json!({"id": id, "bold": true, "all": true}),
        json!({"id": id, "bold": true, "match": ""}),
        json!({"id": id, "bold": true, "range": [3, 3]}),
        json!({"id": id, "bold": true, "range": [3, 11]}),
        json!({"id": id, "bold": true, "range": [-1, 3]}),
        json!({"id": id, "bold": true, "range": [0]}),
        json!({"id": id, "bold": "yes"}),
        json!({"id": id, "color": "#zzzzzz"}),
        json!({"id": id, "color": 5}),
        json!({"id": id}),
        json!({"id": id, "clear": false}),
        json!({"id": id, "bold": true, "cell": [0, 0]}),
        json!({"id": "nope", "bold": true}),
        json!({"id": bare, "bold": true}),
        json!({"id": t, "bold": true}),
        json!({"id": t, "bold": true, "cell": [1, 0]}),
        json!({"id": t, "bold": true, "cell": [0, 9]}),
        json!({"id": empty, "bold": true}),
        json!({"bold": true}),
    ] {
        invalid(&mut client, &board, "format_text", args);
    }
}

#[test]
fn format_text_cannot_act_on_an_image() {
    let (mut c, board) = setup("rt-ft-img");
    // 1×1 PNG
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
    let id = id_of(&ok(&mut c, &board, "add_image", json!({"data_base64": png})));
    let e = invalid(&mut c, &board, "format_text", json!({"id": id, "bold": true}));
    assert!(e.contains("image"), "{e}");
}

#[test]
fn format_text_is_listed_with_its_schema_and_markdown_is_documented() {
    let dir = temp_dir("rt-list");
    let mut c = Client::start(&dir);
    let list = c.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let tool = |n: &str| tools.iter().find(|t| t["name"] == json!(n)).unwrap_or_else(|| panic!("{n} not listed"));
    let ft = tool("format_text");
    assert!(ft["description"].as_str().unwrap().len() > 100);
    assert_eq!(ft["inputSchema"]["type"], json!("object"));
    assert_eq!(ft["inputSchema"]["required"], json!(["id"]));
    for p in ["match", "all", "range", "cell", "bold", "italic", "underline", "strike", "color", "highlight", "clear"] {
        assert!(ft["inputSchema"]["properties"][p].is_object(), "format_text.{p}");
    }
    for n in ["add_shape", "add_table", "add_connector", "update_object"] {
        let m = &tool(n)["inputSchema"]["properties"]["markdown"];
        assert_eq!(m["type"], json!("boolean"), "{n}");
        assert!(m["description"].as_str().unwrap().contains("Default false"), "{n}");
    }
}
