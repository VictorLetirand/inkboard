//! Icon tools (`search_icons`, `list_icon_groups`, `add_icon`) with the real `inkboard-mcp` binary (`--no-live`).

mod common;

use base64::Engine;
use common::{Client, temp_dir};
use inkboard_core::{Document, Kind, Object, serial};
use inkboard_icons::IconRef;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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

/// A client and an empty board.
fn start(tag: &str) -> (Client, PathBuf) {
    let dir = temp_dir(tag);
    let mut c = Client::start(&dir);
    let board = dir.join("b.inkboard");
    c.tool("create_board", json!({"path": board.to_str().unwrap()})).unwrap();
    (c, board)
}

/// Same contract as the other tool tests: an error, the file untouched, the server still answering.
fn invalid(c: &mut Client, board: &Path, tool: &str, args: Value) -> String {
    let before = std::fs::read(board).unwrap();
    let shown = args.to_string();
    let mut params = json!({"name": tool});
    params["arguments"] = args;
    let r = c.request("tools/call", params);
    assert!(r["result"]["isError"] == json!(true), "{tool} with {shown}: error expected, got {r}");
    assert_eq!(std::fs::read(board).unwrap(), before, "{tool}: the file changed despite the error");
    assert_eq!(c.request("ping", json!({}))["result"], json!({}), "{tool}: the server no longer answers");
    r["result"]["content"][0]["text"].as_str().unwrap().to_owned()
}

fn search(c: &mut Client, board: &Path, args: Value) -> Value {
    let mut a = args;
    a["path"] = json!(board.to_str().unwrap());
    c.tool("search_icons", a).unwrap()
}

/// First icon found for a query (as returned by `search_icons`).
fn first(c: &mut Client, board: &Path, args: Value) -> Value {
    let shown = args.to_string();
    let r = search(c, board, args);
    r["icons"].get(0).cloned().unwrap_or_else(|| panic!("no icon for {shown}"))
}

/// SVG of an icon in the library, from its id (no `#n` suffix).
fn library_svg(id: &str) -> String {
    let lib = inkboard_icons::shared().unwrap();
    let (set_id, name) = id.split_once('/').unwrap();
    let set = lib.sets.iter().position(|s| s.id == set_id).unwrap();
    let icon = lib.sets[set].names.iter().position(|n| n == name).unwrap();
    lib.svg(IconRef { set, icon }).unwrap()
}

fn asset_svg(doc: &Document, o: &Object) -> String {
    let Kind::Image(i) = &o.kind else { panic!("not an image: {:?}", o.kind) };
    let a = &doc.assets[&i.asset];
    assert_eq!(a.mime, "image/svg+xml");
    String::from_utf8(a.data.to_vec()).unwrap()
}

fn is_image(o: &Object) -> bool {
    matches!(o.kind, Kind::Image(_))
}

#[test]
fn the_three_tools_are_listed_with_their_schema() {
    let dir = temp_dir("icons-list");
    let mut c = Client::start(&dir);
    let list = c.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    for (name, required) in [("search_icons", "query"), ("list_icon_groups", ""), ("add_icon", "icon")] {
        let t = tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name} missing"));
        assert!(t["description"].as_str().is_some_and(|d| d.len() > 60), "{name}: description");
        assert_eq!(t["inputSchema"]["type"], "object");
        let req: Vec<&str> = t["inputSchema"]["required"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(req, if required.is_empty() { vec![] } else { vec![required] }, "{name}");
    }
}

#[test]
fn search_finds_lambda_in_aws_and_github_in_brands() {
    let (mut c, board) = start("icons-find");
    let lambda = search(&mut c, &board, json!({"query": "lambda"}));
    assert!(
        lambda["icons"].as_array().unwrap().iter().any(|i| i["group"] == "AWS" && i["name"].as_str().unwrap().to_lowercase().contains("lambda")),
        "{lambda}"
    );
    let github = search(&mut c, &board, json!({"query": "GitHub"}));
    assert!(github["icons"].as_array().unwrap().iter().any(|i| i["group"] == "Brands" && i["name"] == "GitHub"), "{github}");
    // shape of an entry
    let i = &github["icons"][0];
    for k in ["id", "name", "set", "group"] {
        assert!(i[k].as_str().is_some_and(|s| !s.is_empty()), "{k} in {i}");
    }
    assert!(i["mono"].is_boolean());
    assert_eq!(github["count"], json!(github["icons"].as_array().unwrap().len()));
}

