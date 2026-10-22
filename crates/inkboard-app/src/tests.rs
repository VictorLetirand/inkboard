//! UI tests: we simulate the mouse and keyboard in a real egui cycle (without a window).

use crate::app::App;
use crate::tools::Tool;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use inkboard_core::*;

pub(super) struct T {
    pub(super) h: Harness<'static, App>,
}

impl T {
    pub(super) fn new() -> Self {
        let mut h = Harness::builder().with_size(vec2(1300.0, 850.0)).build_eframe(|cc| {
            let mut app = App::build(cc, None, None, false);
            app.system_image = || None; // never the real clipboard in tests
            app
        });
        h.step();
        h.step();
        let mut t = T { h };
        t.app().fit_pending = false;
        t.app().set_view(View { x: 300.0, y: 150.0, zoom: 1.0 }); // outside the floating cards (title, tools, panel)
        t.h.step();
        t
    }
    /// Switches tool then lets the style panel (a floating card) settle: its first frames cover too wide an area.
    pub(super) fn tool(&mut self, tool: Tool) {
        self.app().set_tool(tool);
        for _ in 0..3 {
            self.h.step();
        }
    }
    pub(super) fn app(&mut self) -> &mut App {
        self.h.state_mut()
    }
    /// Screen position of a world point (view at scale 1, canvas origin).
    pub(super) fn at(&mut self, x: f64, y: f64) -> Pos2 {
        let r = self.app().canvas_rect;
        let v = self.app().ed.page().view;
        pos2(r.min.x + (x * v.zoom + v.x) as f32, r.min.y + (y * v.zoom + v.y) as f32)
    }
    pub(super) fn mv(&mut self, p: Pos2) {
        self.h.event(Event::PointerMoved(p));
        self.h.step();
    }
    pub(super) fn button(&mut self, p: Pos2, pressed: bool, modifiers: Modifiers) {
        self.h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers });
        self.h.step();
    }
    pub(super) fn drag_mods(&mut self, from: (f64, f64), to: (f64, f64), mods: Modifiers) {
        self.h.input_mut().modifiers = mods;
        let a = self.at(from.0, from.1);
        let b = self.at(to.0, to.1);
        self.mv(a);
        self.button(a, true, mods);
        for k in 1..=4 {
            let t = k as f32 / 4.0;
            self.mv(a.lerp(b, t));
        }
        self.button(b, false, mods);
        self.h.input_mut().modifiers = Modifiers::NONE;
        self.mv(b);
    }
    pub(super) fn drag(&mut self, from: (f64, f64), to: (f64, f64)) {
        self.drag_mods(from, to, Modifiers::NONE);
    }
    pub(super) fn click(&mut self, at: (f64, f64)) {
        let p = self.at(at.0, at.1);
        self.mv(p);
        self.button(p, true, Modifiers::NONE);
        self.button(p, false, Modifiers::NONE);
    }
    pub(super) fn objects(&mut self) -> Vec<Object> {
        self.app().ed.page().objects.clone()
    }
    pub(super) fn boxes(&mut self) -> Vec<(Object, BoxObj)> {
        self.objects().into_iter().filter_map(|o| if let Kind::Box(b) = o.kind.clone() { Some((o, b)) } else { None }).collect()
    }
    pub(super) fn add_rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> Id {
        let o = Object::new_box(BoxGeom::Rect, Frame::new(x, y, w, h), Style { fill: Some(Color::rgb(200, 220, 255)), ..Style::default() }, Label::default());
        self.app().ed.insert(o)
    }
    pub(super) fn frame(&mut self, id: &Id) -> Frame {
        self.app().ed.page().get(id).and_then(Object::frame).expect("object with a frame")
    }
    pub(super) fn key(&mut self, k: Key) {
        self.h.key_press(k);
        self.h.step();
        self.h.step();
    }
    pub(super) fn key_mod(&mut self, m: Modifiers, k: Key) {
        self.h.key_press_modifiers(m, k);
        self.h.step();
        self.h.step();
    }
}

#[test]
fn app_starts_with_an_empty_page() {
    let mut t = T::new();
    assert!(t.objects().is_empty());
    assert!(t.app().canvas_rect.width() > 500.0, "the canvas fills the window");
}

#[test]
fn dragging_with_rect_tool_creates_a_rectangle_and_returns_to_select() {
    let mut t = T::new();
    t.tool(Tool::Shape(BoxGeom::Rect));
    t.drag((100.0, 100.0), (300.0, 220.0));
    let b = t.boxes();
    assert_eq!(b.len(), 1);
    let f = b[0].1.frame;
    assert!((f.x - 100.0).abs() < 1.0 && (f.y - 100.0).abs() < 1.0 && (f.w - 200.0).abs() < 1.0 && (f.h - 120.0).abs() < 1.0, "{f:?}");
    assert_eq!(t.app().tool, Tool::Select);
    assert_eq!(t.app().sel.len(), 1);
}

#[test]
fn clicking_with_a_shape_tool_creates_a_default_sized_shape() {
    let mut t = T::new();
    t.tool(Tool::Shape(BoxGeom::Ellipse));
    t.click((400.0, 300.0));
    let b = t.boxes();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].1.geom, BoxGeom::Ellipse);
    assert!((b[0].1.frame.center().x - 400.0).abs() < 1.0 && (b[0].1.frame.w - 150.0).abs() < 1.0);
}

#[test]
fn shift_makes_a_square_and_undo_removes_it() {
    let mut t = T::new();
    t.tool(Tool::Shape(BoxGeom::Rect));
    t.drag_mods((100.0, 100.0), (300.0, 160.0), Modifiers::SHIFT);
    let f = t.boxes()[0].1.frame;
    assert!((f.w - f.h).abs() < 1.0, "square: {f:?}");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(t.objects().is_empty());
    t.key_mod(Modifiers::COMMAND, Key::Y);
    assert_eq!(t.objects().len(), 1);
}

#[test]
fn pen_draws_a_stroke_with_several_points() {
    let mut t = T::new();
    t.tool(Tool::Pen);
    t.drag((100.0, 100.0), (300.0, 160.0));
    let objs = t.objects();
    assert_eq!(objs.len(), 1);
    let Kind::Pen(p) = &objs[0].kind else { panic!("not a stroke") };
    assert!(p.pts.len() >= 2 && !p.highlighter);
    assert!(p.pts[0].dist(Pt::new(100.0, 100.0)) < 2.0);
}

#[test]
fn highlighter_is_translucent() {
    let mut t = T::new();
    t.tool(Tool::Highlighter);
    t.drag((100.0, 100.0), (300.0, 100.0));
    let o = &t.objects()[0];
    assert!(matches!(&o.kind, Kind::Pen(p) if p.highlighter));
    assert!(o.style.opacity < 0.6 && o.style.width >= 10.0);
}

#[test]
fn arrow_tool_binds_both_ends_to_shapes() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 120.0, 80.0);
    let b = t.add_rect(400.0, 100.0, 120.0, 80.0);
    t.tool(Tool::Arrow);
    t.drag((160.0, 140.0), (460.0, 140.0));
    let conns: Vec<Object> = t.objects().into_iter().filter(|o| matches!(o.kind, Kind::Connector(_))).collect();
    assert_eq!(conns.len(), 1);
    let Kind::Connector(c) = &conns[0].kind else { unreachable!() };
    assert_eq!(c.start.target(), Some(&a));
    assert_eq!(c.end.target(), Some(&b));
    assert_eq!(c.head_end, Arrowhead::Triangle);
    let path = t.app().ed.page().connector_path(c).unwrap();
    assert!((path[0].x - 220.0).abs() < 1.0 && (path.last().unwrap().x - 400.0).abs() < 1.0, "{path:?}");
}

#[test]
fn arrow_ending_on_empty_space_is_free() {
    let mut t = T::new();
    t.add_rect(100.0, 100.0, 120.0, 80.0);
    t.tool(Tool::Line);
    t.drag((160.0, 140.0), (600.0, 500.0));
    let Some(Object { kind: Kind::Connector(c), .. }) = t.objects().into_iter().find(|o| matches!(o.kind, Kind::Connector(_))) else { panic!("no connector") };
    assert!(c.start.target().is_some() && matches!(c.end, End::Free { .. }));
    assert_eq!(c.head_end, Arrowhead::None);
}

#[test]
fn moving_a_shape_drags_its_arrow_along_and_undo_restores_both() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 120.0, 80.0);
    let b = t.add_rect(400.0, 100.0, 120.0, 80.0);
    let c = t.app().ed.connect(&a, &b);
    let path_of = |t: &mut T, id: &Id| {
        let page = t.app().ed.page();
        let Kind::Connector(cn) = &page.get(id).unwrap().kind else { unreachable!() };
        page.connector_path(cn).unwrap()
    };
    let before = path_of(&mut t, &c);
    t.tool(Tool::Select);
    t.drag((410.0, 110.0), (410.0, 310.0));
    let fb = t.frame(&b);
    assert!((fb.y - 300.0).abs() < 1.5, "{fb:?}");
    let after = path_of(&mut t, &c);
    assert!(after.last().unwrap().y > before.last().unwrap().y + 100.0, "the arrow follows the shape");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!((t.frame(&b).y - 100.0).abs() < 1e-9, "one gesture = one undo");
}

#[test]
fn marquee_selects_enclosed_shapes_only() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 80.0, 60.0);
    let b = t.add_rect(220.0, 100.0, 80.0, 60.0);
    let far = t.add_rect(800.0, 500.0, 80.0, 60.0);
    t.drag((60.0, 60.0), (340.0, 200.0));
    let sel = t.app().sel.clone();
    assert!(sel.contains(&a) && sel.contains(&b) && !sel.contains(&far), "{sel:?}");
}

#[test]
fn resize_handle_changes_size_and_is_one_undo_step() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 120.0, 80.0);
    t.app().sel = vec![a.clone()];
    t.h.step();
    t.drag((220.0, 180.0), (320.0, 280.0)); // south-east handle
    let f = t.frame(&a);
    assert!((f.w - 220.0).abs() < 1.5 && (f.h - 180.0).abs() < 1.5, "{f:?}");
    assert!((f.x - 100.0).abs() < 1e-6 && (f.y - 100.0).abs() < 1e-6, "the opposite corner does not move");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!((t.frame(&a).w - 120.0).abs() < 1e-9);
}

#[test]
fn rotation_handle_rotates_the_shape() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 120.0, 80.0);
    t.app().sel = vec![a.clone()];
    t.h.step();
    // rotation handle: 28 px above the top edge, in the middle (160, 100 - 28)
    t.drag((160.0, 72.0), (400.0, 140.0));
    let f = t.frame(&a);
    assert!((f.angle - std::f64::consts::FRAC_PI_2).abs() < 0.12, "angle = {}", f.angle);
}

#[test]
fn delete_and_undo_keep_connectors_consistent() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 120.0, 80.0);
    let b = t.add_rect(400.0, 100.0, 120.0, 80.0);
    let c = t.app().ed.connect(&a, &b);
    t.app().sel = vec![a.clone()];
    t.key(Key::Delete);
    assert!(t.app().ed.page().get(&a).is_none());
    let Kind::Connector(cn) = &t.app().ed.page().get(&c).unwrap().kind else { unreachable!() };
    assert!(matches!(cn.start, End::Free { .. }), "the arrow is detached, not deleted");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(t.app().ed.page().get(&a).is_some());
    let Kind::Connector(cn) = &t.app().ed.page().get(&c).unwrap().kind else { unreachable!() };
    assert!(matches!(cn.start, End::Bound { .. }));
}

#[test]
fn eraser_removes_what_it_touches() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 120.0, 80.0);
    let b = t.add_rect(500.0, 100.0, 120.0, 80.0);
    t.tool(Tool::Eraser);
    t.drag((90.0, 140.0), (240.0, 140.0));
    assert!(t.app().ed.page().get(&a).is_none());
    assert!(t.app().ed.page().get(&b).is_some());
}

#[test]
fn alt_drag_duplicates_instead_of_moving() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 120.0, 80.0);
    t.drag_mods((110.0, 110.0), (310.0, 110.0), Modifiers::ALT);
    assert_eq!(t.objects().len(), 2);
    assert!((t.frame(&a).x - 100.0).abs() < 1e-9, "the original stays in place");
}

#[test]
fn keyboard_shortcuts_select_tools_and_nudge() {
    let mut t = T::new();
    t.key(Key::R);
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect));
    t.key(Key::A);
    assert_eq!(t.app().tool, Tool::Arrow);
    t.key(Key::Escape);
    assert_eq!(t.app().tool, Tool::Select);
    let a = t.add_rect(100.0, 100.0, 50.0, 50.0);
    t.app().sel = vec![a.clone()];
    t.key(Key::ArrowRight);
    assert!((t.frame(&a).x - 101.0).abs() < 1e-9);
}

