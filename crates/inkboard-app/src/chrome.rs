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
use inkboard_core::{BgPreset, BoxGeom, FontFamily, Id, Kind, Pattern, Pt, View};
use std::time::Duration;

/// Window width below which the toolbar moves under the title bar (`@media (max-width:1180px)`).
const NARROW: f32 = 1180.0;

/// Window width below which the row of tools does not fit any more (it then scrolls sideways).
const TOOLS_ROW_WIDTH: f32 = 820.0;

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
    let view_only = app.view_only();
    let (anchor, offset) = if narrow { (Align2::LEFT_TOP, vec2(12.0, 62.0)) } else { (Align2::CENTER_TOP, vec2(0.0, 12.0)) };
    theme::floating(ctx, "tools", anchor, offset, 4, |ui| {
        if narrow && ctx.content_rect().width() < TOOLS_ROW_WIDTH {
            // a phone is narrower than the row of tools (about 760 px): it scrolls sideways (a drag) instead of losing the last tools
            let room = ctx.content_rect().width() - 2.0 * offset.x - 16.0;
            let out = egui::ScrollArea::horizontal()
                .max_width(room.max(120.0))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| tool_row(app, ui, view_only));
            // a chevron tells that there are more tools to drag into view
            if out.state.offset.x + out.inner_rect.width() < out.content_size.x - 1.0 {
                let at = egui::pos2(out.inner_rect.right() - 6.0, out.inner_rect.center().y);
                let color = ui.visuals().strong_text_color();
                ui.painter().text(at, Align2::RIGHT_CENTER, "›", egui::FontId::proportional(22.0), color);
            }
        } else {
            tool_row(app, ui, view_only);
        }
    });
}

/// The tools in one row.
fn tool_row(app: &mut App, ui: &mut Ui, view_only: bool) {
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
                let resp = theme::icon_button(ui, icon, active, !view_only || matches!(tool, Tool::Hand));
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
        if theme::icon_button(ui, "image", false, !view_only).on_hover_text(t("Insert image")).clicked() {
            app.insert_images_dialog();
        }
        if theme::icon_button(ui, "library", app.palette.open, !view_only).on_hover_text(t("Icon library")).clicked() {
            app.palette.toggle();
        }
        let key = Action::for_tool(Tool::Laser).map(|a| app.keymap.label(a)).unwrap_or_default();
        if theme::icon_button(ui, "laser", app.tool == Tool::Laser, true).on_hover_text(format!("{} ({key})", t("Laser pointer"))).clicked() {
            app.set_tool(Tool::Laser);
        }
        theme::vsep(ui);
        let name = t("Keep the tool active after drawing");
        let tip = format!("{name} ({})", app.keymap.label(Action::ToolLock));
        let resp = theme::icon_button(ui, if app.tool_lock { "lock" } else { "unlock" }, app.tool_lock, !view_only);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, app.tool_lock, &name));
        if resp.on_hover_text(tip).clicked() {
            app.tool_lock = !app.tool_lock;
        }
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
            if theme::icon_button(ui, "undo", false, app.can_undo()).on_hover_text(format!("{} ({})", t("Undo"), app.keymap.label(Action::Undo))).clicked() {
                app.undo();
            }
            if theme::icon_button(ui, "redo", false, app.can_redo()).on_hover_text(format!("{} ({})", t("Redo"), app.keymap.label(Action::Redo))).clicked() {
                app.redo();
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

/// Height of a page tab and width of the buttons of the page card.
const TAB_H: f32 = 34.0;
/// Spacing between the items of the page card.
const TAB_GAP: f32 = 2.0;
/// Widest tab before its name is cut with an ellipsis, then the narrowest it may shrink to.
const TAB_MAX: f32 = 140.0;
const TAB_MIN: f32 = 72.0;
/// Free space kept between the page card and its neighbours; padding and border of the card itself.
const PAGES_MARGIN: f32 = 12.0;
const CARD_PAD: f32 = 10.0;
/// From this many pages the page list of the compact mode gets a search field.
const PAGES_SEARCH: usize = 12;

/// How the page card is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PagesLevel {
    /// Every tab at its natural width.
    Full,
    /// Tabs capped at the given width (names cut with an ellipsis).
    Ellipsis(f32),
    /// `◀ name ▾ ▶ +`.
    Compact,
}

