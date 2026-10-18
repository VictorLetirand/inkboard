//! Presentation, dialogs and notifications (the application frame is in `chrome.rs`, the style settings in `panel.rs`).

use crate::app::{App, Dialog};
use crate::i18n::{t, tr};
use crate::keymap::{Action, Shortcut};
use eframe::egui::{self, Color32, RichText as ERich, vec2};
use inkboard_core::*;
use std::time::{Duration, Instant};

/* ------------------------------------------------------- Presentation */

/// Remaining timer as "m:ss"; `None` when no timer is running.
pub fn timer_text(app: &App) -> Option<(String, bool)> {
    let (end, _) = app.timer?;
    let left = end.saturating_duration_since(Instant::now()).as_secs();
    Some((format!("{}:{:02}", left / 60, left % 60), left == 0))
}

/// Discreet presentation-mode overlay: page number and timer.
pub fn present_overlay(app: &mut App, ctx: &egui::Context) {
    let n = app.ed.doc.pages.len();
    let i = app.ed.doc.pages.iter().position(|p| p.id == app.ed.current).unwrap_or(0) + 1;
    if n > 1 {
        egui::Area::new(egui::Id::new("present_page")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -12.0)).order(egui::Order::Tooltip).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::from_black_alpha(150)).show(ui, |ui| {
                ui.label(ERich::new(format!("{i} / {n}")).color(Color32::WHITE));
            });
        });
    }
    if let Some((text, over)) = timer_text(app) {
        egui::Area::new(egui::Id::new("present_timer")).anchor(egui::Align2::RIGHT_TOP, vec2(-14.0, 12.0)).order(egui::Order::Tooltip).show(ctx, |ui| {
            let fill = if over { Color32::from_rgb(0xc9, 0x2a, 0x2a) } else { Color32::from_black_alpha(150) };
            egui::Frame::popup(ui.style()).fill(fill).show(ui, |ui| {
                ui.label(ERich::new(text).size(22.0).color(Color32::WHITE));
            });
        });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

/* ------------------------------------------------------------ Dialogues */

/// Centered dialog window (above the dimmed backdrop).
fn modal(title: impl Into<egui::WidgetText>) -> egui::Window<'static> {
    egui::Window::new(title)
        .fade_in(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .collapsible(false)
        .resizable(false)
}

/// Dimmed backdrop behind a dialog (`#modal`); returns `true` when it is clicked (the dialog closes).
fn backdrop(ctx: &egui::Context) -> bool {
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("modal-backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .fade_in(false)
        .show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(100));
            resp.clicked()
        })
        .inner
}

pub fn dialogs(app: &mut App, ctx: &egui::Context) {
    let Some(dialog) = app.dialog.take() else { return };
    if backdrop(ctx) {
        return; // click outside: the dialog is closed
    }
    let mut keep: Option<Dialog> = None;
    match dialog {
        Dialog::Mermaid { mut text, mut new_board, mut error } => {
            let mut open = true;
            let mut generate = false;
            modal(t("Import Mermaid")).open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
                ui.label(t("flowchart / graph, stateDiagram or sequenceDiagram. The layout is automatic."));
                ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(14).desired_width(f32::INFINITY));
                ui.radio_value(&mut new_board, false, t("Add to this board"));
                ui.radio_value(&mut new_board, true, t("New board"));
                if let Some(e) = &error {
                    ui.colored_label(Color32::from_rgb(0xe0, 0x31, 0x31), e);
                }
                if ui.button(t("Generate")).clicked() {
                    generate = true;
                }
            });
            if generate {
                match app.import_mermaid(&text, new_board) {
                    Ok(()) => open = false,
                    Err(e) => error = Some(e),
                }
            }
            if open {
                keep = Some(Dialog::Mermaid { text, new_board, error });
            }
        }
        Dialog::Formula { mut src, editing, mut preview } => {
            if preview.src.as_deref() != Some(src.as_str()) {
                // typeset again only when the source changes
                match inkboard_formats::math::render(&src, 36.0) {
                    Ok(f) => {
                        let asset = Asset { mime: "image/svg+xml".into(), data: f.svg.into_bytes().into() };
                        preview.tex = crate::render::decode_asset(&asset).map(|img| ctx.load_texture("formula_preview", img, egui::TextureOptions::LINEAR));
                        preview.error = None;
                    }
                    Err(e) => {
                        preview.tex = None;
                        preview.error = Some(e.to_string());
                    }
                }
                preview.src = Some(src.clone());
            }
            let mut open = true;
            let mut accepted = false;
            let title = if editing.is_some() { t("Edit formula") } else { t("Insert a formula") };
            modal(title).open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
                ui.label(t("LaTeX source (math mode): \\frac{a}{b}, x^2, \\sqrt{x}, \\sum, \\int, \\begin{pmatrix}…"));
                ui.add(egui::TextEdit::multiline(&mut src).code_editor().desired_rows(3).desired_width(f32::INFINITY));
                ui.separator();
                match (&preview.tex, &preview.error) {
                    (Some(tex), _) => {
                        let s = tex.size_vec2() / 4.0;
                        let k = (ui.available_width() / s.x).min(1.0);
                        ui.add(egui::Image::new((tex.id(), s * k)).tint(ui.visuals().text_color()));
                    }
                    (None, Some(e)) => {
                        ui.colored_label(Color32::from_rgb(0xe0, 0x31, 0x31), e);
                    }
                    _ => {}
                }
                if ui.add_enabled(preview.tex.is_some(), egui::Button::new(if editing.is_some() { t("Apply") } else { t("Insert") })).clicked() {
                    accepted = true;
                }
            });
            if accepted {
                match app.insert_formula(&src, editing.as_ref()) {
                    Ok(()) => open = false,
                    Err(e) => preview.error = Some(e),
                }
            }
            if open {
                keep = Some(Dialog::Formula { src, editing, preview });
            }
        }
        Dialog::Shortcuts { filter, recording } => {
            keep = shortcuts_dialog(app, ctx, filter, recording);
        }
        Dialog::RenamePage { id, mut name } => {
            let mut open = true;
            let mut accepted = false;
            modal(t("Rename page")).open(&mut open).collapsible(false).show(ctx, |ui| {
                let r = ui.text_edit_singleline(&mut name);
                r.request_focus();
                if ui.button("OK").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    accepted = true;
                }
            });
            if accepted {
                app.ed.rename_page(&id, &name);
                open = false;
            }
            if open {
                keep = Some(Dialog::RenamePage { id, name });
            }
        }
    }
    if app.dialog.is_none() {
        app.dialog = keep;
    }
}

