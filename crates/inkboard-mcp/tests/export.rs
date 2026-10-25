//! The `export` tool: folders created on the way, PNG of a given width, pages named by a number written as text, with the real `inkboard-mcp` binary.

mod common;

use common::{Client, temp_dir};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn png_size(path: &Path) -> (u32, u32) {
    let png = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()))
}

/// A board with one rectangle of the given size.
fn board(c: &mut Client, dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let path = dir.join(name);
    let p = path.display().to_string();
    c.tool("create_board", json!({"path": p})).unwrap();
    c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "A", "x": 0, "y": 0, "w": w, "h": h})).unwrap();
    path
}

fn export(c: &mut Client, board: &Path, extra: Value) -> Result<Value, String> {
    let mut args = extra;
    args["path"] = json!(board.display().to_string());
    c.tool("export", args)
}

#[test]
fn missing_folders_of_the_output_are_created_under_the_root() {
    let dir = temp_dir("export-mkdir");
    let mut c = Client::start(&dir);
    let b = board(&mut c, &dir, "b.inkboard", 200, 100);
    let out = dir.join("out").join("deep").join("a.png");
    export(&mut c, &b, json!({"output": out.display().to_string()})).unwrap();
    assert!(png_size(&out).0 > 0);
    // a relative output is under the root too
    export(&mut c, &b, json!({"output": "rel/dir/b.svg"})).unwrap();
    assert!(dir.join("rel").join("dir").join("b.svg").exists());
}

#[test]
fn no_folder_is_created_outside_the_root() {
    let dir = temp_dir("export-escape");
    let mut c = Client::start(&dir);
    let b = board(&mut c, &dir, "b.inkboard", 200, 100);
    let outside = dir.parent().unwrap().join("export-escape-evil");
    for output in ["../export-escape-evil/a.png", "sub/../../export-escape-evil/a.png"] {
        let e = export(&mut c, &b, json!({"output": output})).unwrap_err();
        assert!(e.contains("outside the allowed root") || e.contains("invalid path") || e.contains("not accessible"), "{output}: {e}");
    }
    let e = export(&mut c, &b, json!({"output": outside.join("a.png").display().to_string()})).unwrap_err();
    assert!(e.contains("outside the allowed root"), "{e}");
    assert!(!outside.exists(), "a folder was created outside the root");
}

#[test]
fn a_png_can_be_given_a_width_whatever_the_size_of_the_diagram() {
    let dir = temp_dir("export-width");
    let mut c = Client::start(&dir);
    let small = board(&mut c, &dir, "small.inkboard", 100, 50);
    let large = board(&mut c, &dir, "large.inkboard", 3000, 900);
    for (b, name) in [(&small, "small.png"), (&large, "large.png")] {
        let out = dir.join(name);
        export(&mut c, b, json!({"output": out.display().to_string(), "width": 800})).unwrap();
        assert_eq!(png_size(&out).0, 800, "{name}");
    }
    let (_, h_small) = png_size(&dir.join("small.png"));
    let (_, h_large) = png_size(&dir.join("large.png"));
    assert!(h_large < 800 && h_small > 0, "the height follows the aspect ratio ({h_small}, {h_large})");
}

#[test]
fn a_png_scale_changes_the_size_and_the_default_is_unchanged() {
    let dir = temp_dir("export-scale");
    let mut c = Client::start(&dir);
    let b = board(&mut c, &dir, "b.inkboard", 400, 200);
    let size = |c: &mut Client, extra: Value, name: &str| {
        let out = dir.join(name);
        let mut args = extra;
        args["output"] = json!(out.display().to_string());
        export(c, &b, args).unwrap();
        png_size(&out)
    };
    let default = size(&mut c, json!({}), "default.png");
    let two = size(&mut c, json!({"scale": 2}), "two.png");
    let one = size(&mut c, json!({"scale": 1}), "one.png");
    assert_eq!(default, two, "the default scale is 2");
    assert!(one.0 * 2 >= two.0 - 2 && one.0 * 2 <= two.0 + 2, "{one:?} {two:?}");
}

#[test]
fn width_and_scale_are_checked() {
    let dir = temp_dir("export-bad");
    let mut c = Client::start(&dir);
    let b = board(&mut c, &dir, "b.inkboard", 200, 100);
    let png = dir.join("x.png").display().to_string();
    let svg = dir.join("x.svg").display().to_string();
    let cases = [
        (json!({"output": png, "width": 800, "scale": 2}), "not both"),
        (json!({"output": png, "width": 5}), "between"),
        (json!({"output": png, "width": 100_000}), "between"),
        (json!({"output": png, "width": "wide"}), "between"),
        (json!({"output": png, "scale": 0}), "between"),
        (json!({"output": png, "scale": 50}), "between"),
        (json!({"output": svg, "width": 800}), "only apply to a .png"),
    ];
    for (args, needle) in cases {
        let e = export(&mut c, &b, args.clone()).unwrap_err();
        assert!(e.contains(needle), "{args}: {e}");
    }
    assert!(!dir.join("x.png").exists() && !dir.join("x.svg").exists());
}

#[test]
fn a_page_number_written_as_text_selects_the_page() {
    let dir = temp_dir("export-page");
    let mut c = Client::start(&dir);
    let b = board(&mut c, &dir, "b.inkboard", 200, 100);
    let p = b.display().to_string();
    c.tool("add_page", json!({"path": p, "name": "second"})).unwrap();
    c.tool("add_shape", json!({"path": p, "page": 2, "geom": "rect", "text": "B", "x": 0, "y": 0, "w": 1000, "h": 500})).unwrap();
    let wide = |page: Value| {
        let out = dir.join("page.png");
        let mut c2 = Client::start(&dir);
        export(&mut c2, &b, json!({"output": out.display().to_string(), "page": page, "scale": 1})).unwrap();
        png_size(&out).0
    };
    assert_eq!(wide(json!(2)), wide(json!("2")), "\"2\" is page 2");
    assert_eq!(wide(json!("second")), wide(json!(2)), "a name still works");
    assert!(wide(json!(1)) < wide(json!("2")));
    let e = export(&mut c, &b, json!({"output": dir.join("n.png").display().to_string(), "page": "7"})).unwrap_err();
    assert!(e.contains("page not found"), "{e}");
}