/// Decides how the page tabs are drawn: `natural` are the tab widths, `available` the room for the tabs (gaps included, "+" excluded).
fn pages_level(natural: &[f32], available: f32) -> PagesLevel {
    let gaps = TAB_GAP * f32::from(u16::try_from(natural.len().saturating_sub(1)).unwrap_or(u16::MAX));
    let total = |cap: f32| natural.iter().map(|w| w.min(cap)).sum::<f32>() + gaps;
    if total(f32::INFINITY) <= available {
        return PagesLevel::Full;
    }
    if total(TAB_MIN) > available {
        return PagesLevel::Compact;
    }
    if total(TAB_MAX) <= available {
        return PagesLevel::Ellipsis(TAB_MAX);
    }
    let (mut lo, mut hi) = (TAB_MIN, TAB_MAX);
    for _ in 0..20 {
        let mid = f32::midpoint(lo, hi);
        if total(mid) <= available {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    PagesLevel::Ellipsis(lo)
}

/// `text` cut with an ellipsis so that it is at most `max` wide.
fn ellipsize(ui: &Ui, text: &str, font: &FontId, max: f32) -> String {
    let width = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x;
    if width(text) <= max {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |n: usize| chars[..n].iter().collect::<String>().trim_end().to_owned() + "…";
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&cut(mid)) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
}

/// Room for the page card: the width between the cards at the bottom left (zoom) and bottom right (help), as drawn on the previous frame.
fn pages_room(ctx: &Context) -> f32 {
    let screen = ctx.content_rect();
    let c = screen.center().x;
    let left = ctx.memory(|m| m.area_rect(egui::Id::new("zoom"))).map_or(screen.left() + 330.0, |r| r.right());
    let right = ctx.memory(|m| m.area_rect(egui::Id::new("help"))).map_or(screen.right() - 170.0, |r| r.left());
    2.0 * (c - left).min(right - c) - 2.0 * PAGES_MARGIN
}

fn goto_page(app: &mut App, id: &Id) {
    app.commit_editing();
    app.ed.current = id.clone();
    app.sel.clear();
}

/// Right-click menu of a page (rename, delete).
fn page_menu(app: &mut App, ui: &mut Ui, id: &Id, name: &str, count: usize) {
    theme::menu_style(ui);
    if theme::menu_item(ui, Some("edit"), "Rename…", "", false, true) {
        app.dialog = Some(Dialog::RenamePage { id: id.clone(), name: name.to_owned() });
        ui.close();
    }
    if theme::menu_item(ui, Some("trash"), "Delete page", "", false, count > 1) {
        app.commit_editing();
        app.ed.remove_page(id);
        app.sel.clear();
        ui.close();
    }
}

fn pages_card(app: &mut App, ctx: &Context) {
    let room = pages_room(ctx);
    theme::floating(ctx, "pages", Align2::CENTER_BOTTOM, vec2(0.0, -12.0), 3, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = TAB_GAP;
            let pages: Vec<(Id, String)> = app.ed.doc.pages.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
            let font = FontId::proportional(13.0);
            let natural: Vec<f32> =
                pages.iter().map(|(_, n)| (ui.painter().layout_no_wrap(n.clone(), font.clone(), Color32::WHITE).size().x + 20.0).max(48.0)).collect();
            let level = pages_level(&natural, room - CARD_PAD - TAB_H - TAB_GAP);
            if level == PagesLevel::Compact {
                compact_pages(app, ui, &pages, room - CARD_PAD);
            } else {
                let cap = if let PagesLevel::Ellipsis(w) = level { w } else { f32::INFINITY };
                for ((id, name), nat) in pages.iter().zip(&natural) {
                    let on = &app.ed.current == id;
                    let p = theme::current(ui.ctx());
                    let w = nat.min(cap);
                    let shown = if w < *nat { ellipsize(ui, name, &font, w - 20.0) } else { name.clone() };
                    let g = ui.painter().layout_no_wrap(shown, font.clone(), if on { p.ac } else { p.tx });
                    let (r, resp) = ui.allocate_exact_size(vec2(w, TAB_H), Sense::click());
                    if on {
                        ui.painter().rect_filled(r, 8.0, p.acbg);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 8.0, p.ui2);
                    }
                    ui.painter().galley(r.center() - g.size() / 2.0, g, p.tx);
                    if resp.clicked() {
                        goto_page(app, id);
                    }
                    let resp = if w < *nat { resp.on_hover_text(name) } else { resp };
                    resp.context_menu(|ui| page_menu(app, ui, id, name, pages.len()));
                }
            }
            new_page_button(app, ui);
        });
    });
}