/// Shortcut settings window: list by group, adding (records the next key press), removal, reset.
fn shortcuts_dialog(app: &mut App, ctx: &egui::Context, mut filter: String, mut recording: Option<Action>) -> Option<Dialog> {
    let mut open = true;
    // recording: the next combination pressed is assigned to the action
    if let Some(a) = recording {
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, m)) = pressed {
            if key == egui::Key::Escape {
                recording = None;
            } else {
                let sc = Shortcut::new(key, m.command, m.shift, m.alt);
                match app.keymap.add(a, sc) {
                    Ok(Some(other)) => {
                        app.toast(&tr!("{} removed from \"{}\"", sc.label(), t(other.label())));
                        app.save_keymap();
                    }
                    Ok(None) => app.save_keymap(),
                    Err(e) => app.toast(&e),
                }
                recording = None;
            }
        }
    }
    modal(t("Keyboard shortcuts")).open(&mut open).collapsible(false).default_width(520.0).default_height(560.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(t("Filter:"));
            ui.text_edit_singleline(&mut filter);
            if ui.button(t("Reset all")).clicked() {
                app.keymap.reset_all();
                app.save_keymap();
                recording = None;
            }
        });
        ui.label(ERich::new(t("Click \"+\" then press the combination you want (Escape to cancel). Click a shortcut to remove it.")).small().weak());
        for (sc, x, y) in app.keymap.conflicts() {
            ui.colored_label(Color32::from_rgb(0xe0, 0x31, 0x31), tr!("Conflict: {} is used by \"{}\" and \"{}\"", sc.label(), t(x.label()), t(y.label())));
        }
        ui.separator();
        let max_h = (ui.ctx().content_rect().height() - 260.0).max(200.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(max_h).show(ui, |ui| {
            let mut group = "";
            let f = filter.to_lowercase();
            for a in Action::ALL {
                let shown =
                    f.is_empty() || t(a.label()).to_lowercase().contains(&f) || app.keymap.bindings(a).iter().any(|s| s.label().to_lowercase().contains(&f));
                if !shown {
                    continue;
                }
                if a.group() != group {
                    group = a.group();
                    ui.add_space(6.0);
                    ui.label(ERich::new(t(group)).strong());
                }
                ui.horizontal(|ui| {
                    let (cell, _) = ui.allocate_exact_size(vec2(190.0, 22.0), egui::Sense::hover());
                    ui.painter().text(
                        cell.left_center(),
                        egui::Align2::LEFT_CENTER,
                        t(a.label()),
                        egui::TextStyle::Body.resolve(ui.style()),
                        ui.visuals().text_color(),
                    );
                    for sc in app.keymap.bindings(a).to_vec() {
                        if ui.button(sc.label()).on_hover_text(t("Remove this shortcut")).clicked() {
                            app.keymap.remove(a, &sc);
                            app.save_keymap();
                        }
                    }
                    if recording == Some(a) {
                        ui.label(ERich::new(t("Press a key…")).color(crate::theme::current(ctx).ac));
                    } else if ui.small_button("+").on_hover_text(t("Add a shortcut")).clicked() {
                        recording = Some(a);
                    }
                    if !app.keymap.is_default(a) && ui.small_button("↺").on_hover_text(t("Restore the default")).clicked() {
                        app.keymap.reset(a);
                        app.save_keymap();
                    }
                });
            }
        });
    });
    open.then_some(Dialog::Shortcuts { filter, recording })
}

pub fn toast(app: &mut App, ctx: &egui::Context) {
    let Some((msg, t)) = app.toast.clone() else { return };
    if t.elapsed() > Duration::from_secs(4) {
        app.toast = None;
        return;
    }
    egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -48.0)).order(egui::Order::Tooltip).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(Color32::from_rgb(0x22, 0x22, 0x22)).show(ui, |ui| {
            ui.label(ERich::new(msg).color(Color32::WHITE));
        });
    });
    ctx.request_repaint_after(Duration::from_millis(300));
}
