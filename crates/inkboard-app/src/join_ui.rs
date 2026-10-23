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
    /// The link names a relay that is neither one of n0 nor a configured one (its domain): the user must confirm before anything connects.
    pub unknown_relay: Option<String>,
    /// The user confirmed the unknown relay of this link.
    pub relay_confirmed: bool,
    /// Custom relays, one `https://` address per line (the `relays` file; empty: the public relays).
    pub relays: String,
    /// The name field already took the keyboard focus once (browser).
    pub name_focused: bool,
}

/// What the relays named by a link mean for the user (see [`judge_relays`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayVerdict {
    /// Only direct addresses, relays of n0 or configured relays.
    Fine,
    /// A public `https` relay that is not ours (its domain): ask first.
    Unknown(String),
    /// A relay address that is never used (not `https`, private/loopback address, odd port, credentials...; its text, cleaned).
    Refused(String),
}

/// Looks at the relay hints of a link before anything connects. `configured`: the relays the user configured (host names or URLs).
/// The hints that are plain socket addresses are direct addresses of the host, not relays.
pub fn judge_relays(ticket: &Ticket, configured: &[String]) -> RelayVerdict {
    use crate::platform::{check_relay_url, is_known_relay};
    let clean = |s: &str| s.chars().filter(char::is_ascii_graphic).take(80).collect::<String>();
    let mut unknown = None;
    for hint in &ticket.relays {
        if hint.parse::<std::net::SocketAddr>().is_ok() || is_known_relay(hint, configured) {
            continue;
        }
        match check_relay_url(hint, false) {
            Err(_) => return RelayVerdict::Refused(clean(hint)),
            Ok(url) => {
                unknown.get_or_insert_with(|| clean(url.host_str().unwrap_or("").trim_end_matches('.')));
            }
        }
    }
    unknown.map_or(RelayVerdict::Fine, RelayVerdict::Unknown)
}