fn new_page_button(app: &mut App, ui: &mut Ui) {
    if theme::icon_button(ui, "plus", false, true).on_hover_text(t("New page")).clicked() {
        app.commit_editing();
        let n = app.ed.doc.pages.len() + 1;
        app.ed.add_page(&format!("Page {n}"));
        app.sel.clear();
        app.fit_pending = true;
    }
}

/// Compact page card: `◀ name ▾ ▶` (the "+" is added by the caller).
fn compact_pages(app: &mut App, ui: &mut Ui, pages: &[(Id, String)], room: f32) {
    let pal = theme::current(ui.ctx());
    let font = FontId::proportional(13.0);
    let cur = pages.iter().position(|(id, _)| *id == app.ed.current).unwrap_or(0);
    let step = |ui: &mut Ui, glyph: &str, enabled: bool, tip: String| -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(TAB_H, TAB_H), if enabled { Sense::click() } else { Sense::hover() });
        if resp.hovered() && enabled {
            ui.painter().rect_filled(rect, 8.0, pal.ui2);
        }
        let color = if enabled { pal.tx } else { pal.tx.gamma_multiply(0.35) };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, font.clone(), color);
        resp.on_hover_text(tip).clicked() && enabled
    };
    if step(ui, "◀", cur > 0, t("Previous page")) {
        goto_page(app, &pages[cur - 1].0.clone());
    }
    let (id, name) = &pages[cur];
    let name_w = (room - 3.0 * TAB_H - 4.0 * TAB_GAP - 34.0).clamp(40.0, 220.0);
    let label = ellipsize(ui, &format!("{} · {name}", cur + 1), &font, name_w - 36.0);
    let gal = ui.painter().layout_no_wrap(label, font.clone(), pal.ac);
    let width = (gal.size().x + 36.0).min(name_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, TAB_H), Sense::click());
    ui.painter().rect_filled(rect, 8.0, if resp.hovered() { pal.ui2 } else { pal.acbg });
    ui.painter().galley(egui::pos2(rect.left() + 10.0, rect.center().y - gal.size().y / 2.0), gal, pal.ac);
    ui.painter().text(egui::pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, "▾", font.clone(), pal.ac);
    let resp = resp.on_hover_text(name);
    resp.context_menu(|ui| page_menu(app, ui, id, name, pages.len()));
    egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(240.0).show(|ui| {
        let search_id = egui::Id::new("pages-search");
        let mut query: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
        if pages.len() > PAGES_SEARCH {
            ui.add(egui::TextEdit::singleline(&mut query).hint_text(t("Search pages…")).desired_width(f32::INFINITY));
            ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
        }
        let q = query.trim().to_lowercase();
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (i, (pid, pname)) in pages.iter().enumerate() {
                if !q.is_empty() && !pname.to_lowercase().contains(&q) {
                    continue;
                }
                let row = ui.add_sized([ui.available_width(), 28.0], egui::Button::selectable(*pid == app.ed.current, format!("{}  {pname}", i + 1)));
                if row.clicked() {
                    goto_page(app, pid);
                    ui.close();
                }
                row.context_menu(|ui| page_menu(app, ui, pid, pname, pages.len()));
            }
        });
    });
    if step(ui, "▶", cur + 1 < pages.len(), t("Next page")) {
        goto_page(app, &pages[cur + 1].0.clone());
    }
}

