//! UI tests, lot F of `context/PLAN_TESTS_AUTO.md`: every command, every tool, translations, custom shortcuts.
//! Same approach as `tests.rs` (real egui cycle without a window); see the plan's journal for what is already covered there.

use crate::app::{App, Dialog};
use crate::i18n::{Lang, MISSING, set_for_thread};
use crate::keymap::{Action, Keymap, Shortcut};
use crate::tests::T;
use crate::tools::Tool;
use eframe::egui::{self, Event, Key, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use inkboard_core::num;
use inkboard_core::text::SpanStyle;
use inkboard_core::*;
use std::path::{Path, PathBuf};

type Check = Result<(), String>;

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        let ok: bool = $cond;
        if !ok {
            return Err(format!($($msg)+));
        }
    };
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("inkboard-ui-{tag}-{}", Id::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/* ------------------------------------------------------------ isolation */

const ISOLATED: &str = "INKBOARD_TEST_DATA_DIR";

/// Runs `body` in a child process (this same test binary, filtered on one test) whose data folder is a throw-away folder.
/// Needed for what touches the application data folder (`%APPDATA%\Inkboard`): environment variables cannot be changed
/// in-process (`unsafe`, and shared with the parallel tests). The child refuses to run if the folder is not the temporary one.
fn isolated(test: &str, body: impl FnOnce(&Path)) {
    isolated_in("tests_ui", test, body);
}

/// Same as [`isolated`] for a test of another module (`tests_visual`).
pub(super) fn isolated_in(module: &str, test: &str, body: impl FnOnce(&Path)) {
    if let Some(dir) = std::env::var_os(ISOLATED).map(PathBuf::from) {
        let data = inkboard_mcp::live::data_dir();
        assert!(data.starts_with(&dir), "refusing to run: the data folder {data:?} is not inside {dir:?}");
        body(&dir);
        return;
    }
    let dir = temp_dir("data");
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([&format!("{module}::{test}"), "--exact", "--include-ignored", "--test-threads=1"])
        .env(ISOLATED, &dir)
        .env("APPDATA", &dir)
        .env("XDG_DATA_HOME", &dir)
        .env("HOME", &dir)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let output = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && output.contains("1 passed"), "isolated test {test} failed:\n{output}");
}

/* ------------------------------------------------------------ helpers */

/// The application as the real one starts (settings read from the data folder), in a window of the given size.
fn harness(size: (f32, f32), restore: bool) -> T {
    let mut h = Harness::builder().with_size(vec2(size.0, size.1)).build_eframe(move |cc| {
        let mut app = App::build(cc, None, None, restore);
        app.system_image = || None; // never the real clipboard in tests
        app
    });
    h.step();
    h.step();
    T { h }
}

/// Three labelled rectangles side by side (z-order x, y, z), nothing selected.
fn board() -> (T, [Id; 3]) {
    let mut t = T::new();
    let ids = [100.0, 300.0, 500.0].map(|x| {
        let label = Label { text: RichText::plain("hello"), ..Label::default() };
        t.app().ed.insert(Object::new_box(BoxGeom::Rect, Frame::new(x, 100.0, 100.0, 60.0), Style::default(), label))
    });
    (t, ids)
}

fn press(t: &mut T, sc: Shortcut) {
    t.key_mod(sc.modifiers(), sc.key);
}

fn order_of(t: &mut T) -> Vec<Id> {
    t.objects().into_iter().map(|o| o.id).collect()
}

fn label_text(t: &mut T, id: &Id) -> RichText {
    let Kind::Box(b) = &t.app().ed.page().get(id).unwrap().kind else { unreachable!() };
    b.label.text.clone()
}

fn type_text(t: &mut T, s: &str) {
    t.h.event(Event::Text(s.to_owned()));
    t.h.step();
    t.h.step();
}

fn toast_of(t: &mut T) -> Option<String> {
    t.app().toast.as_ref().map(|(m, _)| m.clone())
}

/* ------------------------------------------------- every keymap action */

fn check_save(t: &mut T, sc: Shortcut) -> Check {
    let dir = temp_dir("save");
    let path = dir.join("board.inkboard");
    t.app().path = Some(path.clone()); // without a path, saving asks for one with a native dialog
    ensure!(t.app().dirty(), "the new board is not dirty before saving");
    press(t, sc);
    let doc = inkboard_core::serial::load(&path).map_err(|e| format!("saved file unreadable: {e}"));
    let count = doc.as_ref().map(|d| d.pages.iter().map(|p| p.objects.len()).sum::<usize>());
    let clean = !t.app().dirty();
    std::fs::remove_dir_all(&dir).ok();
    ensure!(count == Ok(3), "saved file holds {count:?} objects");
    ensure!(clean, "the board is still dirty after saving");
    Ok(())
}