#[test]
fn search_limit_truncation_and_filters() {
    let (mut c, board) = start("icons-filter");
    let all = search(&mut c, &board, json!({"query": "arrow", "limit": 100}));
    let all = all["icons"].as_array().unwrap().clone();
    assert_eq!(all.len(), 100, "a broad query fills the maximum limit");
    let few = search(&mut c, &board, json!({"query": "arrow", "limit": 3}));
    assert_eq!((few["count"].clone(), few["truncated"].clone()), (json!(3), json!(true)));
    assert_eq!(few["icons"].as_array().unwrap()[..], all[..3], "same order whatever the limit");
    let default = search(&mut c, &board, json!({"query": "arrow"}));
    assert_eq!(default["count"], 20, "default limit");
    let one = search(&mut c, &board, json!({"query": "docker", "set": "Brand logos (Simple Icons)", "limit": 100}));
    assert_eq!(one["truncated"], false, "{one}");

    // group (case-insensitive) and set (title or id) filter
    let aws = search(&mut c, &board, json!({"query": "database", "group": "aws", "limit": 100}));
    assert!(aws["count"].as_u64().unwrap() > 0);
    assert!(aws["icons"].as_array().unwrap().iter().all(|i| i["group"] == "AWS"), "{aws}");
    for set in ["Tabler icons", "tabler"] {
        let tabler = search(&mut c, &board, json!({"query": "database", "set": set, "limit": 100}));
        assert!(tabler["count"].as_u64().unwrap() > 0, "{set}");
        assert!(tabler["icons"].as_array().unwrap().iter().all(|i| i["set"] == "Tabler icons" && i["mono"] == true), "{tabler}");
    }
    let both = search(&mut c, &board, json!({"query": "database", "group": "Brands", "set": "tabler"}));
    assert_eq!(both["count"], 0, "a set outside the group gives nothing");
    // an empty filter is ignored
    assert_eq!(search(&mut c, &board, json!({"query": "arrow", "group": "", "set": " "}))["count"], 20);
    let none = search(&mut c, &board, json!({"query": "zzzzqqqq"}));
    assert_eq!((none["count"].clone(), none["truncated"].clone(), none["icons"].clone()), (json!(0), json!(false), json!([])));
}

#[test]
fn search_rejects_bad_input_and_never_touches_the_file() {
    let (mut c, board) = start("icons-search-err");
    let p = board.to_str().unwrap();
    assert!(invalid(&mut c, &board, "search_icons", json!({"path": p, "query": ""})).contains("query is empty"));
    assert!(invalid(&mut c, &board, "search_icons", json!({"path": p, "query": "   "})).contains("query is empty"));
    assert!(invalid(&mut c, &board, "search_icons", json!({"path": p})).contains("query"));
    assert!(invalid(&mut c, &board, "search_icons", json!({"path": p, "query": "x".repeat(10_000)})).contains("too long"));
    for limit in [json!(0), json!(101), json!(-1), json!(1.5), json!("5")] {
        assert!(invalid(&mut c, &board, "search_icons", json!({"path": p, "query": "a", "limit": limit})).contains("limit"), "{limit}");
    }
    let e = invalid(&mut c, &board, "search_icons", json!({"path": p, "query": "a", "group": "Nope"}));
    assert!(e.contains("unknown group") && e.contains("AWS") && e.contains("Brands"), "{e}");
    let e = invalid(&mut c, &board, "search_icons", json!({"path": p, "query": "a", "set": "Tabl"}));
    assert!(e.contains("unknown set") && e.contains("Tabler icons"), "{e}");
    assert!(invalid(&mut c, &board, "search_icons", json!({"path": p, "query": "a", "set": "zzzz"})).contains("list_icon_groups"));
    // a valid search leaves the file as it was too
    let before = std::fs::read(&board).unwrap();
    search(&mut c, &board, json!({"query": "lambda"}));
    assert_eq!(std::fs::read(&board).unwrap(), before);
}

