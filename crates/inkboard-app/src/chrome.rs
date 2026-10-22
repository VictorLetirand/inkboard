//! Application frame: floating cards above the full-screen canvas, like the web prototype
//! (title bar and menu top left, tools top center, undo/zoom bottom left, help bottom right).

use crate::app::{App, Dialog};
use crate::i18n::{t, tr};
use crate::keymap::Action;
use crate::panel;
use crate::theme;
use crate::tools::Tool;
use eframe::egui::{self, Align2, Color32, Context, FontId, Sense, Stroke, Ui, vec2};
use inkboard_core::editor::Order;
use inkboard_core::*;
use std::time::Duration;

/// Window width below which the toolbar moves under the title bar (`@media (max-width:1180px)`).
const NARROW: f32 = 1180.0;

/// Toolbar tools, in separated groups; `(tool, icon, label)`.
const TOOL_GROUPS: &[&[(Tool, &str, &str)]] = &[
    &[(Tool::Select, "select", "Select"), (Tool::Hand, "hand", "Hand")],
    &[(Tool::Pen, "pen", "Pen"), (Tool::Highlighter, "highlighter", "Highlighter"), (Tool::Eraser, "eraser", "Eraser")],
    &[
        (Tool::Shape(BoxGeom::Rect), "rect", "Rectangle"),
        (Tool::Shape(BoxGeom::Ellipse), "ellipse", "Ellipse"),
        (Tool::Shape(BoxGeom::Diamond), "diamond", "Diamond"),
        (Tool::Shape(BoxGeom::Triangle), "triangle", "Triangle"),
    ],
    &[(Tool::Line, "line", "Line — snaps to shapes"), (Tool::Arrow, "arrow", "Arrow — snaps to shapes")],
    &[(Tool::Text, "text", "Text"), (Tool::Sticky, "sticky", "Sticky note"), (Tool::Table, "table", "Table"), (Tool::Vote, "vote", "Vote dot")],
];

/// Shows the whole frame; call it before the canvas on every frame.
pub fn show(app: &mut App, ctx: &Context) {
    let narrow = ctx.content_rect().width() < NARROW;
    title_card(app, ctx);
    tool_card(app, ctx, narrow);
    panel::show(app, ctx, narrow);
    pages_card(app, ctx);
    zoom_card(app, ctx);
    help_card(app, ctx);
    crate::library::show(app, ctx);
}

/* ---------------------------------------------------------------- titre */

fn title_card(app: &mut App, ctx: &Context) {
    theme::floating(ctx, "title", Align2::LEFT_TOP, vec2(12.0, 12.0), 4, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let menu = theme::icon_button(ui, "menu", false, true).on_hover_text("Menu");
            app.menu_popup = Some(egui::Popup::default_response_id(&menu));
            egui::Popup::from_toggle_button_response(&menu)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(250.0)
                .show(|ui| main_menu(app, ui));
            board_name(app, ui);
            let (text, p) = (save_state(app), theme::current(ui.ctx()));
            ui.label(egui::RichText::new(text).size(11.0).color(p.mute));
            ui.add_space(6.0);
        });
    });
}

/// Board name, editable in place; it is only committed (and only creates an undo step) on validation.
fn board_name(app: &mut App, ui: &mut Ui) {
    let p = theme::current(ui.ctx());
    let id = egui::Id::new("board-name");
    let editing = ui.memory(|m| m.has_focus(id));
    let mut buf: String = if editing { ui.data(|d| d.get_temp(id)).unwrap_or_else(|| app.ed.doc.name.clone()) } else { app.ed.doc.name.clone() };
    let hover_id = id.with("hover");
    let hovered: bool = ui.data(|d| d.get_temp(hover_id)).unwrap_or(false);
    let fill = if editing || hovered { p.ui2 } else { Color32::TRANSPARENT };
    let resp = egui::Frame::new().fill(fill).corner_radius(6.0).show(ui, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut buf)
                .id(id)
                .frame(false)
                .desired_width(150.0)
                .margin(vec2(6.0, 6.0))
                .font(FontId::proportional(14.0))
                .char_limit(80),
        )
    });
    ui.data_mut(|d| d.insert_temp(hover_id, resp.response.hovered()));
    let r = resp.inner;
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    }
    if r.lost_focus() {
        let name = buf.trim();
        if !name.is_empty() && name != app.ed.doc.name {
            app.ed.set_name(name);
        }
        ui.data_mut(|d| d.remove::<String>(id));
    }
}