/* ------------------------------------------------------------ menu principal */

fn mi(ui: &mut Ui, icon: &str, label: &str, key: &str) -> bool {
    mi_if(ui, icon, label, key, true)
}

/// Menu row that may be disabled (grayed out, no click).
fn mi_if(ui: &mut Ui, icon: &str, label: &str, key: &str, enabled: bool) -> bool {
    let clicked = theme::menu_item(ui, Some(icon), label, key, false, enabled);
    if clicked {
        ui.close();
    }
    clicked
}

fn key(app: &App, a: Action) -> String {
    app.keymap.label(a)
}

/// Export entries of the main menu.
fn export_items(app: &mut App, ui: &mut Ui) {
    theme::menu_sep(ui);
    let sel = !app.sel.is_empty();
    if mi(ui, "download", if sel { "Export selection as PNG" } else { "Export as PNG" }, "") {
        app.open_export("png");
    }
    if mi(ui, "download", "Export as SVG", "") {
        app.open_export("svg");
    }
    if mi(ui, "download", "Export as PDF", "") {
        app.open_export("pdf");
    }
    if mi(ui, "copy", "Copy image to clipboard", "") {
        app.copy_as_image();
    }
    theme::menu_label(ui, "Exchange with other tools");
    if mi(ui, "download", "Export as draw.io (.drawio)", "") {
        app.export("drawio");
    }
    if mi(ui, "download", "Export as Excalidraw", "") {
        app.open_export("excalidraw");
    }
    if mi(ui, "download", "Export as JSON Canvas (.canvas)", "") {
        app.open_export("canvas");
    }
}

/// Background color, pattern and grid snapping.
fn background_items(app: &mut App, ui: &mut Ui, p: &theme::Palette) {
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
}

/// Interface language.
fn language_items(app: &mut App, ui: &mut Ui) {
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
}