#[test]
fn text_editing_commits_text_and_fits_the_box() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 60.0, 30.0);
    t.app().start_editing(&a, None);
    t.h.step();
    t.app().editing.as_mut().unwrap().buf = "A text long enough to overflow".into();
    t.app().commit_editing();
    let Kind::Box(b) = &t.app().ed.page().get(&a).unwrap().kind else { unreachable!() };
    assert_eq!(b.label.text.to_plain(), "A text long enough to overflow");
    assert!(b.frame.h > 30.0, "the shape grows to hold the text");
}

#[test]
fn empty_fresh_text_is_discarded_without_polluting_history() {
    let mut t = T::new();
    t.tool(Tool::Text);
    t.click((300.0, 300.0));
    assert!(t.app().editing.is_some(), "the editor opens");
    t.app().commit_editing();
    assert!(t.objects().is_empty());
    assert!(!t.app().ed.can_undo(), "nothing to undo: creation and abandonment cancel out");
}

#[test]
fn table_tool_inserts_a_table_and_cells_are_editable() {
    let mut t = T::new();
    t.tool(Tool::Table);
    t.click((400.0, 300.0));
    let objs = t.objects();
    let Kind::Table(tb) = &objs[0].kind else { panic!("not a table") };
    assert_eq!((tb.rows(), tb.cols()), (3, 3));
    let id = objs[0].id.clone();
    t.app().start_editing(&id, Some((1, 2)));
    t.app().editing.as_mut().unwrap().buf = "cellule".into();
    t.app().commit_editing();
    let Kind::Table(tb) = &t.app().ed.page().get(&id).unwrap().kind else { unreachable!() };
    assert_eq!(tb.cell(1, 2).unwrap().to_plain(), "cellule");
}

#[test]
fn copy_paste_and_group_shortcuts() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 50.0, 50.0);
    let b = t.add_rect(200.0, 100.0, 50.0, 50.0);
    t.app().sel = vec![a, b];
    t.key_mod(Modifiers::COMMAND, Key::G);
    assert!(t.objects().iter().all(|o| o.group.is_some()));
    t.app().copy_selection();
    t.app().paste();
    assert_eq!(t.objects().len(), 4);
    let groups: std::collections::HashSet<_> = t.objects().iter().filter_map(|o| o.group.clone()).collect();
    assert_eq!(groups.len(), 2, "the copy gets its own group");
}

#[test]
fn mermaid_import_adds_bound_objects_and_selects_them() {
    let mut t = T::new();
    t.app().import_mermaid("graph TD\n A[One] --> B[Two]", false).unwrap();
    t.h.step();
    assert_eq!(t.objects().len(), 3);
    assert_eq!(t.app().sel.len(), 3);
    assert!(t.app().import_mermaid("erDiagram\n A ||--o{ B : x", false).is_err());
}

#[test]
fn save_and_reopen_roundtrip_and_foreign_formats() {
    let mut t = T::new();
    t.add_rect(100.0, 100.0, 50.0, 50.0);
    let dir = std::env::temp_dir().join(format!("inkboard-app-test-{}", Id::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let native = dir.join("a.inkboard.json");
    inkboard_core::serial::save(&t.app().ed.doc, &native).unwrap();
    let drawio = dir.join("a.drawio");
    let ctx = t.h.ctx.clone();
    let xml = inkboard_formats::drawio::export(&t.app().ed.doc, &crate::fonts::EguiMeasurer(ctx));
    std::fs::write(&drawio, xml).unwrap();
    let mut t2 = T::new();
    t2.app().open_path(&native);
    assert_eq!(t2.objects().len(), 1);
    assert!(t2.app().path.is_some(), "a native file becomes the current file");
    t2.app().open_path(&drawio);
    assert_eq!(t2.objects().len(), 1);
    assert!(t2.app().path.is_none(), "an exchange format does not replace the current file");
    std::fs::remove_dir_all(&dir).ok();
}

/// `.inkboard`, `.inkboard.json`, `.tableau.json` and a plain `.json` (recognized by its content) all open as the current file,
/// and "Save" writes back under the same name (no silent rename, no extra file).
#[test]
fn every_native_name_opens_and_save_keeps_the_name() {
    let mut t = T::new();
    t.add_rect(100.0, 100.0, 50.0, 50.0);
    let json = inkboard_core::serial::to_json(&t.app().ed.doc, false);
    let dir = std::env::temp_dir().join(format!("inkboard-app-ext-{}", Id::new()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.inkboard", "b.inkboard.json", "c.tableau.json", "d.json"] {
        let path = dir.join(name);
        std::fs::write(&path, &json).unwrap();
        let mut t2 = T::new();
        t2.app().open_path(&path);
        assert_eq!(t2.objects().len(), 1, "{name}");
        assert_eq!(t2.app().path.as_deref(), Some(path.as_path()), "{name}: a native file becomes the current file");
        t2.add_rect(300.0, 100.0, 50.0, 50.0);
        t2.app().save();
        assert_eq!(t2.app().path.as_deref(), Some(path.as_path()), "{name}: saved under its own name");
        assert_eq!(inkboard_core::serial::load(&path).unwrap().pages[0].objects.len(), 2, "{name}: the file was updated");
        assert!(!t2.app().dirty(), "{name}");
    }
    let mut files: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, ["a.inkboard", "b.inkboard.json", "c.tableau.json", "d.json"], "no file created or renamed by saving");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn background_and_pages_are_undoable() {
    let mut t = T::new();
    let mut bg = t.app().ed.doc.background;
    bg.preset = BgPreset::Black;
    t.app().ed.set_background(bg);
    assert!(t.app().ed.doc.background.preset.is_dark());
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(!t.app().ed.doc.background.preset.is_dark());
    t.app().ed.add_page("Two");
    assert_eq!(t.app().ed.doc.pages.len(), 2);
}

#[test]
fn zoom_with_pointer_keeps_the_world_point_under_the_cursor() {
    let mut t = T::new();
    let p = t.at(300.0, 200.0);
    t.mv(p);
    t.h.event(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, 120.0), modifiers: Modifiers::NONE });
    t.h.step();
    let v = t.app().ed.page().view;
    assert!(v.zoom > 1.1, "zoom = {}", v.zoom);
    let r = t.app().canvas_rect;
    let wx = (f64::from(p.x - r.min.x) - v.x) / v.zoom;
    assert!((wx - 300.0).abs() < 1.0, "the point under the cursor stays fixed: {wx}");
}

/* ------------------------------------------------------------ rich text */

use inkboard_core::text::Format;

fn edited_box(t: &mut T) -> Id {
    let a = t.add_rect(100.0, 100.0, 400.0, 120.0);
    t.app().start_editing(&a, None);
    t.h.step();
    t.h.step();
    a
}

fn type_text(t: &mut T, s: &str) {
    t.h.event(Event::Text(s.to_owned()));
    t.h.step();
    t.h.step();
}

fn label_of(t: &mut T, id: &Id) -> RichText {
    let Kind::Box(b) = &t.app().ed.page().get(id).unwrap().kind else { unreachable!() };
    b.label.text.clone()
}

#[test]
fn typing_updates_both_buffer_and_styled_text() {
    let mut t = T::new();
    let a = edited_box(&mut t);
    type_text(&mut t, "Hello world");
    let e = t.app().editing.as_ref().unwrap();
    assert_eq!(e.buf, "Hello world");
    assert_eq!(e.rich.to_plain(), "Hello world");
    t.app().commit_editing();
    assert_eq!(label_of(&mut t, &a).to_plain(), "Hello world");
}

#[test]
fn keyboard_selection_then_ctrl_b_bolds_only_the_selected_word() {
    let mut t = T::new();
    let a = edited_box(&mut t);
    type_text(&mut t, "Hello world");
    t.key(Key::Home);
    for _ in 0..7 {
        t.key_mod(Modifiers::SHIFT, Key::ArrowRight);
    }
    assert_eq!(t.app().editing.as_ref().unwrap().sel, (0, 7), "the keyboard selection is followed");
    t.key_mod(Modifiers::COMMAND, Key::B);
    t.h.step();
    t.app().commit_editing();
    let rt = label_of(&mut t, &a);
    assert_eq!(rt.to_plain(), "Hello world");
    assert!(rt.all(0, 7, |s| s.bold), "\"Hello\" in bold: {rt:?}");
    assert!(!rt.all(7, 13, |s| s.bold) && !rt.all(8, 13, |s| s.bold), "\"world\" unchanged");
}

#[test]
fn color_and_highlight_apply_to_the_selection() {
    let mut t = T::new();
    let a = edited_box(&mut t);
    type_text(&mut t, "Hello world");
    t.app().editing.as_mut().unwrap().sel = (8, 13);
    let red = Color::rgb(0xe0, 0x31, 0x31);
    let yellow = Color::rgb(0xff, 0xec, 0x99);
    t.app().apply_format(Format::Color(Some(red)));
    t.app().apply_format(Format::Highlight(Some(yellow)));
    t.h.step();
    t.h.step();
    t.app().commit_editing();
    let rt = label_of(&mut t, &a);
    assert!(rt.all(8, 13, |s| s.color == Some(red) && s.highlight == Some(yellow)), "{rt:?}");
    assert!(rt.all(0, 8, |s| s.color.is_none() && s.highlight.is_none()));
}

#[test]
fn text_typed_after_bold_text_stays_bold_and_untouched_words_keep_their_style() {
    let mut t = T::new();
    let a = edited_box(&mut t);
    type_text(&mut t, "ab");
    t.app().editing.as_mut().unwrap().sel = (0, 2);
    t.app().apply_format(Format::Bold);
    t.h.step();
    t.h.step();
    t.key(Key::End);
    type_text(&mut t, "c");
    t.app().commit_editing();
    let rt = label_of(&mut t, &a);
    assert_eq!(rt.to_plain(), "abc");
    assert!(rt.all(0, 3, |s| s.bold), "the rest of the typing inherits bold: {rt:?}");
}

#[test]
fn editing_the_middle_of_a_styled_text_keeps_other_styles() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 400.0, 120.0);
    let mut rt = RichText::plain("alpha beta");
    rt.format(6, 10, Format::Italic);
    t.app().ed.update(&a, |o| {
        if let Kind::Box(b) = &mut o.kind {
            b.label.text = rt.clone();
        }
    });
    t.app().start_editing(&a, None);
    t.h.step();
    t.h.step();
    t.key(Key::Home);
    type_text(&mut t, "XY ");
    t.app().commit_editing();
    let rt = label_of(&mut t, &a);
    assert_eq!(rt.to_plain(), "XY alpha beta");
    assert!(rt.all(9, 13, |s| s.italic), "\"beta\" stays italic: {rt:?}");
    assert!(!rt.all(0, 9, |s| s.italic));
}

#[test]
fn format_buttons_without_editing_apply_to_all_selected_text_consistently() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 200.0, 80.0);
    let b = t.add_rect(400.0, 100.0, 200.0, 80.0);
    for (id, txt) in [(&a, "un"), (&b, "deux")] {
        t.app().ed.update(id, |o| {
            if let Kind::Box(bx) = &mut o.kind {
                bx.label.text = RichText::plain(txt);
            }
        });
    }
    // b is already bold: the action must make everything bold (not invert b)
    t.app().ed.update(&b, |o| {
        if let Kind::Box(bx) = &mut o.kind {
            bx.label.text.format(0, 4, Format::Bold);
        }
    });
    t.app().sel = vec![a.clone(), b.clone()];
    t.app().apply_format(Format::Bold);
    assert!(label_of(&mut t, &a).all(0, 2, |s| s.bold) && label_of(&mut t, &b).all(0, 4, |s| s.bold));
    assert!(t.app().format_active(Format::Bold));
    t.app().apply_format(Format::Bold);
    assert!(!label_of(&mut t, &a).all(0, 2, |s| s.bold) && !label_of(&mut t, &b).all(0, 4, |s| s.bold));
    assert!(t.app().ed.can_undo());
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(label_of(&mut t, &b).all(0, 4, |s| s.bold), "one undo = one formatting action");
}

#[test]
fn empty_selection_formats_the_whole_text_in_edit_mode() {
    let mut t = T::new();
    let a = edited_box(&mut t);
    type_text(&mut t, "tout");
    t.app().editing.as_mut().unwrap().sel = (2, 2);
    t.app().apply_format(Format::Italic);
    t.app().commit_editing();
    assert!(label_of(&mut t, &a).all(0, 4, |s| s.italic));
}

/* ------------------------------------------------- custom shortcuts */

use crate::app::Dialog;
use crate::keymap::{Action, Shortcut};

#[test]
fn rebinding_a_tool_key_takes_effect_immediately() {
    let mut t = T::new();
    t.key(Key::R);
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect));
    t.app().keymap.add(Action::ToolEllipse, Shortcut::key(Key::R)).unwrap();
    t.tool(Tool::Select);
    t.key(Key::R);
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Ellipse), "R now belongs to the ellipse");
    t.app().keymap.add(Action::ToolRect, Shortcut::key(Key::K)).unwrap();
    t.key(Key::K);
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect));
}