fn check_selection_action(action: Action, sc: Shortcut, t: &mut T, [x, y, z]: [Id; 3]) -> Check {
    match action {
        Action::Undo => {
            press(t, sc);
            ensure!(order_of(t) == [x, y], "last rectangle not removed");
        }
        Action::Redo => {
            t.app().ed.undo();
            ensure!(t.objects().len() == 2, "setup: undo");
            press(t, sc);
            ensure!(order_of(t) == [x, y, z], "rectangle not restored");
        }
        Action::SelectAll => {
            press(t, sc);
            ensure!(t.app().sel == [x, y, z], "selection is {:?}", t.app().sel);
        }
        Action::Duplicate => {
            t.app().sel = vec![x.clone()];
            press(t, sc);
            ensure!(t.objects().len() == 4, "{} objects", t.objects().len());
            let sel = t.app().sel.clone();
            ensure!(sel.len() == 1 && sel[0] != x, "the copy is not the selection: {sel:?}");
            let frame = t.frame(&sel[0]);
            ensure!((frame.x - 124.0).abs() < 1e-9 && (frame.y - 124.0).abs() < 1e-9, "copy offset by (24, 24), got {frame:?}");
        }
        Action::Delete => {
            t.app().sel = vec![x.clone()];
            press(t, sc);
            ensure!(order_of(t) == [y, z], "x not deleted");
            ensure!(t.app().sel.is_empty(), "selection not cleared");
        }
        Action::Group => {
            t.app().sel = vec![x.clone(), y.clone()];
            press(t, sc);
            let group_of = |t: &mut T, id: &Id| t.app().ed.page().get(id).unwrap().group.clone();
            let (gx, gy, gz) = (group_of(t, &x), group_of(t, &y), group_of(t, &z));
            ensure!(gx.is_some() && gx == gy && gz.is_none(), "groups: {gx:?} {gy:?} {gz:?}");
        }
        Action::Ungroup => {
            t.app().ed.group(&[x.clone(), y.clone()]);
            t.app().sel = vec![x.clone(), y.clone()];
            press(t, sc);
            ensure!(t.objects().iter().all(|o| o.group.is_none()), "still grouped");
        }
        Action::Front => {
            t.app().sel = vec![x.clone()];
            press(t, sc);
            ensure!(order_of(t) == [y, z, x], "x not brought to front");
        }
        Action::Forward => {
            t.app().sel = vec![x.clone()];
            press(t, sc);
            ensure!(order_of(t) == [y, x, z], "x not moved up one step");
        }
        Action::Backward => {
            t.app().sel = vec![z.clone()];
            press(t, sc);
            ensure!(order_of(t) == [x, z, y], "z not moved down one step");
        }
        Action::Back => {
            t.app().sel = vec![z.clone()];
            press(t, sc);
            ensure!(order_of(t) == [z, x, y], "z not sent to back");
        }
        _ => unreachable!("selection action"),
    }
    Ok(())
}

/// Fires one default shortcut of `a` on the representative board and checks the visible effect.
/// The match has no wildcard: a new `Action` does not compile until it is handled here.
fn check_action(action: Action, sc: Shortcut) -> Check {
    let (mut t, [x, y, z]) = board();
    let t = &mut t;
    match action {
        // Deletes the safety copy in the data folder: see `ctrl_n_with_unsaved_changes_asks_first_and_removes_the_autosave_only_when_confirmed`.
        Action::New => {}
        // Native file dialogs would block the test: only check that the shortcut is bound to the action.
        Action::Open | Action::SaveAs => {
            ensure!(Keymap::default().action_for(sc.key, sc.modifiers()) == Some(action), "{} is not bound to {action:?}", sc.label());
        }
        Action::Save => check_save(t, sc)?,
        Action::Undo
        | Action::Redo
        | Action::SelectAll
        | Action::Duplicate
        | Action::Delete
        | Action::Group
        | Action::Ungroup
        | Action::Front
        | Action::Forward
        | Action::Backward
        | Action::Back => check_selection_action(action, sc, t, [x, y, z])?,
        Action::Zoom100 => {
            let view = t.app().ed.page().view;
            t.app().set_view(View { zoom: 2.0, ..view });
            t.h.step();
            press(t, sc);
            ensure!((t.app().ed.page().view.zoom - 1.0).abs() < 1e-9, "zoom is {}", t.app().ed.page().view.zoom);
        }
        Action::ZoomFit => {
            press(t, sc);
            let z = t.app().ed.page().view.zoom;
            ensure!(z > 1.0 && z <= 2.0, "three small rectangles should fill the window, zoom {z}");
        }
        Action::ZoomIn => {
            press(t, sc);
            ensure!((t.app().ed.page().view.zoom - 1.25).abs() < 1e-9, "zoom is {}", t.app().ed.page().view.zoom);
        }
        Action::ZoomOut => {
            press(t, sc);
            ensure!((t.app().ed.page().view.zoom - 0.8).abs() < 1e-9, "zoom is {}", t.app().ed.page().view.zoom);
        }
        Action::EditText => {
            t.app().sel = vec![x.clone()];
            press(t, sc);
            ensure!(t.app().editing.as_ref().is_some_and(|e| e.id == x), "the text editor did not open on x");
        }
        Action::Bold | Action::Italic | Action::Underline | Action::Strike => {
            let on: fn(&SpanStyle) -> bool = match action {
                Action::Bold => |s| s.bold,
                Action::Italic => |s| s.italic,
                Action::Underline => |s| s.underline,
                _ => |s| s.strike,
            };
            t.app().sel = vec![x.clone()];
            press(t, sc);
            ensure!(label_text(t, &x).all(0, 5, on), "the selected text is not formatted");
            ensure!(!label_text(t, &y).all(0, 5, on), "an unselected object was formatted");
        }
        Action::ToolLock => {
            ensure!(!t.app().tool_lock, "setup");
            press(t, sc);
            ensure!(t.app().tool_lock, "lock not enabled");
            press(t, sc);
            ensure!(!t.app().tool_lock, "lock not released by the second press");
        }
        Action::ToolSelect
        | Action::ToolHand
        | Action::ToolPen
        | Action::ToolHighlighter
        | Action::ToolEraser
        | Action::ToolRect
        | Action::ToolEllipse
        | Action::ToolDiamond
        | Action::ToolTriangle
        | Action::ToolLine
        | Action::ToolArrow
        | Action::ToolText
        | Action::ToolSticky
        | Action::ToolTable
        | Action::ToolLaser => {
            let want = action.tool().expect("tool action");
            t.app().tool = if want == Tool::Pen { Tool::Hand } else { Tool::Pen };
            press(t, sc);
            ensure!(t.app().tool == want, "tool is {:?}, expected {want:?}", t.app().tool);
        }
        Action::Present => {
            press(t, sc);
            ensure!(t.app().presenting, "presentation not started");
            press(t, sc);
            ensure!(!t.app().presenting, "presentation not left by the second press");
        }
    }
    Ok(())
}

