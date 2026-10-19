//! Visual regression tests, lot H of `context/PLAN_TESTS_AUTO.md`: window captures (1024 x 700) and exports compared to
//! reference images in `crates/inkboard-app/tests/snapshots/`.
//!
//! The rendering depends on the GPU driver, the OS and the system fonts, so these tests run **only** when the
//! environment variable `INKBOARD_SNAPSHOTS=1` is set; otherwise they end at once, successfully, printing `SKIPPED`.
//! The references are the ones made on Linux with software rendering (job `.github/workflows/visual.yml`:
//! `xvfb-run -a` and `LIBGL_ALWAYS_SOFTWARE=1`). References made on another system will not match.
//!
//! ```text
//! INKBOARD_SNAPSHOTS=1 cargo test -p inkboard-app tests_visual            # compare
//! INKBOARD_SNAPSHOTS=1 UPDATE_SNAPSHOTS=1 cargo test -p inkboard-app tests_visual      # create missing / replace failing references
//! INKBOARD_SNAPSHOTS=1 UPDATE_SNAPSHOTS=force cargo test -p inkboard-app tests_visual  # replace all references
//! ```
//!
//! To regenerate the references: run the `Visual` workflow by hand (`workflow_dispatch`, `update: true`), download its
//! artifact `visual-references` (kept 3 days) and copy the PNG files into `crates/inkboard-app/tests/snapshots/`.
//! `INKBOARD_SNAPSHOT_DIR=<folder>` redirects the references (and the `.new.png` / `.diff.png` files) elsewhere.
//!
//! Tolerance (`THRESHOLD`, `MAX_DIFFERENT_PIXELS`): a pixel counts as different when its perceptual color distance
//! (YIQ, as computed by the `dify` crate) exceeds `THRESHOLD`, which absorbs anti-aliasing differences between two
//! versions of the software rasterizer; the test fails beyond `MAX_DIFFERENT_PIXELS` such pixels (about 0.1 % of a
//! 1024 x 700 capture), which still catches a moved or missing element, a wrong color or a changed text.
//!
//! The application is built with the data folder redirected to a throw-away folder (same mechanism as `tests_ui`):
//! the captures never read or write the real `%APPDATA%\Inkboard`.

use crate::app::App;
use crate::i18n::{Lang, set_for_thread};
use crate::tests::T;
use crate::tests_ui::isolated_in;
use crate::tools::Tool;
use eframe::egui::{self, vec2};
use egui_kittest::{Harness, SnapshotError, SnapshotOptions, try_image_snapshot_options};
use inkboard_core::*;
use std::path::PathBuf;

const SIZE: (f32, f32) = (1024.0, 700.0);
/// Per-pixel tolerance (0 = identical, 1 = anything goes), see the module documentation.
const THRESHOLD: f32 = 0.3;
/// Number of pixels allowed to exceed `THRESHOLD`.
const MAX_DIFFERENT_PIXELS: usize = 700;

/* ------------------------------------------------------------ plumbing */

/// Runs `body` in the isolated child process, but only if `INKBOARD_SNAPSHOTS=1`. `test` is the name of the calling test.
fn visual(test: &str, body: impl FnOnce()) {
    if std::env::var("INKBOARD_SNAPSHOTS").as_deref() != Ok("1") {
        eprintln!("SKIPPED {test}: set INKBOARD_SNAPSHOTS=1 to run the visual regression tests");
        return;
    }
    isolated_in("tests_visual", test, |_| body());
}

fn snapshot_dir() -> PathBuf {
    std::env::var_os("INKBOARD_SNAPSHOT_DIR").map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("snapshots"), PathBuf::from)
}

