//! "Share" interface (lot C): the Share window with the two links and the settings, the participant list, the consent window for
//! newcomers, the permanent indicator in the title bar, and the remote cursors. The session itself is `session_host`; nothing here
//! runs, opens or connects anything until the user clicks "Start sharing".

use crate::app::App;
use crate::i18n::{t, tr};
use crate::presence::{Participant, ParticipantKind, PresenceState, RemotePerson};
use crate::session_host::{
    HostAddress, HostConfig, HostEvent, MemberView, PendingView, SessionHost, SessionNet, load_display_name, save_display_name, world_point,
};
use crate::theme;
use eframe::egui::{self, Align2, Color32, RichText, Sense, vec2};
use inkboard_core::geom::{Pt, Rect};
use inkboard_core::{Id, View};
use inkboard_session::proto::{ParticipantId, Presence, Role};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Minimum delay between two sends of the host's own cursor.
const PRESENCE_EVERY: Duration = Duration::from_millis(50);

/// What a guest last said about its view (the cursor itself lives in `app.agent.remote`, drawn by `presence.rs`).
struct Remote {
    page: Id,
    selection: Vec<Id>,
    viewport: Option<Rect>,
}

/// What the host's own presence message said last: cursor, selection, page, visible area.
type SentPresence = (Option<Pt>, Vec<Id>, Id, Option<Rect>);

/// Everything the interface keeps about sharing. Absent (`host == None`) = the application is offline, as it is by default.
pub struct Share {
    /// Network side (`session_net::host_net`, `iroh`): a configuration only, nothing connects before "Start sharing".
    pub net: Option<Box<dyn SessionNet>>,
    pub host: Option<SessionHost>,
    pub addr: Option<HostAddress>,
    /// The Share window is shown.
    pub open: bool,
    pub error: Option<String>,
    pub host_name: String,
    pub max_participants: usize,
    pub auto_accept_view: bool,
    pub following: Option<ParticipantId>,
    renaming: Option<(ParticipantId, String)>,
    copied: Option<Instant>,
    remote: HashMap<ParticipantId, Remote>,
    started: Instant,
    last_presence: Option<(Instant, SentPresence)>,
}