#[test]
fn removed_shortcut_does_nothing_and_remaining_one_still_works() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 50.0, 50.0);
    t.app().keymap.remove(Action::Delete, &Shortcut::key(Key::Backspace));
    t.app().sel = vec![a.clone()];
    t.key(Key::Backspace);
    assert!(t.app().ed.page().get(&a).is_some(), "Backspace is no longer bound");
    t.key(Key::Delete);
    assert!(t.app().ed.page().get(&a).is_none());
}

#[test]
fn custom_ctrl_shortcut_runs_the_action() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 50.0, 50.0);
    t.app().keymap.add(Action::Duplicate, Shortcut::new(Key::J, true, false, true)).unwrap();
    t.app().sel = vec![a];
    t.key_mod(Modifiers::COMMAND | Modifiers::ALT, Key::J);
    assert_eq!(t.objects().len(), 2);
    // the old default shortcut still works (Ctrl+D)
    t.key_mod(Modifiers::COMMAND, Key::D);
    assert_eq!(t.objects().len(), 3);
}

#[test]
fn custom_bold_shortcut_works_while_editing_text() {
    let mut t = T::new();
    t.app().keymap.add(Action::Bold, Shortcut::ctrl(Key::K)).unwrap();
    let a = edited_box(&mut t);
    type_text(&mut t, "Hello world");
    t.key(Key::Home);
    for _ in 0..7 {
        t.key_mod(Modifiers::SHIFT, Key::ArrowRight);
    }
    t.key_mod(Modifiers::COMMAND, Key::K);
    t.h.step();
    t.app().commit_editing();
    assert!(label_of(&mut t, &a).all(0, 7, |s| s.bold));
}

#[test]
fn shortcut_recording_assigns_the_next_key_and_escape_cancels() {
    let mut t = T::new();
    t.app().dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: Some(Action::ToolEllipse) });
    t.h.step();
    t.key(Key::K);
    assert_eq!(t.app().keymap.action_for(Key::K, Modifiers::NONE), Some(Action::ToolEllipse));
    assert!(matches!(t.app().dialog, Some(Dialog::Shortcuts { recording: None, .. })), "recording stops after the keystroke");
    // while recording, the key does not trigger its old action
    t.app().dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: Some(Action::ToolPen) });
    t.h.step();
    t.key(Key::V);
    assert_eq!(t.app().tool, Tool::Select);
    assert_eq!(t.app().keymap.action_for(Key::V, Modifiers::NONE), Some(Action::ToolPen), "V now belongs to the pen, taken from selection");
    // Escape cancels without changing anything
    let before = t.app().keymap.clone();
    t.app().dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: Some(Action::ToolHand) });
    t.h.step();
    t.key(Key::Escape);
    assert_eq!(t.app().keymap, before);
}

#[test]
fn keymap_persists_to_disk_and_reloads() {
    let mut t = T::new();
    let dir = std::env::temp_dir().join(format!("wb-keymap-{}", Id::new()));
    let path = dir.join("keymap.json");
    t.app().keymap_path = Some(path.clone());
    t.app().keymap.add(Action::ToolPen, Shortcut::new(Key::K, true, true, false)).unwrap();
    t.app().save_keymap();
    let (loaded, ignored) = crate::keymap::Keymap::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded, t.app().keymap);
    assert_eq!(ignored, 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn menu_labels_follow_the_keymap() {
    let mut t = T::new();
    assert_eq!(t.app().keymap.label(Action::Undo), "Ctrl+Z");
    t.app().keymap.reset(Action::Undo);
    t.app().keymap.remove(Action::Undo, &Shortcut::ctrl(Key::Z));
    t.app().keymap.add(Action::Undo, Shortcut::new(Key::W, true, false, true)).unwrap();
    assert_eq!(t.app().keymap.label(Action::Undo), "Ctrl+Alt+W");
}

/* ----------------------------------------------------- system clipboard */

#[test]
fn pasting_plain_text_creates_a_text_box_in_the_view() {
    let mut t = T::new();
    t.h.event(Event::Paste(
        "An idea
on two lines"
            .into(),
    ));
    t.h.step();
    let b = t.boxes();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].1.geom, BoxGeom::Text);
    assert_eq!(
        b[0].1.label.text.to_plain(),
        "An idea
on two lines"
    );
    assert!(b[0].1.frame.w > 20.0 && b[0].1.frame.h > 20.0, "sized from the text");
    assert_eq!(t.app().sel.len(), 1);
}

#[test]
fn pasting_a_system_image_inserts_a_png_asset() {
    let mut t = T::new();
    t.app().system_image = || Some((4, 2, vec![200; 4 * 2 * 4]));
    t.h.event(Event::Paste(String::new()));
    t.h.step();
    let objs = t.objects();
    let Kind::Image(i) = &objs[0].kind else { panic!("not an image: {objs:?}") };
    let asset = t.app().ed.doc.assets.get(&i.asset).expect("asset registered");
    assert_eq!(asset.mime, "image/png");
    let decoded = image::load_from_memory(&asset.data).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (4, 2));
    assert!((i.frame.w / i.frame.h - 2.0).abs() < 1e-9, "proportions kept");
}

#[test]
fn large_pasted_images_are_scaled_down() {
    let mut t = T::new();
    let at = Pt::new(0.0, 0.0);
    t.app().insert_rgba(2000, 1000, &vec![255; 2000 * 1000 * 4], at);
    let id = t.app().sel[0].clone();
    let f = t.frame(&id);
    assert!(f.w <= 480.0 + 1e-9 && (f.w / f.h - 2.0).abs() < 1e-9, "{f:?}");
    assert!(f.center().dist(at) < 1e-6);
}

#[test]
fn paste_marker_pastes_internal_objects_and_empty_paste_falls_back_to_them() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 50.0, 50.0);
    t.app().sel = vec![a];
    t.app().copy_selection();
    t.h.event(Event::Paste(crate::clipboard::MARKER.into()));
    t.h.step();
    assert_eq!(t.objects().len(), 2, "internal object pasted, not a text box with the marker");
    t.h.event(Event::Paste(String::new()));
    t.h.step();
    assert_eq!(t.objects().len(), 3);
    assert!(t.boxes().iter().all(|(_, b)| b.geom == BoxGeom::Rect));
}

#[test]
fn invalid_pixel_buffer_is_rejected_without_panicking() {
    let mut t = T::new();
    t.app().insert_rgba(10, 10, &[0; 5], Pt::ZERO);
    assert!(t.objects().is_empty());
    assert!(t.app().toast.is_some());
}

#[test]
fn rich_format_applies_to_all_cells_of_a_selected_table() {
    let mut t = T::new();
    t.tool(Tool::Table);
    t.click((400.0, 300.0));
    let id = t.objects()[0].id.clone();
    t.app().ed.update(&id, |o| {
        if let Kind::Table(tb) = &mut o.kind {
            tb.cell_mut(0, 0).unwrap().set_plain("a");
            tb.cell_mut(1, 1).unwrap().set_plain("b");
        }
    });
    t.app().sel = vec![id.clone()];
    t.app().apply_format(Format::Bold);
    let Kind::Table(tb) = &t.app().ed.page().get(&id).unwrap().kind else { unreachable!() };
    assert!(tb.cell(0, 0).unwrap().all(0, 1, |s| s.bold) && tb.cell(1, 1).unwrap().all(0, 1, |s| s.bold));
}

/* ------------------------------------------------------------ pen */

fn touch(t: &mut T, phase: egui::TouchPhase, p: Pos2, force: f32) {
    t.h.event(Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(0), phase, pos: p, force: Some(force) });
}

#[test]
fn pen_pressure_from_touch_events_makes_a_variable_width_stroke() {
    let mut t = T::new();
    t.tool(Tool::Pen);
    let pts: Vec<(f64, f64, f32)> = vec![(100.0, 100.0, 0.1), (140.0, 110.0, 0.3), (180.0, 120.0, 0.6), (220.0, 120.0, 0.9), (260.0, 110.0, 1.0)];
    let first = t.at(pts[0].0, pts[0].1);
    touch(&mut t, egui::TouchPhase::Start, first, pts[0].2);
    t.mv(first);
    t.button(first, true, Modifiers::NONE);
    for (x, y, f) in &pts[1..] {
        let p = t.at(*x, *y);
        touch(&mut t, egui::TouchPhase::Move, p, *f);
        t.mv(p);
    }
    let last = t.at(260.0, 110.0);
    touch(&mut t, egui::TouchPhase::End, last, 0.0);
    t.button(last, false, Modifiers::NONE);
    let objs = t.objects();
    let Kind::Pen(p) = &objs[0].kind else { panic!("not a stroke") };
    let pr = p.pressures.as_ref().expect("pressure is kept");
    assert_eq!(pr.len(), p.pts.len(), "one pressure per point, even after simplification");
    assert!((pr[0] - 0.1).abs() < 1e-6 && (pr[pr.len() - 1] - 1.0).abs() < 1e-6, "{pr:?}");
    assert!(pr.windows(2).all(|w| w[0] <= w[1] + 1e-6), "pressions croissantes : {pr:?}");
    assert_eq!(t.app().pen_force, None, "pressure is forgotten on release");
}

#[test]
fn mouse_strokes_have_constant_width_and_highlighter_ignores_pressure() {
    let mut t = T::new();
    t.tool(Tool::Pen);
    t.drag((100.0, 100.0), (300.0, 160.0));
    assert!(matches!(&t.objects()[0].kind, Kind::Pen(p) if p.pressures.is_none()));
    t.tool(Tool::Highlighter);
    let a = t.at(100.0, 300.0);
    touch(&mut t, egui::TouchPhase::Start, a, 0.3);
    t.mv(a);
    t.button(a, true, Modifiers::NONE);
    let b = t.at(300.0, 300.0);
    touch(&mut t, egui::TouchPhase::Move, b, 0.9);
    t.mv(b);
    touch(&mut t, egui::TouchPhase::End, b, 0.0);
    t.button(b, false, Modifiers::NONE);
    let objs = t.objects();
    assert!(matches!(&objs[1].kind, Kind::Pen(p) if p.highlighter && p.pressures.is_none()));
}

/* ------------------------------------------------------------ large boards */

#[test]
fn spatial_index_keeps_picking_and_moving_correct_on_a_large_board() {
    let mut t = T::new();
    let mut ids = Vec::new();
    for i in 0..600 {
        ids.push(t.add_rect((i % 30) as f64 * 150.0, (i / 30) as f64 * 130.0, 100.0, 80.0));
    }
    t.h.step();
    assert!(t.app().caches.spatial.is_some(), "beyond 400 objects the spatial index is active");
    let target = ids[35].clone(); // column 5, row 1
    let f = t.frame(&target);
    t.click((f.x + 50.0, f.y + 40.0));
    assert_eq!(t.app().sel, vec![target.clone()]);
    // move far from the others, then check with the rebuilt index
    t.drag((f.x + 50.0, f.y + 40.0), (f.x + 50.0 + 5000.0, f.y + 40.0 + 3000.0));
    let moved = t.frame(&target);
    assert!((moved.x - (f.x + 5000.0)).abs() < 2.0, "{moved:?}");
    t.app().sel.clear();
    t.app().set_view(View { x: 500.0 - moved.x, y: 300.0 - moved.y, zoom: 1.0 });
    t.h.step();
    t.click((moved.x + 50.0, moved.y + 40.0));
    assert_eq!(t.app().sel, vec![target.clone()], "the object is found at its new position");
    t.app().sel.clear();
    t.app().set_view(View { x: 500.0 - f.x, y: 300.0 - f.y, zoom: 1.0 });
    t.h.step();
    t.click((f.x + 50.0, f.y + 40.0));
    assert!(t.app().sel.is_empty(), "nothing left at the old position");
    t.app().set_view(View { x: 300.0, y: 150.0, zoom: 1.0 }); // outside the floating cards (title, tools, panel)
    t.h.step();
    // lasso over a dense area
    t.drag((125.0, 105.0), (480.0, 300.0)); // start in a gap: lasso, not move
    let sel = t.app().sel.len();
    let linear = t.app().ed.page().objects_in_rect(Rect::new(125.0, 105.0, 355.0, 195.0), true).len();
    assert_eq!(sel, linear, "the lasso gives the same result with or without the index");
    assert!(sel >= 2, "at least {sel} objects expected");
}

/* ------------------------------------------------- waypoints and elbows */

fn linked_pair(t: &mut T) -> (Id, Id, Id) {
    let a = t.add_rect(0.0, 100.0, 120.0, 80.0);
    let b = t.add_rect(400.0, 100.0, 120.0, 80.0);
    let c = t.app().ed.connect(&a, &b);
    (a, b, c)
}