/// Everything the application keeps for joining: the window, how to build a session, and the last attempt (to try again).
#[derive(Default)]
pub struct JoinUi {
    pub window: Option<JoinWindow>,
    pub factory: Option<SessionFactory>,
    pub last: Option<(Ticket, String)>,
    /// Folder of the `display_name` file (the data folder when `None`; tests use a throw-away one).
    pub name_dir: Option<std::path::PathBuf>,
    /// Relays the user configured (host names or URLs): with the public relays of n0, the only ones a link may name without asking.
    pub trusted_relays: Vec<String>,
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
        EndReason::Refused(RefuseReason::TooManyFromPeer) => t("This device already has as many places in the session as the host allows."),
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
        let relays = crate::session_net::read_relays_text(&self.relays_dir());
        self.join.window = Some(JoinWindow { from_system: link.is_some(), link: link.unwrap_or_default(), name, relays, ..JoinWindow::default() });
    }

    /// Folder of the `relays` file, next to `display_name`.
    fn relays_dir(&self) -> std::path::PathBuf {
        self.join.name_dir.clone().unwrap_or_else(crate::platform::data_dir)
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
        // the custom relays are checked before anything is saved or started: https only, nothing silently ignored
        let relays = match crate::session_net::parse_relay_list(&w.relays) {
            Ok(urls) => urls,
            Err(e) => {
                if let Some(win) = self.join.window.as_mut() {
                    win.error = Some(e.message());
                }
                return;
            }
        };
        // the relays of the user (file or window) are known: they never ask for a confirmation
        let mut configured = self.join.trusted_relays.clone();
        configured.extend(relays.iter().cloned());
        // the relay of a link is chosen by whoever made it: nothing connects to an odd or unknown one without the user's say
        match judge_relays(&ticket, &configured) {
            RelayVerdict::Refused(text) => {
                if let Some(win) = self.join.window.as_mut() {
                    win.error = Some(tr!("This link uses a relay address that is not allowed: {}", text));
                }
                return;
            }
            RelayVerdict::Unknown(domain) if !w.relay_confirmed => {
                if let Some(win) = self.join.window.as_mut() {
                    win.unknown_relay = Some(domain);
                }
                return;
            }
            _ => {}
        }
        let name = sanitize_name(&w.name);
        self.remember_name(&name);
        if self.join.factory.is_none() {
            if let Some(win) = self.join.window.as_mut() {
                win.error = Some(t("Joining a session is not connected to the network in this build yet."));
            }
            return;
        }
        crate::session_net::save_relays_to(&self.relays_dir(), &relays);
        self.start_joining(ticket, &name);
    }

    fn start_joining(&mut self, ticket: Ticket, name: &str) {
        if self.join.factory.is_none() {
            return;
        }
        // a board is either hosted or joined: the host side is not polled while a guest session is open
        crate::share_ui::stop(self);
        let Some(make) = self.join.factory.as_ref() else { return };
        let session = make(ticket.clone(), name);
        self.join.last = Some((ticket, name.to_owned()));
        self.start_guest(session);
        if let Some(win) = self.join.window.as_mut() {
            win.error = None;
        }
    }

    /// A link arrived in the address of the browser page: the "Join" window opens with it, to be completed with a name. A link that is
    /// not valid opens the window with its error and the link as text, so that it can be corrected or replaced.
    pub fn join_link_received(&mut self, link: String) {
        if self.guest.is_some() {
            return self.toast(&t("Leave the current session before joining another one."));
        }
        self.open_join_window(Some(link));
        if let Some(w) = self.join.window.as_mut()
            && let Err(e) = parse_join_link(&w.link)
        {
            w.from_system = false;
            w.error = Some(e.message());
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

/// Width of a window on this screen: `want`, or less on a phone.
fn fit(ctx: &egui::Context, want: f32) -> f32 {
    want.min(ctx.content_rect().width() - 24.0).max(200.0)
}

/// Larger buttons and fields for a finger; the usual sizes for a mouse.
fn touch_sizes(ui: &mut egui::Ui) {
    if crate::platform::coarse_pointer() {
        let s = ui.spacing_mut();
        s.interact_size.y = 40.0;
        s.button_padding = vec2(16.0, 8.0);
        s.item_spacing = vec2(10.0, 10.0);
    }
}

/// Inner margin of a text field (the default of egui; taller for a finger).
fn field_margin() -> egui::Vec2 {
    if crate::platform::coarse_pointer() { vec2(8.0, 11.0) } else { vec2(4.0, 2.0) }
}

/// Tells the browser bench where a widget is (nothing happens in the published page nor on the desktop).
fn mark(_name: &str, _r: &egui::Response) {
    #[cfg(all(target_arch = "wasm32", feature = "web-test"))]
    crate::web_test::mark(_name, _r.rect);
}

fn issue_ui(ui: &mut egui::Ui, issue: &NetIssue, diagnosing: bool) {
    ui.set_max_width(fit(ui.ctx(), 460.0));
    ui.label(RichText::new(issue.headline()).strong().color(ui.visuals().text_color()));
    if let Some(w) = issue.warning() {
        ui.add_space(4.0);
        ui.label(RichText::new(w).strong().color(AMBER));
    }
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

/// "This link uses an unknown relay": Cancel / Continue, before anything connects.
fn unknown_relay_window(app: &mut App, ctx: &egui::Context, mut w: JoinWindow, domain: String) {
    let (mut go, mut cancel) = (false, false);
    modal(t("Unknown relay")).show(ctx, |ui| {
        touch_sizes(ui);
        ui.set_max_width(fit(ctx, 440.0));
        ui.label(RichText::new(tr!("This link uses an unknown relay: {}. Continue only if you trust the sender.", domain)).strong().color(AMBER));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            cancel = ui.button(t("Cancel")).clicked();
            go = ui.button(t("Continue")).clicked();
        });
    });
    if cancel {
        return;
    }
    if go {
        w.unknown_relay = None;
        w.relay_confirmed = true;
    }
    app.join.window = Some(w);
    if go {
        app.submit_join();
    }
}

fn form_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut w) = app.join.window.take() else { return };
    if let Some(domain) = w.unknown_relay.clone() {
        return unknown_relay_window(app, ctx, w, domain);
    }
    let (mut join, mut cancel) = (false, false);
    let width = fit(ctx, 420.0);
    modal(t("Join a session")).show(ctx, |ui| {
        touch_sizes(ui);
        ui.set_min_width(width);
        if width < 420.0 {
            ui.set_max_width(width);
        }
        ui.label(t("Invitation link"));
        if w.from_system {
            ui.label(RichText::new(t("Link received")).weak());
        } else {
            let r = ui.add(egui::TextEdit::singleline(&mut w.link).margin(field_margin()).desired_width(f32::INFINITY).hint_text("inkboard://join/…"));
            mark("join_link", &r);
            if r.changed() {
                w.error = None;
                w.relay_confirmed = false;
            }
        }
        ui.add_space(4.0);
        ui.label(t("Your name"));
        let r = ui.add(egui::TextEdit::singleline(&mut w.name).margin(field_margin()).char_limit(NAME_MAX).desired_width(f32::INFINITY));
        mark("join_name", &r);
        // in a browser the name is what is left to do: the cursor is already there
        if cfg!(target_arch = "wasm32") && !w.name_focused {
            r.request_focus();
            w.name_focused = true;
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            join = true;
        }
        ui.label(RichText::new(t("The other participants see this name.")).weak().small());
        ui.add_space(4.0);
        egui::CollapsingHeader::new(t("Custom relays")).default_open(!w.relays.trim().is_empty()).show(ui, |ui| {
            if ui.add(egui::TextEdit::multiline(&mut w.relays).desired_rows(2).desired_width(f32::INFINITY).hint_text("https://relay.example.org")).changed() {
                w.error = None;
            }
            ui.label(RichText::new(t("One https:// address per line. Empty: the public relays.")).weak().small());
        });
        if let Some(e) = &w.error {
            ui.add_space(4.0);
            ui.colored_label(RED, e);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let go = ui.button(t("Join"));
            mark("join_go", &go);
            join |= go.clicked();
            let no = ui.button(t("Cancel"));
            mark("join_cancel", &no);
            cancel = no.clicked();
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
    let width = fit(ctx, 380.0);
    modal(t("Join a session")).show(ctx, |ui| {
        touch_sizes(ui);
        ui.set_min_width(width);
        if !host.is_empty() {
            ui.label(RichText::new(tr!("Session of {}", host)).strong());
        }
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(t("Waiting for the host's approval…"));
        });
        ui.add_space(6.0);
        let no = ui.button(t("Cancel"));
        mark("wait_cancel", &no);
        cancel = no.clicked();
    });
    if cancel {
        app.close_join();
    }
}