impl Share {
    /// `load`: reads the host's name from the data folder (the real application; tests keep the default).
    pub fn new(load: bool) -> Self {
        let host_name = if load { load_display_name() } else { None }.unwrap_or_else(|| t("Host"));
        Self {
            net: None,
            host: None,
            addr: None,
            open: false,
            error: None,
            host_name,
            max_participants: HostConfig::default().max_participants,
            auto_accept_view: false,
            following: None,
            renaming: None,
            copied: None,
            remote: HashMap::new(),
            started: Instant::now(),
            last_presence: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.host.as_ref().is_some_and(SessionHost::is_running)
    }
}

/* ------------------------------------------------------------------------------------------------------- start / stop */

/// "Start sharing": opens the network side and starts the session on the current board.
pub fn start(app: &mut App) {
    app.share.error = None;
    let Some(net) = app.share.net.as_mut() else {
        app.share.error = Some(t("Sharing is not available in this build."));
        return;
    };
    let (transport, addr) = match net.start() {
        Ok(v) => v,
        Err(e) => {
            app.share.error = Some(tr!("Sharing failed: {}", e));
            return;
        }
    };
    let cfg = HostConfig {
        host_name: crate::session_host::sanitize_name(&app.share.host_name, &t("Host")),
        max_participants: app.share.max_participants,
        auto_accept_view: app.share.auto_accept_view,
        ..HostConfig::default()
    };
    match SessionHost::start(cfg, &app.ed, transport) {
        Ok(host) => {
            app.share.host = Some(host);
            app.share.addr = Some(addr);
            app.share.started = Instant::now();
            app.agent.outbox.clear(); // earlier agent actions are not news to the guests
        }
        Err(e) => app.share.error = Some(tr!("Sharing failed: {}", e)),
    }
}

/// "Stop sharing": tells everybody, closes every connection.
pub fn stop(app: &mut App) {
    if let Some(mut h) = app.share.host.take() {
        h.stop();
        app.toast(&t("Sharing stopped"));
    }
    app.share.addr = None;
    app.share.remote.clear();
    app.share.following = None;
    app.agent.remote.clear();
}

/* ------------------------------------------------------------------------------------------------------------ per frame */

/// Call on every frame before drawing: runs the session, forwards the agent and the host's cursor, collects the remote cursors.
pub fn tick(app: &mut App, ctx: &egui::Context) {
    let Some(host) = app.share.host.as_mut() else { return };
    let now_ms = u64::try_from(app.share.started.elapsed().as_millis()).unwrap_or(u64::MAX);
    host.poll(&mut app.ed, now_ms);
    if let Some(addr) = app.share.net.as_mut().and_then(|n| n.refresh()) {
        app.share.addr = Some(addr);
    }
    for ev in app.agent.outbox.drain(..) {
        host.forward_agent(&ev);
    }
    let events = host.take_events();
    let members = host.members();
    for ev in events {
        on_event(app, ev, &members);
    }
    send_own_presence(app, ctx);
    refresh_remote(app, &members);
    ctx.request_repaint_after(PRESENCE_EVERY);
}

fn on_event(app: &mut App, ev: HostEvent, members: &[MemberView]) {
    match ev {
        HostEvent::JoinRequest { name, .. } => {
            app.toast(&tr!("{} wants to join", name));
            app.share.open = true;
        }
        HostEvent::Joined { name, .. } => app.toast(&tr!("{} joined", name)),
        HostEvent::Left { id, name } => {
            app.share.remote.remove(&id);
            app.agent.remote.people.remove(&id.0);
            app.toast(&tr!("{} left", name));
        }
        HostEvent::Excluded { id, name, .. } => {
            app.share.remote.remove(&id);
            app.agent.remote.people.remove(&id.0);
            app.toast(&tr!("{} was removed", name));
        }
        HostEvent::Warned { name, reason, .. } => app.toast(&tr!("{} went over a limit: {}", name, reason)),
        HostEvent::UndoSkipped { .. } => {}
        HostEvent::Presence { id, presence } => {
            let Some(m) = members.iter().find(|m| m.id == id) else { return };
            let now = Instant::now();
            let (r, g, b) = m.color;
            let person = app.agent.remote.people.entry(id.0).or_insert_with(|| RemotePerson {
                page: presence.page.clone(),
                state: PresenceState::new(Participant {
                    id: format!("guest-{}", id.0),
                    name: m.name.clone(),
                    color: Color32::from_rgb(r, g, b),
                    kind: ParticipantKind::Human,
                }),
            });
            match presence.cursor {
                Some(c) => person.state.glide_to(c, now),
                None => person.state.last_activity = Some(now),
            }
            person.page = presence.page.clone();
            app.share.remote.insert(id, Remote { page: presence.page.clone(), selection: presence.selection.clone(), viewport: presence.viewport });
            if app.share.following == Some(id) {
                follow_view(app, id);
            }
        }
    }
}

/// The view follows a participant: same page, same visible area.
fn follow_view(app: &mut App, id: ParticipantId) {
    let Some((page, vp)) = app.share.remote.get(&id).and_then(|r| r.viewport.map(|v| (r.page.clone(), v))) else { return };
    if app.ed.doc.page(&page).is_some() && app.ed.current != page {
        app.commit_editing();
        app.ed.current = page;
        app.sel.clear();
    }
    let (w, h) = (f64::from(app.canvas_size.0.max(100.0)), f64::from(app.canvas_size.1.max(100.0)));
    let zoom = (w / vp.w.max(1.0)).min(h / vp.h.max(1.0)).clamp(0.05, 8.0);
    let c = vp.center();
    app.set_view(View { x: w / 2.0 - c.x * zoom, y: h / 2.0 - c.y * zoom, zoom });
}

fn send_own_presence(app: &mut App, ctx: &egui::Context) {
    if app.share.last_presence.as_ref().is_some_and(|(t0, _)| t0.elapsed() < PRESENCE_EVERY) {
        return;
    }
    let view = app.ed.page().view;
    let cursor = ctx.pointer_latest_pos().filter(|p| app.canvas_rect.contains(*p)).map(|p| world_point(p, app.canvas_rect.min, view));
    let z = view.zoom.max(1e-6);
    let (cw, ch) = (f64::from(app.canvas_size.0), f64::from(app.canvas_size.1));
    let viewport = Some(Rect::new(-view.x / z, -view.y / z, cw / z, ch / z));
    let page = app.ed.current.clone();
    let key: SentPresence = (cursor, app.sel.clone(), page.clone(), viewport);
    if app.share.last_presence.as_ref().is_some_and(|(_, last)| *last == key) {
        return;
    }
    app.share.last_presence = Some((Instant::now(), key.clone()));
    if let Some(h) = app.share.host.as_mut() {
        h.send_host_presence(Presence { participant: None, page, cursor, selection: key.1, tool: None, editing: None, viewport });
    }
}

/// Names, colors and selection outlines of the remote cursors; the canvas draws those on the displayed page.
fn refresh_remote(app: &mut App, members: &[MemberView]) {
    let page = app.ed.page().clone();
    app.agent.remote.page = Some(page.id.clone());
    app.agent.remote.prune(Instant::now());
    for (id, p) in &mut app.agent.remote.people {
        let Some(m) = members.iter().find(|m| m.id.0 == *id) else { continue };
        p.state.participant.name.clone_from(&m.name);
        p.state.participant.color = Color32::from_rgb(m.color.0, m.color.1, m.color.2);
        let sel = app.share.remote.get(&ParticipantId(*id)).map(|r| r.selection.as_slice()).unwrap_or_default();
        p.state.selection = sel.iter().filter_map(|i| page.get(i)).map(|o| page.bounds(o)).collect();
    }
}

/* ----------------------------------------------------------------------------------------------------------- title bar */

/// "Share" button of the title bar and, while a session is open, the permanent indicator (dot and number of participants).
pub fn title_button(app: &mut App, ui: &mut egui::Ui) {
    let running = app.share.is_running();
    let resp = theme::text_button(ui, &t("Share"));
    if resp.on_hover_text(t("Invite people to this board")).clicked() {
        app.share.open = !app.share.open;
    }
    if let (true, Some(h)) = (running, app.share.host.as_ref()) {
        let n = h.participant_count();
        ui.label(RichText::new(format!("● {n}")).color(Color32::from_rgb(0x2f, 0x9e, 0x44)).size(13.0))
            .on_hover_text(tr!("Session active — {} participants", n));
    }
}

/* -------------------------------------------------------------------------------------------------------------- windows */

enum Act {
    Start,
    Stop,
    Accept(u64, Role),
    Decline(u64),
    Kick(ParticipantId),
    SetRole(ParticipantId, Role),
    Rename(ParticipantId, String),
    Follow(Option<ParticipantId>),
    NewLinks,
    HostName(String),
    Max(usize),
    AutoView(bool),
    Copy(String),
}

fn role_label(r: Role) -> String {
    match r {
        Role::Edit => t("Can edit"),
        Role::View => t("Can view"),
    }
}

/// Draws the consent window (when somebody waits) and the Share window (when open). Call after the application frame.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let pending: Vec<PendingView> = app.share.host.as_ref().map(SessionHost::pending).unwrap_or_default();
    if !pending.is_empty() || app.share.open {
        let mut acts: Vec<Act> = Vec::new();
        if !pending.is_empty() {
            consent_window(ctx, &pending, &mut acts);
        }
        if app.share.open {
            share_window(app, ctx, &mut acts);
        }
        for a in acts {
            apply(app, ctx, a);
        }
    }
}

