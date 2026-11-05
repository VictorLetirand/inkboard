//! `add_frame`, `arrange_objects` `grid` and the new `auto_layout` options (`gap_x`, `gap_y`, `ids`, icons, frames), with the real
//! `inkboard-mcp` binary.

#![allow(clippy::many_single_char_names)]

mod common;

use common::{Client, temp_dir};
use inkboard_core::{Align, BoxGeom, Kind, Object, VAlign, serial};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn start(tag: &str) -> (Client, PathBuf, String) {
    let dir = temp_dir(tag);
    let mut c = Client::start(&dir);
    let board = dir.join("f.inkboard");
    let p = board.display().to_string();
    c.tool("create_board", json!({"path": p})).unwrap();
    (c, board, p)
}

fn objects(board: &Path) -> Vec<Object> {
    serial::load(board).unwrap().pages[0].objects.clone()
}

fn object(board: &Path, id: &str) -> Object {
    objects(board).into_iter().find(|o| o.id.0 == id).expect("object")
}

fn z(board: &Path, id: &str) -> usize {
    objects(board).iter().position(|o| o.id.0 == id).expect("object")
}

/// (x, y, w, h) of an object.
fn bx(board: &Path, id: &str) -> (f64, f64, f64, f64) {
    let f = object(board, id).frame().unwrap();
    (f.x, f.y, f.w, f.h)
}

fn id_of(r: &Value) -> String {
    r["id"].as_str().unwrap().to_owned()
}

fn rect(c: &mut Client, p: &str, (x, y, w, h): (f64, f64, f64, f64)) -> String {
    id_of(&c.tool("add_shape", json!({"path": p, "geom": "rect", "x": x, "y": y, "w": w, "h": h})).unwrap())
}

