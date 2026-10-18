//! UI tests: we simulate the mouse and keyboard in a real egui cycle (without a window).

use crate::app::App;
use crate::tools::Tool;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use inkboard_core::*;

struct T {
    h: Harness<'static, App>,
}

impl T {
    fn new() -> Self {
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
    fn tool(&mut self, tool: Tool) {
        self.app().set_tool(tool);
        for _ in 0..3 {
            self.h.step();
        }
    }
    fn app(&mut self) -> &mut App {
        self.h.state_mut()
    }
    /// Screen position of a world point (view at scale 1, canvas origin).
    fn at(&mut self, x: f64, y: f64) -> Pos2 {
        let r = self.app().canvas_rect;
        let v = self.app().ed.page().view;
        pos2(r.min.x + (x * v.zoom + v.x) as f32, r.min.y + (y * v.zoom + v.y) as f32)
    }
    fn mv(&mut self, p: Pos2) {
        self.h.event(Event::PointerMoved(p));
        self.h.step();
    }
    fn button(&mut self, p: Pos2, pressed: bool, modifiers: Modifiers) {
        self.h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers });
        self.h.step();
    }
    fn drag_mods(&mut self, from: (f64, f64), to: (f64, f64), mods: Modifiers) {
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
    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) {
        self.drag_mods(from, to, Modifiers::NONE);
    }
    fn click(&mut self, at: (f64, f64)) {
        let p = self.at(at.0, at.1);
        self.mv(p);
        self.button(p, true, Modifiers::NONE);
        self.button(p, false, Modifiers::NONE);
    }
    fn objects(&mut self) -> Vec<Object> {
        self.app().ed.page().objects.clone()
    }
    fn boxes(&mut self) -> Vec<(Object, BoxObj)> {
        self.objects().into_iter().filter_map(|o| if let Kind::Box(b) = o.kind.clone() { Some((o, b)) } else { None }).collect()
    }
    fn add_rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> Id {
        let o = Object::new_box(BoxGeom::Rect, Frame::new(x, y, w, h), Style { fill: Some(Color::rgb(200, 220, 255)), ..Style::default() }, Label::default());
        self.app().ed.insert(o)
    }
    fn frame(&mut self, id: &Id) -> Frame {
        self.app().ed.page().get(id).and_then(Object::frame).expect("object with a frame")
    }
    fn key(&mut self, k: Key) {
        self.h.key_press(k);
        self.h.step();
        self.h.step();
    }
    fn key_mod(&mut self, m: Modifiers, k: Key) {
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
    std::env::temp_dir().join(format!("wb-watch-{tag}-{}.inkboard.json", std::process::id()))
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
fn live_calls_target_the_open_file_only() {
    let mut t = T::new();
    let live = start_live(&mut t, "path");
    let mine = std::env::temp_dir().join(format!("wb-live-mine-{}.inkboard.json", std::process::id()));
    t.add_rect(0.0, 0.0, 50.0, 50.0);
    t.app().save_to(&mine);
    // same file: applied in the application
    let (err, r) = mcp_call(&mut t, &live, "add_shape", serde_json::json!({"path": mine.to_str().unwrap(), "geom": "ellipse"}));
    assert!(!err, "{r}");
    assert_eq!(t.objects().len(), 2);
    // other file: the application is untouched, file mode takes over
    let other = std::env::temp_dir().join(format!("wb-live-other-{}.inkboard.json", std::process::id()));
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
    assert!(r.left() >= 8.0 && r.width() < 260.0, "narrow floating card: {r:?}");
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