fn consent_window(ctx: &egui::Context, pending: &[PendingView], acts: &mut Vec<Act>) {
    egui::Window::new(t("Join requests"))
        .id(egui::Id::new("share-consent"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 70.0))
        .order(egui::Order::Foreground)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            for p in pending {
                ui.label(RichText::new(tr!("{} wants to join", p.name)).strong());
                ui.horizontal(|ui| {
                    if ui.add_enabled(p.key_role == Role::Edit, egui::Button::new(t("Accept as editor"))).clicked() {
                        acts.push(Act::Accept(p.request, Role::Edit));
                    }
                    if ui.button(t("Accept as viewer")).clicked() {
                        acts.push(Act::Accept(p.request, Role::View));
                    }
                    if ui.button(t("Decline")).clicked() {
                        acts.push(Act::Decline(p.request));
                    }
                });
                ui.add_space(4.0);
            }
        });
}

fn share_window(app: &mut App, ctx: &egui::Context, acts: &mut Vec<Act>) {
    let mut open = true;
    let running = app.share.is_running();
    let members: Vec<MemberView> = app.share.host.as_ref().map(SessionHost::members).unwrap_or_default();
    let links: Option<(String, String)> = match (app.share.host.as_ref(), app.share.addr.as_ref()) {
        (Some(h), Some(a)) => h.link(Role::Edit, a).ok().zip(h.link(Role::View, a).ok()),
        _ => None,
    };
    let count = app.share.host.as_ref().map_or(1, SessionHost::participant_count);
    let p = theme::current(ctx);
    let share = &mut app.share;
    egui::Window::new(t("Share"))
        .id(egui::Id::new("share-window"))
        .anchor(Align2::RIGHT_TOP, vec2(-12.0, 60.0))
        .order(egui::Order::Foreground)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .default_width(380.0)
        .show(ctx, |ui| {
            if share.net.is_none() && !running {
                ui.label(t("Sharing is not available in this build."));
                return;
            }
            // state
            if running {
                ui.label(RichText::new(tr!("Session active — {} participants", count)).color(Color32::from_rgb(0x2f, 0x9e, 0x44)));
            } else {
                ui.label(RichText::new(t("Not shared")).color(p.mute));
                ui.label(RichText::new(t("Everybody who opens a link needs your approval to enter.")).small().color(p.mute));
            }
            if let Some(e) = &share.error {
                ui.colored_label(Color32::from_rgb(0xe0, 0x31, 0x31), e);
            }
            // settings
            ui.horizontal(|ui| {
                ui.label(t("Your name"));
                let r = ui.add(egui::TextEdit::singleline(&mut share.host_name).desired_width(160.0).char_limit(crate::session_host::MAX_NAME_CHARS));
                if r.lost_focus() {
                    acts.push(Act::HostName(share.host_name.clone()));
                }
            });
            ui.horizontal(|ui| {
                ui.label(t("Participants (maximum)"));
                let mut max = share.max_participants;
                if ui.add(egui::DragValue::new(&mut max).range(2..=64)).changed() {
                    acts.push(Act::Max(max));
                }
            });
            let mut auto = share.auto_accept_view;
            if ui.checkbox(&mut auto, t("Let people with the read-only link in without asking")).changed() {
                acts.push(Act::AutoView(auto));
            }
            ui.separator();
            if !running {
                if ui.button(t("Start sharing")).clicked() {
                    acts.push(Act::Start);
                }
                return;
            }
            // links
            if let Some((edit, view)) = &links {
                for (label, url) in [(t("Edit link"), edit), (t("Read-only link"), view)] {
                    ui.label(label);
                    ui.horizontal(|ui| {
                        let mut shown = url.clone();
                        ui.add(egui::TextEdit::singleline(&mut shown).desired_width(270.0));
                        if ui.button(t("Copy")).clicked() {
                            acts.push(Act::Copy(url.clone()));
                        }
                    });
                }
                if share.copied.is_some_and(|c| c.elapsed() < Duration::from_secs(3)) {
                    ui.label(RichText::new(t("Link copied")).small().color(p.mute));
                }
                if ui.button(t("New links (the old ones stop working)")).clicked() {
                    acts.push(Act::NewLinks);
                }
            }
            ui.separator();
            // participants
            ui.label(RichText::new(t("Participants")).strong());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, Color32::from_rgb(0x4f, 0x6d, 0xf5));
                ui.label(format!("{} ({})", share.host.as_ref().map_or("", SessionHost::host_name), t("you")));
            });
            for m in &members {
                participant_row(ui, share, m, acts);
            }
            ui.separator();
            if ui.button(t("Stop sharing")).clicked() {
                acts.push(Act::Stop);
            }
        });
    if !open {
        app.share.open = false;
    }
}