/// Compares `image` to the reference `<name>.png`. A missing reference or a difference fails with a message that says what to do.
fn check(name: &str, image: &image::RgbaImage) {
    let options = SnapshotOptions::new().threshold(THRESHOLD).failed_pixel_count_threshold(MAX_DIFFERENT_PIXELS).output_path(snapshot_dir());
    match try_image_snapshot_options(image, name, &options) {
        Ok(()) => {}
        Err(SnapshotError::OpenSnapshot { path, .. }) => panic!(
            "reference image missing for '{name}': {}\nThe image obtained was saved next to it as {name}.new.png.\n\
             To create the references: INKBOARD_SNAPSHOTS=1 UPDATE_SNAPSHOTS=1 cargo test -p inkboard-app tests_visual",
            path.display()
        ),
        Err(e) => panic!(
            "visual regression on '{name}': {e}
If the change is intended: INKBOARD_SNAPSHOTS=1 UPDATE_SNAPSHOTS=1 cargo test -p inkboard-app tests_visual"
        ),
    }
}

/// The application in a 1024 x 700 window, English (never the language of the system), nothing restored from disk.
fn app(lang: Lang) -> T {
    set_for_thread(Some(lang));
    let mut h = Harness::builder().with_size(vec2(SIZE.0, SIZE.1)).build_eframe(|cc| {
        let mut app = App::build(cc, None, None, false);
        app.system_image = || None; // never the real clipboard in tests
        app
    });
    h.step();
    h.step();
    let mut t = T { h };
    t.app().fit_pending = false;
    settle(&mut t);
    t
}

/// Lets animations, floating cards and popups finish, and hides what changes with time (toast, pointer).
fn settle(t: &mut T) {
    for _ in 0..6 {
        t.h.step();
    }
    t.app().toast = None;
    t.h.remove_cursor();
    t.h.step();
}

fn capture(t: &mut T, name: &str) {
    settle(t);
    let image = t.h.render().unwrap_or_else(|e| panic!("cannot render '{name}': {e}"));
    assert_eq!((image.width(), image.height()), (SIZE.0 as u32, SIZE.1 as u32), "capture size of '{name}'");
    check(name, &image);
    set_for_thread(None);
}

/// Puts the world point `c` at the center of the canvas.
fn center_on(t: &mut T, c: Pt, zoom: f64) {
    let (w, h) = t.app().canvas_size;
    t.app().set_view(View { x: f64::from(w) / 2.0 - c.x * zoom, y: f64::from(h) / 2.0 - c.y * zoom, zoom });
}

fn label(text: &str, family: FontFamily, size: f64, align: Align) -> Label {
    Label { text: RichText::plain(text), family, size, align }
}

fn shape(t: &mut T, geom: BoxGeom, f: (f64, f64, f64, f64), fill: Option<Color>, text: &str) -> Id {
    let style = Style { fill, ..Style::default() };
    let o = Object::new_box(geom, Frame::new(f.0, f.1, f.2, f.3), style, label(text, FontFamily::Sans, 20.0, Align::Center));
    t.app().ed.insert(o)
}

fn move_to(t: &mut T, id: &Id, x: f64, y: f64) {
    let f = t.frame(id);
    t.app().ed.translate(std::slice::from_ref(id), Pt::new(x - f.x, y - f.y));
}

fn add_table(t: &mut T, at: Pt) -> Id {
    let mut table = Table::new(at, 3, 3);
    for (i, text) in ["Name", "Qty", "Price", "Pen", "12", "1.50", "Ruler", "3", "2.00"].iter().enumerate() {
        table.cells[i] = RichText::plain(*text);
    }
    t.app().ed.insert(Object::new(Kind::Table(table), Style::default()))
}

fn add_code(t: &mut T, at: Pt) -> Id {
    t.app().insert_code_block();
    let id = t.app().sel[0].clone();
    move_to(t, &id, at.x, at.y);
    t.app().sel.clear();
    t.app().tool = Tool::Select;
    id
}

