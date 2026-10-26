//! Style reading and editing tools (`describe_board` detail, `find_objects`, `update_object`, `move_objects`, `arrange_objects`,
//! `manage_pages`, `delete_object`, `duplicate_objects`), with the real `inkboard-mcp` binary.

mod common;

use common::{Client, temp_dir};
use inkboard_core::connector::{Connector, End};
use inkboard_core::{BoxGeom, Document, Frame, Id, Kind, Label, Object, Pt, RichText, Style, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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

fn id_of(r: &Value) -> String {
    r["id"].as_str().unwrap_or_else(|| panic!("no id in {r}")).to_owned()
}

fn shape(c: &mut Client, board: &Path, text: &str, x: f64, y: f64) -> String {
    id_of(&ok(c, board, "add_shape", json!({"geom": "rect", "text": text, "x": x, "y": y, "w": 100, "h": 50})))
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

/// Board with fixed ids covering every kind of object, written directly with the engine.
fn reference_board(path: &Path) {
    let mut doc = Document::new("Reference");
    let asset =
        doc.add_asset("image/svg+xml", b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><rect width=\"10\" height=\"10\"/></svg>".to_vec());
    let mut bold = RichText::plain("Hello ");
    bold.0.push(inkboard_core::Span { text: "world".into(), style: inkboard_core::SpanStyle { bold: true, ..Default::default() } });
    let mut objs = Vec::new();
    let mut r1 = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 120.0, 60.0), Style::default(), Label::new("Start", 20.0, inkboard_core::Align::Center));
    r1.id = Id::from("r1");
    objs.push(r1);
    let mut s1 =
        Object::new_box(BoxGeom::Sticky, Frame::new(300.0, 10.0, 200.0, 200.0), Style::default(), Label::new("Note", 20.0, inkboard_core::Align::Center));
    s1.id = Id::from("s1");
    objs.push(s1);
    let mut t1 = Object::new_box(BoxGeom::Text, Frame::new(0.0, 300.0, 80.0, 26.0), Style::default(), Label::new("Free", 20.0, inkboard_core::Align::Left));
    t1.id = Id::from("t1");
    objs.push(t1);
    let mut e1 = Object::new_box(BoxGeom::Ellipse, Frame::new(0.0, 400.0, 180.0, 110.0), Style::default(), Label::new("", 20.0, inkboard_core::Align::Center));
    e1.id = Id::from("e1");
    if let Kind::Box(b) = &mut e1.kind {
        b.label.text = bold;
    }
    objs.push(e1);
    let mut c1 = Connector::new(End::bound(Id::from("r1")), End::bound(Id::from("s1")));
    c1.label = Some(Label::new("goes", 16.0, inkboard_core::Align::Center));
    let mut c1 = Object::new(Kind::Connector(c1), Style::default());
    c1.id = Id::from("c1");
    objs.push(c1);
    let mut c2 = Object::new(Kind::Connector(Connector::straight(Pt::new(0.0, 600.0), Pt::new(50.0, 650.0))), Style::default());
    c2.id = Id::from("c2");
    objs.push(c2);
    let mut tb = Object::new(Kind::Table(inkboard_core::model::Table::new(Pt::new(600.0, 0.0), 2, 2)), Style::default());
    if let Kind::Table(t) = &mut tb.kind {
        *t.cell_mut(0, 0).unwrap() = RichText::plain("A");
        *t.cell_mut(1, 1).unwrap() = RichText::plain("D");
    }
    tb.id = Id::from("tb");
    objs.push(tb);
    let mut im = Object::new(
        Kind::Image(inkboard_core::model::ImageObj {
            frame: Frame::new(600.0, 200.0, 50.0, 50.0),
            asset: asset.clone(),
            latex: None,
            mono: false,
            recolor: Vec::new(),
        }),
        Style::default(),
    );
    im.id = Id::from("im");
    objs.push(im);
    let mut fm = Object::new(
        Kind::Image(inkboard_core::model::ImageObj {
            frame: Frame::new(600.0, 300.0, 50.0, 30.0),
            asset,
            latex: Some("x^2".into()),
            mono: true,
            recolor: Vec::new(),
        }),
        Style::default(),
    );
    fm.id = Id::from("fm");
    objs.push(fm);
    let mut p1 = Object::new_pen(vec![Pt::new(0.0, 700.0), Pt::new(30.0, 720.0)], false, Style::default());
    p1.id = Id::from("p1");
    objs.push(p1);
    doc.pages[0].objects = objs;
    doc.pages.push(inkboard_core::model::Page::new("Second"));
    serial::save(&doc, path).unwrap();
}

fn ref_setup(tag: &str) -> (Client, PathBuf) {
    let dir = temp_dir(tag);
    let c = Client::start(&dir);
    let board = dir.join("ref.inkboard");
    reference_board(&board);
    (c, board)
}

/// One object of the first page, read with `describe_board` (`full` detail).
fn full(c: &mut Client, board: &Path, id: &str) -> Value {
    let d = ok(c, board, "describe_board", json!({"detail": "full"}));
    d["page"]["objects"].as_array().unwrap().iter().find(|o| o["id"] == json!(id)).cloned().unwrap_or_else(|| panic!("{id} not described in {d}"))
}

fn ids_of(r: &Value) -> Vec<String> {
    r["objects"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap().to_owned()).collect()
}

fn frame_x(board: &Path, id: &str) -> f64 {
    frame(board, id).x
}

fn frame(board: &Path, id: &str) -> Frame {
    load(board).pages[0].objects.iter().find(|o| o.id.0 == id).and_then(Object::frame).unwrap_or_else(|| panic!("{id} has no frame"))
}

fn object(board: &Path, id: &str) -> Object {
    load(board).pages[0].objects.iter().find(|o| o.id.0 == id).cloned().unwrap_or_else(|| panic!("object {id} not in the file"))
}

/* ------------------------------------------------------------------ reading */

/// What `describe_board` returned for `reference_board` before this lot (kept literally).
fn previous_describe() -> Value {
    json!({"name":"Reference","page":{"name":"Page 1","objects":[
        {"h":60.0,"id":"r1","text":"Start","type":"rect","w":120.0,"x":0.0,"y":0.0},
        {"h":200.0,"id":"s1","text":"Note","type":"sticky","w":200.0,"x":300.0,"y":10.0},
        {"h":26.0,"id":"t1","text":"Free","type":"text","w":80.0,"x":0.0,"y":300.0},
        {"h":110.0,"id":"e1","markdown":"Hello **world**","text":"Hello world","type":"ellipse","w":180.0,"x":0.0,"y":400.0},
        {"from":{"object":"r1"},"id":"c1","label":"goes","to":{"object":"s1"},"type":"connector"},
        {"from":{"x":0.0,"y":600.0},"id":"c2","to":{"x":50.0,"y":650.0},"type":"connector"},
        {"cols":2,"h":88.0,"id":"tb","rows":2,"type":"table","w":280.0,"x":600.0,"y":0.0},
        {"h":50.0,"id":"im","type":"image","w":50.0,"x":600.0,"y":200.0},
        {"h":30.0,"id":"fm","latex":"x^2","type":"formula","w":50.0,"x":600.0,"y":300.0},
        {"id":"p1","type":"pen"}]},
        "pages":[{"name":"Page 1","number":1,"objects":10},{"name":"Second","number":2,"objects":0}]})
}