fn participant_row(ui: &mut egui::Ui, share: &mut Share, m: &MemberView, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 5.0, Color32::from_rgb(m.color.0, m.color.1, m.color.2));
        if let Some((id, buf)) = share.renaming.as_mut().filter(|(id, _)| *id == m.id) {
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(110.0).char_limit(crate::session_host::MAX_NAME_CHARS));
            r.request_focus();
            if r.lost_focus() {
                acts.push(Act::Rename(*id, buf.clone()));
                share.renaming = None;
            }
        } else {
            let away = if m.connected { String::new() } else { format!(" {}", t("(away)")) };
            let editing = if m.editing.is_some() { format!(" ✎ {}", t("editing")) } else { String::new() };
            ui.label(format!("{}{away}{editing}", m.name));
            if ui.small_button(t("Rename")).clicked() {
                share.renaming = Some((m.id, m.name.clone()));
            }
        }
        let other = if m.role == Role::Edit { Role::View } else { Role::Edit };
        if ui.small_button(role_label(m.role)).on_hover_text(t("Change the access")).clicked() {
            acts.push(Act::SetRole(m.id, other));
        }
        let following = share.following == Some(m.id);
        if ui.small_button(if following { t("Stop following") } else { t("Follow") }).clicked() {
            acts.push(Act::Follow((!following).then_some(m.id)));
        }
        if ui.small_button(t("Remove")).clicked() {
            acts.push(Act::Kick(m.id));
        }
    });
}