fn add_formula(t: &mut T, latex: &str, at: Pt) -> Id {
    t.app().insert_formula(latex, None).expect("formula");
    let id = t.app().sel[0].clone();
    move_to(t, &id, at.x, at.y);
    t.app().sel.clear();
    t.app().tool = Tool::Select;
    id
}

/// One object of every kind: shapes, sticky note, free text, strokes, connectors (straight and elbow), table, code block, formula.
fn demo_board(t: &mut T) {
    let blue = Some(Color::rgb(0xa5, 0xd8, 0xff));
    let rect = shape(t, BoxGeom::Rect, (140.0, 150.0, 150.0, 80.0), blue, "Rectangle");
    let ellipse = shape(t, BoxGeom::Ellipse, (360.0, 150.0, 150.0, 80.0), Some(Color::rgb(0xb2, 0xf2, 0xbb)), "Ellipse");
    let diamond = shape(t, BoxGeom::Diamond, (580.0, 135.0, 160.0, 110.0), Some(Color::rgb(0xff, 0xec, 0x99)), "Diamond");
    shape(t, BoxGeom::Triangle, (810.0, 140.0, 120.0, 100.0), Some(Color::rgb(0xff, 0xc9, 0xc9)), "");
    let sticky = shape(t, BoxGeom::Sticky, (140.0, 300.0, 140.0, 110.0), None, "Sticky note");
    let text = Object::new_box(BoxGeom::Text, Frame::new(330.0, 320.0, 160.0, 30.0), Style::default(), label("Free text", FontFamily::Hand, 24.0, Align::Left));
    t.app().ed.insert(text);
    // connectors: straight (bound), elbow (bound, dashed)
    t.app().ed.connect(&rect, &ellipse);
    let elbow = t.app().ed.connect(&ellipse, &diamond);
    t.app().ed.update(&elbow, |o| {
        o.style.dash = Dash::Dashed;
        if let Kind::Connector(c) = &mut o.kind {
            c.route = Route::Elbow;
        }
    });
    t.app().ed.connect(&sticky, &rect);
    // strokes: pen, pen with pressure, highlighter
    let wave: Vec<Pt> = (0..=24).map(|i| Pt::new(140.0 + f64::from(i) * 8.0, 500.0 + (f64::from(i) * 0.5).sin() * 20.0)).collect();
    t.app().ed.insert(Object::new_pen(wave.clone(), false, Style { stroke: Color::rgb(0xe0, 0x31, 0x31), width: 4.0, ..Style::default() }));
    let pressures: Vec<f32> = (0..wave.len()).map(|i| 0.2 + 0.8 * i as f32 / wave.len() as f32).collect();
    let low: Vec<Pt> = wave.iter().map(|p| Pt::new(p.x, p.y + 50.0)).collect();
    t.app().ed.insert(Object::new_pen_with_pressure(low, pressures, false, Style { width: 6.0, ..Style::default() }));
    let band = vec![Pt::new(140.0, 440.0), Pt::new(330.0, 440.0)];
    t.app().ed.insert(Object::new_pen(band, true, Style { stroke: Color::rgb(0xff, 0xd4, 0x3b), width: 18.0, opacity: 0.5, ..Style::default() }));
    add_table(t, Pt::new(520.0, 290.0));
    add_code(t, Pt::new(380.0, 470.0));
    add_formula(t, r"\frac{-b \pm \sqrt{b^2-4ac}}{2a}", Pt::new(740.0, 440.0));
    t.app().sel.clear();
    center_on(t, Pt::new(535.0, 350.0), 1.0);
}

/* ------------------------------------------------------------ captures */

#[test]
fn empty_board_light() {
    visual("empty_board_light", || {
        let mut t = app(Lang::En);
        capture(&mut t, "empty_board_light");
    });
}

#[test]
fn empty_board_dark() {
    visual("empty_board_dark", || {
        let mut t = app(Lang::En);
        let mut bg = t.app().ed.doc.background;
        bg.preset = BgPreset::Slate;
        t.app().ed.set_background(bg);
        capture(&mut t, "empty_board_dark");
    });
}