#[test]
fn every_keymap_action_works_from_its_default_shortcut() {
    let mut failures = Vec::new();
    let mut fired = 0;
    for a in Action::ALL {
        let shortcuts = Keymap::default_for(a);
        assert!(!shortcuts.is_empty(), "{a:?} has no default shortcut");
        for sc in shortcuts {
            fired += 1;
            if let Err(e) = check_action(a, sc) {
                failures.push(format!("{a:?} ({}): {e}", sc.label()));
            }
        }
    }
    assert!(fired >= Action::ALL.len());
    assert!(failures.is_empty(), "{} action(s) failed:\n{}", failures.len(), failures.join("\n"));
}

fn autosave_file() -> PathBuf {
    inkboard_mcp::live::data_dir().join("autosave.inkboard.json")
}

fn confirm_new_is_open(t: &mut T) -> bool {
    matches!(t.app().dialog, Some(Dialog::ConfirmNew))
}

#[test]
fn ctrl_n_with_unsaved_changes_asks_first_and_removes_the_autosave_only_when_confirmed() {
    // `new_document` removes the autosave of the data folder: never run it against the real one
    isolated("ctrl_n_with_unsaved_changes_asks_first_and_removes_the_autosave_only_when_confirmed", |_| {
        let (mut t, [x, ..]) = board();
        t.app().sel = vec![x];
        t.app().path = Some(PathBuf::from("old.inkboard.json"));
        std::fs::create_dir_all(autosave_file().parent().unwrap()).unwrap();
        std::fs::write(autosave_file(), "{}").unwrap();
        assert!(t.app().dirty(), "setup: the board has unsaved changes");

        // Ctrl+N: confirmation, nothing changed yet
        press(&mut t, Keymap::default_for(Action::New)[0]);
        assert!(confirm_new_is_open(&mut t), "no confirmation dialog");
        assert_eq!(t.objects().len(), 3);
        assert!(autosave_file().exists(), "the autosave must stay until confirmed");

        // Cancel: everything is kept
        t.h.get_by_label("Cancel").click();
        t.h.step();
        t.h.step();
        assert!(!confirm_new_is_open(&mut t), "the dialog is closed");
        assert_eq!(t.objects().len(), 3, "cancel keeps the board");
        assert!(t.app().path.is_some() && t.app().sel.len() == 1 && t.app().dirty());
        assert!(autosave_file().exists(), "cancel keeps the autosave");

        // Ctrl+N again then confirm: new empty board, autosave removed
        press(&mut t, Keymap::default_for(Action::New)[0]);
        assert!(confirm_new_is_open(&mut t));
        t.h.get_by_label("New board").click();
        t.h.step();
        t.h.step();
        assert!(!confirm_new_is_open(&mut t));
        assert!(t.objects().is_empty(), "the board is emptied");
        assert!(t.app().sel.is_empty() && t.app().path.is_none());
        assert!(!t.app().dirty(), "a new board is clean");
        assert!(!autosave_file().exists(), "the autosave is removed after confirmation");
    });
}

/// The safety copy keeps its historical name (`autosave.inkboard.json`) so that the copies already on a user's disk are still
/// found after the move to `.inkboard` (D8): it is written under that name and an existing one is restored at startup.
#[test]
fn the_autosave_keeps_its_name_and_an_existing_one_is_restored() {
    isolated("the_autosave_keeps_its_name_and_an_existing_one_is_restored", |dir| {
        let data = autosave_file().parent().unwrap().to_path_buf();
        assert!(data.starts_with(dir), "never the real data folder");
        assert_eq!(autosave_file().file_name().unwrap(), "autosave.inkboard.json");
        std::fs::create_dir_all(&data).unwrap();

        // writing: a modified board is saved under the historical name only
        let (mut t, _) = board();
        assert!(t.app().dirty());
        t.app().last_autosave = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(60)).unwrap();
        t.h.step();
        assert!(autosave_file().exists(), "the safety copy is written as autosave.inkboard.json");
        assert!(!data.join("autosave.inkboard").exists(), "no new-style safety copy");
        assert_eq!(inkboard_core::serial::load(&autosave_file()).unwrap().pages[0].objects.len(), 3);

        // reading: a copy left by a previous version is restored at startup
        let mut t2 = harness((1300.0, 850.0), true);
        assert_eq!(t2.objects().len(), 3, "the previous board is restored");
        assert!(t2.app().path.is_none());
    });
}

#[test]
fn ctrl_n_without_unsaved_changes_starts_a_new_board_without_asking() {
    isolated("ctrl_n_without_unsaved_changes_starts_a_new_board_without_asking", |_| {
        let new = Keymap::default_for(Action::New)[0];
        // saved board
        let (mut t, [x, ..]) = board();
        t.app().sel = vec![x];
        t.app().path = Some(PathBuf::from("old.inkboard.json"));
        t.app().saved_rev = t.app().ed.revision();
        assert!(!t.app().dirty(), "setup: the board is saved");
        press(&mut t, new);
        assert!(!confirm_new_is_open(&mut t), "a saved board needs no confirmation");
        assert!(t.objects().is_empty() && t.app().path.is_none() && t.app().sel.is_empty());
        // empty board
        let mut t = T::new();
        press(&mut t, new);
        assert!(!confirm_new_is_open(&mut t), "an empty board needs no confirmation");
        assert_eq!(t.objects(), [] as [inkboard_core::Object; 0]);
    });
}

