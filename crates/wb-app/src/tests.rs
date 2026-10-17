//! Tests d'interface : on simule la souris et le clavier dans un vrai cycle egui (sans fenêtre).

use crate::app::App;
use crate::tools::Tool;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use wb_core::*;

struct T {
    h: Harness<'static, App>,
}

impl T {
    fn new() -> Self {
        let mut h = Harness::builder().with_size(vec2(1300.0, 850.0)).build_eframe(|cc| {
            let mut app = App::build(cc, None, None, false);
            app.system_image = || None; // jamais le vrai presse-papiers dans les tests
            app
        });
        h.step();
        h.step();
        let mut t = T { h };
        t.app().fit_pending = false;
        t.app().set_view(View { x: 0.0, y: 0.0, zoom: 1.0 });
        t.h.step();
        t
    }
    fn app(&mut self) -> &mut App {
        self.h.state_mut()
    }
    /// Position écran d'un point monde (vue à l'échelle 1, origine du canvas).
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
        self.app().ed.page().get(id).and_then(Object::frame).expect("objet à cadre")
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
    assert!(t.app().canvas_rect.width() > 500.0, "le canvas occupe la fenêtre");
}

#[test]
fn dragging_with_rect_tool_creates_a_rectangle_and_returns_to_select() {
    let mut t = T::new();
    t.app().set_tool(Tool::Shape(BoxGeom::Rect));
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
    t.app().set_tool(Tool::Shape(BoxGeom::Ellipse));
    t.click((400.0, 300.0));
    let b = t.boxes();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].1.geom, BoxGeom::Ellipse);
    assert!((b[0].1.frame.center().x - 400.0).abs() < 1.0 && (b[0].1.frame.w - 150.0).abs() < 1.0);
}

#[test]
fn shift_makes_a_square_and_undo_removes_it() {
    let mut t = T::new();
    t.app().set_tool(Tool::Shape(BoxGeom::Rect));
    t.drag_mods((100.0, 100.0), (300.0, 160.0), Modifiers::SHIFT);
    let f = t.boxes()[0].1.frame;
    assert!((f.w - f.h).abs() < 1.0, "carré : {f:?}");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!(t.objects().is_empty());
    t.key_mod(Modifiers::COMMAND, Key::Y);
    assert_eq!(t.objects().len(), 1);
}

#[test]
fn pen_draws_a_stroke_with_several_points() {
    let mut t = T::new();
    t.app().set_tool(Tool::Pen);
    t.drag((100.0, 100.0), (300.0, 160.0));
    let objs = t.objects();
    assert_eq!(objs.len(), 1);
    let Kind::Pen(p) = &objs[0].kind else { panic!("pas un trait") };
    assert!(p.pts.len() >= 2 && !p.highlighter);
    assert!(p.pts[0].dist(Pt::new(100.0, 100.0)) < 2.0);
}

#[test]
fn highlighter_is_translucent() {
    let mut t = T::new();
    t.app().set_tool(Tool::Highlighter);
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
    t.app().set_tool(Tool::Arrow);
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
    t.app().set_tool(Tool::Line);
    t.drag((160.0, 140.0), (600.0, 500.0));
    let Some(Object { kind: Kind::Connector(c), .. }) = t.objects().into_iter().find(|o| matches!(o.kind, Kind::Connector(_))) else {
        panic!("pas de connecteur")
    };
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
    t.app().set_tool(Tool::Select);
    t.drag((410.0, 110.0), (410.0, 310.0));
    let fb = t.frame(&b);
    assert!((fb.y - 300.0).abs() < 1.5, "{fb:?}");
    let after = path_of(&mut t, &c);
    assert!(after.last().unwrap().y > before.last().unwrap().y + 100.0, "la flèche suit la forme");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!((t.frame(&b).y - 100.0).abs() < 1e-9, "un seul geste = une seule annulation");
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
    t.drag((220.0, 180.0), (320.0, 280.0)); // poignée sud-est
    let f = t.frame(&a);
    assert!((f.w - 220.0).abs() < 1.5 && (f.h - 180.0).abs() < 1.5, "{f:?}");
    assert!((f.x - 100.0).abs() < 1e-6 && (f.y - 100.0).abs() < 1e-6, "le coin opposé ne bouge pas");
    t.key_mod(Modifiers::COMMAND, Key::Z);
    assert!((t.frame(&a).w - 120.0).abs() < 1e-9);
}