#[test]
fn brief_describe_is_identical_to_the_previous_output() {
    let (mut c, board) = ref_setup("ed-brief");
    assert_eq!(ok(&mut c, &board, "describe_board", json!({})), previous_describe());
    assert_eq!(ok(&mut c, &board, "describe_board", json!({"detail": "brief"})), previous_describe());
    let msg = invalid(&mut c, &board, "describe_board", json!({"detail": "verbose"}));
    assert!(msg.contains("brief, full"), "{msg}");
}

#[test]
fn full_describe_adds_style_font_and_order_for_every_type() {
    let (mut c, board) = ref_setup("ed-full");
    let d = ok(&mut c, &board, "describe_board", json!({"detail": "full"}));
    let brief = previous_describe();
    for (f, b) in d["page"]["objects"].as_array().unwrap().iter().zip(brief["page"]["objects"].as_array().unwrap()) {
        for (k, v) in b.as_object().unwrap() {
            assert_eq!(&f[k], v, "full must keep the brief field {k} of {}", b["id"]);
        }
        assert!(f["style"]["stroke"].is_string() && f["style"]["dash"] == json!("solid"), "{f}");
        assert_eq!(f["style"]["opacity"], json!(1.0));
    }
    let objects = d["page"]["objects"].as_array().unwrap();
    for (z, o) in objects.iter().enumerate() {
        assert_eq!(o["z"], json!(z), "{o}");
    }
    let r1 = &objects[0];
    assert_eq!(r1["style"], json!({"stroke": "ink", "fill": null, "width": 3.0, "dash": "solid", "opacity": 1.0, "roundness": 0.0}));
    assert_eq!(r1["font"], json!({"family": "sans", "size": 20.0, "align": "center"}));
    assert!(r1.get("rotation").is_none() && r1.get("group").is_none() && r1.get("locked").is_none(), "{r1}");
    assert_eq!(objects[2]["font"]["align"], json!("left"));
    let c1 = &objects[4];
    assert_eq!((c1["route"].clone(), c1["arrow"].clone(), c1["stroke"].clone()), (json!("straight"), json!("end"), json!("ink")));
    assert_eq!(c1["font"], json!({"family": "sans", "size": 16.0, "align": "center"}));
    assert!(objects[5].get("font").is_none(), "a connector without label has no font");
    assert_eq!(objects[6]["cells"], json!([["A", ""], ["", "D"]]));
    assert_eq!(objects[6]["font"], json!({"family": "sans", "size": 18.0}));
    assert!(objects[7].get("font").is_none() && objects[9].get("font").is_none());
    assert_eq!(objects[8]["type"], json!("formula"));
    assert_eq!(objects[9]["z"], json!(9));
    // group, lock and rotation appear once set
    ok(&mut c, &board, "arrange_objects", json!({"ids": ["r1", "s1"], "action": "group"}));
    ok(&mut c, &board, "arrange_objects", json!({"ids": ["t1"], "action": "lock"}));
    ok(&mut c, &board, "update_object", json!({"id": "e1", "rotation": 45}));
    ok(&mut c, &board, "update_object", json!({"id": "im", "rotation": -90}));
    let (r1, s1) = (full(&mut c, &board, "r1"), full(&mut c, &board, "s1"));
    assert!(r1["group"].is_string() && r1["group"] == s1["group"]);
    assert_eq!(full(&mut c, &board, "t1")["locked"], json!(true));
    assert_eq!(full(&mut c, &board, "e1")["rotation"], json!(45.0));
    assert_eq!(full(&mut c, &board, "im")["rotation"], json!(-90.0));
}

#[test]
fn full_describe_tells_exotic_arrowheads() {
    let (mut c, board) = ref_setup("ed-heads");
    let mut doc = load(&board);
    if let Some(Object { kind: Kind::Connector(cn), .. }) = doc.pages[0].objects.iter_mut().find(|o| o.id.0 == "c2") {
        cn.head_end = inkboard_core::Arrowhead::Dot;
    }
    serial::save(&doc, &board).unwrap();
    let c2 = full(&mut c, &board, "c2");
    assert_eq!(c2["arrow"], json!("end"));
    assert_eq!(c2["heads"], json!({"start": "none", "end": "dot"}));
}

/* ------------------------------------------------------------- find_objects */

#[test]
fn find_objects_by_text_regex_type_and_region() {
    let (mut c, board) = ref_setup("ed-find");
    let find = |c: &mut Client, args: Value| ok(c, &board, "find_objects", args);
    assert_eq!(ids_of(&find(&mut c, json!({"text": "WORLD"}))), ["e1"]);
    assert_eq!(ids_of(&find(&mut c, json!({"text": "goes"}))), ["c1"]);
    assert_eq!(ids_of(&find(&mut c, json!({"text": "x^2"}))), ["fm"]);
    assert_eq!(ids_of(&find(&mut c, json!({"text": "d"}))), ["e1", "tb"], "table cells are searched; the match is a substring");
    assert_eq!(ids_of(&find(&mut c, json!({"regex": "^S.*t$"}))), ["r1"]);
    assert_eq!(ids_of(&find(&mut c, json!({"regex": "(?i)^(note|free)$"}))), ["s1", "t1"]);
    assert_eq!(ids_of(&find(&mut c, json!({"type": "connector"}))), ["c1", "c2"]);
    assert_eq!(ids_of(&find(&mut c, json!({"type": "table"}))), ["tb"]);
    assert_eq!(ids_of(&find(&mut c, json!({"type": "formula"}))), ["fm"]);
    assert_eq!(ids_of(&find(&mut c, json!({"type": "pen"}))), ["p1"]);
    assert_eq!(ids_of(&find(&mut c, json!({"in_region": {"x": 550, "y": -10, "w": 200, "h": 300}}))), ["tb", "im"]);
    // filters combine
    assert_eq!(ids_of(&find(&mut c, json!({"type": "image", "in_region": {"x": 550, "y": -10, "w": 200, "h": 300}}))), ["im"]);
    assert_eq!(ids_of(&find(&mut c, json!({"text": "o", "type": "rect"}))), Vec::<String>::new());
    let r = find(&mut c, json!({"text": "start"}));
    assert_eq!((r["count"].clone(), r["truncated"].clone()), (json!(1), json!(false)));
    assert_eq!(r["objects"][0]["text"], json!("Start"), "the summaries are the ones of describe_board");
    // another page
    let r = find(&mut c, json!({"page": 2}));
    assert_eq!((r["count"].clone(), r["objects"].clone()), (json!(0), json!([])));
}