#[test]
fn the_new_board_confirmation_is_translated() {
    set_for_thread(Some(Lang::Fr));
    let (mut t, _) = board();
    press(&mut t, Keymap::default_for(Action::New)[0]);
    assert!(confirm_new_is_open(&mut t));
    for en in ["Cancel", "New board", "Unsaved changes"] {
        let fr = crate::i18n::t(en);
        assert_ne!(fr, en, "{en} is not translated");
        t.h.get_by_label(&fr);
    }
    set_for_thread(None);
}

#[test]
fn keyboard_zoom_does_not_rescale_the_whole_interface() {
    // egui also zooms its UI on Ctrl+Plus / Ctrl+Minus / Ctrl+0, the same keys as the board zoom
    let mut t = T::new();
    for a in [Action::ZoomIn, Action::ZoomOut, Action::Zoom100] {
        for sc in Keymap::default_for(a) {
            press(&mut t, sc);
            assert_eq!(t.h.ctx.zoom_factor(), 1.0, "{a:?} ({}) also rescaled the interface", sc.label());
        }
    }
}

/* ------------------------------------------------------------- tools */

#[derive(Clone, Copy)]
enum Gesture {
    Click,
    Drag,
}

/// Tools that create one object and give the hand back (unless the lock is on).
const ONE_SHOT: [(Tool, Gesture); 9] = [
    (Tool::Shape(BoxGeom::Rect), Gesture::Click),
    (Tool::Shape(BoxGeom::Ellipse), Gesture::Click),
    (Tool::Shape(BoxGeom::Diamond), Gesture::Click),
    (Tool::Shape(BoxGeom::Triangle), Gesture::Click),
    (Tool::Line, Gesture::Drag),
    (Tool::Arrow, Gesture::Drag),
    (Tool::Text, Gesture::Click),
    (Tool::Sticky, Gesture::Click),
    (Tool::Table, Gesture::Click),
];

/// Tools that stay active by design: one object per stroke or click.
const CONTINUOUS: [(Tool, Gesture); 3] = [(Tool::Pen, Gesture::Drag), (Tool::Highlighter, Gesture::Drag), (Tool::Vote, Gesture::Click)];

const P1: (f64, f64) = (150.0, 100.0);
const P2: (f64, f64) = (150.0, 380.0);
const AWAY: (f64, f64) = (700.0, 600.0);

fn make(t: &mut T, gesture: Gesture, at: (f64, f64)) {
    match gesture {
        Gesture::Click => t.click(at),
        Gesture::Drag => t.drag(at, (at.0 + 150.0, at.1 + 40.0)),
    }
}

fn is_made_by(tool: Tool, o: &Object) -> bool {
    match (&o.kind, tool) {
        (Kind::Box(b), Tool::Shape(g)) => b.geom == g,
        (Kind::Box(b), Tool::Sticky) => b.geom == BoxGeom::Sticky,
        (Kind::Box(b), Tool::Text) => b.geom == BoxGeom::Text,
        (Kind::Box(b), Tool::Vote) => b.geom == BoxGeom::Ellipse,
        (Kind::Connector(c), Tool::Arrow) => c.head_end == Arrowhead::Triangle,
        (Kind::Connector(c), Tool::Line) => c.head_end == Arrowhead::None,
        (Kind::Table(_), Tool::Table) => true,
        (Kind::Pen(p), Tool::Pen) => !p.highlighter,
        (Kind::Pen(p), Tool::Highlighter) => p.highlighter,
        _ => false,
    }
}

/// Text and sticky notes open their editor on creation: type something and click away to finish.
fn finish_editing(t: &mut T, tool: Tool) -> Check {
    if matches!(tool, Tool::Text | Tool::Sticky) {
        ensure!(t.app().editing.is_some(), "{tool:?}: the editor should be open after creation");
        type_text(t, "x");
        t.click(AWAY);
        ensure!(t.app().editing.is_none(), "{tool:?}: a click elsewhere should end the edit");
    }
    Ok(())
}

fn check_one_shot(tool: Tool, gesture: Gesture) -> Check {
    let mut t = T::new();
    t.tool(tool);
    make(&mut t, gesture, P1);
    let objs = t.objects();
    ensure!(objs.len() == 1 && is_made_by(tool, &objs[0]), "{tool:?}: expected one matching object, got {} ({:?})", objs.len(), objs.first().map(|o| &o.kind));
    ensure!(t.app().tool == Tool::Select, "{tool:?}: tool is {:?} instead of Select", t.app().tool);
    ensure!(t.app().sel == [objs[0].id.clone()], "{tool:?}: the new object is not selected");
    finish_editing(&mut t, tool)?;
    t.click(P2); // the tool is Select now: an empty click creates nothing
    ensure!(t.objects().len() == 1, "{tool:?}: a second click created {} objects in total", t.objects().len());
    Ok(())
}