#[test]
fn rotation_handle_rotates_the_shape() {
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 120.0, 80.0);
    t.app().sel = vec![a.clone()];
    t.h.step();
    // poignée de rotation : 28 px au-dessus du bord haut, au milieu (160, 100 - 28)
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
    assert!(matches!(cn.start, End::Free { .. }), "la flèche est détachée, pas supprimée");
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
    t.app().set_tool(Tool::Eraser);
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
    assert!((t.frame(&a).x - 100.0).abs() < 1e-9, "l'original reste en place");
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
    t.app().editing.as_mut().unwrap().buf = "Un texte assez long pour déborder".into();
    t.app().commit_editing();
    let Kind::Box(b) = &t.app().ed.page().get(&a).unwrap().kind else { unreachable!() };
    assert_eq!(b.label.text.to_plain(), "Un texte assez long pour déborder");
    assert!(b.frame.h > 30.0, "la forme grandit pour contenir le texte");
}

#[test]
fn empty_fresh_text_is_discarded_without_polluting_history() {
    let mut t = T::new();
    t.app().set_tool(Tool::Text);
    t.click((300.0, 300.0));
    assert!(t.app().editing.is_some(), "l'éditeur s'ouvre");
    t.app().commit_editing();
    assert!(t.objects().is_empty());
    assert!(!t.app().ed.can_undo(), "rien à annuler : création et abandon s'annulent");
}

#[test]
fn table_tool_inserts_a_table_and_cells_are_editable() {
    let mut t = T::new();
    t.app().set_tool(Tool::Table);
    t.click((400.0, 300.0));
    let objs = t.objects();
    let Kind::Table(tb) = &objs[0].kind else { panic!("pas de tableau") };
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
    assert_eq!(groups.len(), 2, "la copie reçoit son propre groupe");
}

#[test]
fn mermaid_import_adds_bound_objects_and_selects_them() {
    let mut t = T::new();
    t.app().import_mermaid("graph TD\n A[Un] --> B[Deux]", false).unwrap();
    t.h.step();
    assert_eq!(t.objects().len(), 3);
    assert_eq!(t.app().sel.len(), 3);
    assert!(t.app().import_mermaid("erDiagram\n A ||--o{ B : x", false).is_err());
}

#[test]
fn save_and_reopen_roundtrip_and_foreign_formats() {
    let mut t = T::new();
    t.add_rect(100.0, 100.0, 50.0, 50.0);
    let dir = std::env::temp_dir().join(format!("wb-app-test-{}", Id::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let native = dir.join("a.tableau.json");
    wb_core::serial::save(&t.app().ed.doc, &native).unwrap();
    let drawio = dir.join("a.drawio");
    let ctx = t.h.ctx.clone();
    let xml = wb_formats::drawio::export(&t.app().ed.doc, &crate::fonts::EguiMeasurer(ctx));
    std::fs::write(&drawio, xml).unwrap();
    let mut t2 = T::new();
    t2.app().open_path(&native);
    assert_eq!(t2.objects().len(), 1);
    assert!(t2.app().path.is_some(), "un fichier natif devient le fichier courant");
    t2.app().open_path(&drawio);
    assert_eq!(t2.objects().len(), 1);
    assert!(t2.app().path.is_none(), "un format d'échange ne remplace pas le fichier courant");
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
    t.app().ed.add_page("Deux");
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
    assert!((wx - 300.0).abs() < 1.0, "le point sous le curseur reste fixe : {wx}");
}

/* ------------------------------------------------------------ texte riche */

use wb_core::text::Format;

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
    type_text(&mut t, "Bonjour monde");
    let e = t.app().editing.as_ref().unwrap();
    assert_eq!(e.buf, "Bonjour monde");
    assert_eq!(e.rich.to_plain(), "Bonjour monde");
    t.app().commit_editing();
    assert_eq!(label_of(&mut t, &a).to_plain(), "Bonjour monde");
}

#[test]
fn keyboard_selection_then_ctrl_b_bolds_only_the_selected_word() {
    let mut t = T::new();
    let a = edited_box(&mut t);
    type_text(&mut t, "Bonjour monde");
    t.key(Key::Home);
    for _ in 0..7 {
        t.key_mod(Modifiers::SHIFT, Key::ArrowRight);
    }
    assert_eq!(t.app().editing.as_ref().unwrap().sel, (0, 7), "la sélection au clavier est suivie");
    t.key_mod(Modifiers::COMMAND, Key::B);
    t.h.step();
    t.app().commit_editing();
    let rt = label_of(&mut t, &a);
    assert_eq!(rt.to_plain(), "Bonjour monde");
    assert!(rt.all(0, 7, |s| s.bold), "« Bonjour » en gras : {rt:?}");
    assert!(!rt.all(7, 13, |s| s.bold) && !rt.all(8, 13, |s| s.bold), "« monde » inchangé");
}

#[test]
fn color_and_highlight_apply_to_the_selection() {
    let mut t = T::new();
    let a = edited_box(&mut t);
    type_text(&mut t, "Bonjour monde");
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
    assert!(rt.all(0, 3, |s| s.bold), "la suite de la frappe hérite du gras : {rt:?}");
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
    assert!(rt.all(9, 13, |s| s.italic), "« beta » reste en italique : {rt:?}");
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
    // b est déjà en gras : l'action doit mettre tout le monde en gras (pas inverser b)
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
    assert!(label_of(&mut t, &b).all(0, 4, |s| s.bold), "une annulation = une action de mise en forme");
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

/* ------------------------------------------------- raccourcis personnalisés */

use crate::app::Dialog;
use crate::keymap::{Action, Shortcut};

#[test]
fn rebinding_a_tool_key_takes_effect_immediately() {
    let mut t = T::new();
    t.key(Key::R);
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect));
    t.app().keymap.add(Action::ToolEllipse, Shortcut::key(Key::R)).unwrap();
    t.app().set_tool(Tool::Select);
    t.key(Key::R);
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Ellipse), "R appartient maintenant à l'ellipse");
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
    assert!(t.app().ed.page().get(&a).is_some(), "Retour arrière n'est plus lié");
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
    // l'ancien raccourci par défaut fonctionne toujours (Ctrl+D)
    t.key_mod(Modifiers::COMMAND, Key::D);
    assert_eq!(t.objects().len(), 3);
}