fn link(c: &mut Client, p: &str, a: &str, b: &str) -> String {
    id_of(&c.tool("add_connector", json!({"path": p, "from": a, "to": b})).unwrap())
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

fn issue_kinds(c: &mut Client, p: &str) -> Vec<String> {
    let r = c.tool("check_layout", json!({"path": p})).unwrap();
    r["issues"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap().to_owned()).collect()
}

/* ---------------------------------------------------------------- add_frame */

#[test]
fn a_frame_is_one_rect_sized_to_the_union_with_the_title_inside() {
    let (mut c, board, p) = start("fr-basic");
    let a = rect(&mut c, &p, (100.0, 100.0, 100.0, 60.0));
    let b = rect(&mut c, &p, (300.0, 200.0, 100.0, 60.0));
    let r = c.tool("add_frame", json!({"path": p, "ids": [a, b], "title": "Zone"})).unwrap();
    let id = id_of(&r);
    // the boxes include half the outline (2): union x 99..401, y 99..261; padding 40 on every side, the title adds room on top
    let (x, y, w, h) = bx(&board, &id);
    assert!(near(x, 59.0) && near(w, 382.0), "{r}");
    assert!(near(y + h, 301.0), "bottom = 261 + 40: {r}");
    let top = 59.0 - y;
    assert!((30.0..=60.0).contains(&top), "title line (18 * 1.3) + 12 above the padding: {top}");
    assert_eq!([r["x"].as_f64(), r["w"].as_f64()], [Some(x.round()), Some(w.round())]);
    let o = object(&board, &id);
    let Kind::Box(bo) = &o.kind else { panic!("a box") };
    assert_eq!(bo.geom, BoxGeom::Rect);
    assert_eq!((bo.label.align, bo.label.valign), (Align::Left, VAlign::Top));
    assert_eq!(bo.label.text.to_plain(), "Zone");
    assert!(bo.label.text.0.iter().all(|s| s.style.bold) && (bo.label.size - 18.0).abs() < 1e-9);
    assert!(o.group.is_none(), "not grouped with its content");
    assert!(z(&board, &id) < z(&board, &a) && z(&board, &id) < z(&board, &b), "behind the content");
    assert_eq!(objects(&board).len(), 3, "one object: the title is not a separate text");
}

#[test]
fn frame_options_padding_style_and_title_font() {
    let (mut c, board, p) = start("fr-options");
    let a = rect(&mut c, &p, (100.0, 100.0, 100.0, 60.0));
    let r = c.tool("add_frame", json!({"path": p, "ids": [a], "padding": 10})).unwrap();
    assert_eq!(
        [r["x"].as_f64(), r["y"].as_f64(), r["w"].as_f64(), r["h"].as_f64()],
        [Some(89.0), Some(89.0), Some(122.0), Some(82.0)],
        "no title: padding only"
    );
    let o = object(&board, &id_of(&r));
    assert_eq!((o.style.fill.map(inkboard_core::Color::to_hex), o.style.stroke.to_hex()), (Some("#f1f3f5".to_owned()), "#adb5bd".to_owned()));
    assert_eq!(o.style.dash, inkboard_core::Dash::Solid);
    let r2 = c
        .tool(
            "add_frame",
            json!({"path": p, "ids": [a], "padding": 10, "title": "Big", "title_font_size": 40, "title_bold": false, "fill": "#ffffff", "stroke": "#ff0000",
                "dash": "dashed", "roundness": 0.5}),
        )
        .unwrap();
    assert!(r2["h"].as_f64().unwrap() > r["h"].as_f64().unwrap() + 52.0, "a 40 px title takes 52 + 12 more on top: {r2}");
    let o = object(&board, &id_of(&r2));
    let Kind::Box(bo) = &o.kind else { panic!() };
    assert!(bo.label.text.0.iter().all(|s| !s.style.bold) && (bo.label.size - 40.0).abs() < 1e-9);
    assert_eq!(
        (o.style.fill.map(inkboard_core::Color::to_hex), o.style.stroke.to_hex(), o.style.dash),
        (Some("#ffffff".to_owned()), "#ff0000".to_owned(), inkboard_core::Dash::Dashed)
    );
    assert!((f64::from(o.style.roundness) - 0.5).abs() < 1e-6);
}

#[test]
fn a_title_wider_than_the_content_widens_the_frame() {
    let (mut c, board, p) = start("fr-title-wide");
    let a = rect(&mut c, &p, (0.0, 0.0, 40.0, 40.0));
    let r = c.tool("add_frame", json!({"path": p, "ids": [a], "padding": 5, "title": "A rather long title for such a small group of objects"})).unwrap();
    assert!(r["w"].as_f64().unwrap() > 300.0, "{r}");
    assert!(r["warnings"].to_string().contains("widened"), "{r}");
    let (_, _, w, h) = bx(&board, &id_of(&r));
    assert!(h < 120.0, "one line, not wrapped: {h} (w {w})");
}

#[test]
fn an_icon_is_framed_with_its_label() {
    let (mut c, board, p) = start("fr-icon");
    let icon = c.tool("search_icons", json!({"path": p, "query": "router", "limit": 1})).unwrap()["icons"][0]["id"].as_str().unwrap().to_owned();
    let r = c.tool("add_icon", json!({"path": p, "icon": icon, "x": 100, "y": 100, "size": 64, "label": "Core router"})).unwrap();
    let (icon_id, label_id) = (id_of(&r), r["label_id"].as_str().unwrap().to_owned());
    let f = c.tool("add_frame", json!({"path": p, "ids": [icon_id], "padding": 20})).unwrap();
    let (_, y, _, h) = bx(&board, &id_of(&f));
    let (_, ly, _, lh) = bx(&board, &label_id);
    assert!(y + h >= ly + lh + 20.0 - 0.01, "the frame goes down to the label plus the padding: {f}");
    assert!(z(&board, &id_of(&f)) < z(&board, &icon_id));
}

#[test]
fn a_frame_inside_a_frame_is_above_the_outer_one_and_below_its_content() {
    let (mut c, board, p) = start("fr-nested");
    let a = rect(&mut c, &p, (100.0, 100.0, 80.0, 50.0));
    let b = rect(&mut c, &p, (220.0, 100.0, 80.0, 50.0));
    let d = rect(&mut c, &p, (400.0, 100.0, 80.0, 50.0));
    let outer = id_of(&c.tool("add_frame", json!({"path": p, "ids": [a, b, d], "title": "Outer"})).unwrap());
    let inner = id_of(&c.tool("add_frame", json!({"path": p, "ids": [a, b], "title": "Inner", "padding": 20})).unwrap());
    let (zo, zi) = (z(&board, &outer), z(&board, &inner));
    assert!(zo < zi && zi < z(&board, &a) && zi < z(&board, &b), "outer {zo} < inner {zi} < content");
    // a frame around a frame goes under it
    let around = id_of(&c.tool("add_frame", json!({"path": p, "ids": [outer], "padding": 10})).unwrap());
    assert!(z(&board, &around) < z(&board, &outer));
}

#[test]
fn a_frame_is_refitted_after_the_content_moved() {
    let (mut c, board, p) = start("fr-refit");
    let a = rect(&mut c, &p, (100.0, 100.0, 100.0, 60.0));
    let b = rect(&mut c, &p, (300.0, 100.0, 100.0, 60.0));
    let id = id_of(&c.tool("add_frame", json!({"path": p, "ids": [a, b], "title": "Keep me"})).unwrap());
    let before = bx(&board, &id);
    c.tool("move_objects", json!({"path": p, "ids": [b], "dx": 500, "dy": 100})).unwrap();
    assert!(near(bx(&board, &id).2, before.2), "left behind");
    let r = c.tool("add_frame", json!({"path": p, "frame": id, "ids": [a, b]})).unwrap();
    assert_eq!(id_of(&r), id);
    let after = bx(&board, &id);
    assert!(near(after.0, before.0) && near(after.2, before.2 + 500.0) && after.3 > before.3 + 99.0, "{before:?} -> {after:?}");
    assert_eq!(objects(&board).len(), 3, "no new frame");
    let Kind::Box(bo) = &object(&board, &id).kind else { panic!() };
    assert_eq!(bo.label.text.to_plain(), "Keep me", "title kept");
    assert!(z(&board, &id) < z(&board, &a));
    // changing the title and the colors at the same time
    c.tool("add_frame", json!({"path": p, "frame": id, "ids": [a, b], "title": "New", "fill": "#ffec99"})).unwrap();
    let o = object(&board, &id);
    assert_eq!(o.style.fill.map(inkboard_core::Color::to_hex), Some("#ffec99".to_owned()));
    // the frame has to be a rect
    let t = id_of(&c.tool("add_shape", json!({"path": p, "geom": "ellipse", "x": 0, "y": 0})).unwrap());
    assert!(c.tool("add_frame", json!({"path": p, "frame": t, "ids": [a]})).unwrap_err().contains("rect"));
}

#[test]
fn an_exact_rectangle_goes_to_the_back_and_bad_arguments_are_refused() {
    let (mut c, board, p) = start("fr-errors");
    let a = rect(&mut c, &p, (100.0, 100.0, 100.0, 60.0));
    let r = c.tool("add_frame", json!({"path": p, "x": 0, "y": 0, "w": 500, "h": 400, "title": "Page"})).unwrap();
    assert_eq!(z(&board, &id_of(&r)), 0);
    assert_eq!(bx(&board, &id_of(&r)), (0.0, 0.0, 500.0, 400.0));
    let r = c.tool("add_frame", json!({"path": p, "x": 0, "y": 0, "w": 500, "h": 400, "ids": [a]})).unwrap();
    assert!(z(&board, &id_of(&r)) < z(&board, &a), "stacked under the ids");
    let n = objects(&board).len();
    for (args, needle) in [
        (json!({"path": p}), "ids"),
        (json!({"path": p, "ids": ["ghost"]}), "ghost"),
        (json!({"path": p, "ids": [a], "padding": -5}), "padding"),
        (json!({"path": p, "ids": [a], "padding": "wide"}), "padding"),
        (json!({"path": p, "ids": [a], "fill": "red"}), "color"),
        (json!({"path": p, "ids": [a], "dash": "wavy"}), "dash"),
        (json!({"path": p, "ids": [a], "title_font_size": 1}), "title_font_size"),
        (json!({"path": p, "ids": [a], "title_bold": "yes"}), "title_bold"),
        (json!({"path": p, "x": 1, "y": 2}), "together"),
        (json!({"path": p, "x": 1, "y": 2, "w": 0, "h": 5}), "\"w\""),
        (json!({"path": p, "frame": "ghost", "ids": [a]}), "ghost"),
        (json!({"path": p, "frame": a}), "re-fitting"),
    ] {
        let e = c.tool("add_frame", args.clone()).unwrap_err();
        assert!(e.contains(needle), "{args}: {e}");
    }
    assert_eq!(objects(&board).len(), n, "nothing was added by the refused calls");
}

/* --------------------------------------------------------------------- grid */

/// Four shapes (100x60, 140x80, 100x60, 100x60) scattered on the page.
fn four(c: &mut Client, p: &str) -> Vec<String> {
    [(500.0, 400.0, 100.0, 60.0), (30.0, 700.0, 140.0, 80.0), (900.0, 50.0, 100.0, 60.0), (10.0, 10.0, 100.0, 60.0)].iter().map(|r| rect(c, p, *r)).collect()
}

fn grid(c: &mut Client, p: &str, ids: &[String], extra: &Value) -> Value {
    let mut a = json!({"path": p, "ids": ids, "action": "grid"});
    for (k, v) in extra.as_object().unwrap() {
        a[k] = v.clone();
    }
    c.tool("arrange_objects", a).unwrap()
}

fn positions(board: &Path, ids: &[String]) -> Vec<(f64, f64)> {
    ids.iter().map(|i| (bx(board, i).0, bx(board, i).1)).collect()
}

#[test]
fn grid_with_uniform_cells_centers_every_object_in_its_cell() {
    let (mut c, board, p) = start("grid-max");
    let ids = four(&mut c, &p);
    let r = grid(&mut c, &p, &ids, &json!({"columns": 2, "gap_x": 20, "gap_y": 20, "origin": {"x": 0, "y": 0}}));
    assert_eq!((r["rows"].as_u64(), r["columns"].as_u64()), (Some(2), Some(2)));
    // cell 140 x 80; item 0 is 100 x 60: centered at (70, 40)
    assert_eq!(positions(&board, &ids), [(20.0, 10.0), (160.0, 0.0), (20.0, 110.0), (180.0, 110.0)]);
    assert_eq!(r["placed"].as_array().unwrap().len(), 4);
}

#[test]
fn grid_start_alignment_and_default_origin_gaps() {
    let (mut c, board, p) = start("grid-start");
    let ids = four(&mut c, &p);
    grid(&mut c, &p, &ids, &json!({"columns": 2, "align": "start"}));
    // default origin: top-left of the union (10, 10); default gaps 80; cell 140 x 80
    assert_eq!(positions(&board, &ids), [(10.0, 10.0), (230.0, 10.0), (10.0, 170.0), (230.0, 170.0)]);
}

#[test]
fn grid_fit_sizes_each_column_and_row_to_its_largest_member() {
    let (mut c, board, p) = start("grid-fit");
    let ids = four(&mut c, &p);
    grid(&mut c, &p, &ids, &json!({"columns": 2, "cell": "fit", "gap_x": 20, "gap_y": 20, "origin": {"x": 0, "y": 0}}));
    // columns 100 and 140 wide, rows 80 and 60 tall
    assert_eq!(positions(&board, &ids), [(0.0, 10.0), (120.0, 0.0), (0.0, 100.0), (140.0, 100.0)]);
    // order of ids is the order of the cells: reversed ids reverse the grid
    let rev: Vec<String> = ids.iter().rev().cloned().collect();
    grid(&mut c, &p, &rev, &json!({"columns": 4, "cell": "fit", "gap_x": 0, "gap_y": 0, "origin": {"x": 0, "y": 0}, "align": "start"}));
    assert_eq!(positions(&board, &rev), [(0.0, 0.0), (100.0, 0.0), (200.0, 0.0), (340.0, 0.0)]);
}

#[test]
fn grid_moves_an_icon_with_its_label_and_connectors_follow() {
    let (mut c, board, p) = start("grid-group");
    let icon = c.tool("search_icons", json!({"path": p, "query": "router", "limit": 1})).unwrap()["icons"][0]["id"].as_str().unwrap().to_owned();
    let r = c.tool("add_icon", json!({"path": p, "icon": icon, "x": 700, "y": 300, "size": 64, "label": "R1"})).unwrap();
    let (icon_id, label_id) = (id_of(&r), r["label_id"].as_str().unwrap().to_owned());
    let a = rect(&mut c, &p, (0.0, 0.0, 100.0, 60.0));
    let k = link(&mut c, &p, &a, &icon_id);
    let offset = (bx(&board, &label_id).0 - bx(&board, &icon_id).0, bx(&board, &label_id).1 - bx(&board, &icon_id).1);
    // the label is listed too (counts once), the connector is skipped
    let r = grid(
        &mut c,
        &p,
        &[icon_id.clone(), label_id.clone(), a.clone(), k.clone()],
        &json!({"columns": 2, "origin": {"x": 0, "y": 0}, "align": "start", "cell": "fit", "gap_x": 10}),
    );
    assert_eq!(r["skipped_connectors"], json!([k]));
    assert_eq!(r["placed"].as_array().unwrap().len(), 2);
    // the group's box (icon + label) is the first cell
    let (ix, iy, iw, _) = bx(&board, &icon_id);
    let (lx, ly, lw, lh) = bx(&board, &label_id);
    assert!((ix.min(lx)).abs() < 0.01 && iy.min(ly).abs() < 0.01, "top-left of the group at the origin: icon {ix},{iy} label {lx},{ly}");
    assert!(near(lx - ix, offset.0) && near(ly - iy, offset.1), "the label keeps its place");
    let group_w = (ix + iw).max(lx + lw) - ix.min(lx);
    let group_h = (iy + bx(&board, &icon_id).3).max(ly + lh) - iy.min(ly);
    assert!(near(bx(&board, &a).0, group_w + 10.0), "second column after the group's width + gap");
    assert!(group_h > 64.0, "the cell is as tall as icon + label");
    let Kind::Connector(conn) = &object(&board, &k).kind else { panic!() };
    assert!(conn.start.target().is_some() && conn.end.target().is_some(), "still bound");
    assert!(!issue_kinds(&mut c, &p).contains(&"unbound_connector".to_owned()));
}

#[test]
fn grid_refuses_bad_arguments_and_skips_locked_objects() {
    let (mut c, board, p) = start("grid-errors");
    let ids = four(&mut c, &p);
    let before = positions(&board, &ids);
    for (extra, needle) in [
        (json!({}), "columns"),
        (json!({"columns": 0}), "columns"),
        (json!({"columns": 2.5}), "columns"),
        (json!({"columns": 2, "gap_x": -1}), "gap_x"),
        (json!({"columns": 2, "gap_y": "big"}), "gap_y"),
        (json!({"columns": 2, "cell": "huge"}), "cell"),
        (json!({"columns": 2, "align": "end"}), "align"),
        (json!({"columns": 2, "origin": {"x": 1}}), "origin"),
    ] {
        let mut a = json!({"path": p, "ids": ids, "action": "grid"});
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        let e = c.tool("arrange_objects", a).unwrap_err();
        assert!(e.contains(needle), "{extra}: {e}");
    }
    assert_eq!(positions(&board, &ids), before);
    c.tool("arrange_objects", json!({"path": p, "ids": [ids[0]], "action": "lock"})).unwrap();
    let r = grid(&mut c, &p, &ids, &json!({"columns": 3, "origin": {"x": 0, "y": 0}, "align": "start"}));
    assert_eq!(r["skipped_locked"], json!([ids[0]]));
    assert_eq!(positions(&board, &ids)[0], before[0], "the locked object stays");
    assert_eq!(positions(&board, &ids)[1].1, 0.0);
}

/* -------------------------------------------------------------- auto_layout */

#[test]
fn auto_layout_gaps_set_the_room_between_layers_and_nodes() {
    let (mut c, board, p) = start("al-gaps");
    let a = rect(&mut c, &p, (0.0, 0.0, 100.0, 60.0));
    let b = rect(&mut c, &p, (500.0, 0.0, 100.0, 60.0));
    let d = rect(&mut c, &p, (900.0, 0.0, 100.0, 60.0));
    link(&mut c, &p, &a, &b);
    link(&mut c, &p, &a, &d);
    let centers = |ids: &[&String]| ids.iter().map(|i| inkboard_core::Object::frame(&object(&board, i)).unwrap().center()).collect::<Vec<_>>();
    c.tool("auto_layout", json!({"path": p})).unwrap();
    let base = centers(&[&a, &b, &d]);
    assert!(near(base[1].y - base[0].y, 60.0 + 84.0) && near(base[2].x - base[1].x, 100.0 + 48.0), "defaults unchanged: {base:?}");
    c.tool("auto_layout", json!({"path": p, "gap_x": 200, "gap_y": 150})).unwrap();
    let wide = centers(&[&a, &b, &d]);
    assert!(near(wide[1].y - wide[0].y, 60.0 + 150.0) && near(wide[2].x - wide[1].x, 100.0 + 200.0), "TB: gap_y between layers, gap_x between nodes: {wide:?}");
    c.tool("auto_layout", json!({"path": p, "direction": "LR", "gap_x": 200, "gap_y": 150})).unwrap();
    let lr = centers(&[&a, &b, &d]);
    assert!(near(lr[1].x - lr[0].x, 100.0 + 200.0) && near(lr[2].y - lr[1].y, 60.0 + 150.0), "LR: gap_x between layers, gap_y between nodes: {lr:?}");
    for (extra, needle) in [(json!({"gap_x": -1}), "gap_x"), (json!({"gap_y": "a"}), "gap_y"), (json!({"ids": ["ghost"]}), "ghost")] {
        let mut args = json!({"path": p});
        for (k, v) in extra.as_object().unwrap() {
            args[k] = v.clone();
        }
        assert!(c.tool("auto_layout", args).unwrap_err().contains(needle));
    }
}

#[test]
fn auto_layout_with_ids_leaves_the_other_objects_alone() {
    let (mut c, board, p) = start("al-ids");
    let [a, b, d, e] = [(0.0, 0.0), (300.0, 40.0), (50.0, 500.0), (700.0, 520.0)].map(|(x, y)| rect(&mut c, &p, (x, y, 100.0, 60.0)));
    let k1 = link(&mut c, &p, &a, &b);
    let k2 = link(&mut c, &p, &d, &e);
    let via2 = |board: &Path| if let Kind::Connector(k) = &object(board, &k2).kind { k.via.clone() } else { panic!() };
    let (d0, e0, v0) = (bx(&board, &d), bx(&board, &e), via2(&board));
    let r = c.tool("auto_layout", json!({"path": p, "ids": [a, b]})).unwrap();
    assert_eq!(r["nodes"], 2);
    let (fa, fb) = (bx(&board, &a), bx(&board, &b));
    assert!(near(fa.0, fb.0) && fb.1 > fa.1, "a and b are in a column now: {fa:?} {fb:?}");
    assert_eq!((bx(&board, &d), bx(&board, &e), via2(&board)), (d0, e0, v0), "the other graph is untouched");
    let _ = k1;
}

#[test]
fn auto_layout_treats_an_icon_and_its_label_as_one_node() {
    let (mut c, board, p) = start("al-icons");
    let icon = c.tool("search_icons", json!({"path": p, "query": "router", "limit": 1})).unwrap()["icons"][0]["id"].as_str().unwrap().to_owned();
    let mut ids = Vec::new();
    for (x, y) in [(0.0, 0.0), (300.0, 300.0), (50.0, 700.0)] {
        let r = c.tool("add_icon", json!({"path": p, "icon": icon, "x": x, "y": y, "size": 64, "label": "A router with a label"})).unwrap();
        ids.push((id_of(&r), r["label_id"].as_str().unwrap().to_owned()));
    }
    link(&mut c, &p, &ids[0].0, &ids[1].0);
    link(&mut c, &p, &ids[0].0, &ids[2].0);
    let offsets = |board: &Path| ids.iter().map(|(i, l)| (bx(board, l).0 - bx(board, i).0, bx(board, l).1 - bx(board, i).1)).collect::<Vec<_>>();
    let before = offsets(&board);
    let r = c.tool("auto_layout", json!({"path": p, "direction": "LR"})).unwrap();
    assert_eq!(r["nodes"], 3, "icon + label counts once: {r}");
    let after = offsets(&board);
    for (b, a) in before.iter().zip(&after) {
        assert!(near(b.0, a.0) && near(b.1, a.1), "label kept its place: {before:?} {after:?}");
    }
    let kinds = issue_kinds(&mut c, &p);
    assert!(!kinds.contains(&"overlap".to_owned()), "{kinds:?}");
    let (x0, x1) = (bx(&board, &ids[0].0).0, bx(&board, &ids[1].0).0);
    assert!(x1 > x0, "left to right");
}

#[test]
fn auto_layout_lays_a_frame_out_as_a_cluster_around_its_content() {
    let (mut c, board, p) = start("al-frames");
    let [a, b, d, e] = [(100.0, 100.0), (500.0, 700.0), (50.0, 900.0), (800.0, 40.0)].map(|(x, y)| rect(&mut c, &p, (x, y, 100.0, 60.0)));
    // a frame around a and b (it holds nothing else); d and e are outside
    let f = id_of(&c.tool("add_frame", json!({"path": p, "ids": [a, b], "title": "Cluster"})).unwrap());
    link(&mut c, &p, &a, &b);
    link(&mut c, &p, &b, &d);
    link(&mut c, &p, &e, &a);
    let r = c.tool("auto_layout", json!({"path": p})).unwrap();
    assert_eq!((r["nodes"].as_u64(), r["frames"].as_u64()), (Some(4), Some(1)), "{r}");
    let fr = bx(&board, &f);
    let inside = |id: &str| {
        let r = bx(&board, id);
        r.0 >= fr.0 && r.1 >= fr.1 && r.0 + r.2 <= fr.0 + fr.2 && r.1 + r.3 <= fr.1 + fr.3
    };
    assert!(inside(&a) && inside(&b), "the frame holds its content: {fr:?}");
    assert!(!inside(&d) && !inside(&e), "and nothing else");
    let kinds = issue_kinds(&mut c, &p);
    assert!(!kinds.contains(&"overlap".to_owned()), "no overlap after the layout: {kinds:?}");
    // title room above the content
    let (fa, fb) = (bx(&board, &a), bx(&board, &b));
    assert!(fa.1.min(fb.1) - fr.1 >= 40.0 + 23.0, "padding + title above the content: {fr:?} {fa:?}");
    assert!(z(&board, &f) < z(&board, &a), "frame stays behind");
    // gaps apply inside and outside
    c.tool("auto_layout", json!({"path": p, "gap_y": 200})).unwrap();
    let kinds = issue_kinds(&mut c, &p);
    assert!(!kinds.contains(&"overlap".to_owned()), "{kinds:?}");
}

#[test]
fn auto_layout_nests_frames() {
    let (mut c, board, p) = start("al-nested");
    let [a, b, d] = [(100.0, 100.0), (300.0, 100.0), (700.0, 100.0)].map(|(x, y)| rect(&mut c, &p, (x, y, 100.0, 60.0)));
    let outer = id_of(&c.tool("add_frame", json!({"path": p, "ids": [a, b, d], "title": "Outer"})).unwrap());
    let inner = id_of(&c.tool("add_frame", json!({"path": p, "ids": [a, b], "title": "Inner", "padding": 20})).unwrap());
    link(&mut c, &p, &a, &b);
    link(&mut c, &p, &b, &d);
    let r = c.tool("auto_layout", json!({"path": p})).unwrap();
    assert_eq!(r["frames"], 2, "{r}");
    let (o, i) = (bx(&board, &outer), bx(&board, &inner));
    let within = |inn: (f64, f64, f64, f64), out: (f64, f64, f64, f64)| {
        inn.0 >= out.0 && inn.1 >= out.1 && inn.0 + inn.2 <= out.0 + out.2 && inn.1 + inn.3 <= out.1 + out.3
    };
    assert!(within(i, o) && within(bx(&board, &a), i) && within(bx(&board, &b), i) && within(bx(&board, &d), o) && !within(bx(&board, &d), i));
    assert!(!issue_kinds(&mut c, &p).contains(&"overlap".to_owned()));
}