fn check_locked(tool: Tool, gesture: Gesture) -> Check {
    let mut t = T::new();
    t.key(Key::Q);
    ensure!(t.app().tool_lock, "Q should enable the lock");
    t.tool(tool);
    make(&mut t, gesture, P1);
    ensure!(t.objects().len() == 1 && is_made_by(tool, &t.objects()[0]), "{tool:?}: first object missing or wrong");
    ensure!(t.app().tool == tool, "{tool:?}: tool became {:?} although locked", t.app().tool);
    finish_editing(&mut t, tool)?;
    ensure!(t.app().tool == tool, "{tool:?}: tool became {:?} after ending the edit", t.app().tool);
    make(&mut t, gesture, P2);
    let objs = t.objects();
    ensure!(objs.len() == 2 && objs.iter().all(|o| is_made_by(tool, o)), "{tool:?}: second click gave {} objects", objs.len());
    ensure!(t.app().tool == tool, "{tool:?}: tool became {:?} after the second object", t.app().tool);
    Ok(())
}

fn run_all(cases: &[(Tool, Gesture)], check: fn(Tool, Gesture) -> Check) {
    let failures: Vec<String> = cases.iter().filter_map(|(tool, g)| check(*tool, *g).err()).collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn creation_tools_create_one_object_and_return_to_select() {
    run_all(&ONE_SHOT, check_one_shot);
}

#[test]
fn creation_tools_with_the_lock_stay_active_and_create_a_second_object() {
    run_all(&ONE_SHOT, check_locked);
}

#[test]
fn drawing_tools_stay_active_with_or_without_the_lock_and_create_one_object_per_gesture() {
    // without the lock: pen, highlighter and vote dots stay selected by design
    for (tool, gesture) in CONTINUOUS {
        let mut t = T::new();
        t.tool(tool);
        make(&mut t, gesture, P1);
        make(&mut t, gesture, P2);
        let objs = t.objects();
        assert!(objs.len() == 2 && objs.iter().all(|o| is_made_by(tool, o)), "{tool:?}: {} objects", objs.len());
        assert_eq!(t.app().tool, tool);
    }
    run_all(&CONTINUOUS, check_locked);
}

#[test]
fn hand_tool_pans_without_creating_anything() {
    let mut t = T::new();
    t.tool(Tool::Hand);
    let before = t.app().ed.page().view;
    t.drag((200.0, 200.0), (260.0, 230.0));
    let after = t.app().ed.page().view;
    assert_eq!(t.objects(), [] as [inkboard_core::Object; 0]);
    assert!((after.x - before.x - 60.0).abs() < 1.0 && (after.y - before.y - 30.0).abs() < 1.0, "{before:?} -> {after:?}");
    assert_eq!(t.app().tool, Tool::Hand);
}

#[test]
fn text_tool_with_the_lock_ends_the_edit_on_the_next_click_without_an_empty_box() {
    let mut t = T::new();
    t.key(Key::Q);
    t.tool(Tool::Text);
    // nothing typed: the click elsewhere ends the edit and the empty box is discarded, with no trace in the history
    t.click(P1);
    assert!(t.app().editing.is_some() && t.objects().len() == 1, "the box exists while it is edited");
    t.click(AWAY);
    assert!(t.app().editing.is_none(), "the edit is over");
    assert!(t.objects().is_empty(), "no empty text box is left");
    assert!(!t.app().ed.can_undo(), "creation and abandonment cancel out");
    assert_eq!(t.app().tool, Tool::Text, "the lock keeps the tool");
    // text typed: the click elsewhere keeps it and does not start another box
    t.click(P1);
    type_text(&mut t, "hi");
    t.click(AWAY);
    assert!(t.app().editing.is_none());
    let objs = t.objects();
    assert_eq!(objs.len(), 1, "no second (empty) box");
    assert_eq!(label_text(&mut t, &objs[0].id).to_plain(), "hi");
    assert_eq!(t.app().tool, Tool::Text);
    // the next click starts a new box
    t.click(AWAY);
    assert!(t.app().editing.is_some() && t.objects().len() == 2);
}

/* --------------------------------------------- tool lock (D9) */

/// Center of the toolbar button with this accessible name (English; the lock button has its own name).
fn button_center(t: &T, name: &str) -> Pos2 {
    let b = t.h.get_by_label(name).accesskit_node().bounding_box().unwrap_or_else(|| panic!("button {name} has no box"));
    Pos2::new(num::f32(f64::midpoint(b.x0, b.x1)), num::f32(f64::midpoint(b.y0, b.y1)))
}

/// Accessible state of a button: `Some(true)` pressed / locked, `Some(false)` not pressed.
fn is_pressed(t: &T, name: &str) -> Option<bool> {
    t.h.get_by_label(name).accesskit_node().data().toggled().map(|v| v == egui::accesskit::Toggled::True)
}

fn double_click_button(t: &mut T, name: &str) {
    let p = button_center(t, name);
    t.mv(p);
    // each step advances the clock by 0.25 s, more than egui's double-click delay: give the events explicit, close times
    let t0 = t.h.ctx.input(|i| i.time);
    for (k, pressed) in [true, false, true, false].into_iter().enumerate() {
        t.h.input_mut().time = Some(t0 + 0.05 * num::f64_of(k + 1));
        t.button(p, pressed, egui::Modifiers::NONE);
    }
    for _ in 0..3 {
        t.h.step();
    }
}

fn click_button(t: &mut T, name: &str) {
    let p = button_center(t, name);
    t.mv(p);
    t.button(p, true, egui::Modifiers::NONE);
    t.button(p, false, egui::Modifiers::NONE);
    for _ in 0..3 {
        t.h.step();
    }
}

#[test]
fn double_clicking_a_creation_tool_locks_it_and_the_state_is_visible_in_the_ui() {
    set_for_thread(Some(Lang::En));
    let mut t = T::new();
    let lock = "Keep the tool active after drawing";
    assert_eq!(is_pressed(&t, lock), Some(false));
    double_click_button(&mut t, "Rectangle");
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect));
    assert!(t.app().tool_lock, "double-click should lock the tool");
    assert_eq!(is_pressed(&t, lock), Some(true), "the global lock button reflects the state");
    assert_eq!(is_pressed(&t, "Rectangle (locked)"), Some(true), "the locked tool is exposed as locked");
    assert_eq!(is_pressed(&t, "Ellipse"), Some(false));
    t.click(P1);
    t.click(P2);
    assert_eq!(t.objects().len(), 2, "the tool stays active: a second rectangle is created");
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect));
    set_for_thread(None);
}