fn via_of(t: &mut T, c: &Id) -> Vec<Pt> {
    let Kind::Connector(cn) = &t.app().ed.page().get(c).unwrap().kind else { unreachable!() };
    cn.via.clone()
}

#[test]
fn dragging_a_segment_midpoint_handle_inserts_a_waypoint_in_one_undo_step() {
    let mut t = T::new();
    let (_a, _b, c) = linked_pair(&mut t);
    t.app().sel = vec![c.clone()];
    t.h.step();
    t.drag((260.0, 140.0), (260.0, 320.0));
    let via = via_of(&mut t, &c);
    assert_eq!(via.len(), 1, "{via:?}");
    assert!(via[0].dist(Pt::new(260.0, 320.0)) < 2.0);
    let page = t.app().ed.page();
    let Kind::Connector(cn) = &page.get(&c).unwrap().kind else { unreachable!() };
    let path = page.connector_path(cn).unwrap();
    assert!(path.iter().any(|p| p.y > 300.0), "the arrow goes through the new point");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(via_of(&mut t, &c).is_empty());
}

#[test]
fn waypoints_can_be_moved_and_removed() {
    let mut t = T::new();
    let (_a, _b, c) = linked_pair(&mut t);
    t.app().ed.update(&c, |o| {
        if let Kind::Connector(cn) = &mut o.kind {
            cn.via = vec![Pt::new(260.0, 300.0)];
        }
    });
    t.app().sel = vec![c.clone()];
    t.h.step();
    t.drag((260.0, 300.0), (300.0, 360.0));
    let via = via_of(&mut t, &c);
    assert!(via[0].dist(Pt::new(300.0, 360.0)) < 2.0, "{via:?}");
    // Alt + click on the handle: removes the point
    let p = t.at(300.0, 360.0);
    t.h.input_mut().modifiers = Modifiers::ALT;
    t.mv(p);
    t.button(p, true, Modifiers::ALT);
    t.button(p, false, Modifiers::ALT);
    t.h.input_mut().modifiers = Modifiers::NONE;
    assert!(via_of(&mut t, &c).is_empty());
}

#[test]
fn elbow_connectors_route_around_shapes_placed_in_the_way() {
    let mut t = T::new();
    let (_a, _b, c) = linked_pair(&mut t);
    t.app().ed.update(&c, |o| {
        if let Kind::Connector(cn) = &mut o.kind {
            cn.route = Route::Elbow;
        }
    });
    let wall = t.add_rect(230.0, 60.0, 60.0, 160.0);
    let wall_rect = t.frame(&wall).rect();
    let page = t.app().ed.page();
    let Kind::Connector(cn) = &page.get(&c).unwrap().kind else { unreachable!() };
    let path = page.connector_path(cn).unwrap();
    assert!(!inkboard_core::route::crosses(&path, &[wall_rect]), "{path:?}");
    // the wall moved out of the way: the route becomes the simplest again
    t.app().ed.update(&wall, |o| o.translate(Pt::new(0.0, 600.0)));
    let page = t.app().ed.page();
    let Kind::Connector(cn) = &page.get(&c).unwrap().kind else { unreachable!() };
    let direct = page.connector_path(cn).unwrap();
    assert_eq!(direct.len(), 2, "straight line when nothing is in the way: {direct:?}");
}

/* ------------------------------------------------------ prefabricated elements */

#[test]
fn code_block_is_a_dark_monospace_box_in_one_undo_step() {
    let mut t = T::new();
    t.app().insert_code_block();
    t.h.step();
    let boxes = t.boxes();
    assert_eq!(boxes.len(), 1);
    let b = &boxes[0].1;
    assert_eq!(b.label.family, FontFamily::Mono);
    assert_eq!(b.label.align, Align::Left);
    assert!(boxes[0].0.style.fill.is_some());
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(t.objects().is_empty());
}

#[test]
fn formula_is_composed_as_a_tinted_svg_image_that_keeps_its_source() {
    let mut t = T::new();
    t.app().insert_formula(r"\frac{a+b}{2}", None).unwrap();
    t.h.step();
    let o = t.objects().into_iter().next().unwrap();
    let Kind::Image(i) = &o.kind else { panic!("a formula is an image") };
    assert_eq!(i.latex.as_deref(), Some(r"\frac{a+b}{2}"));
    assert!(i.frame.w > 20.0 && i.frame.h > 20.0);
    let asset = t.app().ed.doc.assets.get(&i.asset).unwrap();
    assert_eq!(asset.mime, "image/svg+xml");
    // SVG export replaces white with the stroke color (dark ink on a light background)
    let page = t.app().ed.page().clone();
    let svg = inkboard_core::svg::export(&t.app().ed.doc, &page, None, &ApproxMeasurer, &Default::default()).unwrap();
    assert!(!String::from_utf8_lossy(&base64_payload(&svg)).contains("#ffffff"), "formula not tinted on export");
    // an invalid source creates nothing
    assert!(t.app().insert_formula(r"\notacommand", None).is_err());
    assert!(t.app().insert_formula("  ", None).is_err());
    assert_eq!(t.objects().len(), 1);
}

/// First base64 payload of a `data:` image in an exported SVG.
fn base64_payload(svg: &str) -> Vec<u8> {
    use base64::Engine;
    let b64 = svg.split("base64,").nth(1).unwrap().split('"').next().unwrap();
    base64::engine::general_purpose::STANDARD.decode(b64).unwrap()
}

#[test]
fn editing_a_formula_keeps_its_center_and_scale() {
    let mut t = T::new();
    t.app().insert_formula("x^2", None).unwrap();
    let id = t.objects()[0].id.clone();
    // the user enlarges the formula ×2
    t.app().ed.update(&id, |o| {
        if let Kind::Image(i) = &mut o.kind {
            let c = i.frame.center();
            i.frame = Frame::new(c.x - i.frame.w, c.y - i.frame.h, i.frame.w * 2.0, i.frame.h * 2.0);
        }
    });
    let before = t.frame(&id);
    assert!(t.app().edit_formula(&id), "a formula opens for editing");
    t.app().insert_formula(r"\frac{x^2}{3}", Some(&id)).unwrap();
    let after = t.frame(&id);
    assert!(after.center().dist(before.center()) < 0.5, "center kept");
    let plain = inkboard_formats::math::render("x^2", 36.0).unwrap();
    assert!((before.h / plain.h - 2.0).abs() < 0.05, "initial enlargement ×2");
    let new = inkboard_formats::math::render(r"\frac{x^2}{3}", 36.0).unwrap();
    assert!((after.h / new.h - 2.0).abs() < 0.05, "scale kept: {} / {}", after.h, new.h);
    let Kind::Image(i) = &t.app().ed.page().get(&id).unwrap().kind else { unreachable!() };
    assert_eq!(i.latex.as_deref(), Some(r"\frac{x^2}{3}"));
    t.key_mod(Modifiers::COMMAND, Key::Z);
    let Kind::Image(i) = &t.app().ed.page().get(&id).unwrap().kind else { unreachable!() };
    assert_eq!(i.latex.as_deref(), Some("x^2"), "undoing an edit takes one step");
}

#[test]
fn formula_survives_save_and_reload() {
    let mut t = T::new();
    t.app().insert_formula(r"\sqrt{2}", None).unwrap();
    let json = inkboard_core::serial::to_json(&t.app().ed.doc, false);
    let back = inkboard_core::serial::from_json(&json).unwrap();
    let Kind::Image(i) = &back.pages[0].objects[0].kind else { panic!() };
    assert_eq!(i.latex.as_deref(), Some(r"\sqrt{2}"));
    assert!(back.assets.contains_key(&i.asset));
}

#[test]
fn vote_tool_drops_one_dot_per_click() {
    let mut t = T::new();
    t.tool(Tool::Vote);
    t.h.step();
    t.click((200.0, 200.0));
    t.click((300.0, 200.0));
    assert_eq!(t.boxes().len(), 2);
    assert!(t.boxes().iter().all(|(_, b)| b.geom == BoxGeom::Ellipse));
}

/* ------------------------------------------------- presentation and timer */

fn page_index(t: &mut T) -> usize {
    let app = t.app();
    app.ed.doc.pages.iter().position(|p| p.id == app.ed.current).unwrap()
}

#[test]
fn presentation_mode_hides_the_ui_and_navigates_between_pages() {
    let mut t = T::new();
    t.app().ed.add_page("Two");
    t.app().ed.add_page("Trois");
    let first = t.app().ed.doc.pages[0].id.clone();
    t.app().ed.current = first.clone();
    t.key(Key::F5);
    t.h.step();
    assert!(t.app().presenting);
    t.key(Key::ArrowRight);
    assert_eq!(page_index(&mut t), 1);
    t.key(Key::Space);
    assert_eq!(page_index(&mut t), 2);
    t.key(Key::ArrowRight);
    assert_eq!(page_index(&mut t), 2, "no page after the last one");
    t.key(Key::ArrowLeft);
    assert_eq!(page_index(&mut t), 1);
    t.key(Key::Escape);
    t.h.step();
    assert!(!t.app().presenting);
    assert_eq!(t.app().tool, Tool::Select);
}

#[test]
fn timer_counts_down_and_reports_when_over() {
    let mut t = T::new();
    assert!(crate::ui::timer_text(t.app()).is_none());
    let d = std::time::Duration::from_secs(90);
    t.app().timer = Some((std::time::Instant::now() + d, d));
    let (txt, over) = crate::ui::timer_text(t.app()).unwrap();
    assert!(txt == "1:30" || txt == "1:29", "{txt}");
    assert!(!over);
    t.app().timer = Some((std::time::Instant::now(), d));
    assert_eq!(crate::ui::timer_text(t.app()), Some(("0:00".to_owned(), true)));
}

#[test]
fn code_block_is_colored_and_recolor_applies_to_edited_code() {
    let mut t = T::new();
    t.app().insert_code_block();
    t.h.step();
    let spans = t.boxes()[0].1.label.text.0.clone();
    assert!(spans.iter().any(|s| s.style.color.is_some()), "the code block is colored");
    let id = t.objects()[0].id.clone();
    t.app().ed.update(&id, |o| {
        if let Kind::Box(b) = &mut o.kind {
            b.label.text = RichText::plain("let x = 1;");
        }
    });
    t.app().recolor_code(&id);
    assert!(t.boxes()[0].1.label.text.0.iter().any(|s| s.style.color.is_some()));
    assert_eq!(t.boxes()[0].1.label.text.to_plain(), "let x = 1;");
}

/* ------------------------------------------------------------ pen eraser */

#[test]
fn pen_eraser_end_erases_whatever_the_active_tool() {
    let mut t = T::new();
    let id = t.add_rect(100.0, 100.0, 200.0, 120.0);
    t.tool(Tool::Pen);
    t.h.step();
    // without the eraser: the pen draws, the shape stays
    t.drag((400.0, 400.0), (450.0, 450.0));
    assert!(t.app().ed.page().get(&id).is_some());
    // eraser end detected: the gesture erases the shape under the pen
    t.app().eraser_probe = || true;
    t.drag((120.0, 160.0), (280.0, 160.0));
    assert!(t.app().ed.page().get(&id).is_none(), "the shape is erased");
    assert_eq!(t.app().tool, Tool::Pen, "the active tool did not change");
}

/* ------------------------------------------------ file modified by another program */

fn external_edit(path: &std::path::Path, text: &str) {
    let mut doc = inkboard_core::serial::load(path).unwrap();
    doc.pages[0].objects.push(Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 100.0, 60.0), Style::default(), Label::new(text, 20.0, Align::Center)));
    inkboard_core::serial::save(&doc, path).unwrap();
    // the filesystem clock can be coarse: we force a different date
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
}

fn temp_board(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wb-watch-{tag}-{}.inkboard", std::process::id()))
}