fn save_state(app: &App) -> String {
    match (app.dirty(), app.path.is_some()) {
        (true, _) => t("Unsaved"),
        (false, true) => t("Saved"),
        (false, false) => String::new(),
    }
}

/* -------------------------------------------------------------- outils */

fn tool_card(app: &mut App, ctx: &Context, narrow: bool) {
    let (anchor, offset) = if narrow { (Align2::LEFT_TOP, vec2(12.0, 62.0)) } else { (Align2::CENTER_TOP, vec2(0.0, 12.0)) };
    theme::floating(ctx, "tools", anchor, offset, 4, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            for (gi, group) in TOOL_GROUPS.iter().enumerate() {
                if gi > 0 {
                    theme::vsep(ui);
                }
                for (tool, icon, label) in *group {
                    let key = Action::for_tool(*tool).map(|a| app.keymap.label(a)).unwrap_or_default();
                    let mut tip = if key.is_empty() { t(label) } else { format!("{} ({key})", t(label)) };
                    let lockable = lockable(*tool);
                    if lockable {
                        tip = format!("{tip}\n{}", t("Double-click to lock"));
                    }
                    let (active, locked) = (app.tool == *tool, app.tool == *tool && app.tool_lock && lockable);
                    let resp = theme::icon_button(ui, icon, active, true);
                    if locked {
                        theme::lock_badge(ui, resp.rect);
                    }
                    let name = if locked { format!("{} ({})", t(label), t("locked")) } else { t(label) };
                    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, &name));
                    let resp = resp.on_hover_text(tip);
                    if resp.double_clicked() && lockable {
                        app.set_tool(*tool);
                        app.tool_lock = true;
                    } else if resp.clicked() {
                        app.set_tool(*tool);
                    }
                }
            }
            if theme::icon_button(ui, "image", false, true).on_hover_text(t("Insert image")).clicked() {
                app.insert_images_dialog();
            }
            if theme::icon_button(ui, "library", app.palette.open, true).on_hover_text(t("Icon library")).clicked() {
                app.palette.toggle();
            }
            let key = Action::for_tool(Tool::Laser).map(|a| app.keymap.label(a)).unwrap_or_default();
            if theme::icon_button(ui, "laser", app.tool == Tool::Laser, true).on_hover_text(format!("{} ({key})", t("Laser pointer"))).clicked() {
                app.set_tool(Tool::Laser);
            }
            theme::vsep(ui);
            let name = t("Keep the tool active after drawing");
            let tip = format!("{name} ({})", app.keymap.label(Action::ToolLock));
            let resp = theme::icon_button(ui, if app.tool_lock { "lock" } else { "unlock" }, app.tool_lock, true);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, app.tool_lock, &name));
            if resp.on_hover_text(tip).clicked() {
                app.tool_lock = !app.tool_lock;
            }
        });
    });
}

/// Tools that create an object and then give the hand back, so the lock makes sense for them (not Select, Hand, Eraser, Laser, nor the continuous Pen, Highlighter, Vote).
fn lockable(tool: Tool) -> bool {
    matches!(tool, Tool::Shape(_) | Tool::Line | Tool::Arrow | Tool::Text | Tool::Sticky | Tool::Table)
}

/* ---------------------------------------------------------- barre du bas */