#[allow(clippy::too_many_lines, reason = "one menu, one entry after the other")]
fn main_menu(app: &mut App, ui: &mut Ui) {
    // opening the menu validates the text being typed (saving or exporting from it must include it)
    app.commit_editing();
    let p = theme::current(ui.ctx());
    let max_h = ui.ctx().content_rect().height() * 0.8;
    ui.set_max_height(max_h);
    egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
        theme::menu_style(ui);
        ui.set_min_width(238.0);
        // in a shared session the board is the host's: no new board, no opening another one
        let (guest, view_only) = (app.guest_active(), app.view_only());
        if mi_if(ui, "filenew", "New board", &key(app, Action::New), !guest) {
            app.request_new_document();
        }
        if mi_if(ui, "folder", "Open / import a file…", &key(app, Action::Open), !guest) {
            app.open_dialog();
        }
        if guest || app.guest.is_some() {
            if mi(ui, "play", "Leave the session", "") {
                app.leave_guest();
                app.join.window = None;
            }
        } else if mi(ui, "play", "Join a session…", "") {
            app.open_join_window(None);
        }
        if mi_if(ui, "mermaid", "Import Mermaid…", "", !guest) {
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
        export_items(app, ui);
        theme::menu_sep(ui);
        theme::menu_label(ui, "Insert");
        if mi_if(ui, "image", "Image…", "", !view_only) {
            app.insert_images_dialog();
        }
        if mi_if(ui, "library", "Icon library…", "", !view_only) {
            app.palette.toggle();
        }
        if mi_if(ui, "code", "Code block", "", !view_only) {
            app.insert_code_block();
        }
        if mi_if(ui, "formula", "Formula (LaTeX)…", "", !view_only) {
            app.dialog = Some(Dialog::Formula {
                src: r"E = mc^2, \quad \sum_{i=1}^{n} x_i \leq \frac{a+b}{2}".into(),
                editing: None,
                preview: crate::app::FormulaPreview::default(),
            });
        }
        if mi_if(ui, "vote", "Vote dot", "", !view_only) {
            app.set_tool(Tool::Vote);
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "Edit");
        if theme::menu_item(ui, Some("undo"), "Undo", &key(app, Action::Undo), false, app.can_undo()) {
            app.undo();
            ui.close();
        }
        if theme::menu_item(ui, Some("redo"), "Redo", &key(app, Action::Redo), false, app.can_redo()) {
            app.redo();
            ui.close();
        }
        if mi(ui, "all", "Select all", &key(app, Action::SelectAll)) {
            app.select_all();
        }
        if mi(ui, "layers", "Layers…", "") {
            app.layers_open = !app.layers_open;
        }
        theme::menu_sep(ui);
        background_items(app, ui, &p);
        if theme::menu_item(ui, Some("select"), "Animate the agent", "", app.agent.animate, true) {
            app.agent.animate = !app.agent.animate;
            crate::presence::save_animate(app.agent.animate);
            if !app.agent.animate {
                app.agent.state.clear();
            }
        }
        if !crate::fonts::ALWAYS_EMBEDDED && theme::menu_item(ui, Some("edit"), "Use built-in fonts", "", app.fonts_pref, true) {
            app.fonts_pref = !app.fonts_pref;
            crate::fonts::save_embedded_pref(app.fonts_pref);
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
        language_items(app, ui);
        theme::menu_sep(ui);
        if mi(ui, "keyboard", "Keyboard shortcuts…", "") {
            app.dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: None });
        }
        if mi_if(ui, "trashAll", "Clear the whole board", "", !guest) {
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
    type Op = fn(&mut inkboard_core::Table, usize, usize);
    let [id] = sel else { return };
    let Some((r, c)) = app.ed.page().get(id).and_then(|o| if let Kind::Table(t) = &o.kind { t.cell_at(app.ctx_pos) } else { None }) else { return };
    theme::menu_sep(ui);
    theme::menu_label_raw(ui, &tr!("Cell row {} · column {}", r + 1, c + 1));
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

#[cfg(test)]
mod pages_tests {
    use super::*;

    fn full(n: usize, w: f32) -> f32 {
        w * f32::from(u16::try_from(n).unwrap()) + TAB_GAP * f32::from(u16::try_from(n.saturating_sub(1)).unwrap())
    }

    #[test]
    fn everything_fits() {
        assert_eq!(pages_level(&[80.0, 90.0], 172.0), PagesLevel::Full);
        assert!(matches!(pages_level(&[80.0, 90.0], 171.9), PagesLevel::Ellipsis(_)));
        assert_eq!(pages_level(&[], 0.0), PagesLevel::Full);
    }

    #[test]
    fn one_page() {
        assert_eq!(pages_level(&[100.0], 100.0), PagesLevel::Full);
        assert_eq!(pages_level(&[500.0], 300.0), PagesLevel::Ellipsis(TAB_MAX));
        assert_eq!(pages_level(&[500.0], 50.0), PagesLevel::Compact);
    }

    #[test]
    fn long_names_are_capped_then_shrunk() {
        let n = [300.0; 4];
        assert_eq!(pages_level(&n, full(4, TAB_MAX)), PagesLevel::Ellipsis(TAB_MAX));
        match pages_level(&n, full(4, 100.0)) {
            PagesLevel::Ellipsis(w) => assert!((w - 100.0).abs() < 0.01, "{w}"),
            other => panic!("{other:?}"),
        }
        match pages_level(&n, full(4, TAB_MIN)) {
            PagesLevel::Ellipsis(w) => assert!((w - TAB_MIN).abs() < 0.01, "{w}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(pages_level(&n, full(4, TAB_MIN) - 0.1), PagesLevel::Compact);
    }

    #[test]
    fn many_pages_go_compact() {
        let n = [60.0; 50];
        assert_eq!(pages_level(&n, 1000.0), PagesLevel::Compact);
        assert_eq!(pages_level(&n, full(50, 60.0)), PagesLevel::Full);
        let long = [400.0; 50];
        assert_eq!(pages_level(&long, 1500.0), PagesLevel::Compact);
    }
}