fn apply(app: &mut App, ctx: &egui::Context, act: Act) {
    match act {
        Act::Start => start(app),
        Act::Stop => stop(app),
        Act::Accept(r, role) => {
            if let Some(h) = app.share.host.as_mut() {
                h.accept(r, role);
            }
        }
        Act::Decline(r) => {
            if let Some(h) = app.share.host.as_mut() {
                h.decline(r);
            }
        }
        Act::Kick(id) => {
            if let Some(h) = app.share.host.as_mut() {
                h.kick(id);
            }
        }
        Act::SetRole(id, role) => {
            if let Some(h) = app.share.host.as_mut() {
                h.set_role(id, role);
            }
        }
        Act::Rename(id, name) => {
            if let Some(h) = app.share.host.as_mut() {
                h.rename(id, &name);
            }
        }
        Act::Follow(id) => {
            app.share.following = id;
            if let Some(id) = id {
                follow_view(app, id);
            }
        }
        Act::NewLinks => {
            if let Some(h) = app.share.host.as_mut()
                && let Err(e) = h.regenerate_links()
            {
                app.share.error = Some(tr!("Sharing failed: {}", e));
            }
        }
        Act::HostName(name) => {
            save_display_name(&name);
            if let Some(h) = app.share.host.as_mut() {
                h.set_host_name(&name);
            }
        }
        Act::Max(n) => {
            app.share.max_participants = n;
            if let Some(h) = app.share.host.as_mut() {
                h.config_mut().max_participants = n;
            }
        }
        Act::AutoView(on) => {
            app.share.auto_accept_view = on;
            if let Some(h) = app.share.host.as_mut() {
                h.config_mut().auto_accept_view = on;
            }
        }
        Act::Copy(url) => {
            ctx.copy_text(url);
            app.share.copied = Some(Instant::now());
        }
    }
}