fn zoom_card(app: &mut App, ctx: &Context) {
    theme::floating(ctx, "zoom", Align2::LEFT_BOTTOM, vec2(12.0, -12.0), 3, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            if theme::icon_button(ui, "undo", false, app.ed.can_undo()).on_hover_text(format!("{} ({})", t("Undo"), app.keymap.label(Action::Undo))).clicked() {
                app.ed.undo();
            }
            if theme::icon_button(ui, "redo", false, app.ed.can_redo()).on_hover_text(format!("{} ({})", t("Redo"), app.keymap.label(Action::Redo))).clicked() {
                app.ed.redo();
            }
            theme::vsep(ui);
            if theme::icon_button(ui, "minus", false, true).on_hover_text(t("Zoom out")).clicked() {
                app.zoom_by(0.8);
            }
            let v = app.ed.page().view;
            if theme::text_button(ui, &format!("{:.0} %", v.zoom * 100.0))
                .on_hover_text(format!("{} ({})", t("Back to 100 %"), app.keymap.label(Action::Zoom100)))
                .clicked()
            {
                app.set_view(View { zoom: 1.0, ..v });
            }
            if theme::icon_button(ui, "plus", false, true).on_hover_text(t("Zoom in")).clicked() {
                app.zoom_by(1.25);
            }
            if theme::icon_button(ui, "fit", false, true).on_hover_text(format!("{} ({})", t("Fit all"), app.keymap.label(Action::ZoomFit))).clicked() {
                app.fit_view(false);
            }
            if let Some((text, over)) = crate::ui::timer_text(app) {
                if over && !app.timer_done {
                    app.timer_done = true;
                    app.toast(&t("Time's up"));
                }
                theme::vsep(ui);
                let p = theme::current(ui.ctx());
                ui.label(egui::RichText::new(format!("⏱ {text}")).color(if over { Color32::from_rgb(0xe0, 0x31, 0x31) } else { p.tx }));
                ui.add_space(6.0);
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        });
    });
}

/// Timer choices: a countdown shown next to the zoom (and large in presentation mode); it turns red and warns when it reaches zero.
fn timer_menu(app: &mut App, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        let p = theme::current(ui.ctx());
        ui.label(egui::RichText::new(t("Timer")).size(13.0).color(p.tx));
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_space(6.0);
        for min in [1u64, 5, 10, 15, 30] {
            if ui.button(tr!("{} min", min)).clicked() {
                app.start_timer(min);
                ui.close();
            }
        }
        if app.timer.is_some() && ui.button(t("Stop")).clicked() {
            app.timer = None;
            ui.close();
        }
    });
}

fn help_card(app: &mut App, ctx: &Context) {
    theme::floating(ctx, "help", Align2::RIGHT_BOTTOM, vec2(-12.0, -12.0), 3, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            crate::share_ui::title_button(app, ui);
            let timer = theme::icon_button(ui, "timer", app.timer.is_some(), true).on_hover_text(t("Timer (time-box a workshop or a presentation)"));
            egui::Popup::from_toggle_button_response(&timer).close_behavior(egui::PopupCloseBehavior::CloseOnClick).width(230.0).show(|ui| timer_menu(app, ui));
            if theme::icon_button(ui, "help", false, true).on_hover_text(t("Keyboard shortcuts")).clicked() {
                app.dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: None });
            }
        });
    });
}

fn pages_card(app: &mut App, ctx: &Context) {
    theme::floating(ctx, "pages", Align2::CENTER_BOTTOM, vec2(0.0, -12.0), 3, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let pages: Vec<(Id, String)> = app.ed.doc.pages.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
            for (id, name) in &pages {
                let on = &app.ed.current == id;
                let p = theme::current(ui.ctx());
                let g = ui.painter().layout_no_wrap(name.clone(), FontId::proportional(13.0), if on { p.ac } else { p.tx });
                let (r, resp) = ui.allocate_exact_size(vec2((g.size().x + 20.0).max(48.0), 34.0), Sense::click());
                if on {
                    ui.painter().rect_filled(r, 8.0, p.acbg);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 8.0, p.ui2);
                }
                ui.painter().galley(r.center() - g.size() / 2.0, g, p.tx);
                if resp.clicked() {
                    app.commit_editing();
                    app.ed.current = id.clone();
                    app.sel.clear();
                }
                resp.context_menu(|ui| {
                    theme::menu_style(ui);
                    if theme::menu_item(ui, Some("edit"), "Rename…", "", false, true) {
                        app.dialog = Some(Dialog::RenamePage { id: id.clone(), name: name.clone() });
                        ui.close();
                    }
                    if theme::menu_item(ui, Some("trash"), "Delete page", "", false, pages.len() > 1) {
                        app.commit_editing();
                        app.ed.remove_page(id);
                        app.sel.clear();
                        ui.close();
                    }
                });
            }
            if theme::icon_button(ui, "plus", false, true).on_hover_text(t("New page")).clicked() {
                app.commit_editing();
                let n = app.ed.doc.pages.len() + 1;
                app.ed.add_page(&format!("Page {n}"));
                app.sel.clear();
                app.fit_pending = true;
            }
        });
    });
}

