//! Interface of the guest mode: the "Join a session" window (link and name, waiting for the host, network failure message with the
//! diagnostic to copy) and the banners shown over a joined session (who hosts, reconnecting, session over with "Save a copy").

use crate::app::App;
use crate::i18n::{t, tr};
use crate::session_guest::{EndReason, GuestSession, GuestState, NAME_MAX, NetIssue, parse_join_link, sanitize_name};
use eframe::egui::{self, Align2, Color32, RichText, vec2};
use inkboard_session::crypto::Ticket;
use inkboard_session::proto::RefuseReason;

/// Builds the session of a guest (real network, diagnosis…). The application plugs the real one in; tests plug a simulated network.
pub type SessionFactory = Box<dyn Fn(Ticket, &str) -> GuestSession>;

/// State of the "Join a session" window.
#[derive(Clone, Debug, Default)]
pub struct JoinWindow {
    pub link: String,
    pub name: String,
    /// The link came from the system (an `inkboard://` link was opened): it is shown, not typed.
    pub from_system: bool,
    pub error: Option<String>,
}

/// Everything the application keeps for joining: the window, how to build a session, and the last attempt (to try again).
#[derive(Default)]
pub struct JoinUi {
    pub window: Option<JoinWindow>,
    pub factory: Option<SessionFactory>,
    pub last: Option<(Ticket, String)>,
    /// Folder of the `display_name` file (the data folder when `None`; tests use a throw-away one).
    pub name_dir: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for JoinUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinUi").field("window", &self.window).finish_non_exhaustive()
    }
}

const AMBER: Color32 = Color32::from_rgb(0xb2, 0x6a, 0x00);
const RED: Color32 = Color32::from_rgb(0xc9, 0x2a, 0x2a);

fn end_message(r: &EndReason) -> String {
    match r {
        EndReason::HostLeft => t("The host has ended the session."),
        EndReason::Kicked => t("You were removed from the session."),
        EndReason::Left => t("You left the session."),
        EndReason::ConnectionLost => t("The connection was lost for too long."),
        EndReason::Protocol(d) => tr!("The session stopped because of an error: {}", d),
        EndReason::Refused(RefuseReason::Version { .. }) => t("The host uses another version of Inkboard: update Inkboard (or reload the page)."),
        EndReason::Refused(RefuseReason::BadProof) => t("This invitation link is not valid any more: ask for a new one."),
        EndReason::Refused(RefuseReason::Full) => t("The session is full."),
        EndReason::Refused(RefuseReason::Declined) => t("The host declined your request."),
        EndReason::Refused(RefuseReason::Banned) => t("You were removed from this session."),
    }
}

impl App {
    /// Opens the "Join a session" window; `link` is a link received from the system (`inkboard://…`) or pasted.
    pub fn open_join_window(&mut self, link: Option<String>) {
        let name = match &self.join.name_dir {
            Some(d) => crate::session_guest::load_name_from(d),
            None => crate::session_guest::load_name(),
        }
        .unwrap_or_else(|| t("Guest"));
        self.join.window = Some(JoinWindow { from_system: link.is_some(), link: link.unwrap_or_default(), name, error: None });
    }

    fn remember_name(&self, name: &str) {
        match &self.join.name_dir {
            Some(d) => crate::session_guest::save_name_to(d, name),
            None => crate::session_guest::save_name(name),
        }
    }

    /// Reads the window's link and name and starts the session. On a problem the window shows why.
    pub fn submit_join(&mut self) {
        let Some(w) = self.join.window.clone() else { return };
        let ticket = match parse_join_link(&w.link) {
            Ok(t) => t,
            Err(e) => {
                if let Some(win) = self.join.window.as_mut() {
                    win.error = Some(e.message());
                }
                return;
            }
        };
        let name = sanitize_name(&w.name);
        self.remember_name(&name);
        if self.join.factory.is_none() {
            if let Some(win) = self.join.window.as_mut() {
                win.error = Some(t("Joining a session is not connected to the network in this build yet."));
            }
            return;
        }
        self.start_joining(ticket, &name);
    }

    fn start_joining(&mut self, ticket: Ticket, name: &str) {
        let Some(make) = self.join.factory.as_ref() else { return };
        let session = make(ticket.clone(), name);
        self.join.last = Some((ticket, name.to_owned()));
        self.start_guest(session);
        if let Some(win) = self.join.window.as_mut() {
            win.error = None;
        }
    }

    /// Closes the window and gives up (or leaves) the session.
    fn close_join(&mut self) {
        self.join.window = None;
        self.leave_guest();
    }
}

fn modal(title: String) -> egui::Window<'static> {
    egui::Window::new(title).fade_in(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).order(egui::Order::Foreground).collapsible(false).resizable(false)
}

fn issue_ui(ui: &mut egui::Ui, issue: &NetIssue, diagnosing: bool) {
    ui.set_max_width(460.0);
    ui.label(RichText::new(issue.headline()).strong().color(ui.visuals().text_color()));
    if diagnosing {
        ui.label(RichText::new(t("Checking the connection…")).weak());
    }
    let tips = issue.tips();
    if !tips.is_empty() {
        ui.add_space(6.0);
        for (i, tip) in tips.iter().enumerate() {
            ui.label(format!("{}. {tip}", i + 1));
        }
    }
}

/// Windows and banners of the guest mode; call it after the dialogs.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let state = app.guest.as_ref().map(|g| g.state());
    let mirror = app.guest_active();
    match (&state, mirror) {
        (Some(GuestState::Joining), false) => joining_window(app, ctx),
        (Some(GuestState::Failed(issue)), _) => failed_window(app, ctx, issue.clone()),
        (Some(GuestState::Ended(r)), false) => ended_window(app, ctx, r.clone()),
        (None, _) => form_window(app, ctx),
        _ => {}
    }
    if mirror && let Some(state) = state {
        banners(app, ctx, &state);
    }
}