#[test]
fn list_icon_groups_sums_to_the_total() {
    let (mut c, board) = start("icons-groups");
    let before = std::fs::read(&board).unwrap();
    let r = c.tool("list_icon_groups", json!({"path": board.to_str().unwrap()})).unwrap();
    assert_eq!(std::fs::read(&board).unwrap(), before);
    let groups = r["groups"].as_array().unwrap();
    let sum: u64 = groups.iter().flat_map(|g| g["sets"].as_array().unwrap()).map(|s| s["icons"].as_u64().unwrap()).sum();
    assert_eq!(r["total"].as_u64().unwrap(), sum);
    assert_eq!(usize::try_from(r["total"].as_u64().unwrap()).unwrap(), inkboard_icons::shared().unwrap().icon_count());
    let names: Vec<&str> = groups.iter().map(|g| g["group"].as_str().unwrap()).collect();
    for g in ["AWS", "Brands", "General icons"] {
        assert!(names.contains(&g), "{g} in {names:?}");
    }
    assert!(groups.iter().all(|g| !g["sets"].as_array().unwrap().is_empty()));
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "each group once");
}

#[test]
fn add_icon_places_the_svg_of_the_icon_with_the_right_mono_flag() {
    let (mut client, board) = start("icons-add");
    let p = board.to_str().unwrap();
    let color = first(&mut client, &board, json!({"query": "lambda", "group": "AWS"}));
    let mono = first(&mut client, &board, json!({"query": "arrow", "set": "tabler"}));
    assert_eq!((color["mono"].clone(), mono["mono"].clone()), (json!(false), json!(true)));

    let a = client.tool("add_icon", json!({"path": p, "icon": color["id"], "x": 10, "y": 20})).unwrap();
    let b = client.tool("add_icon", json!({"path": p, "icon": mono["id"], "x": 200, "y": 20})).unwrap();
    assert_eq!(a["icon"], color["id"]);
    let doc = load(&board);
    let (oa, ob) = (object(&board, &id_of(&a)), object(&board, &id_of(&b)));
    for (o, icon) in [(&oa, &color), (&ob, &mono)] {
        assert!(is_image(o));
        assert_eq!(asset_svg(&doc, o), library_svg(icon["id"].as_str().unwrap()));
        assert_eq!(o.style.width, 0.0);
        assert!(o.style.fill.is_none());
        assert!(o.is_attachable());
    }
    let mono_of = |o: &Object| matches!(&o.kind, Kind::Image(i) if i.mono);
    assert_eq!((mono_of(&oa), mono_of(&ob)), (false, true));
    let f = oa.frame().unwrap();
    assert_eq!((f.x, f.y), (10.0, 20.0));
    assert_eq!((a["x"].clone(), a["y"].clone()), (json!(10.0), json!(20.0)));
    assert!(a.get("label_id").is_none() && a.get("warnings").is_none());
}

#[test]
fn add_icon_without_position_goes_below_the_content() {
    let (mut c, board) = start("icons-below");
    let p = board.to_str().unwrap();
    let r = c.tool("add_shape", json!({"path": p, "geom": "rect", "x": 0, "y": 0, "w": 100, "h": 50})).unwrap();
    let lambda = first(&mut c, &board, json!({"query": "lambda"}));
    let i = c.tool("add_icon", json!({"path": p, "icon": lambda["id"]})).unwrap();
    assert!(object(&board, &id_of(&i)).frame().unwrap().y >= object(&board, &id_of(&r)).frame().unwrap().y + 50.0);
}