#[test]
fn open_file_reloads_when_another_program_changes_it() {
    let mut t = T::new();
    let path = temp_board("reload");
    t.add_rect(0.0, 0.0, 50.0, 50.0);
    t.app().save_to(&path);
    t.app().last_watch = std::time::Instant::now() - std::time::Duration::from_secs(1);
    t.h.step();
    assert_eq!(t.objects().len(), 1, "no reload while nothing has changed");
    external_edit(&path, "Added by an agent");
    t.app().last_watch = std::time::Instant::now() - std::time::Duration::from_secs(1);
    t.h.step();
    assert_eq!(t.objects().len(), 2, "the board is reloaded");
    assert!(!t.app().dirty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn unsaved_local_changes_are_not_overwritten_by_an_external_change() {
    let mut t = T::new();
    let path = temp_board("dirty");
    t.add_rect(0.0, 0.0, 50.0, 50.0);
    t.app().save_to(&path);
    t.add_rect(200.0, 0.0, 50.0, 50.0); // unsaved local change
    external_edit(&path, "Externe");
    t.app().last_watch = std::time::Instant::now() - std::time::Duration::from_secs(1);
    t.h.step();
    assert_eq!(t.objects().len(), 2, "the 2 local shapes are kept");
    assert!(t.app().dirty());
    let _ = std::fs::remove_file(&path);
}

/* --------------------------------------------- live mode (inkboard-mcp drives the application) */

struct Live {
    srv: inkboard_mcp::Server,
    file: std::path::PathBuf,
}

fn start_live(t: &mut T, tag: &str) -> Live {
    let file = std::env::temp_dir().join(format!("wb-live-{tag}-{}.json", std::process::id()));
    let ctx = t.h.ctx.clone();
    t.app().live = Some(crate::live::LiveServer::start(ctx, file.clone()).unwrap());
    Live { srv: inkboard_mcp::Server { live_file: Some(file.clone()), ..Default::default() }, file }
}

/// Calls an MCP tool from another thread (as `inkboard-mcp` does) while the UI runs.
fn mcp_call(t: &mut T, live: &Live, tool: &str, args: serde_json::Value) -> (bool, serde_json::Value) {
    let srv = live.srv.clone();
    let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": args}});
    let th = std::thread::spawn(move || srv.handle(&msg).unwrap());
    let start = std::time::Instant::now();
    while !th.is_finished() {
        assert!(start.elapsed() < std::time::Duration::from_secs(10), "no response");
        t.h.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let r = th.join().unwrap();
    let text = r["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{r}")).to_owned();
    (r["result"]["isError"] == serde_json::json!(true), serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)))
}

#[test]
fn an_agent_edits_the_open_board_live_and_each_call_is_one_undo_step() {
    let mut t = T::new();
    let live = start_live(&mut t, "edit");
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect", "text": "Added live"}));
    assert!(!err, "{r}");
    assert_eq!(t.boxes().len(), 1);
    assert_eq!(t.boxes()[0].1.label.text.to_plain(), "Added live");
    assert_eq!(t.app().sel.len(), 1, "the created object is selected");
    assert!(t.app().dirty(), "unsaved change: the user stays in control");
    let (err, r) = mcp_call(&mut t, &live, "add_formula", serde_json::json!({"latex": r"\frac{1}{2}"}));
    assert!(!err, "{r}");
    assert_eq!(t.objects().len(), 2);
    // Ctrl+Z undoes only the agent's last call
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert_eq!(t.objects().len(), 1);
    // the agent sees the current state and the user's selection
    let id = t.objects()[0].id.clone();
    t.app().sel = vec![id.clone()];
    let (err, d) = mcp_call(&mut t, &live, "describe_board", serde_json::json!({}));
    assert!(!err);
    assert_eq!(d["live"], true);
    assert_eq!(d["selection"], serde_json::json!([id.0]));
    // tool errors returned to the agent, without changing anything
    let (err, e) = mcp_call(&mut t, &live, "add_formula", serde_json::json!({"latex": r"\inconnu"}));
    assert!(err, "{e}");
    assert_eq!(t.objects().len(), 1);
    let _ = std::fs::remove_file(&live.file);
}

#[test]
fn every_agent_tool_call_emits_a_presence_event_reads_included_without_touching_the_history() {
    use crate::presence::Action;
    let mut t = T::new();
    let live = start_live(&mut t, "presence");
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect", "text": "A", "x": 100, "y": 100}));
    assert!(!err, "{r}");
    let id = r["id"].as_str().unwrap().to_owned();
    {
        let ev = t.app().agent.outbox.back().expect("event");
        assert_eq!((ev.action, ev.tool.as_str(), ev.name.as_str()), (Action::Act, "add_shape", "Claude"));
        assert_eq!(ev.targets().map(|x| x.id.0.clone()).collect::<Vec<_>>(), vec![id.clone()]);
    }
    assert!(t.app().agent.state.cursor.is_some(), "the cursor is on its way to the object");
    // reading tools: a look, no target, and neither the revision nor the undo history change
    let (rev, undo) = (t.app().ed.revision(), t.app().ed.can_undo());
    for tool in ["describe_board", "find_objects", "board_changes"] {
        let args = if tool == "find_objects" { serde_json::json!({"text": "A"}) } else { serde_json::json!({}) };
        let (err, r) = mcp_call(&mut t, &live, tool, args);
        assert!(!err, "{tool}: {r}");
        let ev = t.app().agent.outbox.back().unwrap();
        assert_eq!((ev.action, ev.tool.as_str(), ev.targets().count()), (Action::Look, tool, 0));
        assert_eq!((t.app().ed.revision(), t.app().ed.can_undo()), (rev, undo), "{tool}");
    }
    assert_eq!(t.app().agent.outbox.len(), 4);
    // the animation is not an undo step: one Ctrl+Z removes the shape
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(t.objects().is_empty());
    let _ = std::fs::remove_file(&live.file);
}

#[test]
fn an_agent_batch_gives_one_stop_per_operation_and_vanished_objects_are_ignored() {
    let mut t = T::new();
    let live = start_live(&mut t, "presence-batch");
    let ops = serde_json::json!({"operations": [
        {"tool": "add_shape", "args": {"geom": "rect", "text": "A", "x": 0, "y": 0}},
        {"tool": "add_shape", "args": {"geom": "rect", "text": "B", "x": 300, "y": 0}},
        {"tool": "add_connector", "args": {"from": "$0.id", "to": "$1.id"}},
    ]});
    let (err, r) = mcp_call(&mut t, &live, "batch", ops);
    assert!(!err, "{r}");
    let ev = t.app().agent.outbox.back().unwrap().clone();
    assert_eq!(ev.steps.len(), 3);
    assert!(ev.steps[0].iter().all(|x| t.app().ed.page().get(&x.id).is_some()));
    assert_eq!(t.app().agent.state.halo_count(), 3, "A, B, then the connector");
    // a batch that creates then deletes: the targets are gone after the call, nothing breaks
    let ops = serde_json::json!({"operations": [
        {"tool": "add_shape", "args": {"geom": "rect", "x": 0, "y": 300}},
        {"tool": "delete_object", "args": {"id": "$0.id"}},
    ]});
    let (err, r) = mcp_call(&mut t, &live, "batch", ops);
    assert!(!err, "{r}");
    assert_eq!(t.app().agent.outbox.back().unwrap().steps.len(), 0);
    // deleting an existing object directly
    let id = r["results"][0]["id"].as_str().unwrap_or("none").to_owned();
    let (err, _) = mcp_call(&mut t, &live, "delete_object", serde_json::json!({"id": id}));
    assert!(err, "already deleted");
    let first = t.objects()[0].id.0.clone();
    let (err, r) = mcp_call(&mut t, &live, "delete_object", serde_json::json!({"id": first}));
    assert!(!err, "{r}");
    assert_eq!(t.app().agent.outbox.back().unwrap().targets().count(), 0);
    let _ = std::fs::remove_file(&live.file);
}

#[test]
fn the_agent_animation_can_be_turned_off_and_is_skipped_when_capturing() {
    let mut t = T::new();
    let live = start_live(&mut t, "presence-off");
    t.app().agent.animate = false;
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect"}));
    assert!(!err, "{r}");
    assert!(t.app().agent.state.cursor.is_none() && t.app().agent.state.last_activity.is_none(), "nothing is animated");
    assert_eq!(t.app().agent.outbox.len(), 1, "the event is still emitted (guests of a session will need it)");
    // capture mode (no visible window): same
    t.app().agent.animate = true;
    t.app().agent.headless = true;
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect", "x": 400}));
    assert!(!err, "{r}");
    assert!(t.app().agent.state.cursor.is_none());
    assert_eq!(t.app().agent.outbox.len(), 2);
    // and back to normal
    t.app().agent.headless = false;
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect", "x": 800}));
    assert!(!err, "{r}");
    assert!(t.app().agent.state.cursor.is_some());
    let _ = std::fs::remove_file(&live.file);
}

#[test]
fn live_calls_target_the_open_file_only() {
    let mut t = T::new();
    let live = start_live(&mut t, "path");
    let mine = std::env::temp_dir().join(format!("wb-live-mine-{}.inkboard", std::process::id()));
    t.add_rect(0.0, 0.0, 50.0, 50.0);
    t.app().save_to(&mine);
    // same file: applied in the application
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"path": mine.to_str().unwrap(), "geom": "ellipse"}));
    assert!(!err, "{r}");
    assert_eq!(t.objects().len(), 2);
    // other file: the application is untouched, file mode takes over
    let other = std::env::temp_dir().join(format!("wb-live-other-{}.inkboard", std::process::id()));
    inkboard_core::serial::save(&Document::new("Other"), &other).unwrap();
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"path": other.to_str().unwrap(), "geom": "rect", "text": "in the file"}));
    assert!(!err, "{r}");
    assert_eq!(t.objects().len(), 2, "the application did not change");
    assert_eq!(inkboard_core::serial::load(&other).unwrap().pages[0].objects.len(), 1, "the file was modified");
    // save_board saves the displayed board
    assert!(t.app().dirty());
    let (err, r) = mcp_call(&mut t, &live, "save_board", serde_json::json!({}));
    assert!(!err, "{r}");
    assert!(!t.app().dirty());
    assert_eq!(inkboard_core::serial::load(&mine).unwrap().pages[0].objects.len(), 2);
    for f in [&mine, &other, &live.file] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn live_server_rejects_a_wrong_token_and_cleans_up_its_discovery_file() {
    use std::io::{BufRead, BufReader, Write};
    let mut t = T::new();
    let live = start_live(&mut t, "token");
    let info = inkboard_mcp::live::read_info(&live.file).expect("discovery file");
    let mut s = std::net::TcpStream::connect(("127.0.0.1", info.port)).unwrap();
    writeln!(s, "{}", serde_json::json!({"token": "faux", "tool": "describe_board", "args": {}})).unwrap();
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).unwrap();
    let r: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("token"), "{r}");
    assert!(t.objects().is_empty(), "nothing is executed without a valid token");
    // on close, the discovery file disappears
    t.app().live = None;
    assert!(!live.file.exists());
}