#[test]
fn a_single_click_on_a_tool_does_not_lock_it_and_other_tools_have_no_lock() {
    set_for_thread(Some(Lang::En));
    let mut t = T::new();
    click_button(&mut t, "Rectangle");
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect));
    assert!(!t.app().tool_lock, "a single click must not lock");
    assert_eq!(is_pressed(&t, "Rectangle"), Some(true), "active but not locked");
    t.click(P1);
    assert_eq!(t.app().tool, Tool::Select, "unlocked: the hand is given back");
    for name in ["Select", "Hand", "Pen", "Eraser"] {
        double_click_button(&mut t, name);
        assert!(!t.app().tool_lock, "double-click on {name} must not lock");
    }
    set_for_thread(None);
}

#[test]
fn the_arrowhead_size_steps_resize_the_selected_connectors_in_one_undo_step() {
    set_for_thread(Some(Lang::En));
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 80.0, 50.0);
    let b = t.add_rect(400.0, 100.0, 80.0, 50.0);
    let id = t.app().ed.insert(Object::new(Kind::Connector(Connector::new(End::bound(a), End::bound(b))), Style::default()));
    // the panel draws its size row for the selected connector
    t.app().sel = vec![id.clone()];
    t.h.step();
    t.h.step();
    let scale = |t: &mut T| match &t.app().ed.page().get(&id).expect("connector").kind {
        Kind::Connector(c) => c.head_scale,
        _ => panic!("connector"),
    };
    assert!((scale(&mut t) - 1.0).abs() < 1e-6);
    assert!(crate::panel::HEAD_SIZES.iter().any(|s| (s.0 - 1.0).abs() < 1e-6), "the usual size is one of the steps");
    crate::panel::set_head_scale(t.app(), std::slice::from_ref(&id), 1.6);
    assert!((scale(&mut t) - 1.6).abs() < 1e-6);
    crate::panel::set_head_scale(t.app(), std::slice::from_ref(&id), 0.6);
    assert!((scale(&mut t) - 0.6).abs() < 1e-6);
    t.h.step();
    t.app().ed.undo();
    assert!((scale(&mut t) - 1.6).abs() < 1e-6, "one step per change");
    t.app().ed.undo();
    assert!((scale(&mut t) - 1.0).abs() < 1e-6);
    set_for_thread(Some(Lang::Fr));
    for en in ["Arrowhead size", "Small arrowheads", "Medium arrowheads", "Large arrowheads", "Extra large arrowheads"] {
        assert_ne!(crate::i18n::t(en), en, "{en} is not translated");
    }
    set_for_thread(None);
}

#[test]
fn the_shadow_toggle_applies_to_the_selection_in_one_undo_step_and_every_kind_is_drawn_with_it() {
    set_for_thread(Some(Lang::En));
    let mut t = T::new();
    let a = t.add_rect(100.0, 100.0, 80.0, 50.0);
    let b = t.add_rect(400.0, 100.0, 80.0, 50.0);
    let c = t.app().ed.insert(Object::new(Kind::Connector(Connector::new(End::bound(a.clone()), End::bound(b.clone()))), Style::default()));
    let table = t.app().ed.insert(Object::new(Kind::Table(Table::new(Pt::new(100.0, 300.0), 2, 2)), Style::default()));
    let ids = vec![a.clone(), b, c, table];
    let shadows = |t: &mut T| t.objects().iter().filter(|o| o.style.shadow).count();
    assert_eq!(shadows(&mut t), 0);
    // the panel shows its row for the selection
    t.app().sel = ids.clone();
    t.h.step();
    t.h.step();
    crate::panel::set_shadow(t.app(), &ids, true);
    assert_eq!(shadows(&mut t), 4);
    assert!(t.app().style.shadow, "the next objects drawn get it too");
    // drawn on screen, close up and far away (no shadow below the detail threshold)
    for zoom in [1.0, 0.1] {
        t.app().set_view(View { x: 0.0, y: 0.0, zoom });
        t.h.step();
    }
    t.app().ed.undo();
    assert_eq!(shadows(&mut t), 0, "one step for the whole selection");
    t.app().ed.redo();
    assert_eq!(shadows(&mut t), 4);
    set_for_thread(Some(Lang::Fr));
    for en in ["Shadow", "Drop shadow"] {
        assert_ne!(crate::i18n::t(en), en, "{en} is not translated");
    }
    set_for_thread(None);
}

#[test]
fn the_lock_button_toggles_and_the_tool_tips_are_translated() {
    set_for_thread(Some(Lang::En));
    let mut t = T::new();
    click_button(&mut t, "Keep the tool active after drawing");
    assert!(t.app().tool_lock);
    click_button(&mut t, "Keep the tool active after drawing");
    assert!(!t.app().tool_lock);
    set_for_thread(Some(Lang::Fr));
    for en in ["Keep the tool active after drawing", "Double-click to lock", "locked"] {
        assert_ne!(crate::i18n::t(en), en, "{en} is not translated");
    }
    set_for_thread(None);
}