/* ------------------------------------------------------------ menu principal */

fn mi(ui: &mut Ui, icon: &str, label: &str, key: &str) -> bool {
    let clicked = theme::menu_item(ui, Some(icon), label, key, false, true);
    if clicked {
        ui.close();
    }
    clicked
}

fn key(app: &App, a: Action) -> String {
    app.keymap.label(a)
}

fn main_menu(app: &mut App, ui: &mut Ui) {
    // opening the menu validates the text being typed (saving or exporting from it must include it)
    app.commit_editing();
    let p = theme::current(ui.ctx());
    let max_h = ui.ctx().content_rect().height() * 0.8;
    ui.set_max_height(max_h);
    egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
        theme::menu_style(ui);
        ui.set_min_width(238.0);
        if mi(ui, "filenew", "New board", &key(app, Action::New)) {
            app.request_new_document();
        }
        if mi(ui, "folder", "Open / import a file…", &key(app, Action::Open)) {
            app.open_dialog();
        }
        if mi(ui, "mermaid", "Import Mermaid…", "") {
            app.dialog = Some(Dialog::Mermaid {
                text: t("flowchart TD\n  A[Idea] --> B{Feasible?}\n  B -->|Yes| C[Prototype]\n  B -->|No| D[Drop it]"),
                new_board: false,
                error: None,
            });
        }
        if mi(ui, "save", "Save", &key(app, Action::Save)) {
            app.save();
        }
        if mi(ui, "save", "Save as…", &key(app, Action::SaveAs)) {
            app.save_as();
        }
        theme::menu_sep(ui);
        let sel = !app.sel.is_empty();
        if mi(ui, "download", if sel { "Export selection as PNG" } else { "Export as PNG" }, "") {
            app.export("png");
        }
        if mi(ui, "download", "Export as SVG", "") {
            app.export("svg");
        }
        if mi(ui, "download", "Export as PDF", "") {
            app.export("pdf");
        }
        if mi(ui, "copy", "Copy image to clipboard", "") {
            app.copy_as_image();
        }
        theme::menu_label(ui, "Exchange with other tools");
        if mi(ui, "download", "Export as draw.io (.drawio)", "") {
            app.export("drawio");
        }
        if mi(ui, "download", "Export as Excalidraw", "") {
            app.export("excalidraw");
        }
        if mi(ui, "download", "Export as JSON Canvas (.canvas)", "") {
            app.export("canvas");
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "Insert");
        if mi(ui, "image", "Image…", "") {
            app.insert_images_dialog();
        }
        if mi(ui, "library", "Icon library…", "") {
            app.palette.toggle();
        }
        if mi(ui, "code", "Code block", "") {
            app.insert_code_block();
        }
        if mi(ui, "formula", "Formula (LaTeX)…", "") {
            app.dialog =
                Some(Dialog::Formula { src: r"E = mc^2, \quad \sum_{i=1}^{n} x_i \leq \frac{a+b}{2}".into(), editing: None, preview: Default::default() });
        }
        if mi(ui, "vote", "Vote dot", "") {
            app.set_tool(Tool::Vote);
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "Edit");
        if theme::menu_item(ui, Some("undo"), "Undo", &key(app, Action::Undo), false, app.ed.can_undo()) {
            app.ed.undo();
            ui.close();
        }
        if theme::menu_item(ui, Some("redo"), "Redo", &key(app, Action::Redo), false, app.ed.can_redo()) {
            app.ed.redo();
            ui.close();
        }
        if mi(ui, "all", "Select all", &key(app, Action::SelectAll)) {
            app.select_all();
        }
        if mi(ui, "layers", "Layers…", "") {
            app.layers_open = !app.layers_open;
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "Background");
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            for preset in BgPreset::ALL {
                let on = app.ed.doc.background.preset == preset;
                let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                let (cr, cg, cb) = preset.rgb();
                ui.painter().circle_filled(r.center(), 11.0, Color32::from_rgb(cr, cg, cb));
                ui.painter().circle_stroke(r.center(), 12.0, Stroke::new(2.0_f32, if on { p.ac } else { p.bd }));
                if on {
                    ui.painter().circle_stroke(r.center(), 14.0, Stroke::new(2.0_f32, p.acbg));
                }
                if resp.on_hover_text(t(preset.label())).clicked() {
                    let mut bg = app.ed.doc.background;
                    bg.preset = preset;
                    app.ed.set_background(bg);
                }
            }
        });
        ui.add_space(4.0);
        for (label, pat) in [("No pattern", Pattern::None), ("Dots", Pattern::Dots), ("Grid", Pattern::Grid), ("Lines", Pattern::Lines)] {
            if theme::menu_item(ui, Some("grid"), label, "", app.ed.doc.background.pattern == pat, true) {
                let mut bg = app.ed.doc.background;
                bg.pattern = pat;
                app.ed.set_background(bg);
            }
        }
        if theme::menu_item(ui, Some("grid"), "Snap to grid", "", app.snap, true) {
            app.snap = !app.snap;
        }
        if theme::menu_item(ui, Some("select"), "Animate the agent", "", app.agent.animate, true) {
            app.agent.animate = !app.agent.animate;
            crate::presence::save_animate(app.agent.animate);
            if !app.agent.animate {
                app.agent.state.clear();
            }
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "View");
        if mi(ui, "play", "Present full screen", &key(app, Action::Present)) {
            app.present_req = Some(true);
        }
        if mi(ui, "fit", "Fit all", &key(app, Action::ZoomFit)) {
            app.fit_view(false);
        }
        if mi(ui, "full", "Zoom 100 %", &key(app, Action::Zoom100)) {
            let v = app.ed.page().view;
            app.set_view(View { zoom: 1.0, ..v });
        }
        timer_menu(app, ui);
        theme::menu_sep(ui);
        theme::menu_label(ui, "Language");
        for (label, pref) in [
            ("Automatic (system language)", crate::i18n::Pref::Auto),
            ("English", crate::i18n::Pref::Lang(crate::i18n::Lang::En)),
            ("Français", crate::i18n::Pref::Lang(crate::i18n::Lang::Fr)),
        ] {
            if theme::menu_item(ui, Some("page"), label, "", app.lang_pref == pref, true) {
                app.lang_pref = pref;
                crate::i18n::save_pref(pref);
                crate::i18n::set(crate::i18n::resolve(pref));
            }
        }
        theme::menu_sep(ui);
        if mi(ui, "keyboard", "Keyboard shortcuts…", "") {
            app.dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: None });
        }
        if mi(ui, "trashAll", "Clear the whole board", "") {
            let ids: Vec<Id> = app.ed.page().objects.iter().map(|o| o.id.clone()).collect();
            app.ed.remove(&ids);
            app.sel.clear();
        }
        if mi(ui, "full", "Quit", "") {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

/* ----------------------------------------------------- menus contextuels */

/// Right-click menu on the canvas background.
pub fn canvas_menu(app: &mut App, ui: &mut Ui) {
    theme::menu_style(ui);
    if mi(ui, "paste", "Paste", "Ctrl+V") {
        app.paste_from_menu();
    }
    if mi(ui, "all", "Select all", &key(app, Action::SelectAll)) {
        app.select_all();
    }
    if mi(ui, "fit", "Fit all", &key(app, Action::ZoomFit)) {
        app.fit_view(false);
    }
}

/// Right-click menu on the selection.
pub fn selection_menu(app: &mut App, ui: &mut Ui) {
    theme::menu_style(ui);
    let sel = app.sel.clone();
    if mi(ui, "cut", "Cut", "Ctrl+X") {
        app.copy_selection();
        app.delete_selection();
    }
    if mi(ui, "copy", "Copy", "Ctrl+C") {
        app.copy_selection();
    }
    if mi(ui, "image", "Copy as image", "") {
        app.copy_as_image();
    }
    if mi(ui, "paste", "Paste", "Ctrl+V") {
        app.paste_from_menu();
    }
    if mi(ui, "copy", "Duplicate", &key(app, Action::Duplicate)) {
        app.sel = app.ed.duplicate(&sel, Pt::new(24.0, 24.0));
    }
    if let [id] = sel.as_slice() {
        let kind = app.ed.page().get(id).map(|o| o.kind.clone());
        if matches!(kind, Some(Kind::Box(_) | Kind::Table(_) | Kind::Connector(_))) && mi(ui, "edit", "Edit text", &key(app, Action::EditText)) {
            let cell = match &kind {
                Some(Kind::Table(t)) => t.cell_at(app.ctx_pos),
                _ => None,
            };
            app.start_editing(id, cell);
        }
        if matches!(&kind, Some(Kind::Image(i)) if i.latex.is_some()) && mi(ui, "formula", "Edit formula…", "") {
            app.edit_formula(id);
        }
        if matches!(&kind, Some(Kind::Box(b)) if b.label.family == FontFamily::Mono) && mi(ui, "code", "Color the code", "") {
            app.recolor_code(id);
        }
        if matches!(&kind, Some(Kind::Connector(c)) if !c.via.is_empty()) && mi(ui, "line", "Straighten (remove waypoints)", "") {
            app.ed.update(id, |o| {
                if let Kind::Connector(c) = &mut o.kind {
                    c.via.clear();
                }
            });
        }
    }
    table_menu(app, ui, &sel);
    theme::menu_sep(ui);
    for (icon, label, o) in [
        ("front", "Bring to front", Order::Front),
        ("forward", "Bring forward", Order::Forward),
        ("backward", "Send backward", Order::Backward),
        ("back", "Send to back", Order::Back),
    ] {
        if mi(ui, icon, label, "") {
            app.ed.reorder(&sel, o);
        }
    }
    theme::menu_sep(ui);
    if sel.len() > 1 && mi(ui, "group", "Group", &key(app, Action::Group)) {
        app.ed.group(&sel);
    }
    if sel.iter().any(|i| app.ed.page().get(i).is_some_and(|o| o.group.is_some())) && mi(ui, "ungroup", "Ungroup", &key(app, Action::Ungroup)) {
        app.ed.ungroup(&sel);
    }
    let all_locked = sel.iter().all(|i| app.ed.page().get(i).is_some_and(|o| o.locked));
    if mi(ui, if all_locked { "unlock" } else { "lock" }, if all_locked { "Unlock" } else { "Lock" }, "") {
        app.ed.set_locked(&sel, !all_locked);
    }
    theme::menu_sep(ui);
    if mi(ui, "trash", "Delete", &key(app, Action::Delete)) {
        app.delete_selection();
    }
}

/// Operations on the row / column under the right click, when a table is selected.
fn table_menu(app: &mut App, ui: &mut Ui, sel: &[Id]) {
    let [id] = sel else { return };
    let Some((r, c)) = app.ed.page().get(id).and_then(|o| if let Kind::Table(t) = &o.kind { t.cell_at(app.ctx_pos) } else { None }) else { return };
    theme::menu_sep(ui);
    theme::menu_label_raw(ui, &tr!("Cell row {} · column {}", r + 1, c + 1));
    type Op = fn(&mut inkboard_core::Table, usize, usize);
    let ops: [(&str, Op); 6] = [
        ("Insert row above", |t, r, _| t.insert_row(r)),
        ("Insert row below", |t, r, _| t.insert_row(r + 1)),
        ("Insert column to the left", |t, _, c| t.insert_col(c)),
        ("Insert column to the right", |t, _, c| t.insert_col(c + 1)),
        ("Delete row", |t, r, _| t.remove_row(r)),
        ("Delete column", |t, _, c| t.remove_col(c)),
    ];
    for (label, op) in ops {
        if mi(ui, "table", label, "") {
            app.ed.update(id, |o| {
                if let Kind::Table(t) = &mut o.kind {
                    op(t, r, c);
                }
            });
        }
    }
}