#[test]
fn find_objects_limit_and_truncation() {
    let (mut c, board) = ref_setup("ed-find-limit");
    let r = ok(&mut c, &board, "find_objects", json!({}));
    assert_eq!((r["count"].clone(), r["truncated"].clone()), (json!(10), json!(false)));
    let r = ok(&mut c, &board, "find_objects", json!({"limit": 3}));
    assert_eq!((r["count"].clone(), r["truncated"].clone(), ids_of(&r)), (json!(10), json!(true), vec!["r1".to_owned(), "s1".into(), "t1".into()]));
    let r = ok(&mut c, &board, "find_objects", json!({"limit": 500}));
    assert_eq!(r["objects"].as_array().unwrap().len(), 10);
}

#[test]
fn find_objects_rejects_bad_input_and_changes_nothing() {
    let (mut c, board) = ref_setup("ed-find-err");
    for (args, needle) in [
        (json!({"regex": "("}), "invalid regex"),
        (json!({"regex": ""}), "regex"),
        (json!({"regex": "a".repeat(501)}), "500"),
        (json!({"regex": "a{1000}{1000}{1000}"}), "regex"),
        (json!({"text": "a", "regex": "a"}), "either"),
        (json!({"text": ""}), "empty"),
        (json!({"text": 5}), "text"),
        (json!({"type": "circle"}), "rect, ellipse"),
        (json!({"limit": 0}), "limit"),
        (json!({"limit": 501}), "limit"),
        (json!({"limit": "ten"}), "limit"),
        (json!({"in_region": {"x": 0, "y": 0, "w": -1, "h": 5}}), "in_region"),
        (json!({"in_region": {"x": 0}}), "in_region"),
        (json!({"page": 9}), "page"),
    ] {
        let msg = invalid(&mut c, &board, "find_objects", args.clone());
        assert!(msg.contains(needle), "{args}: {msg}");
    }
}

/* ------------------------------------------------------------ update_object */