#[test]
fn add_icon_sizes_keep_the_ratio() {
    let (mut client, board) = start("icons-size");
    let p = board.to_str().unwrap();
    // an icon whose natural ratio is not 1
    let lib = inkboard_icons::shared().unwrap();
    let wide = lib
        .search("aws", usize::MAX)
        .into_iter()
        .find(|r| {
            let (w, h) = inkboard_icons::svg_dimensions(&lib.svg(*r).unwrap()).unwrap_or((1.0, 1.0));
            (w / h - 1.0).abs() > 0.2 && lib.sets[r.set].names[r.icon].find(['#', '/']).is_none()
        })
        .expect("a non-square AWS icon");
    let id = format!("{}/{}", lib.sets[wide.set].id, lib.sets[wide.set].names[wide.icon]);
    let (nw, nh) = inkboard_icons::svg_dimensions(&lib.svg(wide).unwrap()).unwrap();
    let ratio = nw / nh;
    let frame = |c: &mut Client, args: Value| {
        let mut a = args;
        a["path"] = json!(p);
        a["icon"] = json!(id);
        let r = c.tool("add_icon", a).unwrap();
        let f = object(&board, &id_of(&r)).frame().unwrap();
        (f.w, f.h)
    };
    let (w, h) = frame(&mut client, json!({}));
    assert!((w.max(h) - 64.0).abs() < 1e-6 && (w / h - ratio).abs() < 1e-6, "default size 64: {w} x {h}");
    let (w, h) = frame(&mut client, json!({"size": 200}));
    assert!((w.max(h) - 200.0).abs() < 1e-6 && (w / h - ratio).abs() < 1e-6, "{w} x {h}");
    let (w, h) = frame(&mut client, json!({"w": 120}));
    assert!((w - 120.0).abs() < 1e-6 && (w / h - ratio).abs() < 1e-6, "{w} x {h}");
    let (w, h) = frame(&mut client, json!({"h": 90}));
    assert!((h - 90.0).abs() < 1e-6 && (w / h - ratio).abs() < 1e-6, "{w} x {h}");
    assert_eq!(frame(&mut client, json!({"w": 50, "h": 70})), (50.0, 70.0), "both given: exactly that");
    for size in [8.0, 2000.0] {
        let (w, h) = frame(&mut client, json!({"size": size}));
        assert!((w.max(h) - size).abs() < 1e-6, "bound {size}: {w} x {h}");
    }
}