#[test]
fn the_lock_tool_tip_shows_the_current_shortcut_of_the_action() {
    set_for_thread(Some(Lang::En));
    let mut t = T::new();
    let sc = Shortcut::key(Key::J);
    t.app().keymap.reset_all();
    t.app().keymap.remove(Action::ToolLock, Shortcut::key(Key::Q));
    t.app().keymap.add(Action::ToolLock, sc).unwrap();
    let p = button_center(&t, "Keep the tool active after drawing");
    t.mv(p);
    t.mv(p + vec2(1.0, 0.0));
    for _ in 0..30 {
        t.h.step();
    }
    t.h.get_by_label("Keep the tool active after drawing (J)");
    set_for_thread(None);
}

/* ------------------------------------------------------ translations */

#[test]
fn every_french_translation_is_filled_in_and_keeps_its_placeholders() {
    let mut bad = Vec::new();
    for (en, fr) in crate::i18n_fr::FR {
        if fr.trim().is_empty() && !en.trim().is_empty() {
            bad.push(format!("empty translation: {en:?}"));
        }
        if en.matches("{}").count() != fr.matches("{}").count() {
            bad.push(format!("placeholders differ: {en:?} -> {fr:?}"));
        }
        if en.contains('\n') != fr.contains('\n') && !en.starts_with("flowchart") {
            bad.push(format!("line breaks differ: {en:?} -> {fr:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn every_command_name_and_group_has_a_french_translation() {
    let table: std::collections::HashMap<&str, &str> = crate::i18n_fr::FR.iter().copied().collect();
    let mut missing = Vec::new();
    for a in Action::ALL {
        for text in [a.label(), a.group()] {
            if !table.contains_key(text) {
                missing.push(format!("{a:?}: {text:?}"));
            }
        }
    }
    assert!(missing.is_empty(), "no French translation for:\n{}", missing.join("\n"));
}

fn rects_overlap(a: egui::Rect, b: egui::Rect) -> bool {
    let i = a.intersect(b);
    i.width() > 0.5 && i.height() > 0.5
}

/// Rectangles of every widget that reacts to clicks, from the AccessKit tree.
fn clickable_rects(t: &T) -> Vec<egui::Rect> {
    t.h.root()
        .children_recursive()
        .filter_map(|n| {
            let node = n.accesskit_node();
            let b = node.bounding_box()?;
            node.data()
                .supports_action(egui::accesskit::Action::Click)
                .then(|| egui::Rect::from_min_max(Pos2::new(num::f32(b.x0), num::f32(b.y0)), Pos2::new(num::f32(b.x1), num::f32(b.y1))))
        })
        .collect()
}

fn card(t: &T, name: &str) -> egui::Rect {
    t.h.ctx.memory(|m| m.area_rect(egui::Id::new(name))).unwrap_or_else(|| panic!("card {name} not shown"))
}

fn check_no_overlap(what: &str, rects: &[egui::Rect]) -> Check {
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            ensure!(!rects_overlap(*a, *b), "{what}: {a:?} overlaps {b:?}");
        }
    }
    Ok(())
}

/// French, 1024 x 700, something to undo (so every button of the bottom bar is enabled). The caller resets the language.
fn french_app() -> T {
    set_for_thread(Some(Lang::Fr));
    MISSING.with(|m| m.borrow_mut().clear());
    let mut t = harness((1024.0, 700.0), false);
    t.add_rect(100.0, 100.0, 80.0, 50.0);
    for _ in 0..3 {
        t.h.step();
    }
    t
}

fn open_main_menu(t: &mut T) {
    let id = t.app().menu_popup.expect("menu button shown");
    egui::Popup::open_id(&t.h.ctx, id);
    for _ in 0..3 {
        t.h.step();
    }
}

#[test]
fn in_french_in_a_1024_by_700_window_the_toolbar_and_the_menu_fit_without_overlap() {
    let mut t = french_app();
    let screen = egui::Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 700.0));

    // the floating cards: inside the window and not on top of each other
    let names = ["title", "tools", "pages", "zoom", "help"];
    let cards: Vec<egui::Rect> = names.iter().map(|n| card(&t, n)).collect();
    for (n, r) in names.iter().zip(&cards) {
        assert!(screen.expand(0.5).contains_rect(*r), "card {n} leaves the window: {r:?}");
    }
    check_no_overlap("cards", &cards).unwrap();

    // toolbar buttons
    let tools = card(&t, "tools");
    let buttons: Vec<egui::Rect> = clickable_rects(&t).into_iter().filter(|r| tools.expand(1.0).contains_rect(*r) && r.width() < tools.width() - 2.0).collect();
    assert!(buttons.len() >= 19, "only {} toolbar buttons found", buttons.len());
    for r in &buttons {
        assert!(screen.contains_rect(*r) && tools.expand(1.0).contains_rect(*r), "toolbar button leaves the window or its card: {r:?}");
    }
    check_no_overlap("toolbar", &buttons).unwrap();

    // main menu
    open_main_menu(&mut t);
    let menu =
        t.h.ctx
            .memory(|m| {
                m.areas()
                    .visible_layer_ids()
                    .into_iter()
                    .filter(|l| l.order == egui::Order::Foreground)
                    .filter_map(|l| m.area_rect(l.id))
                    .max_by(|a, b| a.area().total_cmp(&b.area()))
            })
            .expect("the menu is open");
    assert!(screen.expand(0.5).contains_rect(menu), "menu leaves the window: {menu:?}");
    let items: Vec<egui::Rect> =
        clickable_rects(&t).into_iter().filter(|r| r.width() < 600.0 && r.height() < 100.0 && !cards.iter().any(|c| c.expand(1.0).contains_rect(*r))).collect();
    assert!(items.len() >= 25, "only {} menu items found", items.len());
    for r in &items {
        assert!(
            r.min.x >= menu.min.x - 0.5 && r.max.x <= menu.max.x + 0.5 && screen.min.x <= r.min.x && r.max.x <= screen.max.x,
            "menu item sticks out of the menu {menu:?}: {r:?}"
        );
    }
    check_no_overlap("menu", &items).unwrap();

    set_for_thread(None);
}