fn failed_window(app: &mut App, ctx: &egui::Context, issue: NetIssue) {
    let diagnosing = app.guest.as_ref().is_some_and(|g| g.is_diagnosing());
    let (mut copy, mut retry, mut close) = (false, false, false);
    modal(t("Cannot join the session")).show(ctx, |ui| {
        touch_sizes(ui);
        issue_ui(ui, &issue, diagnosing);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let (c, r, x) = (ui.button(t("Copy the diagnostic")), ui.button(t("Try again")), ui.button(t("Close")));
            mark("issue_copy", &c);
            mark("issue_retry", &r);
            mark("issue_close", &x);
            (copy, retry, close) = (c.clicked(), r.clicked(), x.clicked());
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
        touch_sizes(ui);
        ui.set_max_width(fit(ctx, 420.0));
        ui.label(end_message(&reason));
        ui.add_space(6.0);
        let x = ui.button(t("Close"));
        mark("ended_close", &x);
        close = x.clicked();
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
fn bar_button(ui: &mut egui::Ui, name: &str, text: String) -> bool {
    let min = if crate::platform::coarse_pointer() { vec2(0.0, 36.0) } else { vec2(0.0, 0.0) };
    let r = ui.add(egui::Button::new(RichText::new(text).color(Color32::BLACK)).fill(Color32::WHITE).min_size(min));
    mark(name, &r);
    r.clicked()
}

fn bar(ctx: &egui::Context, id: &str, anchor: Align2, offset: egui::Vec2, fill: Color32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Area::new(egui::Id::new(id)).anchor(anchor, offset).order(egui::Order::Foreground).fade_in(false).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(fill).show(ui, |ui| {
            // a phone is too narrow for one row: the banner then wraps
            let room = ctx.content_rect().width() - 24.0;
            if room < 560.0 {
                ui.set_max_width(room.max(200.0));
                ui.horizontal_wrapped(add);
            } else {
                ui.horizontal(add);
            }
        });
    });
}

/// Height taken by the session bar once it sits under the tools (phone): what is placed below it moves down by as much.
pub const PHONE_BAR_SHIFT: f32 = 60.0;

fn banners(app: &mut App, ctx: &egui::Context, state: &GuestState) {
    let host = app.guest.as_ref().map(|g| g.host_name().to_owned()).unwrap_or_default();
    let view = app.view_only();
    let pending = app.guest.as_ref().map_or(0, |g| g.pending_len());
    let sending = app.guest.as_ref().and_then(|g| g.upload_progress());
    let title = if host.is_empty() { t("Shared session") } else { tr!("Session of {}", host) };
    let attempt = app.guest.as_ref().map_or(0, |g| g.reconnect_attempt());
    // under the row of tools when it moves below the title (narrow window or phone)
    let top = if ctx.content_rect().width() < 1180.0 { 124.0 } else { 72.0 };
    // on a phone the session bar would cover the title of the board (top left): it goes under the tools, the other bars under it
    let phone = ctx.content_rect().width() < 560.0;
    let (bar_anchor, bar_offset) = if phone { (Align2::CENTER_TOP, vec2(0.0, top - 8.0)) } else { (Align2::RIGHT_TOP, vec2(-12.0, 12.0)) };
    let top = if phone { top + PHONE_BAR_SHIFT } else { top };
    let (mut leave, mut save, mut retry) = (false, false, false);
    bar(ctx, "guest-bar", bar_anchor, bar_offset, Color32::from_rgb(0x2b, 0x4c, 0xb8), |ui| {
        ui.label(white(title).strong());
        if view {
            ui.label(white(format!("· {}", t("Read-only"))));
        }
        if let Some(p) = sending {
            ui.label(white(format!("· {}", tr!("Sending images… {}%", p.percent))));
        }
        if !matches!(state, GuestState::Ended(_)) {
            leave = bar_button(ui, "bar_leave", t("Leave the session"));
        }
    });
    match state {
        GuestState::Reconnecting => bar(ctx, "guest-reconnect", Align2::CENTER_TOP, vec2(0.0, top), AMBER, |ui| {
            ui.spinner();
            let title = if attempt > 0 { tr!("Reconnecting… (attempt {})", attempt) } else { t("Reconnecting…") };
            ui.label(white(title).strong());
            let msg = if pending > 0 {
                t("Your changes are kept and will be sent as soon as the connection is back.")
            } else {
                t("The board is shown as it was; it is updated when the connection is back.")
            };
            ui.label(white(msg));
            retry = bar_button(ui, "bar_retry", t("Retry now"));
        }),
        GuestState::Ended(r) => {
            let msg = end_message(r);
            bar(ctx, "guest-ended", Align2::CENTER_TOP, vec2(0.0, top), RED, |ui| {
                ui.label(white(msg).strong());
                save = bar_button(ui, "bar_save", t("Save a copy of the board"));
                leave = bar_button(ui, "bar_leave", t("Leave"));
            });
        }
        _ => {}
    }
    if save {
        app.save_as();
    }
    if retry && let Some(g) = app.guest.as_mut() {
        g.retry_now();
    }
    if leave {
        app.close_join();
    }
}