#[test]
fn add_icon_rejects_bad_sizes_and_input_without_touching_the_file() {
    let (mut c, board) = start("icons-add-err");
    let p = board.to_str().unwrap();
    let lambda = first(&mut c, &board, json!({"query": "lambda"}));
    let icon = lambda["id"].clone();
    for (k, v) in
        [("size", json!(7)), ("size", json!(2001)), ("size", json!(0)), ("size", json!(-5)), ("size", json!("big")), ("w", json!(7)), ("h", json!(5000))]
    {
        let e = invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": icon, k: v}));
        assert!(e.contains(k) && e.contains("between"), "{k}={v}: {e}");
    }
    let e = invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": icon, "size": 100, "w": 50}));
    assert!(e.contains("either"), "{e}");
    assert!(invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": icon, "stroke": "not-a-color"})).contains("invalid color"));
    assert!(invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": icon, "label": "x".repeat(501)})).contains("too long"));
    assert!(invalid(&mut c, &board, "add_icon", json!({"path": p})).contains("icon"));
    assert!(invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": "x".repeat(10_000)})).contains("too long"));
    assert!(invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": "", "size": 10})).contains("not found"));
    assert_eq!(objects(&board), [] as [inkboard_core::Object; 0]);
}

#[test]
fn an_unknown_icon_gives_suggestions_and_leaves_the_file_alone() {
    let (mut c, board) = start("icons-unknown");
    let p = board.to_str().unwrap();
    let e = invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": "ec2"}));
    assert!(e.starts_with("icon not found: \"ec2\""), "{e}");
    let tail = e.split("did you mean: ").nth(1).unwrap_or_else(|| panic!("suggestions expected: {e}"));
    let suggestions: Vec<&str> = tail.split(" ? ").next().unwrap().split(", ").collect();
    assert!((1..=5).contains(&suggestions.len()), "{suggestions:?}");
    // every suggestion is a valid id
    for s in suggestions {
        c.tool("add_icon", json!({"path": p, "icon": s})).unwrap_or_else(|e| panic!("{s}: {e}"));
    }
    // right set, wrong name; wrong set; no resemblance at all
    let e = invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": "drawio-aws4/Lambdaa"}));
    assert!(e.contains("not found"), "{e}");
    assert!(invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": "nope/Lambda"})).contains("did you mean"));
    assert!(invalid(&mut c, &board, "add_icon", json!({"path": p, "icon": "zzzzqqqq"})).contains("search_icons"));
}

#[test]
fn stroke_tints_monochrome_icons_and_is_reported_ignored_for_colored_ones() {
    let (mut client, board) = start("icons-tint");
    let path = board.to_str().unwrap();
    let mono = first(&mut client, &board, json!({"query": "arrow", "set": "tabler"}));
    let color = first(&mut client, &board, json!({"query": "lambda", "group": "AWS"}));

    let a = client.tool("add_icon", json!({"path": path, "icon": mono["id"], "stroke": "#ff0000"})).unwrap();
    assert!(a.get("warnings").is_none());
    let obj = object(&board, &id_of(&a));
    assert_eq!(obj.style.stroke, inkboard_core::Color::parse("#ff0000").unwrap());
    let svg_path = board.with_file_name("tinted.svg");
    client.tool("export", json!({"path": path, "output": svg_path.to_str().unwrap()})).unwrap();
    let svg = std::fs::read_to_string(&svg_path).unwrap();
    let payload = embedded_images(&svg).concat();
    assert!(payload.contains("#ff0000") && !payload.contains("#ffffff"), "tint missing in the exported icon");

    // the default tint is the text color of the theme, never white
    let b = client.tool("add_icon", json!({"path": path, "icon": mono["id"]})).unwrap();
    assert!(b.get("warnings").is_none());
    let doc = load(&board);
    let o = doc.pages[0].objects.iter().find(|o| o.id.0 == id_of(&b)).unwrap();
    assert_eq!(o.style.stroke, inkboard_core::Color::Ink);

    // a colored icon keeps its colors, and the agent is told
    let colored = client.tool("add_icon", json!({"path": path, "icon": color["id"], "stroke": "#ff0000"})).unwrap();
    let warnings = colored["warnings"].as_array().unwrap_or_else(|| panic!("warnings expected: {colored}"));
    assert!(warnings.len() == 1 && warnings[0].as_str().unwrap().contains("stroke ignored"), "{warnings:?}");
    let o = object(&board, &id_of(&colored));
    assert_eq!(o.style.stroke, inkboard_core::Style::default().stroke);
    assert!(matches!(&o.kind, Kind::Image(i) if !i.mono));
}

/// Decoded payloads of the `data:` images of an exported SVG.
fn embedded_images(svg: &str) -> Vec<String> {
    svg.split("href=\"data:image/svg+xml;base64,")
        .skip(1)
        .map(|s| String::from_utf8(base64::engine::general_purpose::STANDARD.decode(s.split('"').next().unwrap()).unwrap()).unwrap())
        .collect()
}

#[test]
fn a_label_is_a_text_centered_under_the_icon_and_grouped_with_it() {
    let (mut c, board) = start("icons-label");
    let p = board.to_str().unwrap();
    let lambda = first(&mut c, &board, json!({"query": "lambda"}));
    let r = c.tool("add_icon", json!({"path": p, "icon": lambda["id"], "x": 100, "y": 50, "size": 80, "label": "Order handler"})).unwrap();
    let (icon, label) = (object(&board, &id_of(&r)), object(&board, r["label_id"].as_str().unwrap()));
    assert!(is_image(&icon));
    let Kind::Box(b) = &label.kind else { panic!("label is not a text box") };
    assert_eq!(b.label.text.to_plain(), "Order handler");
    assert_eq!(b.geom, inkboard_core::BoxGeom::Text);
    let (fi, fl) = (icon.frame().unwrap(), label.frame().unwrap());
    assert!((fl.center().x - fi.center().x).abs() < 0.01, "centered: {fl:?} / {fi:?}");
    assert!(fl.y >= fi.y + fi.h, "below the icon");
    assert!(icon.group.is_some() && icon.group == label.group, "grouped");
    assert_eq!(objects(&board).iter().filter(|o| o.group == icon.group).count(), 2);
    assert_eq!(objects(&board).len(), 2);
    // an empty label adds nothing
    let r = c.tool("add_icon", json!({"path": p, "icon": lambda["id"], "label": ""})).unwrap();
    assert!(r.get("label_id").is_none());
    assert_eq!(objects(&board).len(), 3);
}

#[test]
fn the_same_icon_twice_stores_one_asset() {
    let (mut c, board) = start("icons-dedup");
    let p = board.to_str().unwrap();
    let lambda = first(&mut c, &board, json!({"query": "lambda"}));
    let a = c.tool("add_icon", json!({"path": p, "icon": lambda["id"]})).unwrap();
    let b = c.tool("add_icon", json!({"path": p, "icon": lambda["id"], "size": 30})).unwrap();
    assert_ne!(id_of(&a), id_of(&b));
    let doc = load(&board);
    assert_eq!(doc.pages[0].objects.len(), 2);
    assert_eq!(doc.assets.len(), 1, "one asset for both objects");
}

#[test]
fn two_icons_can_be_connected_and_the_board_exports() {
    let (mut client, board) = start("icons-connect");
    let p = board.to_str().unwrap();
    let lambda = first(&mut client, &board, json!({"query": "lambda", "group": "AWS"}));
    let github = first(&mut client, &board, json!({"query": "github", "group": "Brands"}));
    let a = client.tool("add_icon", json!({"path": p, "icon": lambda["id"], "x": 0, "y": 0, "label": "Lambda"})).unwrap();
    let b = client.tool("add_icon", json!({"path": p, "icon": github["id"], "x": 300, "y": 0})).unwrap();
    client.tool("add_connector", json!({"path": p, "from": a["id"], "to": b["id"]})).unwrap();
    assert!(objects(&board).iter().any(|o| matches!(o.kind, Kind::Connector(_))));
    for out in ["icons.png", "icons.svg"] {
        let out = board.with_file_name(out);
        client.tool("export", json!({"path": p, "output": out.to_str().unwrap()})).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.len() > 500, "{} is almost empty", out.display());
        if out.extension().is_some_and(|e| e == "png") {
            assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        } else {
            assert_eq!(embedded_images(&String::from_utf8(bytes).unwrap()).len(), 2, "both icons are drawn");
        }
    }
    // the description of the board lists the icons as images
    let d = client.tool("describe_board", json!({"path": p})).unwrap();
    assert!(d.to_string().contains("image"), "{d}");
}

#[test]
fn icon_ids_with_a_repeated_name_can_be_placed() {
    let (mut c, board) = start("icons-dup-name");
    let p = board.to_str().unwrap();
    let all = search(&mut c, &board, json!({"query": "Backstage", "set": "simple-icons", "limit": 100}));
    let ids: Vec<&str> = all["icons"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"simple-icons/Backstage") && ids.contains(&"simple-icons/Backstage#2"), "{ids:?}");
    for id in ["simple-icons/Backstage", "simple-icons/Backstage#2"] {
        let r = c.tool("add_icon", json!({"path": p, "icon": id})).unwrap();
        assert_eq!(r["icon"], id);
    }
    // two different icons of the same name: two assets
    assert_eq!(load(&board).assets.len(), 2);
    // names that contain `/` or `#`
    for id in ["simple-icons/shadcn/ui", "simple-icons/F#"] {
        assert_eq!(c.tool("add_icon", json!({"path": p, "icon": id})).unwrap()["icon"], id);
    }
}

#[test]
fn the_label_can_be_markdown_and_is_literal_by_default() {
    let (mut c, board) = start("icons-label-md");
    let p = board.to_str().unwrap();
    let lambda = first(&mut c, &board, json!({"query": "lambda", "group": "AWS"}));
    let plain = c.tool("add_icon", json!({"path": p, "icon": lambda["id"], "label": "**Order** handler"})).unwrap();
    let rich = c.tool("add_icon", json!({"path": p, "icon": lambda["id"], "label": "**Order** {color=#e03131}handler{/}", "markdown": true})).unwrap();
    let text = |r: &Value| match object(&board, r["label_id"].as_str().unwrap()).kind {
        Kind::Box(b) => b.label.text,
        _ => panic!("label is not a text box"),
    };
    assert_eq!(text(&plain).to_plain(), "**Order** handler", "literal without markdown");
    let t = text(&rich);
    assert_eq!(t.to_plain(), "Order handler");
    assert!(t.0.iter().any(|s| s.style.bold) && t.0.iter().any(|s| s.style.color.is_some()));
}