#[test]
fn custom_bold_shortcut_works_while_editing_text() {
    let mut t = T::new();
    t.app().keymap.add(Action::Bold, Shortcut::ctrl(Key::K)).unwrap();
    let a = edited_box(&mut t);
    type_text(&mut t, "Bonjour monde");
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
    assert!(matches!(t.app().dialog, Some(Dialog::Shortcuts { recording: None, .. })), "l'enregistrement s'arrête après la frappe");
    // pendant l'enregistrement, la touche ne déclenche pas son ancienne action
    t.app().dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: Some(Action::ToolPen) });
    t.h.step();
    t.key(Key::V);
    assert_eq!(t.app().tool, Tool::Select);
    assert_eq!(t.app().keymap.action_for(Key::V, Modifiers::NONE), Some(Action::ToolPen), "V est désormais au crayon, repris à la sélection");
    // Échap annule sans rien changer
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

/* ----------------------------------------------------- presse-papiers système */

#[test]
fn pasting_plain_text_creates_a_text_box_in_the_view() {
    let mut t = T::new();
    t.h.event(Event::Paste(
        "Une idée
sur deux lignes"
            .into(),
    ));
    t.h.step();
    let b = t.boxes();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].1.geom, BoxGeom::Text);
    assert_eq!(
        b[0].1.label.text.to_plain(),
        "Une idée
sur deux lignes"
    );
    assert!(b[0].1.frame.w > 20.0 && b[0].1.frame.h > 20.0, "dimensionné d'après le texte");
    assert_eq!(t.app().sel.len(), 1);
}

#[test]
fn pasting_a_system_image_inserts_a_png_asset() {
    let mut t = T::new();
    t.app().system_image = || Some((4, 2, vec![200; 4 * 2 * 4]));
    t.h.event(Event::Paste(String::new()));
    t.h.step();
    let objs = t.objects();
    let Kind::Image(i) = &objs[0].kind else { panic!("pas d'image : {objs:?}") };
    let asset = t.app().ed.doc.assets.get(&i.asset).expect("ressource enregistrée");
    assert_eq!(asset.mime, "image/png");
    let decoded = image::load_from_memory(&asset.data).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (4, 2));
    assert!((i.frame.w / i.frame.h - 2.0).abs() < 1e-9, "proportions conservées");
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
    assert_eq!(t.objects().len(), 2, "objet interne collé, pas une zone de texte avec le repère");
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
    t.app().set_tool(Tool::Table);
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

/* ------------------------------------------------------------ stylet */