#[test]
fn update_object_sets_every_style_parameter() {
    let (mut c, board) = ref_setup("ed-style");
    let r = ok(
        &mut c,
        &board,
        "update_object",
        json!({"id": "r1", "stroke_width": 5, "dash": "dotted", "opacity": 0.5, "roundness": 0.3, "font_size": 30, "font_family": "mono", "align": "right",
            "locked": true, "rotation": 30, "fill": "#ff0000"}),
    );
    assert_eq!(r["avertissements"], json!([]), "{r}");
    assert!(r.get("warnings").is_none(), "{r}");
    let o = full(&mut c, &board, "r1");
    assert_eq!(o["style"], json!({"stroke": "ink", "fill": "#ff0000", "width": 5.0, "dash": "dotted", "opacity": 0.5, "roundness": 0.3}));
    assert_eq!(o["font"], json!({"family": "mono", "size": 30.0, "align": "right"}));
    assert_eq!((o["locked"].clone(), o["rotation"].clone()), (json!(true), json!(30.0)));
    // absolute, reversible
    ok(&mut c, &board, "update_object", json!({"id": "r1", "rotation": 0, "locked": false, "dash": "solid", "font_family": "sans", "align": "left"}));
    let o = full(&mut c, &board, "r1");
    assert!(o.get("rotation").is_none() && o.get("locked").is_none(), "{o}");
    assert_eq!((o["style"]["dash"].clone(), o["font"]["family"].clone(), o["font"]["align"].clone()), (json!("solid"), json!("sans"), json!("left")));
    // the angle is stored in radians by the model
    ok(&mut c, &board, "update_object", json!({"id": "r1", "rotation": 90}));
    assert!((frame(&board, "r1").angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    // position is untouched by a rotation (it turns around the center)
    assert_eq!((frame(&board, "r1").x, frame(&board, "r1").y), (0.0, 0.0));
}

#[test]
fn update_object_font_change_refits_the_shape() {
    let (mut c, board) = ref_setup("ed-refit");
    let before = frame(&board, "r1").h;
    ok(&mut c, &board, "update_object", json!({"id": "r1", "text": "a long long long long long long long long text", "font_size": 40}));
    assert!(frame(&board, "r1").h > before, "the shape grows to hold a bigger text");
}

#[test]
fn update_object_validates_everything_before_applying() {
    let (mut c, board) = ref_setup("ed-validate");
    for (args, needle) in [
        (json!({"stroke_width": -1}), "stroke_width"),
        (json!({"stroke_width": 101}), "stroke_width"),
        (json!({"stroke_width": "thick"}), "stroke_width"),
        (json!({"dash": "wavy"}), "solid, dashed, dotted"),
        (json!({"dash": 3}), "dash"),
        (json!({"opacity": 1.5}), "opacity"),
        (json!({"opacity": -0.1}), "opacity"),
        (json!({"roundness": 2}), "roundness"),
        (json!({"font_size": 2}), "font_size"),
        (json!({"font_size": 401}), "font_size"),
        (json!({"font_family": "comic"}), "sans, hand, serif, mono"),
        (json!({"align": "justify"}), "left, center, right"),
        (json!({"locked": "yes"}), "locked"),
        (json!({"rotation": 361}), "rotation"),
        (json!({"route": "zigzag"}), "straight, elbow, curve"),
        (json!({"arrow": "double"}), "end, start, both, none"),
        (json!({"from": "nope"}), "object not found: nope"),
        (json!({"to": "c1"}), "cannot be connected"),
        (json!({"to": "p1"}), "cannot be connected"),
        (json!({"cell": [0, 0], "text": "x"}), "only applies to a table"),
        (json!({"fill": "not-a-color"}), "invalid color"),
    ] {
        // a valid change travels with the invalid one: it must not be applied either
        let mut a = args.clone();
        a["id"] = json!("r1");
        a["stroke"] = json!("#00ff00");
        a["text"] = a.get("text").cloned().unwrap_or(json!("changed"));
        a["x"] = json!(77);
        let msg = invalid(&mut c, &board, "update_object", a);
        assert!(msg.contains(needle), "{args}: {msg}");
    }
}

#[test]
fn update_object_edits_a_table_cell() {
    let (mut c, board) = ref_setup("ed-cell");
    let w0 = full(&mut c, &board, "tb")["w"].as_f64().unwrap();
    let r = ok(&mut c, &board, "update_object", json!({"id": "tb", "cell": [0, 1], "text": "**Total** {color=#e03131}red{/}", "markdown": true}));
    assert!(r.get("warnings").is_none() && r["avertissements"] == json!([]), "{r}");
    let t = full(&mut c, &board, "tb");
    assert_eq!(t["cells"], json!([["A", "Total red"], ["", "D"]]));
    assert_eq!(t["cells_markdown"], json!([["A", "**Total** {color=#e03131}red{/}"], ["", "D"]]));
    // plain by default: markup stays literal
    ok(&mut c, &board, "update_object", json!({"id": "tb", "cell": [1, 0], "text": "**x**"}));
    let t = full(&mut c, &board, "tb");
    assert_eq!(t["cells"][1][0], json!("**x**"));
    assert_eq!(t["cells_markdown"][1][0], json!("\\*\\*x\\*\\*"));
    // a long text widens its column, as add_table does
    ok(&mut c, &board, "update_object", json!({"id": "tb", "cell": [1, 1], "text": "a very very very long cell text that needs room"}));
    assert!(full(&mut c, &board, "tb")["w"].as_f64().unwrap() > w0);
    // markdown warnings are prefixed by the cell
    let r = ok(&mut c, &board, "update_object", json!({"id": "tb", "cell": [0, 0], "text": "**open", "markdown": true}));
    assert!(r["warnings"][0].as_str().unwrap().starts_with("cell [0, 0]: "), "{r}");
    assert_eq!(full(&mut c, &board, "tb")["cells"][0][0], json!("**open"));
    // table-level font and the other parameters
    ok(&mut c, &board, "update_object", json!({"id": "tb", "font_size": 24, "font_family": "serif", "stroke_width": 4}));
    let t = full(&mut c, &board, "tb");
    assert_eq!((t["font"].clone(), t["style"]["width"].clone()), (json!({"family": "serif", "size": 24.0}), json!(4.0)));
}

#[test]
fn update_object_cell_errors_leave_the_file_alone() {
    let (mut c, board) = ref_setup("ed-cell-err");
    for (args, needle) in [
        (json!({"cell": [2, 0], "text": "x"}), "2 rows, 2 columns"),
        (json!({"cell": [0, 2], "text": "x"}), "outside the table"),
        (json!({"cell": [0], "text": "x"}), "[row, column]"),
        (json!({"cell": [-1, 0], "text": "x"}), "[row, column]"),
        (json!({"cell": "A1", "text": "x"}), "[row, column]"),
        (json!({"cell": [0, 0]}), "text"),
        (json!({"cell": [0, 0], "text": "x", "markdown": "yes"}), "markdown"),
    ] {
        let mut a = args.clone();
        a["id"] = json!("tb");
        let msg = invalid(&mut c, &board, "update_object", a);
        assert!(msg.contains(needle), "{args}: {msg}");
    }
    // without `cell`, text on a table is still a warning, as before
    let r = ok(&mut c, &board, "update_object", json!({"id": "tb", "text": "x"}));
    assert_eq!(r["avertissements"], json!(["text can only be changed on shapes and connectors"]));
    assert!(r.get("warnings").is_none(), "warnings is for Markdown only: {r}");
}

#[test]
fn update_object_reroutes_and_rebinds_a_connector() {
    let (mut c, board) = ref_setup("ed-connector");
    ok(&mut c, &board, "update_object", json!({"id": "c1", "route": "elbow", "arrow": "both", "stroke": "#0000ff", "stroke_width": 4}));
    let o = full(&mut c, &board, "c1");
    assert_eq!((o["route"].clone(), o["arrow"].clone(), o["stroke"].clone()), (json!("elbow"), json!("both"), json!("#0000ff")));
    ok(&mut c, &board, "update_object", json!({"id": "c1", "from": "e1", "to": "tb"}));
    let o = full(&mut c, &board, "c1");
    assert_eq!((o["from"].clone(), o["to"].clone()), (json!({"object": "e1"}), json!({"object": "tb"})));
    assert_eq!(o["label"], json!("goes"), "the label is kept");
    // a free connector can be attached
    ok(&mut c, &board, "update_object", json!({"id": "c2", "to": "im"}));
    let o = full(&mut c, &board, "c2");
    assert_eq!((o["from"].clone(), o["to"].clone()), (json!({"x": 0.0, "y": 650.0 - 50.0}), json!({"object": "im"})));
    // label font applies to the label; without a label it is a warning
    ok(&mut c, &board, "update_object", json!({"id": "c1", "font_size": 24, "align": "left"}));
    let o = full(&mut c, &board, "c1");
    assert_eq!((o["font"]["size"].clone(), o["font"]["align"].clone()), (json!(24.0), json!("left")));
    let r = ok(&mut c, &board, "update_object", json!({"id": "c2", "font_size": 24}));
    assert!(r["avertissements"][0].as_str().unwrap().contains("no label"), "{r}");
    // new text with a font in the same call
    ok(&mut c, &board, "update_object", json!({"id": "c2", "text": "later", "font_size": 22}));
    assert_eq!(full(&mut c, &board, "c2")["font"]["size"], json!(22.0));
}

#[test]
fn update_object_warns_about_parameters_that_do_not_suit_the_object() {
    let (mut c, board) = ref_setup("ed-warn");
    let r = ok(&mut c, &board, "update_object", json!({"id": "p1", "font_size": 20, "rotation": 10, "route": "elbow", "opacity": 0.4}));
    let w = r["avertissements"][0].as_str().unwrap();
    assert!(w.contains("font_size") && w.contains("rotation") && w.contains("route") && w.contains("pen"), "{r}");
    assert!(r.get("warnings").is_none(), "warnings is for Markdown only: {r}");
    assert_eq!(full(&mut c, &board, "p1")["style"]["opacity"], json!(0.4), "the other parameters apply");
    let r = ok(&mut c, &board, "update_object", json!({"id": "tb", "rotation": 10, "align": "left"}));
    assert!(r["avertissements"][0].as_str().unwrap().contains("rotation"), "{r}");
    let r = ok(&mut c, &board, "update_object", json!({"id": "r1", "arrow": "both", "from": "s1"}));
    assert!(r["avertissements"][0].as_str().unwrap().contains("not applicable to a rect"), "{r}");
    let r = ok(&mut c, &board, "update_object", json!({"id": "im", "font_size": 20}));
    assert!(r["avertissements"][0].as_str().unwrap().contains("image"), "{r}");
}

#[test]
fn update_object_keeps_working_as_before() {
    let (mut c, board) = ref_setup("ed-compat");
    let r = ok(&mut c, &board, "update_object", json!({"id": "r1", "text": "New", "x": 10, "y": 20, "w": 150, "fill": "#ffec99"}));
    assert_eq!(r, json!({"id": "r1", "avertissements": []}));
    let o = full(&mut c, &board, "r1");
    assert_eq!((o["text"].clone(), o["x"].clone(), o["y"].clone(), o["w"].clone()), (json!("New"), json!(10.0), json!(20.0), json!(150.0)));
    assert!(invalid(&mut c, &board, "update_object", json!({"id": "ghost", "text": "x"})).contains("object not found"));
}

#[test]
fn a_locked_object_is_not_protected_from_the_tools_except_by_align_and_distribute() {
    let (mut c, board) = ref_setup("ed-locked");
    ok(&mut c, &board, "arrange_objects", json!({"ids": ["r1"], "action": "lock"}));
    ok(&mut c, &board, "update_object", json!({"id": "r1", "text": "still editable", "x": 5}));
    assert_eq!(frame_x(&board, "r1"), 5.0);
    ok(&mut c, &board, "move_objects", json!({"ids": ["r1"], "dx": 10}));
    assert_eq!(frame_x(&board, "r1"), 15.0);
    let d = ok(&mut c, &board, "duplicate_objects", json!({"ids": ["r1"]}));
    assert!(object(&board, d["ids"][0].as_str().unwrap()).locked, "the copy is locked too");
    ok(&mut c, &board, "delete_object", json!({"id": "r1"}));
    assert!(load(&board).pages[0].objects.iter().all(|o| o.id.0 != "r1"));
}

/* -------------------------------------------------------------- move_objects */

#[test]
fn move_objects_relative_and_absolute() {
    let (mut c, board) = ref_setup("ed-move");
    let r = ok(&mut c, &board, "move_objects", json!({"ids": ["r1", "s1", "tb", "p1"], "dx": 100, "dy": -20}));
    assert_eq!((r["moved"].clone(), r["dx"].clone(), r["dy"].clone()), (json!(4), json!(100.0), json!(-20.0)));
    let (r1, s1, tb) = (full(&mut c, &board, "r1"), full(&mut c, &board, "s1"), full(&mut c, &board, "tb"));
    assert_eq!((r1["x"].clone(), r1["y"].clone()), (json!(100.0), json!(-20.0)));
    assert_eq!((s1["x"].clone(), s1["y"].clone(), tb["x"].clone(), tb["y"].clone()), (json!(400.0), json!(-10.0), json!(700.0), json!(-20.0)));
    let pen = match object(&board, "p1").kind {
        Kind::Pen(p) => p.pts[0],
        k => panic!("{k:?}"),
    };
    assert_eq!((pen.x, pen.y), (100.0, 680.0));
    // only dx: dy stays 0
    ok(&mut c, &board, "move_objects", json!({"ids": ["r1"], "dx": 1}));
    assert_eq!(full(&mut c, &board, "r1")["y"], json!(-20.0));
    // absolute: top-left of the bounding box of the whole batch (r1 at 101,-20 and s1 at 400,-10)
    let r = ok(&mut c, &board, "move_objects", json!({"ids": ["r1", "s1"], "x": 0, "y": 0}));
    assert_eq!((r["dx"].clone(), r["dy"].clone()), (json!(-101.0), json!(20.0)));
    let (r1, s1) = (full(&mut c, &board, "r1"), full(&mut c, &board, "s1"));
    assert_eq!((r1["x"].clone(), r1["y"].clone(), s1["x"].clone(), s1["y"].clone()), (json!(0.0), json!(0.0), json!(299.0), json!(10.0)));
    // one coordinate only
    ok(&mut c, &board, "move_objects", json!({"ids": ["r1", "s1"], "y": 50}));
    assert_eq!((full(&mut c, &board, "r1")["x"].clone(), full(&mut c, &board, "r1")["y"].clone()), (json!(0.0), json!(50.0)));
    // for one object it is exactly update_object's x/y
    ok(&mut c, &board, "move_objects", json!({"ids": ["e1"], "x": 7, "y": 9}));
    assert_eq!((full(&mut c, &board, "e1")["x"].clone(), full(&mut c, &board, "e1")["y"].clone()), (json!(7.0), json!(9.0)));
}

#[test]
fn move_objects_makes_bound_connectors_follow() {
    let (mut c, board) = ref_setup("ed-move-follow");
    let before = {
        let d = load(&board);
        d.pages[0].bounds(d.pages[0].get(&Id::from("c1")).unwrap())
    };
    ok(&mut c, &board, "move_objects", json!({"ids": ["r1", "s1"], "dx": 0, "dy": 300}));
    let d = load(&board);
    let after = d.pages[0].bounds(d.pages[0].get(&Id::from("c1")).unwrap());
    assert!((after.y - before.y - 300.0).abs() < 1e-6, "the connector is bound to both objects: {before:?} -> {after:?}");
    // moving only one end object stretches the connector
    ok(&mut c, &board, "move_objects", json!({"ids": ["s1"], "dx": 0, "dy": 500}));
    let d = load(&board);
    let stretched = d.pages[0].bounds(d.pages[0].get(&Id::from("c1")).unwrap());
    assert!(stretched.h > after.h + 400.0, "{after:?} -> {stretched:?}");
}

#[test]
fn move_objects_rejects_bad_input_and_changes_nothing() {
    let (mut c, board) = ref_setup("ed-move-err");
    for (args, needle) in [
        (json!({"dx": 5}), "ids"),
        (json!({"ids": [], "dx": 5}), "1 to 2000"),
        (json!({"ids": "r1", "dx": 5}), "ids"),
        (json!({"ids": [3], "dx": 5}), "id strings"),
        (json!({"ids": ["r1", "ghost", "ghost2"], "dx": 5}), "ghost, ghost2"),
        (json!({"ids": ["r1"]}), "dx/dy"),
        (json!({"ids": ["r1"], "dx": 5, "x": 5}), "not both"),
        (json!({"ids": ["r1"], "dx": "far"}), "dx"),
        (json!({"ids": ["r1"], "dx": 1e12}), "out of range"),
        (json!({"ids": ["r1"], "y": -1e12}), "out of range"),
        (json!({"ids": vec!["r1"; 2001], "dx": 1}), "1 to 2000"),
    ] {
        let msg = invalid(&mut c, &board, "move_objects", args.clone());
        assert!(msg.contains(needle), "{args}: {msg}");
    }
    // duplicated ids are counted once
    let r = ok(&mut c, &board, "move_objects", json!({"ids": ["r1", "r1"], "dx": 5}));
    assert_eq!(r["moved"], json!(1));
    assert_eq!(frame_x(&board, "r1"), 5.0);
}

/* ----------------------------------------------------------- arrange_objects */

/// Three 100x50 shapes with different positions: ids and the file.
fn three(c: &mut Client, board: &Path, pos: [(f64, f64); 3]) -> Vec<String> {
    pos.iter().map(|(x, y)| shape(c, board, "s", *x, *y)).collect()
}

fn xs(board: &Path, ids: &[String]) -> Vec<f64> {
    ids.iter().map(|i| frame(board, i).x).collect()
}

fn ys(board: &Path, ids: &[String]) -> Vec<f64> {
    ids.iter().map(|i| frame(board, i).y).collect()
}

fn arrange(c: &mut Client, board: &Path, ids: &[String], action: &str) -> Value {
    ok(c, board, "arrange_objects", json!({"ids": ids, "action": action}))
}

#[test]
fn arrange_aligns_on_the_common_box() {
    let (mut c, board) = setup("ed-align");
    let ids = three(&mut c, &board, [(10.0, 0.0), (50.0, 30.0), (90.0, 60.0)]);
    arrange(&mut c, &board, &ids, "align_left");
    assert_eq!(xs(&board, &ids), [10.0; 3]);
    arrange(&mut c, &board, &ids, "align_right");
    assert_eq!(xs(&board, &ids), [10.0; 3], "already aligned: the widths are equal");
    ok(&mut c, &board, "move_objects", json!({"ids": [ids[1].clone()], "dx": 80}));
    ok(&mut c, &board, "move_objects", json!({"ids": [ids[2].clone()], "dx": 40}));
    assert_eq!(xs(&board, &ids), [10.0, 90.0, 50.0]);
    arrange(&mut c, &board, &ids, "align_right");
    assert_eq!(xs(&board, &ids), [90.0; 3]);
    ok(&mut c, &board, "move_objects", json!({"ids": [ids[0].clone()], "dx": -80}));
    arrange(&mut c, &board, &ids, "align_center_x");
    assert_eq!(xs(&board, &ids), [50.0; 3], "union [9, 191]: center 100");
    assert_eq!(ys(&board, &ids), [0.0, 30.0, 60.0]);
    arrange(&mut c, &board, &ids, "align_top");
    assert_eq!(ys(&board, &ids), [0.0; 3]);
    ok(&mut c, &board, "move_objects", json!({"ids": [ids[1].clone()], "dy": 30}));
    ok(&mut c, &board, "move_objects", json!({"ids": [ids[2].clone()], "dy": 60}));
    arrange(&mut c, &board, &ids, "align_bottom");
    assert_eq!(ys(&board, &ids), [60.0; 3]);
    ok(&mut c, &board, "move_objects", json!({"ids": [ids[0].clone()], "dy": -60}));
    arrange(&mut c, &board, &ids, "align_center_y");
    assert_eq!(ys(&board, &ids), [30.0; 3], "union [-1, 111]: center 55");
}

#[test]
fn arrange_distributes_with_equal_gaps() {
    let (mut c, board) = setup("ed-distribute");
    let ids = three(&mut c, &board, [(0.0, 0.0), (50.0, 100.0), (400.0, 300.0)]);
    arrange(&mut c, &board, &ids, "distribute_horizontal");
    assert_eq!(xs(&board, &ids), [0.0, 200.0, 400.0]);
    assert_eq!(ys(&board, &ids), [0.0, 100.0, 300.0], "y untouched");
    arrange(&mut c, &board, &ids, "distribute_vertical");
    assert_eq!(ys(&board, &ids), [0.0, 150.0, 300.0]);
    // order of ids does not matter: positions decide
    let shuffled = vec![ids[2].clone(), ids[0].clone(), ids[1].clone()];
    ok(&mut c, &board, "move_objects", json!({"ids": [ids[1].clone()], "dx": -190}));
    arrange(&mut c, &board, &shuffled, "distribute_horizontal");
    assert_eq!(xs(&board, &ids), [0.0, 200.0, 400.0]);
}

#[test]
fn arrange_refuses_what_the_engine_would_silently_skip() {
    let (mut c, board) = setup("ed-arrange-err");
    let ids = three(&mut c, &board, [(0.0, 0.0), (50.0, 100.0), (400.0, 300.0)]);
    for (args, needle) in [
        (json!({"ids": ids[..2], "action": "distribute_horizontal"}), "at least 3 unlocked"),
        (json!({"ids": ids[..2], "action": "distribute_vertical"}), "at least 3 unlocked"),
        (json!({"ids": ids[..1], "action": "align_left"}), "at least 2 unlocked"),
        (json!({"ids": ids[..1], "action": "group"}), "at least 2 objects"),
        (json!({"ids": ids, "action": "spin"}), "align_left"),
        (json!({"ids": ids}), "action"),
        (json!({"action": "lock"}), "ids"),
        (json!({"ids": [], "action": "lock"}), "1 to 2000"),
        (json!({"ids": ["ghost"], "action": "lock"}), "ghost"),
    ] {
        let msg = invalid(&mut c, &board, "arrange_objects", args.clone());
        assert!(msg.contains(needle), "{args}: {msg}");
    }
    // locked objects are skipped, and count against the minimum
    arrange(&mut c, &board, &ids[..1], "lock");
    let msg = invalid(&mut c, &board, "arrange_objects", json!({"ids": ids[..2], "action": "align_left"}));
    assert!(msg.contains("got 1 of 2"), "{msg}");
    let r = arrange(&mut c, &board, &ids, "align_left");
    assert_eq!(r["skipped_locked"], json!([ids[0]]));
    assert_eq!(xs(&board, &ids), [0.0, 50.0, 50.0], "the locked shape stayed; the others aligned on the box of the unlocked ones");
    // distribute needs 3 unlocked: here only 2
    let msg = invalid(&mut c, &board, "arrange_objects", json!({"ids": ids, "action": "distribute_horizontal"}));
    assert!(msg.contains("got 2 of 3"), "{msg}");
}

#[test]
fn arrange_changes_the_stacking_order() {
    let (mut c, board) = setup("ed-order");
    let ids = three(&mut c, &board, [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
    let z = |c: &mut Client, i: usize| {
        let d = ok(c, &board, "describe_board", json!({"detail": "full"}));
        d["page"]["objects"].as_array().unwrap().iter().find(|o| o["id"] == json!(ids[i])).unwrap()["z"].as_u64().unwrap()
    };
    assert_eq!([z(&mut c, 0), z(&mut c, 1), z(&mut c, 2)], [0, 1, 2]);
    let a = vec![ids[0].clone()];
    arrange(&mut c, &board, &a, "bring_to_front");
    assert_eq!([z(&mut c, 0), z(&mut c, 1), z(&mut c, 2)], [2, 0, 1]);
    arrange(&mut c, &board, &a, "send_to_back");
    assert_eq!([z(&mut c, 0), z(&mut c, 1), z(&mut c, 2)], [0, 1, 2]);
    arrange(&mut c, &board, &a, "bring_forward");
    assert_eq!([z(&mut c, 0), z(&mut c, 1), z(&mut c, 2)], [1, 0, 2]);
    arrange(&mut c, &board, &a, "send_backward");
    assert_eq!([z(&mut c, 0), z(&mut c, 1), z(&mut c, 2)], [0, 1, 2]);
    // geometry is unchanged
    assert_eq!(xs(&board, &ids), [0.0, 10.0, 20.0]);
}

#[test]
fn arrange_groups_ungroups_locks_and_unlocks() {
    let (mut c, board) = setup("ed-group");
    let ids = three(&mut c, &board, [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
    let r = arrange(&mut c, &board, &ids[..2], "group");
    let g = r["group"].as_str().expect("group id").to_owned();
    assert_eq!(object(&board, &ids[0]).group.as_ref().map(|g| g.0.clone()), Some(g.clone()));
    assert_eq!(object(&board, &ids[1]).group.as_ref().map(|g| g.0.clone()), Some(g.clone()));
    assert_eq!(object(&board, &ids[2]).group, None);
    assert_eq!(full_group(&mut c, &board, &ids[0]), json!(g));
    // groups are not expanded: ungrouping one member releases only it
    let r = arrange(&mut c, &board, &ids[..1], "ungroup");
    assert_eq!(r["ungrouped"], json!(1));
    assert_eq!(object(&board, &ids[0]).group, None);
    assert!(object(&board, &ids[1]).group.is_some());
    let r = arrange(&mut c, &board, &ids, "ungroup");
    assert_eq!(r["ungrouped"], json!(1));
    // lock / unlock
    arrange(&mut c, &board, &ids[1..], "lock");
    assert_eq!([object(&board, &ids[0]).locked, object(&board, &ids[1]).locked, object(&board, &ids[2]).locked], [false, true, true]);
    arrange(&mut c, &board, &ids, "unlock");
    assert!(ids.iter().all(|i| !object(&board, i).locked));
}

fn full_group(c: &mut Client, board: &Path, id: &str) -> Value {
    full(c, board, id)["group"].clone()
}

/* -------------------------------------------------------------- manage_pages */

#[test]
fn manage_pages_lists_renames_and_deletes() {
    let (mut c, board) = setup("ed-pages");
    ok(&mut c, &board, "add_page", json!({"name": "Two"}));
    ok(&mut c, &board, "add_page", json!({"name": "Three"}));
    ok(&mut c, &board, "add_shape", json!({"geom": "rect", "text": "on two", "page": 2}));
    let r = ok(&mut c, &board, "manage_pages", json!({"action": "list"}));
    assert_eq!(
        r["pages"],
        json!([{"number": 1, "name": "Page 1", "objects": 0}, {"number": 2, "name": "Two", "objects": 1}, {"number": 3, "name": "Three", "objects": 0}])
    );
    let r = ok(&mut c, &board, "manage_pages", json!({"action": "rename", "page": 2, "name": "  Renamed  "}));
    assert_eq!((r["page"].clone(), r["name"].clone(), r["previous_name"].clone()), (json!(2), json!("Renamed"), json!("Two")));
    assert!(r.get("warnings").is_none());
    let r = ok(&mut c, &board, "manage_pages", json!({"action": "rename", "page": "Three", "name": "Renamed"}));
    assert!(r["warnings"][0].as_str().unwrap().contains("already named"), "{r}");
    assert_eq!(load(&board).pages.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), ["Page 1", "Renamed", "Renamed"]);
    let r = ok(&mut c, &board, "manage_pages", json!({"action": "delete", "page": 2}));
    assert_eq!((r["deleted"].clone(), r["objects_removed"].clone(), r["pages"].clone()), (json!({"number": 2, "name": "Renamed"}), json!(1), json!(2)));
    assert_eq!(load(&board).pages.len(), 2);
    ok(&mut c, &board, "manage_pages", json!({"action": "delete", "page": "Renamed"}));
    assert_eq!(load(&board).pages.len(), 1);
    let msg = invalid(&mut c, &board, "manage_pages", json!({"action": "delete", "page": 1}));
    assert!(msg.contains("last page"), "{msg}");
}

#[test]
fn manage_pages_rejects_bad_input_and_changes_nothing() {
    let (mut c, board) = setup("ed-pages-err");
    ok(&mut c, &board, "add_page", json!({"name": "Two"}));
    for (args, needle) in [
        (json!({}), "action"),
        (json!({"action": "merge"}), "rename, delete, list"),
        (json!({"action": "delete"}), "\"page\" is required"),
        (json!({"action": "rename", "name": "x"}), "\"page\" is required"),
        (json!({"action": "rename", "page": 1}), "\"name\" is required"),
        (json!({"action": "rename", "page": 1, "name": "   "}), "\"name\" is required"),
        (json!({"action": "rename", "page": 1, "name": "x".repeat(121)}), "too long"),
        (json!({"action": "rename", "page": 1, "name": 5}), "name"),
        (json!({"action": "delete", "page": 9}), "page not found"),
        (json!({"action": "delete", "page": "Nope"}), "page not found"),
        (json!({"action": "delete", "page": 0}), "page not found"),
    ] {
        let msg = invalid(&mut c, &board, "manage_pages", args.clone());
        assert!(msg.contains(needle), "{args}: {msg}");
    }
}

/* ------------------------------------------------------------- delete_object */

#[test]
fn delete_object_accepts_one_id_or_a_list() {
    let (mut c, board) = ref_setup("ed-delete");
    let r = ok(&mut c, &board, "delete_object", json!({"id": "p1"}));
    assert_eq!(r, json!({"deleted": "p1"}), "the single form is unchanged");
    let r = ok(&mut c, &board, "delete_object", json!({"ids": ["r1", "t1", "r1"]}));
    assert_eq!(r, json!({"deleted": ["r1", "t1"]}));
    let left: Vec<String> = load(&board).pages[0].objects.iter().map(|o| o.id.0.clone()).collect();
    assert_eq!(left, ["s1", "e1", "c1", "c2", "tb", "im", "fm"]);
    // the connector bound to a deleted object stays, detached
    ok(&mut c, &board, "delete_object", json!({"ids": ["s1"]}));
    let c1 = full(&mut c, &board, "c1");
    assert_eq!(c1["from"].as_object().map(|o| o.contains_key("x")), Some(true), "{c1}");
}

#[test]
fn delete_object_rejects_bad_input_and_changes_nothing() {
    let (mut c, board) = ref_setup("ed-delete-err");
    for (args, needle) in [
        (json!({}), "\"id\""),
        (json!({"ids": []}), "1 to 2000"),
        (json!({"ids": ["r1", "ghost"]}), "ghost"),
        (json!({"ids": "r1"}), "ids"),
        (json!({"id": "r1", "ids": ["s1"]}), "not both"),
        (json!({"id": "ghost"}), "object not found"),
    ] {
        let msg = invalid(&mut c, &board, "delete_object", args.clone());
        assert!(msg.contains(needle), "{args}: {msg}");
    }
}

/* --------------------------------------------------------- duplicate_objects */

#[test]
fn duplicate_objects_copies_with_offset_and_keeps_inner_connectors() {
    let (mut c, board) = ref_setup("ed-duplicate");
    let r = ok(&mut c, &board, "duplicate_objects", json!({"ids": ["s1", "c1", "r1"], "dx": 0, "dy": 500}));
    assert_eq!(r["sources"], json!(["r1", "s1", "c1"]), "page order");
    let new: Vec<String> = r["ids"].as_array().unwrap().iter().map(|i| i.as_str().unwrap().to_owned()).collect();
    assert_eq!((r["created"].clone(), new.len()), (json!(3), 3));
    let d = load(&board);
    assert_eq!(d.pages[0].objects.len(), 13);
    let (r1, s1) = (frame(&board, &new[0]), frame(&board, &new[1]));
    assert_eq!((r1.x, r1.y, s1.x, s1.y), (0.0, 500.0, 300.0, 510.0));
    // the copy of the connector joins the copies, not the originals
    match object(&board, &new[2]).kind {
        Kind::Connector(cn) => {
            assert_eq!(cn.start, End::bound(Id(new[0].clone())));
            assert_eq!(cn.end, End::bound(Id(new[1].clone())));
            assert_eq!(cn.label.map(|l| l.text.to_plain()), Some("goes".to_owned()));
        }
        k => panic!("{k:?}"),
    }
    // the originals are untouched
    match object(&board, "c1").kind {
        Kind::Connector(cn) => assert_eq!((cn.start, cn.end), (End::bound(Id::from("r1")), End::bound(Id::from("s1")))),
        k => panic!("{k:?}"),
    }
    // the new objects are on top
    assert_eq!(d.pages[0].objects[10..].iter().map(|o| o.id.0.clone()).collect::<Vec<_>>(), new);
}

#[test]
fn duplicate_objects_detaches_a_connector_end_that_is_not_copied() {
    let (mut c, board) = ref_setup("ed-duplicate-out");
    let r = ok(&mut c, &board, "duplicate_objects", json!({"ids": ["r1", "c1"]}));
    let new = r["ids"].as_array().unwrap();
    match object(&board, new[1].as_str().unwrap()).kind {
        Kind::Connector(cn) => {
            assert_eq!(cn.start, End::bound(Id(new[0].as_str().unwrap().to_owned())));
            assert!(matches!(cn.end, End::Free { .. }), "the end bound to s1 (not copied) is free in the copy: {:?}", cn.end);
        }
        k => panic!("{k:?}"),
    }
    // default offset (40, 40)
    let f = frame(&board, new[0].as_str().unwrap());
    assert_eq!((f.x, f.y), (40.0, 40.0));
}

#[test]
fn duplicate_objects_makes_several_copies_and_new_groups() {
    let (mut c, board) = setup("ed-duplicate-n");
    let ids = three(&mut c, &board, [(0.0, 0.0), (200.0, 0.0), (400.0, 0.0)]);
    let g = arrange(&mut c, &board, &ids[..2], "group")["group"].as_str().unwrap().to_owned();
    let r = ok(&mut c, &board, "duplicate_objects", json!({"ids": ids[..2], "dx": 0, "dy": 100, "count": 3}));
    let new: Vec<String> = r["ids"].as_array().unwrap().iter().map(|i| i.as_str().unwrap().to_owned()).collect();
    assert_eq!(new.len(), 6);
    assert_eq!(ys(&board, &new), [100.0, 100.0, 200.0, 200.0, 300.0, 300.0], "copy k is shifted by k times the offset");
    assert_eq!(xs(&board, &new[..2]), [0.0, 200.0]);
    let groups: Vec<Option<String>> = new.iter().map(|i| object(&board, i).group.map(|g| g.0)).collect();
    assert!(groups.iter().all(Option::is_some));
    assert_eq!(groups[0], groups[1]);
    assert_ne!(groups[0], groups[2], "each copy has its own group");
    assert!(groups.iter().all(|x| x.as_deref() != Some(g.as_str())), "never the original group");
}

#[test]
fn duplicate_objects_rejects_bad_input_and_changes_nothing() {
    let (mut c, board) = ref_setup("ed-duplicate-err");
    for (args, needle) in [
        (json!({}), "ids"),
        (json!({"ids": []}), "1 to 2000"),
        (json!({"ids": ["ghost"]}), "ghost"),
        (json!({"ids": ["r1"], "count": 0}), "count"),
        (json!({"ids": ["r1"], "count": 51}), "count"),
        (json!({"ids": ["r1"], "count": 1.5}), "count"),
        (json!({"ids": ["r1"], "dx": "a"}), "dx"),
        (json!({"ids": ["r1"], "dy": 1e12}), "out of range"),
    ] {
        let msg = invalid(&mut c, &board, "duplicate_objects", args.clone());
        assert!(msg.contains(needle), "{args}: {msg}");
    }
}

#[test]
fn duplicate_objects_refuses_an_excessive_number_of_copies() {
    let (mut c, board) = setup("ed-duplicate-big");
    let mut doc = load(&board);
    doc.pages[0].objects =
        (0..300).map(|i| Object::new_box(BoxGeom::Rect, Frame::new(f64::from(i) * 10.0, 0.0, 5.0, 5.0), Style::default(), Label::default())).collect();
    let ids: Vec<String> = doc.pages[0].objects.iter().map(|o| o.id.0.clone()).collect();
    serial::save(&doc, &board).unwrap();
    let msg = invalid(&mut c, &board, "duplicate_objects", json!({"ids": ids, "count": 40}));
    assert!(msg.contains("too many copies"), "{msg}");
    let r = ok(&mut c, &board, "duplicate_objects", json!({"ids": ids, "count": 2}));
    assert_eq!(r["created"], json!(600));
}

/* ------------------------------------------------------------------ catalog */

#[test]
fn the_new_tools_are_listed_with_complete_schemas() {
    let (mut c, _board) = setup("ed-list");
    let r = c.request("tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().unwrap();
    let get = |n: &str| tools.iter().find(|t| t["name"] == json!(n)).cloned().unwrap_or_else(|| panic!("{n} not listed"));
    for n in ["find_objects", "move_objects", "arrange_objects", "manage_pages", "duplicate_objects"] {
        let t = get(n);
        assert!(t["description"].as_str().is_some_and(|d| d.len() > 40), "{n}");
        assert_eq!(t["inputSchema"]["type"], json!("object"), "{n}");
        assert!(t["inputSchema"]["properties"]["path"].is_object() && t["inputSchema"]["properties"]["page"].is_object(), "{n}");
    }
    assert_eq!(get("move_objects")["inputSchema"]["required"], json!(["ids"]));
    assert_eq!(get("arrange_objects")["inputSchema"]["properties"]["action"]["enum"].as_array().unwrap().len(), 16);
    assert_eq!(get("describe_board")["inputSchema"]["properties"]["detail"]["enum"], json!(["brief", "full"]));
    assert_eq!(get("delete_object")["inputSchema"]["properties"]["ids"]["type"], json!("array"));
    for p in ["stroke_width", "dash", "opacity", "roundness", "font_size", "font_family", "align", "locked", "rotation", "route", "arrow", "from", "to", "cell"]
    {
        assert!(get("update_object")["inputSchema"]["properties"][p].is_object(), "update_object.{p}");
    }
    // only the read-only tool leaves the file alone: a call to it must not rewrite it
    let (mut c, board) = ref_setup("ed-readonly");
    let before = std::fs::metadata(&board).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    ok(&mut c, &board, "find_objects", json!({"type": "rect"}));
    assert_eq!(std::fs::metadata(&board).unwrap().modified().unwrap(), before);
}