#[test]
fn main_menu_open() {
    visual("main_menu_open", || {
        let mut t = app(Lang::En);
        let id = t.app().menu_popup.expect("menu button shown");
        egui::Popup::open_id(&t.h.ctx, id);
        capture(&mut t, "main_menu_open");
    });
}

#[test]
fn style_panel() {
    visual("style_panel", || {
        let mut t = app(Lang::En);
        let id = shape(&mut t, BoxGeom::Rect, (420.0, 260.0, 200.0, 110.0), Some(Color::rgb(0xa5, 0xd8, 0xff)), "Selected");
        t.app().sel = vec![id];
        center_on(&mut t, Pt::new(520.0, 315.0), 1.0);
        capture(&mut t, "style_panel");
    });
}

#[test]
fn demo_board_english() {
    visual("demo_board_english", || {
        let mut t = app(Lang::En);
        demo_board(&mut t);
        capture(&mut t, "demo_board_english");
    });
}

#[test]
fn demo_board_french() {
    visual("demo_board_french", || {
        let mut t = app(Lang::Fr);
        demo_board(&mut t);
        capture(&mut t, "demo_board_french");
    });
}

#[test]
fn table_board() {
    visual("table_board", || {
        let mut t = app(Lang::En);
        let id = add_table(&mut t, Pt::new(0.0, 0.0));
        let c = t.frame(&id).center();
        center_on(&mut t, c, 1.5);
        capture(&mut t, "table_board");
    });
}

#[test]
fn code_block_board() {
    visual("code_block_board", || {
        let mut t = app(Lang::En);
        let id = add_code(&mut t, Pt::new(0.0, 0.0));
        let c = t.frame(&id).center();
        center_on(&mut t, c, 1.5);
        capture(&mut t, "code_block_board");
    });
}

#[test]
fn formula_board() {
    visual("formula_board", || {
        let mut t = app(Lang::En);
        let id = add_formula(&mut t, r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}", Pt::new(0.0, 0.0));
        let c = t.frame(&id).center();
        center_on(&mut t, c, 2.0);
        capture(&mut t, "formula_board");
    });
}

/* ------------------------------------------------------------ exports */

/// The demo board as the application exports it (same call as `App::export`).
fn demo_svg() -> String {
    let mut t = app(Lang::En);
    demo_board(&mut t);
    let page = t.app().ed.page().clone();
    let app = t.app();
    let svg = inkboard_core::svg::export(&app.ed.doc, &page, None, &app.measurer, &Default::default()).expect("something to export");
    set_for_thread(None);
    svg
}

#[test]
fn export_demo_board_svg_rendered_to_png() {
    visual("export_demo_board_svg_rendered_to_png", || {
        let svg = demo_svg();
        // pixels of `raster.rs` (thumbnail path): premultiplied RGBA -> straight RGBA
        let r = inkboard_formats::raster::svg_to_raster_fit(&svg, 900, true).expect("SVG rendered");
        let mut pixels = r.rgba_premultiplied;
        for p in pixels.chunks_exact_mut(4) {
            let a = u32::from(p[3]);
            if a != 0 && a != 255 {
                for c in &mut p[..3] {
                    *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        let image = image::RgbaImage::from_raw(r.width, r.height, pixels).expect("pixel buffer");
        check("export_demo_board_svg_to_png", &image);
    });
}

#[test]
fn export_demo_board_png() {
    visual("export_demo_board_png", || {
        let svg = demo_svg();
        // the PNG export of the application (`App::export("png")`) renders the SVG at 2x
        let png = inkboard_formats::raster::svg_to_png(&svg, 2.0).expect("PNG export");
        let image = image::load_from_memory(&png).expect("PNG decoded").to_rgba8();
        check("export_demo_board_png", &image);
    });
}