fn form_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut w) = app.join.window.take() else { return };
    let (mut join, mut cancel) = (false, false);
    modal(t("Join a session")).show(ctx, |ui| {
        ui.set_min_width(420.0);
        ui.label(t("Invitation link"));
        if w.from_system {
            ui.label(RichText::new(t("Link received")).weak());
        } else {
            let r = ui.add(egui::TextEdit::singleline(&mut w.link).desired_width(f32::INFINITY).hint_text("inkboard://join/…"));
            if r.changed() {
                w.error = None;
            }
        }
        ui.add_space(4.0);
        ui.label(t("Your name"));
        let r = ui.add(egui::TextEdit::singleline(&mut w.name).char_limit(NAME_MAX).desired_width(f32::INFINITY));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            join = true;
        }
        if let Some(e) = &w.error {
            ui.add_space(4.0);
            ui.colored_label(RED, e);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            join |= ui.button(t("Join")).clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if cancel {
        return;
    }
    app.join.window = Some(w);
    if join {
        app.submit_join();
    }
}

fn joining_window(app: &mut App, ctx: &egui::Context) {
    let host = app.guest.as_ref().map(|g| g.host_name().to_owned()).unwrap_or_default();
    let mut cancel = false;
    modal(t("Join a session")).show(ctx, |ui| {
        ui.set_min_width(380.0);
        if !host.is_empty() {
            ui.label(RichText::new(tr!("Session of {}", host)).strong());
        }
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(t("Waiting for the host's approval…"));
        });
        ui.add_space(6.0);
        cancel = ui.button(t("Cancel")).clicked();
    });
    if cancel {
        app.close_join();
    }
}

fn failed_window(app: &mut App, ctx: &egui::Context, issue: NetIssue) {
    let diagnosing = app.guest.as_ref().is_some_and(|g| g.is_diagnosing());
    let (mut copy, mut retry, mut close) = (false, false, false);
    modal(t("Cannot join the session")).show(ctx, |ui| {
        issue_ui(ui, &issue, diagnosing);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            copy = ui.button(t("Copy the diagnostic")).clicked();
            retry = ui.button(t("Try again")).clicked();
            close = ui.button(t("Close")).clicked();
        });
    });
    if copy {
        ctx.copy_text(issue.diagnostic_text());
        app.toast(&t("Diagnostic copied"));
    }
    if retry && let Some((ticket, name)) = app.join.last.clone() {
        app.start_joining(ticket, &name);
    }
    if close {
        app.close_join();
    }
}

fn ended_window(app: &mut App, ctx: &egui::Context, reason: EndReason) {
    let mut close = false;
    modal(t("Cannot join the session")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.label(end_message(&reason));
        ui.add_space(6.0);
        close = ui.button(t("Close")).clicked();
    });
    if close {
        app.close_join();
    }
}

/// White text of a banner (the theme colors the default text of labels and "strong" text differently).
fn white(text: impl Into<String>) -> RichText {
    RichText::new(text).color(Color32::WHITE)
}

/// Button of a banner: white with dark text.
fn bar_button(ui: &mut egui::Ui, text: String) -> bool {
    ui.add(egui::Button::new(RichText::new(text).color(Color32::BLACK)).fill(Color32::WHITE)).clicked()
}

fn bar(ctx: &egui::Context, id: &str, anchor: Align2, offset: egui::Vec2, fill: Color32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Area::new(egui::Id::new(id)).anchor(anchor, offset).order(egui::Order::Foreground).fade_in(false).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(fill).show(ui, |ui| {
            ui.horizontal(add);
        });
    });
}

fn banners(app: &mut App, ctx: &egui::Context, state: &GuestState) {
    let host = app.guest.as_ref().map(|g| g.host_name().to_owned()).unwrap_or_default();
    let view = app.view_only();
    let pending = app.guest.as_ref().map_or(0, |g| g.pending_len());
    let title = if host.is_empty() { t("Shared session") } else { tr!("Session of {}", host) };
    let (mut leave, mut save) = (false, false);
    bar(ctx, "guest-bar", Align2::RIGHT_TOP, vec2(-12.0, 12.0), Color32::from_rgb(0x2b, 0x4c, 0xb8), |ui| {
        ui.label(white(title).strong());
        if view {
            ui.label(white(format!("· {}", t("Read-only"))));
        }
        if !matches!(state, GuestState::Ended(_)) {
            leave = bar_button(ui, t("Leave the session"));
        }
    });
    match state {
        GuestState::Reconnecting => bar(ctx, "guest-reconnect", Align2::CENTER_TOP, vec2(0.0, 72.0), AMBER, |ui| {
            ui.spinner();
            ui.label(white(t("Reconnecting…")).strong());
            let msg = if pending > 0 {
                t("Your changes are kept and will be sent as soon as the connection is back.")
            } else {
                t("The board is shown as it was; it is updated when the connection is back.")
            };
            ui.label(white(msg));
        }),
        GuestState::Ended(r) => {
            let msg = end_message(r);
            bar(ctx, "guest-ended", Align2::CENTER_TOP, vec2(0.0, 72.0), RED, |ui| {
                ui.label(white(msg).strong());
                save = bar_button(ui, t("Save a copy of the board"));
                leave = bar_button(ui, t("Leave"));
            });
        }
        _ => {}
    }
    if save {
        app.save_as();
    }
    if leave {
        app.close_join();
    }
}