#[test]
fn bug_11_the_french_toolbar_cards_and_menu_have_no_missing_translation() {
    let mut t = french_app();
    open_main_menu(&mut t);
    let missing: std::collections::BTreeSet<String> = MISSING.with(|m| m.borrow().iter().cloned().collect());
    set_for_thread(None);
    // the names of the languages stay in their own language
    let missing: Vec<&String> = missing.iter().filter(|s| !matches!(s.as_str(), "English" | "Fran\u{e7}ais")).collect();
    assert!(missing.is_empty(), "texts shown in French (toolbar, cards, menu) without a translation: {missing:?}");
}

/* ------------------------------------------------- custom shortcuts */

/// Writes `keymap.json` in the (isolated) data folder, then starts the application as the real one does.
fn start_with_keymap_file(bytes: &[u8]) -> T {
    let data = inkboard_mcp::live::data_dir();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("language"), "en").unwrap(); // do not depend on the language of the machine
    std::fs::write(data.join("keymap.json"), bytes).unwrap();
    harness((1300.0, 850.0), true)
}

fn assert_defaults_apply(t: &mut T, what: &str) {
    assert_eq!(t.app().keymap, Keymap::default(), "{what}: default shortcuts expected");
    let (a, b) = (t.add_rect(100.0, 100.0, 50.0, 50.0), t.add_rect(300.0, 100.0, 50.0, 50.0));
    t.app().sel = vec![b];
    t.key(Key::Delete);
    assert_eq!(order_of(t), [a], "{what}: Delete should still delete");
    t.key(Key::R);
    assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect), "{what}: R should still select the rectangle");
}

#[test]
fn an_invalid_keymap_file_is_ignored_reported_and_the_defaults_apply() {
    isolated("an_invalid_keymap_file_is_ignored_reported_and_the_defaults_apply", |_| {
        let good = Keymap::default().to_json();
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(good.as_bytes());
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("not JSON", b"this is not json".to_vec()),
            ("empty file", Vec::new()),
            ("truncated", good.as_bytes()[..good.len() / 2].to_vec()),
            ("no bindings", b"{}".to_vec()),
            ("bindings is not an object", br#"{"version":1,"bindings":[1,2]}"#.to_vec()),
            ("UTF-8 with BOM", bom),
        ];
        for (what, bytes) in cases {
            let mut t = start_with_keymap_file(&bytes);
            let toast = toast_of(&mut t).unwrap_or_default();
            assert!(toast.starts_with("Shortcuts file ignored"), "{what}: the error must be reported, toast is {toast:?}");
            assert_defaults_apply(&mut t, what);
            assert_eq!(std::fs::read(inkboard_mcp::live::data_dir().join("keymap.json")).unwrap(), bytes, "{what}: the file must not be overwritten at start");
        }
    });
}

#[test]
fn a_partly_invalid_keymap_file_keeps_its_valid_entries_and_reports_the_rest() {
    isolated("a_partly_invalid_keymap_file_keeps_its_valid_entries_and_reports_the_rest", |_| {
        // positive control first: a valid file is loaded and silent
        let mut t = start_with_keymap_file(br#"{"version":1,"bindings":{"tool.rect":["K"]}}"#);
        assert_eq!(toast_of(&mut t), None);
        t.key(Key::K);
        assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect), "custom K");
        t.key(Key::E);
        assert_eq!(t.app().tool, Tool::Eraser, "untouched actions keep their default");
        // one unknown action, one unreadable shortcut: reported, the valid entry is kept
        let mut t = start_with_keymap_file(br#"{"version":1,"bindings":{"tool.rect":["K"],"action.invented":["X"],"tool.pen":["Ctrl+Thing"]}}"#);
        let toast = toast_of(&mut t).unwrap_or_default();
        assert_eq!(toast, "Shortcuts: 2 invalid entries ignored");
        t.key(Key::K);
        assert_eq!(t.app().tool, Tool::Shape(BoxGeom::Rect));
        t.key(Key::E);
        assert_eq!(t.app().tool, Tool::Eraser);
    });
}

#[test]
fn bug_9_a_binding_that_is_not_a_list_is_reported_and_keeps_the_default() {
    // `{"tool.rect": "R"}` (a string instead of ["R"]) is easy to write by hand
    let (km, ignored) = Keymap::from_json(r#"{"version":1,"bindings":{"tool.rect":"R","tool.pen":42}}"#).unwrap();
    assert_eq!(ignored, 2, "both unreadable entries should be counted (and so reported at start)");
    assert!(km.is_default(Action::ToolRect) && km.is_default(Action::ToolPen), "unreadable entries should leave the default shortcut");
}

#[test]
fn bug_10_a_keymap_file_that_cannot_be_read_is_reported() {
    isolated("bug_10_a_keymap_file_that_cannot_be_read_is_reported", |_| {
        let json = r#"{"version":1,"bindings":{"tool.rect":["K"]}}"#;
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(json.encode_utf16().flat_map(u16::to_le_bytes));
        let mut t = start_with_keymap_file(&utf16);
        let loaded = t.app().keymap != Keymap::default();
        assert!(loaded || toast_of(&mut t).is_some(), "the file was neither loaded nor reported");
    });
}