/// Sends any JSON-RPC request to the MCP server (another thread) while the UI runs.
fn mcp_rpc(t: &mut T, live: &Live, method: &str, params: serde_json::Value) -> serde_json::Value {
    let srv = live.srv.clone();
    let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let th = std::thread::spawn(move || srv.handle(&msg).unwrap());
    let start = std::time::Instant::now();
    while !th.is_finished() {
        assert!(start.elapsed() < std::time::Duration::from_secs(10), "no response");
        t.h.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    th.join().unwrap()
}

#[test]
fn live_board_is_a_resource_and_subscribers_are_notified_when_the_user_edits() {
    let mut t = T::new();
    let live = start_live(&mut t, "res");
    let list = mcp_rpc(&mut t, &live, "resources/list", serde_json::json!({}));
    let uris: Vec<&str> = list["result"]["resources"].as_array().unwrap().iter().map(|r| r["uri"].as_str().unwrap()).collect();
    assert!(uris.contains(&inkboard_mcp::resources::LIVE_URI), "{list}");
    t.add_rect(0.0, 0.0, 80.0, 50.0);
    let read = mcp_rpc(&mut t, &live, "resources/read", serde_json::json!({"uri": inkboard_mcp::resources::LIVE_URI}));
    let body: serde_json::Value = serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["live"], true);
    assert_eq!(body["page"]["objects"].as_array().unwrap().len(), 1);
    // abonnement + fil de surveillance
    mcp_rpc(&mut t, &live, "resources/subscribe", serde_json::json!({"uri": inkboard_mcp::resources::LIVE_URI}));
    let (tx, rx) = std::sync::mpsc::channel();
    live.srv.spawn_watcher(tx);
    let wait = |t: &mut T, secs: u64| {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        while std::time::Instant::now() < end {
            t.h.step();
            if let Ok(v) = rx.try_recv() {
                return Some(v);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        None
    };
    assert!(wait(&mut t, 2).is_none(), "nothing changes: no notification");
    t.add_rect(200.0, 0.0, 80.0, 50.0); // the user draws
    let n = wait(&mut t, 5).expect("notification after the user's change");
    assert_eq!(n["method"], "notifications/resources/updated");
    assert_eq!(n["params"]["uri"], inkboard_mcp::resources::LIVE_URI);
    let _ = std::fs::remove_file(&live.file);
}

#[test]
fn live_board_changes_and_images() {
    let mut t = T::new();
    let live = start_live(&mut t, "chg");
    let (err, r) = mcp_call(&mut t, &live, "board_changes", serde_json::json!({}));
    assert!(!err && r["baseline"] == true, "{r}");
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect", "text": "Agent"}));
    assert!(!err, "{r}");
    t.add_rect(300.0, 300.0, 60.0, 40.0); // the user adds a shape
    let (err, ch) = mcp_call(&mut t, &live, "board_changes", serde_json::json!({}));
    assert!(!err);
    assert_eq!(ch["added"].as_array().unwrap().len(), 1, "only the user's shape is reported: {ch}");
    assert_eq!(ch["selection"].as_array().map(Vec::len), Some(t.app().sel.len()));
    // the first object deleted by the user is reported
    let victim = t.objects()[0].id.clone();
    t.app().ed.remove(std::slice::from_ref(&victim));
    let (_, ch) = mcp_call(&mut t, &live, "board_changes", serde_json::json!({}));
    assert_eq!(ch["removed"], serde_json::json!([victim.0]), "{ch}");
    // image from base64 data: inserted live, undoable
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
    let before = t.objects().len();
    let (err, r) = mcp_call(&mut t, &live, "add_image", serde_json::json!({"data_base64": png, "w": 100}));
    assert!(!err, "{r}");
    assert_eq!(t.objects().len(), before + 1);
    assert!(matches!(t.objects().last().unwrap().kind, Kind::Image(_)));
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert_eq!(t.objects().len(), before);
    let _ = std::fs::remove_file(&live.file);
}

/* ---------------------------------------------------- floating frame and laser pointer */

#[test]
fn laser_pointer_leaves_a_fading_trail_and_never_edits_the_board() {
    let mut t = T::new();
    t.add_rect(0.0, 0.0, 100.0, 60.0);
    let rev = t.app().ed.revision();
    t.tool(Tool::Laser);
    t.drag((50.0, 200.0), (300.0, 260.0));
    assert!(!t.app().laser.is_empty(), "the trail is recorded");
    assert_eq!(t.app().ed.revision(), rev, "the laser never modifies the board");
    assert_eq!(t.app().tool, Tool::Laser, "the tool stays active");
    // the points fade with time
    t.app().laser.iter_mut().for_each(|(_, at)| *at -= std::time::Duration::from_secs(5));
    t.h.step();
    // only the point under the pointer (freshly recorded) can remain
    assert!(t.app().laser.len() <= 1, "{}", t.app().laser.len());
}

#[test]
fn style_panel_is_a_floating_card_shown_only_when_there_is_something_to_set() {
    let mut t = T::new();
    t.h.step();
    let panel = |t: &mut T| t.h.ctx.memory(|m| m.area_rect(egui::Id::new("props")));
    assert!(panel(&mut t).is_none() || t.app().sel.is_empty() && !crate::panel::visible(t.app()), "nothing to adjust: no panel");
    t.tool(Tool::Pen);
    assert!(crate::panel::visible(t.app()));
    let r = panel(&mut t).expect("panel visible with a drawing tool");
    assert!(r.left() >= 8.0 && r.width() < 300.0, "narrow floating card: {r:?}");
    // the canvas fills the whole window, the cards float above
    assert!(t.app().canvas_rect.width() >= 1299.0 && t.app().canvas_rect.min.x == 0.0);
    t.tool(Tool::Select);
    assert!(!crate::panel::visible(t.app()));
}

#[test]
fn every_toolbar_tool_has_an_icon_and_every_icon_is_known() {
    let ctx = egui::Context::default();
    for name in crate::icons::names() {
        assert!(crate::icons::texture(&ctx, name).is_some(), "{name}");
    }
    for name in [
        "select",
        "hand",
        "pen",
        "highlighter",
        "eraser",
        "rect",
        "ellipse",
        "diamond",
        "triangle",
        "line",
        "arrow",
        "text",
        "sticky",
        "table",
        "vote",
        "image",
        "laser",
        "lock",
        "unlock",
    ] {
        assert!(crate::icons::texture(&ctx, name).is_some(), "missing tool icon: {name}");
    }
}

/* ------------------------------------------------------------ languages */

#[test]
fn french_mode_renders_every_dialog_and_panel_without_missing_translations() {
    use crate::app::Dialog;
    use crate::i18n::{Lang, MISSING, set_for_thread};
    set_for_thread(Some(Lang::Fr));
    MISSING.with(|m| m.borrow_mut().clear());
    let mut t = T::new();
    let id = t.add_rect(100.0, 100.0, 80.0, 50.0);
    t.app().sel = vec![id];
    for tool in [Tool::Select, Tool::Pen, Tool::Shape(BoxGeom::Rect), Tool::Line, Tool::Arrow, Tool::Text, Tool::Table, Tool::Vote, Tool::Laser] {
        t.tool(tool);
    }
    let dialogs = [
        Dialog::Mermaid { text: String::new(), new_board: false, error: Some("x".into()) },
        Dialog::Shortcuts { filter: String::new(), recording: None },
        Dialog::Shortcuts { filter: String::new(), recording: Some(crate::keymap::Action::Delete) },
        Dialog::Formula { src: "x^2".into(), editing: None, preview: Default::default() },
    ];
    for d in dialogs {
        t.app().dialog = Some(d);
        for _ in 0..3 {
            t.h.step();
        }
    }
    let missing: std::collections::BTreeSet<String> = MISSING.with(|m| m.borrow().iter().cloned().collect());
    set_for_thread(None);
    assert!(missing.is_empty(), "texts shown in French mode without a translation: {missing:?}");
}

/* ------------------------------------------------------------ code languages */

#[test]
fn code_blocks_have_a_language_that_drives_the_coloring() {
    use inkboard_core::highlight::Lang;
    let mut t = T::new();
    t.app().insert_code_block();
    let id = t.objects()[0].id.clone();
    let lang = |t: &mut T| match &t.app().ed.page().get(&id).unwrap().kind {
        Kind::Box(b) => b.lang,
        _ => None,
    };
    assert_eq!(lang(&mut t), Some(Lang::Auto));
    // Python: `#` starts a comment; in Rust it does not
    t.app().ed.update(&id, |o| {
        if let Kind::Box(b) = &mut o.kind {
            b.label.text = RichText::plain("x = 1 # note");
        }
    });
    t.app().set_code_lang(std::slice::from_ref(&id), Lang::Python);
    assert_eq!(lang(&mut t), Some(Lang::Python));
    let comment_colored = |t: &mut T| match &t.app().ed.page().get(&id).unwrap().kind {
        Kind::Box(b) => b.label.text.style_at(8).is_some_and(|s| s.color.is_some()),
        _ => false,
    };
    assert!(comment_colored(&mut t));
    t.app().set_code_lang(std::slice::from_ref(&id), Lang::Rust);
    assert!(!comment_colored(&mut t), "`#` is not a comment in Rust");
}

#[test]
fn editing_a_code_block_recolors_it_with_its_language() {
    use inkboard_core::highlight::Lang;
    let mut t = T::new();
    t.app().insert_code_block();
    let id = t.objects()[0].id.clone();
    t.app().set_code_lang(std::slice::from_ref(&id), Lang::Sql);
    t.app().start_editing(&id, None);
    t.app().editing.as_mut().unwrap().buf = "SELECT 1".into();
    t.app().commit_editing();
    let Kind::Box(b) = &t.app().ed.page().get(&id).unwrap().kind else { panic!("not a box") };
    assert_eq!(b.label.text.to_plain(), "SELECT 1");
    assert!(b.label.text.style_at(0).is_some_and(|s| s.color.is_some()), "the keyword is colored after the edit");
}

#[test]
fn starting_a_timer_resets_the_time_is_up_warning() {
    let mut t = T::new();
    t.app().timer_done = true;
    t.app().start_timer(5);
    assert!(!t.app().timer_done);
    let (end, total) = t.app().timer.unwrap();
    assert_eq!(total, std::time::Duration::from_secs(300));
    assert!(end > std::time::Instant::now());
}

/* ------------------------------------------------------------ icon library */

#[test]
fn library_icons_are_inserted_as_svg_images_tinted_when_monochrome() {
    use inkboard_icons::{IconRef, Library};
    let lib = Library::open(include_bytes!("../assets/icons.bin")).unwrap();
    assert!(lib.icon_count() > 20_000, "{} icons", lib.icon_count());
    let mut t = T::new();
    let mono = lib.sets.iter().position(|s| s.mono).unwrap();
    let colored = lib.sets.iter().position(|s| !s.mono).unwrap();
    t.app().insert_icon(&lib, IconRef { set: mono, icon: 0 });
    t.app().insert_icon(&lib, IconRef { set: colored, icon: 0 });
    let imgs: Vec<ImageObj> = t.objects().into_iter().filter_map(|o| if let Kind::Image(i) = &o.kind { Some(i.clone()) } else { None }).collect();
    assert_eq!(imgs.len(), 2);
    assert!(imgs[0].mono && imgs[0].is_tinted() && !imgs[1].mono);
    for i in &imgs {
        assert_eq!(t.app().ed.doc.assets.get(&i.asset).unwrap().mime, "image/svg+xml");
        assert!((i.frame.w.max(i.frame.h) - 96.0).abs() < 1e-6, "longest side 96, got {:?}", i.frame);
    }
    // search across the whole library finds vendor icons by name
    assert!(!lib.search("lambda", 5).is_empty() && !lib.search("router", 5).is_empty() && !lib.search("github", 5).is_empty());
    // one undo step per insertion
    t.app().ed.undo();
    assert_eq!(t.objects().len(), 1);
}

#[test]
fn the_icon_palette_opens_filters_and_renders_thumbnails() {
    let mut t = T::new();
    t.app().palette.toggle();
    t.app().palette.group = Some("Diagram shapes".into());
    for _ in 0..6 {
        t.h.step();
    }
    assert!(t.app().palette.open);
    t.app().palette.query = "flowchart".into();
    for _ in 0..3 {
        t.h.step();
    }
    t.app().palette.toggle();
    t.h.step();
    assert!(!t.app().palette.open);
}

/// Every icon of the pack parses and rasterizes (slow: run with `cargo test --release -p inkboard-app -- --ignored`).
#[test]
#[ignore]
fn every_library_icon_renders() {
    use inkboard_icons::{IconRef, Library};
    let lib = Library::open(include_bytes!("../assets/icons.bin")).unwrap();
    let mut failures = Vec::new();
    for (si, s) in lib.sets.iter().enumerate() {
        for (ii, name) in s.names.iter().enumerate() {
            let svg = lib.svg(IconRef { set: si, icon: ii }).unwrap();
            if inkboard_formats::raster::svg_to_raster(&svg, 0.1, false).is_err() {
                failures.push(format!("{} / {name}", s.title));
            }
        }
    }
    assert!(failures.is_empty(), "{} icons do not render, e.g. {:?}", failures.len(), &failures[..failures.len().min(10)]);
}

/* ------------------------------------------------------------ svg resolution */

#[test]
fn svg_images_are_rasterized_at_the_size_they_are_shown() {
    use crate::render::TextureCache;
    let ctx = egui::Context::default();
    let mut t = T::new();
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" width="20" height="20"><circle cx="10" cy="10" r="9" fill="#08f"/></svg>"##;
    let aid = t.app().ed.doc.add_asset("image/svg+xml", svg.to_vec());
    let asset = t.app().ed.doc.assets.get(&aid).unwrap().clone();
    let mut cache = TextureCache::default();
    // power-of-two levels: never stretched, at most halved (one request per frame: the time budget never interferes)
    let get = |cache: &mut TextureCache, px: f32| {
        let _ = ctx.run(Default::default(), |_| {});
        cache.get(&ctx, &aid, &asset, px).unwrap().size()
    };
    assert_eq!(get(&mut cache, 40.0), [64, 64], "small");
    assert_eq!(get(&mut cache, 700.0), [1024, 1024], "enlarged icon: sharp, not a stretched thumbnail");
    assert_eq!(get(&mut cache, 1500.0), [2048, 2048]);
    assert_eq!(get(&mut cache, 40.0), [64, 64], "already cached levels are free");
    // with the frame's rasterization budget spent, a request is answered with the closest existing texture
    cache.raster_budget_ms = 0.0;
    let _ = ctx.run(Default::default(), |_| {});
    assert_eq!(
        cache.get(&ctx, &aid, &asset, 3000.0).unwrap().size(),
        [2048, 2048],
        "the first request of a frame is always served (capped by the texture size limit of this context)"
    );
    let other = t
        .app()
        .ed
        .doc
        .add_asset("image/svg+xml", br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"##.to_vec());
    let other_asset = t.app().ed.doc.assets.get(&other).unwrap().clone();
    assert!(cache.get(&ctx, &other, &other_asset, 90.0).is_some(), "first of the frame");
    assert!(cache.get(&ctx, &aid, &asset, 1500.0).unwrap().size()[0] >= 2048);
    assert!(cache.stats.deferred == 0, "cached levels need no rasterization");
    assert!(cache.get(&ctx, &other, &other_asset, 700.0).is_some_and(|t| t.size() == [128, 128]), "deferred: closest texture");
    assert_eq!(cache.stats.deferred, 1);
}

/// Many small icons are drawn from a shared atlas and load over several frames, with bounded rasterization work per frame.
#[test]
fn many_small_icons_share_atlas_pages_and_load_over_several_frames() {
    use crate::render::TextureCache;
    let ctx = egui::Context::default();
    let lib = inkboard_icons::Library::open(inkboard_icons::PACK).unwrap();
    let doc = crate::bench_icons::document(&lib, 300, 300);
    let assets: Vec<(Id, Asset)> = doc.assets.iter().map(|(id, a)| (id.clone(), a.clone())).collect();
    assert!(assets.len() > 250, "{} distinct assets", assets.len());
    let mut cache = TextureCache::default();
    // a zero budget allows exactly one rasterization per frame (the first of a frame is always served): deterministic, whatever the machine
    cache.raster_budget_ms = 0.0;
    let mut ids = std::collections::HashSet::new();
    let mut frames = 0;
    loop {
        let _ = ctx.run(Default::default(), |_| {});
        frames += 1;
        let before = cache.stats.rasterized;
        ids.clear();
        let mut ready = 0;
        for (id, a) in &assets {
            if let Some(t) = cache.get(&ctx, id, a, 20.0) {
                ready += 1;
                ids.insert(t.id);
            }
        }
        let done = cache.stats.rasterized - before;
        assert!(done <= 1, "{done} rasterizations in one frame with no budget");
        if ready == assets.len() {
            break;
        }
        assert_eq!(done, 1, "progress every frame");
        assert!(frames <= assets.len(), "{ready}/{} icons after {frames} frames", assets.len());
    }
    assert_eq!(frames, assets.len(), "one icon per frame");
    assert!(ids.len() <= 2, "{} texture pages for {} small icons: they must share the atlas", ids.len(), assets.len());
    assert_eq!(cache.stats.rasterized as usize, assets.len(), "each icon rasterized once");
    assert_eq!(cache.stats.evicted, 0);
    // asking again is free
    let before = cache.stats;
    let _ = ctx.run(Default::default(), |_| {});
    for (id, a) in &assets {
        assert!(cache.get(&ctx, id, a, 20.0).is_some());
    }
    assert_eq!(cache.stats, before);
}

/* ------------------------------------------------------------ text reliability */

/// Adds a box of the given geometry with a text and returns its id.
fn add_text_box(t: &mut T, geom: BoxGeom, frame: Frame, text: &str, align: Align) -> Id {
    let o = Object::new_box(geom, frame, Style::default(), Label::new(text, 20.0, align));
    t.app().ed.insert(o)
}

/// Where the final rendering puts the lines of the box's text (screen coordinates: left, top), computed with the shared placement function.
fn final_rows(t: &mut T, id: &Id) -> Vec<(f32, f32)> {
    let Kind::Box(b) = t.app().ed.page().get(id).unwrap().kind.clone() else { panic!("not a box") };
    let lay = b.layout(&t.app().measurer);
    let origins = crate::render::line_origins(&lay, b.label.align, b.label_rect());
    lay.lines
        .iter()
        .zip(origins)
        .filter(|(line, _)| !line.is_empty())
        .map(|(_, (x, y))| {
            let w = b.frame.to_world(Pt::new(x, y));
            let p = t.at(w.x, w.y);
            (p.x, p.y)
        })
        .collect()
}

fn editor_rows(t: &mut T) -> Vec<(f32, f32)> {
    t.app().editor_probe.clone().map(|p| p.rows).unwrap_or_default()
}

fn assert_rows_match(editor: &[(f32, f32)], expected: &[(f32, f32)], what: &str) {
    assert_eq!(editor.len(), expected.len(), "{what}: number of rows {editor:?} vs {expected:?}");
    for (i, (a, b)) in editor.iter().zip(expected).enumerate() {
        assert!(
            (a.0 - b.0).abs() < 2.0 && (a.1 - b.1).abs() < 2.5,
            "{what}: row {i} is at {a:?} in the editor but {b:?} in the final rendering (all rows: {editor:?} vs {expected:?})"
        );
    }
}

/// Whatever the shape, the alignment and the text, the editor puts every line exactly where the final rendering will, so nothing moves when
/// editing starts or ends.
#[test]
fn the_editor_shows_the_text_where_the_final_rendering_puts_it_for_every_shape_and_alignment() {
    let texts = ["Hello", "Hello\nall of you,\nI am called Victor Letirand", "a long text that has to wrap inside the shape because it is wider than it"];
    for geom in [BoxGeom::Rect, BoxGeom::Ellipse, BoxGeom::Diamond, BoxGeom::Triangle, BoxGeom::Sticky, BoxGeom::Text] {
        for align in [Align::Left, Align::Center, Align::Right] {
            for text in texts {
                let mut t = T::new();
                let id = add_text_box(&mut t, geom, Frame::new(300.0, 200.0, 260.0, 180.0), text, align);
                t.app().sel = vec![id.clone()];
                t.h.step();
                let before = final_rows(&mut t, &id);
                t.app().start_editing(&id, None);
                for _ in 0..3 {
                    t.h.step();
                }
                let what = format!("{geom:?} {align:?} {:?}", &text[..text.len().min(12)]);
                let after_live_fit = final_rows(&mut t, &id);
                assert_rows_match(&editor_rows(&mut t), &after_live_fit, &what);
                if geom != BoxGeom::Text {
                    // shapes keep their frame unless the text overflows: the lines did not move when editing started
                    let f = t.frame(&id);
                    if (f.h - 180.0).abs() < 1e-9 {
                        assert_rows_match(&before, &after_live_fit, &format!("{what} (start of editing)"));
                    }
                }
                // ending the editing changes nothing visible either
                let editing = editor_rows(&mut t);
                t.app().commit_editing();
                t.h.step();
                assert_rows_match(&editing, &final_rows(&mut t, &id), &format!("{what} (end of editing)"));
            }
        }
    }
}

/// The last word of a paragraph has no space after it: the editor (egui) and the final rendering must still break the line at the same place when
/// that word ends within one space width of the wrap width. The width is built from the measurer, so the case does not depend on the fonts.
#[test]
fn the_editor_and_the_final_rendering_agree_when_the_last_word_ends_just_past_the_wrap_width() {
    let mut t = T::new();
    let m = t.app().measurer.clone();
    let w = |s: &str| inkboard_core::text::Measurer::width(&m, s, FontFamily::Sans, 20.0, false);
    // a rectangle's text area is 16 units narrower than the frame: make it a quarter of a space narrower than "Victor Letirand"
    let frame_w = w("Victor Letirand") + 16.0 - w(" ") / 4.0;
    let id = add_text_box(&mut t, BoxGeom::Rect, Frame::new(300.0, 200.0, frame_w, 180.0), "Victor Letirand", Align::Left);
    t.app().sel = vec![id.clone()];
    t.h.step();
    t.app().start_editing(&id, None);
    for _ in 0..3 {
        t.h.step();
    }
    let (editor, fin) = (editor_rows(&mut t), final_rows(&mut t, &id));
    assert_eq!(fin.len(), 1, "the last word is allowed to end within one space of the wrap width: {fin:?}");
    assert_rows_match(&editor, &fin, "Rect Left \"Victor Letirand\"");
}

/// A new free text starts exactly where the user clicked, grows with what is typed, and its frame always contains the text.
#[test]
fn a_new_free_text_is_anchored_at_the_click_and_its_frame_follows_the_typing() {
    let mut t = T::new();
    t.tool(Tool::Text);
    let p = t.at(200.0, 150.0);
    t.h.event(Event::PointerMoved(p));
    t.h.step();
    t.button(p, true, Modifiers::NONE);
    t.button(p, false, Modifiers::NONE);
    t.h.step();
    assert!(t.app().editing.is_some(), "typing can start");
    let id = t.app().editing.as_ref().unwrap().id.clone();
    type_text(&mut t, "Hello");
    let f = t.frame(&id);
    let rows = editor_rows(&mut t);
    assert_eq!(rows.len(), 1);
    let left = t.at(f.x, f.y).x;
    // the text starts inside its frame (3 units of margin), not left of it
    assert!((rows[0].0 - (left + 3.0)).abs() < 2.0, "text at {} but frame at {left}", rows[0].0);
    assert!(f.w > 30.0, "the frame grew with the text: {f:?}");
    let right = t.at(f.x + f.w, f.y).x;
    assert!(rows[0].0 + 20.0 < right, "text is inside the frame");
    // more text: the frame keeps following, still anchored at the same point
    type_text(&mut t, " world and more");
    let g = t.frame(&id);
    assert!(g.w > f.w + 50.0 && (g.x - f.x).abs() < 1e-9 && (g.y - f.y).abs() < 1e-9, "{f:?} -> {g:?}");
    // validated: no jump
    t.app().commit_editing();
    assert!((t.frame(&id).w - g.w).abs() < 1e-9, "no jump on validation");
}

/// Undoing a text edit gives back the text and the size the shape had before, in one step.
#[test]
fn undoing_a_text_edit_restores_the_text_and_the_size_in_one_step() {
    let mut t = T::new();
    let id = add_text_box(&mut t, BoxGeom::Rect, Frame::new(300.0, 200.0, 120.0, 50.0), "a", Align::Center);
    let original = t.frame(&id);
    t.app().start_editing(&id, None);
    t.app().editing.as_mut().unwrap().buf = "a text long enough to overflow a small rectangle and make it grow taller".into();
    for _ in 0..3 {
        t.h.step();
    }
    assert!(t.frame(&id).h > original.h, "the shape grows while typing");
    t.app().commit_editing();
    assert_eq!(label_of(&mut t, &id).to_plain(), "a text long enough to overflow a small rectangle and make it grow taller");
    let grown = t.frame(&id);
    assert!(grown.h > original.h);
    t.app().ed.undo();
    assert_eq!(label_of(&mut t, &id).to_plain(), "a");
    assert_eq!(t.frame(&id), original, "size restored by the same undo");
    t.app().ed.redo();
    assert_eq!(t.frame(&id), grown, "and redo gives the grown shape back");
}

/// The text being typed is never lost: saving, switching page or the safety copy include it.
#[test]
fn typed_text_survives_saving_page_changes_and_the_safety_copy() {
    let mut t = T::new();
    let id = add_text_box(&mut t, BoxGeom::Rect, Frame::new(300.0, 200.0, 200.0, 80.0), "old", Align::Center);
    let path = std::env::temp_dir().join(format!("wb-pending-{}.inkboard", std::process::id()));
    t.app().path = Some(path.clone());
    t.app().start_editing(&id, None);
    t.app().editing.as_mut().unwrap().buf = "typed but not validated".into();
    t.h.step();
    // the document itself is untouched, but the pending copy has the text and the application counts as modified
    assert_eq!(label_of(&mut t, &id).to_plain(), "old");
    assert!(t.app().dirty());
    let pending = t.app().pending_document().expect("something is pending");
    let Kind::Box(b) = &pending.pages[0].get(&id).unwrap().kind else { panic!() };
    assert_eq!(b.label.text.to_plain(), "typed but not validated");
    // saving validates the text first
    t.app().run_action(crate::keymap::Action::Save);
    let saved = inkboard_core::serial::load(&path).unwrap();
    let Kind::Box(b) = &saved.pages[0].get(&id).unwrap().kind else { panic!() };
    assert_eq!(b.label.text.to_plain(), "typed but not validated");
    let _ = std::fs::remove_file(&path);
    assert!(t.app().editing.is_none());
    // changing page keeps what was typed
    t.app().ed.add_page("Two");
    let two = t.app().ed.current.clone();
    t.app().ed.current = t.app().ed.doc.pages[0].id.clone();
    assert_ne!(t.app().ed.current, two);
    t.app().start_editing(&id, None);
    t.app().editing.as_mut().unwrap().buf = "second edit".into();
    t.app().step_page(1);
    assert_eq!(t.app().ed.current, two);
    let Kind::Box(b) = &t.app().ed.doc.pages[0].get(&id).unwrap().kind else { panic!() };
    assert_eq!(b.label.text.to_plain(), "second edit");
}

/// The alignment chosen from the formatting bar is shown at once while editing and is one undo step with the text.
#[test]
fn text_alignment_chosen_while_editing_is_live_and_undoable_with_the_text() {
    let mut t = T::new();
    let id = add_text_box(&mut t, BoxGeom::Rect, Frame::new(300.0, 200.0, 240.0, 100.0), "Hello world", Align::Left);
    t.app().start_editing(&id, None);
    t.h.step();
    let left = editor_rows(&mut t)[0].0;
    t.app().editing.as_mut().unwrap().align = Some(Align::Right);
    for _ in 0..3 {
        t.h.step();
    }
    let right = editor_rows(&mut t)[0].0;
    assert!(right > left + 40.0, "right-aligned text moved to the right ({left} -> {right})");
    assert_rows_match(&editor_rows(&mut t), &final_rows(&mut t, &id), "right-aligned while editing");
    t.app().commit_editing();
    let Kind::Box(b) = t.app().ed.page().get(&id).unwrap().kind.clone() else { panic!() };
    assert_eq!(b.label.align, Align::Right);
    t.app().ed.undo();
    let Kind::Box(b) = t.app().ed.page().get(&id).unwrap().kind.clone() else { panic!() };
    assert_eq!(b.label.align, Align::Left, "undone with a single step");
}

/// The caret is drawn where the typed text ends (not beside it), whatever the alignment, also on a line that ends with a line break.
#[test]
fn the_caret_follows_the_text_for_every_alignment() {
    for align in [Align::Left, Align::Center, Align::Right] {
        let mut t = T::new();
        let id = add_text_box(
            &mut t,
            BoxGeom::Rect,
            Frame::new(300.0, 200.0, 260.0, 180.0),
            "Hello
all of you",
            align,
        );
        t.app().start_editing(&id, None);
        t.h.step();
        // caret after "Hello" (before the line break)
        t.app().editing.as_mut().unwrap().restore_sel = Some((5, 5));
        for _ in 0..3 {
            t.h.step();
        }
        let rows = editor_rows(&mut t);
        let caret = t.app().editor_probe.clone().and_then(|p| p.caret).expect("a caret");
        let m = t.app().measurer.clone();
        let w = f32::from_bits((inkboard_core::text::Measurer::width(&m, "Hello", FontFamily::Sans, 20.0, false) as f32).to_bits());
        let end = rows[0].0 + w;
        assert!((caret.0 - end).abs() < 3.0, "{align:?}: caret at {} but the text ends at {end}", caret.0);
    }
}

/* --------------------------------------------- opening a corrupted file */

#[test]
fn opening_a_corrupted_file_shows_an_error_and_keeps_the_current_board() {
    let mut t = T::new();
    t.add_rect(10.0, 10.0, 50.0, 50.0);
    t.add_rect(100.0, 10.0, 50.0, 50.0);
    let doc = t.app().ed.doc.clone();
    let revision = t.app().ed.revision();
    let dir = std::env::temp_dir().join(format!("inkboard-app-corrupt-{}", Id::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = inkboard_core::serial::to_json(&doc, false);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("truncated.inkboard.json", good.as_bytes()[..good.len() / 2].to_vec()),
        ("garbage.inkboard.json", b"\x00\x01 not json at all".to_vec()),
        ("too-new.inkboard.json", br#"{"format":"inkboard","version":99,"document":{"name":"x","pages":[]}}"#.to_vec()),
        ("foreign.inkboard.json", br#"{"format":"other","version":1,"document":{"name":"x","pages":[]}}"#.to_vec()),
        ("broken.drawio", b"<mxfile><diagram><mxGraphModel".to_vec()),
        ("not-utf8.inkboard.json", vec![0xff, 0xfe, 0xfd]),
    ];
    for (name, bytes) in cases {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        t.app().toast = None;
        t.app().open_path(&path);
        t.h.step();
        let toast = t.app().toast.clone().map(|(m, _)| m);
        assert!(toast.as_deref().is_some_and(|m| !m.trim().is_empty()), "{name}: an error must be shown");
        assert_eq!(t.app().ed.doc, doc, "{name}: the current board must be kept");
        assert_eq!(t.app().ed.revision(), revision, "{name}: nothing must be recorded in the history");
        assert!(t.app().path.is_none(), "{name}: the current file must not change");
    }
    t.app().toast = None;
    t.app().open_path(&dir.join("missing.inkboard.json"));
    assert!(t.app().toast.is_some(), "a missing file is reported");
    assert_eq!(t.app().ed.doc, doc);
    std::fs::remove_dir_all(&dir).ok();
}

/* ------------------------------------------- live mode: the tools of the MCP v2 plan */

#[test]
fn live_batch_is_one_undo_step_and_a_failed_atomic_batch_leaves_no_trace() {
    let mut t = T::new();
    let live = start_live(&mut t, "batch");
    let ops = serde_json::json!([
        {"tool": "add_shape", "args": {"geom": "rect", "text": "A"}},
        {"tool": "add_shape", "args": {"geom": "ellipse", "text": "B"}},
        {"tool": "add_connector", "args": {"from": "$0.id", "to": "$1.id", "label": "**go**", "markdown": true}},
    ]);
    let (err, r) = mcp_call(&mut t, &live, "batch", serde_json::json!({"operations": ops}));
    assert!(!err, "{r}");
    assert_eq!(t.objects().len(), 3);
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(t.objects().is_empty(), "the whole batch is one undo step");
    t.app().ed.redo();
    assert_eq!(t.objects().len(), 3, "and one redo step");

    // atomic failure: the board, the revision and the history are untouched
    let (rev, can_redo) = (t.app().ed.revision(), t.app().ed.can_redo());
    let bad = serde_json::json!([
        {"tool": "add_shape", "args": {"geom": "rect", "text": "C"}},
        {"tool": "add_shape", "args": {"geom": "hexagon"}},
    ]);
    let (err, r) = mcp_call(&mut t, &live, "batch", serde_json::json!({"operations": bad}));
    assert!(err, "{r}");
    assert_eq!(t.objects().len(), 3, "nothing of the failed batch remains");
    assert_eq!(t.app().ed.revision(), rev, "the board does not look modified");
    assert_eq!(t.app().ed.can_redo(), can_redo);
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(t.objects().is_empty(), "the history still holds the first batch as a single step");
    let _ = std::fs::remove_file(&live.file);
}

#[test]
fn live_read_only_tools_never_modify_the_board() {
    let mut t = T::new();
    let live = start_live(&mut t, "readonly");
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect", "text": "Hello **x**", "markdown": true}));
    assert!(!err, "{r}");
    let (rev, undo) = (t.app().ed.revision(), t.app().ed.can_undo());
    for (tool, args) in [
        ("describe_board", serde_json::json!({"detail": "full"})),
        ("find_objects", serde_json::json!({"text": "hello"})),
        ("search_icons", serde_json::json!({"query": "lambda"})),
        ("list_icon_groups", serde_json::json!({})),
    ] {
        let (err, r) = mcp_call(&mut t, &live, tool, args);
        assert!(!err, "{tool}: {r}");
    }
    // `render` answers with an image block first, then the text of the picture
    let r = mcp_rpc(&mut t, &live, "tools/call", serde_json::json!({"name": "render", "arguments": {"show_ids": true, "max_px": 400}}));
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(r["result"]["content"][0]["type"], "image");
    assert_eq!(r["result"]["content"][0]["mimeType"], "image/png");
    assert_eq!(r["result"]["content"][1]["type"], "text");
    assert_eq!((t.app().ed.revision(), t.app().ed.can_undo()), (rev, undo), "reading tools do not touch the board");
    let _ = std::fs::remove_file(&live.file);
}

#[test]
fn live_icon_text_and_edit_tools_are_one_undo_step_each() {
    let mut t = T::new();
    let live = start_live(&mut t, "v2");
    // an icon with its label: two objects, one step
    let (err, r) = mcp_call(&mut t, &live, "search_icons", serde_json::json!({"query": "lambda", "group": "AWS", "limit": 1}));
    assert!(!err, "{r}");
    let icon = r["icons"][0]["id"].as_str().expect("an icon").to_owned();
    let (err, r) = mcp_call(&mut t, &live, "add_icon", serde_json::json!({"icon": icon, "label": "Function"}));
    assert!(!err, "{r}");
    assert_eq!(t.objects().len(), 2);
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(t.objects().is_empty(), "icon and label are one undo step");
    // a shape, formatted words, moved, arranged: one step each
    let (_, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect", "text": "Important words"}));
    let a = r["id"].as_str().unwrap().to_owned();
    let (_, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"geom": "rect", "text": "Other", "x": 400, "y": 300}));
    let b = r["id"].as_str().unwrap().to_owned();
    let (err, r) = mcp_call(&mut t, &live, "format_text", serde_json::json!({"id": a, "match": "Important", "bold": true, "color": "#e03131"}));
    assert!(!err, "{r}");
    let (err, r) = mcp_call(&mut t, &live, "move_objects", serde_json::json!({"ids": [a], "dx": 10, "dy": 0}));
    assert!(!err, "{r}");
    let (err, r) = mcp_call(&mut t, &live, "arrange_objects", serde_json::json!({"ids": [a, b], "action": "align_top"}));
    assert!(!err, "{r}");
    let top = |t: &mut T, id: &str| t.objects().iter().find(|o| o.id.0 == id).and_then(|o| o.frame()).map(|f| f.y).unwrap();
    let left = |t: &mut T, id: &str| t.objects().iter().find(|o| o.id.0 == id).and_then(|o| o.frame()).map(|f| f.x).unwrap();
    assert_eq!(top(&mut t, &a), top(&mut t, &b), "aligned");
    let moved = left(&mut t, &a);
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert_ne!(top(&mut t, &a), top(&mut t, &b), "the alignment alone is undone");
    assert_eq!(left(&mut t, &a), moved, "the move is still there");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!((moved - left(&mut t, &a) - 10.0).abs() < 1e-9, "the move alone is undone");
    let _ = std::fs::remove_file(&live.file);
}

/* ------------------------------------------------------------ images: paste and drop */

fn image_objects(t: &mut T) -> Vec<Object> {
    t.objects().into_iter().filter(|o| matches!(o.kind, Kind::Image(_))).collect()
}

/// The release of V (the key press itself never reaches the application when the clipboard holds no text).
fn release_v(t: &mut T, m: Modifiers) {
    t.h.event(Event::Key { key: Key::V, physical_key: None, pressed: false, repeat: false, modifiers: m });
    t.h.step();
}

fn temp_file(tag: &str, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wb-img-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn ctrl_v_with_only_an_image_on_the_clipboard_pastes_it() {
    let mut t = T::new();
    t.app().system_image = || Some((2, 2, vec![255; 16]));
    release_v(&mut t, Modifiers::COMMAND);
    assert_eq!(image_objects(&mut t).len(), 1, "the clipboard image is on the board");
    assert!(t.app().ed.can_undo());
    // the release of a plain V (the letter, no Ctrl) pastes nothing
    release_v(&mut t, Modifiers::NONE);
    assert_eq!(image_objects(&mut t).len(), 1);
}

#[test]
fn no_second_paste_when_the_clipboard_has_text_and_the_paste_event_arrived() {
    let mut t = T::new();
    t.app().system_image = || Some((2, 2, vec![255; 16]));
    let id = t.add_rect(0.0, 0.0, 80.0, 40.0);
    t.app().sel = vec![id];
    t.app().copy_selection();
    t.h.event(Event::Paste(crate::clipboard::MARKER.to_owned()));
    t.h.step();
    assert_eq!(t.objects().len(), 2, "the copied shape was pasted");
    release_v(&mut t, Modifiers::COMMAND);
    assert_eq!(t.objects().len(), 2, "the key release does not paste again, nor adds the clipboard image");
}

#[test]
fn dropped_image_files_are_inserted_and_other_files_are_refused() {
    let mut t = T::new();
    let mut png = Vec::new();
    image::RgbaImage::new(200, 100).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let png = temp_file("drop", "photo.png", &png);
    let svg = temp_file("drop", "icon.SVG", br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/></svg>"#);
    let txt = temp_file("drop", "notes.txt", b"hello");
    let file = |p: &std::path::PathBuf| egui::DroppedFile { path: Some(p.clone()), ..Default::default() };
    t.h.input_mut().dropped_files = vec![file(&png), file(&svg), file(&txt)];
    t.h.step();
    t.h.input_mut().dropped_files.clear();
    t.h.step();
    let images = image_objects(&mut t);
    assert_eq!(images.len(), 2, "the PNG and the SVG, not the text file");
    let mimes: Vec<String> = images
        .iter()
        .filter_map(|o| match &o.kind {
            Kind::Image(i) => t.app().ed.doc.assets.get(&i.asset).map(|a| a.mime.clone()),
            _ => None,
        })
        .collect();
    assert!(mimes.contains(&"image/png".to_owned()) && mimes.contains(&"image/svg+xml".to_owned()), "{mimes:?}");
    let widths: Vec<f64> = images.iter().filter_map(|o| o.frame().map(|f| f.w)).collect();
    assert!(widths.iter().any(|w| (*w - 200.0).abs() < 1e-6), "the PNG keeps its size: {widths:?}");
    assert!(widths.iter().any(|w| *w > 100.0), "an icon-sized SVG is enlarged: {widths:?}");
    let toast = t.app().toast.as_ref().map(|(m, _)| m.clone());
    assert_eq!(toast.as_deref(), Some(crate::i18n::t("Only images can be dropped here").as_str()));
    for p in [png, svg, txt] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn an_unreadable_svg_is_refused_without_changing_the_board() {
    let mut t = T::new();
    let bad = temp_file("badsvg", "bad.svg", b"<svg><path/></svg>");
    let at = t.app().view_center();
    t.app().insert_image_files(std::slice::from_ref(&bad), at);
    assert!(t.objects().is_empty());
    let _ = std::fs::remove_file(bad);
}