fn touch(t: &mut T, phase: egui::TouchPhase, p: Pos2, force: f32) {
    t.h.event(Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(0), phase, pos: p, force: Some(force) });
}

#[test]
fn pen_pressure_from_touch_events_makes_a_variable_width_stroke() {
    let mut t = T::new();
    t.app().set_tool(Tool::Pen);
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
    let Kind::Pen(p) = &objs[0].kind else { panic!("pas de trait") };
    let pr = p.pressures.as_ref().expect("la pression est conservée");
    assert_eq!(pr.len(), p.pts.len(), "une pression par point, même après simplification");
    assert!((pr[0] - 0.1).abs() < 1e-6 && (pr[pr.len() - 1] - 1.0).abs() < 1e-6, "{pr:?}");
    assert!(pr.windows(2).all(|w| w[0] <= w[1] + 1e-6), "pressions croissantes : {pr:?}");
    assert_eq!(t.app().pen_force, None, "la pression est oubliée au relâchement");
}

#[test]
fn mouse_strokes_have_constant_width_and_highlighter_ignores_pressure() {
    let mut t = T::new();
    t.app().set_tool(Tool::Pen);
    t.drag((100.0, 100.0), (300.0, 160.0));
    assert!(matches!(&t.objects()[0].kind, Kind::Pen(p) if p.pressures.is_none()));
    t.app().set_tool(Tool::Highlighter);
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

/* ------------------------------------------------------------ grands tableaux */

#[test]
fn spatial_index_keeps_picking_and_moving_correct_on_a_large_board() {
    let mut t = T::new();
    let mut ids = Vec::new();
    for i in 0..600 {
        ids.push(t.add_rect((i % 30) as f64 * 150.0, (i / 30) as f64 * 130.0, 100.0, 80.0));
    }
    t.h.step();
    assert!(t.app().caches.spatial.is_some(), "au-delà de 400 objets l'index spatial est actif");
    let target = ids[35].clone(); // colonne 5, ligne 1
    let f = t.frame(&target);
    t.click((f.x + 50.0, f.y + 40.0));
    assert_eq!(t.app().sel, vec![target.clone()]);
    // déplacement loin des autres, puis vérification avec l'index reconstruit
    t.drag((f.x + 50.0, f.y + 40.0), (f.x + 50.0 + 5000.0, f.y + 40.0 + 3000.0));
    let moved = t.frame(&target);
    assert!((moved.x - (f.x + 5000.0)).abs() < 2.0, "{moved:?}");
    t.app().sel.clear();
    t.app().set_view(View { x: 500.0 - moved.x, y: 300.0 - moved.y, zoom: 1.0 });
    t.h.step();
    t.click((moved.x + 50.0, moved.y + 40.0));
    assert_eq!(t.app().sel, vec![target.clone()], "l'objet est retrouvé à sa nouvelle position");
    t.app().sel.clear();
    t.app().set_view(View { x: 500.0 - f.x, y: 300.0 - f.y, zoom: 1.0 });
    t.h.step();
    t.click((f.x + 50.0, f.y + 40.0));
    assert!(t.app().sel.is_empty(), "plus rien à l'ancienne position");
    t.app().set_view(View { x: 0.0, y: 0.0, zoom: 1.0 });
    t.h.step();
    // lasso sur une zone dense
    t.drag((125.0, 105.0), (480.0, 300.0)); // départ dans un interstice : lasso, pas déplacement
    let sel = t.app().sel.len();
    let linear = t.app().ed.page().objects_in_rect(Rect::new(125.0, 105.0, 355.0, 195.0), true).len();
    assert_eq!(sel, linear, "le lasso donne le même résultat avec ou sans index");
    assert!(sel >= 2, "{sel} objets attendus au moins");
}

/* ------------------------------------------------- points de passage et coudés */

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
    assert!(path.iter().any(|p| p.y > 300.0), "la flèche passe par le nouveau point");
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
    // Alt + clic sur la poignée : retire le point
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
    assert!(!wb_core::route::crosses(&path, &[wall_rect]), "{path:?}");
    // le mur déplacé hors du chemin : le tracé redevient le plus simple
    t.app().ed.update(&wall, |o| o.translate(Pt::new(0.0, 600.0)));
    let page = t.app().ed.page();
    let Kind::Connector(cn) = &page.get(&c).unwrap().kind else { unreachable!() };
    let direct = page.connector_path(cn).unwrap();
    assert_eq!(direct.len(), 2, "ligne droite quand rien ne gêne : {direct:?}");
}
