//! "Share" interface (lot C): the Share window with the two links and the settings, the participant list, the consent window for
//! newcomers, the permanent indicator in the title bar, and the remote cursors. The session itself is `session_host`; nothing here
//! runs, opens or connects anything until the user clicks "Start sharing".

use crate::app::App;
use crate::i18n::{t, tr};
use crate::presence::{Participant, ParticipantKind, PresenceState, RemoteGhost, RemotePerson};
use crate::session_host::{
    HostAddress, HostConfig, HostEvent, HostTransport, MemberView, PendingView, SessionHost, SessionNet, StartResult, load_display_name, save_display_name,
    world_point,
};
use crate::theme;
use crate::tools::Drag;
use eframe::egui::{self, Align2, Color32, RichText, Sense, vec2};
use inkboard_core::geom::{Pt, Rect};
use inkboard_core::{Id, Object};
use inkboard_session::proto::{ParticipantId, Presence, Role};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

/// Minimum delay between two sends of the host's own cursor.
const PRESENCE_EVERY: Duration = Duration::from_millis(50);
/// An unchanged presence is still repeated this often: it keeps the cursor, the "is typing" badge and the gestures of the host alive.
const PRESENCE_KEEPALIVE: Duration = Duration::from_secs(2);
/// Minimum delay between two sends of the host's own gesture in progress.
const GESTURE_EVERY_MS: u64 = 50;
/// At most this many ghosts (objects being moved by guests) are kept.
const MAX_GHOSTS: usize = 256;

/// What a guest last said about its view (the cursor itself lives in `app.agent.remote`, drawn by `presence.rs`).
struct Remote {
    selection: Vec<Id>,
}

/// What the host's own presence message said last: cursor, selection, page, visible area, object being edited.
type SentPresence = (Option<Pt>, Vec<Id>, Id, Option<Rect>, Option<Id>);

/// What the host last sent of its own gesture in progress (a move, a resize, a stroke), so that the guests see it as it happens.
#[derive(Default)]
struct HostGesture {
    last_ms: u64,
    /// Objects as last sent, to send only what changed.
    last_sent: Vec<Object>,
    /// Where the round-robin over the objects of a group gesture goes on (one object per tick).
    turn: usize,
    /// The stroke being drawn: its identifier and how many points were sent.
    stroke: Option<(Id, usize)>,
}

/// A "Start sharing" running on a thread (binding the endpoint and waiting for the relay take up to seconds): the interface stays
/// usable, and "Cancel" drops it.
pub struct Starting {
    rx: Receiver<StartResult>,
    cancel: Arc<AtomicBool>,
}

/// Everything the interface keeps about sharing. Absent (`host == None`) = the application is offline, as it is by default.
pub struct Share {
    /// Network side (`session_net::host_net`, `iroh`): a configuration only, nothing connects before "Start sharing".
    pub net: Option<Box<dyn SessionNet>>,
    pub host: Option<SessionHost>,
    /// The start in progress, if any ("Starting…" in the Share window).
    pub starting: Option<Starting>,
    /// Folder of the `relays` file (the data folder when `None`; tests use a throw-away one).
    pub data_dir: Option<std::path::PathBuf>,
    /// The "Custom relays" field: loaded from the file when the window first shows it; the error of the last edit.
    relays_text: Option<String>,
    relays_error: Option<String>,
    pub addr: Option<HostAddress>,
    /// The Share window is shown.
    pub open: bool,
    pub error: Option<String>,
    pub host_name: String,
    pub max_participants: usize,
    pub auto_accept_view: bool,
    /// The links are shown in clear (hidden by default: screen sharing, over-the-shoulder readers).
    pub links_shown: bool,
    /// Leave the direct (IP) addresses of this computer out of the links; guests then go through a relay.
    pub relays_only: bool,
    /// Tests: the session's time in ms, moved by hand (the real clock otherwise).
    pub test_clock: Option<Arc<AtomicU64>>,
    renaming: Option<(ParticipantId, String)>,
    copied: Option<Instant>,
    remote: HashMap<ParticipantId, Remote>,
    started: Instant,
    last_presence: Option<(Instant, SentPresence)>,
    gesture: HostGesture,
}

impl Share {
    /// `load`: reads the host's name from the data folder (the real application; tests keep the default).
    pub fn new(load: bool) -> Self {
        let host_name = if load { load_display_name() } else { None }.unwrap_or_else(|| t("Host"));
        Self {
            net: None,
            host: None,
            starting: None,
            data_dir: None,
            relays_text: None,
            relays_error: None,
            addr: None,
            open: false,
            error: None,
            host_name,
            max_participants: HostConfig::default().max_participants,
            auto_accept_view: false,
            links_shown: false,
            relays_only: false,
            test_clock: None,
            renaming: None,
            copied: None,
            remote: HashMap::new(),
            started: Instant::now(),
            last_presence: None,
            gesture: HostGesture::default(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.host.as_ref().is_some_and(SessionHost::is_running)
    }
}

/* ------------------------------------------------------------------------------------------------------- start / stop */

/// The address as given to the links: with "relays only", the direct (IP) addresses of this computer are left out.
pub fn published_address(addr: &HostAddress, relays_only: bool) -> HostAddress {
    let relays = addr.relays.iter().filter(|h| !relays_only || h.parse::<std::net::SocketAddr>().is_err()).cloned().collect();
    HostAddress { host: addr.host.clone(), relays }
}

/// "Start sharing": opens the network side and starts the session on the current board. A real network opens on a thread (the
/// interface keeps running, "Starting…" is shown and the start can be cancelled): `tick` finishes the job; a simulated one blocks.
pub fn start(app: &mut App) {
    app.share.error = None;
    if app.share.starting.is_some() {
        return;
    }
    app.share.links_shown = false;
    let Some(net) = app.share.net.as_mut() else {
        app.share.error = Some(t("Sharing is not available in this build."));
        return;
    };
    if let Some(job) = net.start_job() {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let spawned = std::thread::Builder::new().name("inkboard-share-start".to_owned()).spawn(move || {
            // a send that fails means the user cancelled: the result (a transport) is dropped here, on this thread
            let _ = tx.send(job(&flag));
        });
        match spawned {
            Ok(_) => app.share.starting = Some(Starting { rx, cancel }),
            Err(e) => app.share.error = Some(tr!("Sharing failed: {}", e)),
        }
        return;
    }
    match net.start() {
        Ok((transport, addr)) => begin_host(app, transport, addr),
        Err(e) => app.share.error = Some(tr!("Sharing failed: {}", e)),
    }
}

/// "Cancel" while starting: the thread ends on its own and what it opened is closed.
pub fn cancel_start(app: &mut App) {
    if let Some(s) = app.share.starting.take() {
        s.cancel.store(true, Ordering::SeqCst);
    }
}

/// Finishes a start that ran on a thread (call on every frame).
fn poll_start(app: &mut App, ctx: &egui::Context) {
    let Some(s) = app.share.starting.as_ref() else { return };
    match s.rx.try_recv() {
        Ok(Ok((transport, addr))) => {
            app.share.starting = None;
            begin_host(app, transport, addr);
        }
        Ok(Err(e)) => {
            app.share.starting = None;
            app.share.error = Some(tr!("Sharing failed: {}", e));
        }
        Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
        Err(TryRecvError::Disconnected) => {
            app.share.starting = None;
            app.share.error = Some(tr!("Sharing failed: {}", "the network thread stopped"));
        }
    }
}

/// The network side is open: the session starts on the current board.
fn begin_host(app: &mut App, transport: Box<dyn HostTransport>, addr: HostAddress) {
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

/// "Stop sharing": tells everybody, closes every connection (the sockets are released on a thread: no wait on screen).
pub fn stop(app: &mut App) {
    cancel_start(app);
    if let Some(mut h) = app.share.host.take() {
        h.stop();
        app.toast(&t("Sharing stopped"));
    }
    app.share.addr = None;
    app.share.remote.clear();
    app.agent.remote.clear();
}

/* ------------------------------------------------------------------------------------------------------------ per frame */

/// Call on every frame before drawing: runs the session, forwards the agent and the host's cursor, collects the remote cursors.
pub fn tick(app: &mut App, ctx: &egui::Context) {
    poll_start(app, ctx);
    let Some(host) = app.share.host.as_mut() else { return };
    let now_ms = match &app.share.test_clock {
        Some(c) => c.load(std::sync::atomic::Ordering::Relaxed),
        None => u64::try_from(app.share.started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
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
    send_host_gesture(app, now_ms);
    refresh_remote(app, &members);
    ctx.request_repaint_after(PRESENCE_EVERY);
}

/// The intermediate state of the host's own gesture (like `GuestSession` does for a guest): at most one object and the new points of
/// a stroke every 50 ms. Nothing is written to the board; the validated result goes out as an ordinary modification, which ends it.
fn send_host_gesture(app: &mut App, now: u64) {
    let Share { host, gesture, .. } = &mut app.share;
    let Some(host) = host.as_mut().filter(|h| h.is_running()) else { return };
    if now.saturating_sub(gesture.last_ms) < GESTURE_EVERY_MS {
        return;
    }
    let ids: Vec<Id> = match &app.drag {
        Some(Drag::Move { orig, .. } | Drag::Resize { orig, .. } | Drag::Rotate { orig, .. }) => orig.iter().map(|o| o.id.clone()).collect(),
        Some(Drag::Endpoint { orig, .. } | Drag::Via { orig, .. }) => vec![orig.id.clone()],
        _ => app.prop_edit.as_ref().map(|v| v.iter().map(|o| o.id.clone()).collect()).unwrap_or_default(),
    };
    if ids.is_empty() {
        gesture.last_sent.clear();
    } else {
        let page = app.ed.page();
        let current: Vec<Object> = ids.iter().filter_map(|i| page.get(i).cloned()).collect();
        let n = current.len();
        // one object per tick (a group gesture takes turns), as the guests do: the limits of the receivers are the same
        if let Some(i) = (0..n).map(|k| (gesture.turn + k) % n).find(|i| !gesture.last_sent.iter().any(|l| *l == current[*i])) {
            let o = &current[i];
            host.send_host_transient(o.id.clone(), o.clone());
            gesture.last_sent.retain(|l| l.id != o.id);
            gesture.last_sent.push(o.clone());
            gesture.turn = i + 1;
            gesture.last_ms = now;
        }
    }
    match &app.drag {
        Some(Drag::Draw { pts, .. }) => {
            let (id, sent) = gesture.stroke.get_or_insert_with(|| (Id::new(), 0));
            if pts.len() > *sent {
                host.send_host_stroke(id.clone(), pts[*sent..].to_vec());
                *sent = pts.len();
                gesture.last_ms = now;
            }
        }
        _ => gesture.stroke = None,
    }
}

fn on_event(app: &mut App, ev: HostEvent, members: &[MemberView]) {
    let now = app.agent.remote.now();
    match ev {
        HostEvent::JoinRequest { name, .. } => {
            app.toast(&tr!("{} wants to join", name));
            app.share.open = true;
        }
        HostEvent::Joined { name, .. } => app.toast(&tr!("{} joined", name)),
        HostEvent::Left { id, name } => {
            forget_person(app, id);
            app.toast(&tr!("{} left", name));
        }
        HostEvent::Excluded { id, name, .. } => {
            forget_person(app, id);
            app.toast(&tr!("{} was removed", name));
        }
        HostEvent::Warned { name, reason, .. } => app.toast(&tr!("{} went over a limit: {}", name, reason)),
        HostEvent::UndoSkipped { .. } => {}
        HostEvent::Presence { id, presence } => {
            let Some(m) = members.iter().find(|m| m.id == id) else { return };
            let (r, g, b) = m.color;
            let person = app.agent.remote.people.entry(id.0).or_insert_with(|| {
                RemotePerson::new(
                    presence.page.clone(),
                    PresenceState::new(Participant {
                        id: format!("guest-{}", id.0),
                        name: m.name.clone(),
                        color: Color32::from_rgb(r, g, b),
                        kind: ParticipantKind::Human,
                    }),
                )
            });
            match presence.cursor {
                Some(c) => person.state.glide_to(c, now),
                None => person.state.last_activity = Some(now),
            }
            person.page = presence.page.clone();
            person.viewport = presence.viewport;
            person.editing = presence.editing.clone().map(|e| (e, now));
            app.agent.remote.touch(id.0, now);
            app.share.remote.insert(id, Remote { selection: presence.selection.clone() });
        }
        HostEvent::Transient { id, object, state } => {
            // only objects the board has: a guest cannot make the host draw things out of nothing
            let known = app.ed.doc.pages.iter().any(|p| p.index_of(&object).is_some());
            let ghosts = &mut app.agent.remote.ghosts;
            if known && (ghosts.len() < MAX_GHOSTS || ghosts.contains_key(&object)) {
                ghosts.insert(object, RemoteGhost { author: id.0, object: *state, last: now });
            }
        }
        HostEvent::Stroke { id, stroke, points } => app.agent.remote.add_stroke(stroke, id.0, points, now),
        HostEvent::Settled { id } => app.agent.remote.settle(id.0),
    }
}

/// A participant is gone: its cursor, what it was drawing and the following of it end.
fn forget_person(app: &mut App, id: ParticipantId) {
    app.share.remote.remove(&id);
    app.agent.remote.people.remove(&id.0);
    app.agent.remote.settle(id.0);
    if app.agent.remote.following == Some(id.0) {
        crate::presence::follow(app, None);
    }
}

fn send_own_presence(app: &mut App, ctx: &egui::Context) {
    if app.share.last_presence.as_ref().is_some_and(|(t0, _)| t0.elapsed() < PRESENCE_EVERY) {
        return;
    }
    let editing = app.editing.as_ref().map(|e| e.id.clone());
    let view = app.ed.page().view;
    let cursor = ctx.pointer_latest_pos().filter(|p| app.canvas_rect.contains(*p)).map(|p| world_point(p, app.canvas_rect.min, view));
    let z = view.zoom.max(1e-6);
    let (cw, ch) = (f64::from(app.canvas_size.0), f64::from(app.canvas_size.1));
    let viewport = Some(Rect::new(-view.x / z, -view.y / z, cw / z, ch / z));
    let page = app.ed.current.clone();
    let key: SentPresence = (cursor, app.sel.clone(), page.clone(), viewport, editing.clone());
    if app.share.last_presence.as_ref().is_some_and(|(t0, last)| *last == key && t0.elapsed() < PRESENCE_KEEPALIVE) {
        return;
    }
    app.share.last_presence = Some((Instant::now(), key.clone()));
    if let Some(h) = app.share.host.as_mut() {
        h.send_host_presence(Presence { participant: None, page, cursor, selection: key.1, tool: None, editing, viewport });
    }
}

/// Names, colors and selection outlines of the remote cursors; the canvas draws those on the displayed page.
fn refresh_remote(app: &mut App, members: &[MemberView]) {
    let page = app.ed.page().clone();
    app.agent.remote.page = Some(page.id.clone());
    app.agent.remote.prune(app.agent.remote.now());
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
    CancelStart,
    Stop,
    Relays(String),
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

/// The "Custom relays" field was edited: a valid list (https only) is saved in the `relays` file, otherwise the field says why and
/// nothing is saved.
pub fn set_relays(app: &mut App, text: &str) {
    match crate::session_net::parse_relay_list(text) {
        Ok(urls) => {
            crate::session_net::save_relays_to(&relays_dir(&app.share), &urls);
            app.share.relays_text = Some(urls.join("\n"));
            app.share.relays_error = None;
        }
        Err(e) => app.share.relays_error = Some(e.message()),
    }
}

/// Folder of the `relays` file.
fn relays_dir(share: &Share) -> std::path::PathBuf {
    share.data_dir.clone().unwrap_or_else(inkboard_mcp::live::data_dir)
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
                let kind = if p.key_role == Role::Edit { t("Edit link") } else { t("Read-only link") };
                ui.label(RichText::new(tr!("Link used: {} (guest ID {})", kind, p.short_id)).small());
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
    let published = app.share.addr.as_ref().map(|a| published_address(a, app.share.relays_only));
    // "relays only" with no relay known yet: a link would reach nobody, so none is offered
    let waiting_for_relay = app.share.relays_only && published.as_ref().is_some_and(|a| a.relays.is_empty());
    let links: Option<(String, String)> = match (app.share.host.as_ref(), published.as_ref()) {
        (Some(h), Some(a)) if !waiting_for_relay => h.link(Role::Edit, a).ok().zip(h.link(Role::View, a).ok()),
        _ => None,
    };
    let count = app.share.host.as_ref().map_or(1, SessionHost::participant_count);
    let following = app.agent.remote.following;
    let p = theme::current(ctx);
    let starting = app.share.starting.is_some();
    if app.share.relays_text.is_none() {
        app.share.relays_text = Some(crate::session_net::read_relays_text(&relays_dir(&app.share)));
    }
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
            ui.label(t("Custom relays"));
            let text = share.relays_text.get_or_insert_with(String::new);
            let r = ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(f32::INFINITY).hint_text("https://relay.example.org"));
            if r.lost_focus() {
                acts.push(Act::Relays(text.clone()));
            }
            let hint = if running {
                t("One https:// address per line. Used the next time sharing starts.")
            } else {
                t("One https:// address per line. Empty: the public relays.")
            };
            ui.label(RichText::new(hint).small().color(p.mute));
            if let Some(e) = &share.relays_error {
                ui.colored_label(Color32::from_rgb(0xe0, 0x31, 0x31), e);
            }
            ui.separator();
            if !running {
                if starting {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t("Starting…"));
                        if ui.button(t("Cancel")).clicked() {
                            acts.push(Act::CancelStart);
                        }
                    });
                } else if ui.button(t("Start sharing")).clicked() {
                    acts.push(Act::Start);
                }
                return;
            }
            // links (hidden until asked: the link is the secret)
            ui.checkbox(&mut share.relays_only, t("Relays only (do not publish my direct address)"));
            ui.label(RichText::new(t("A link normally contains the IP addresses of this computer, which anybody holding the link can read. \"Relays only\" leaves them out; connections then go through a relay.")).small().color(p.mute));
            if waiting_for_relay {
                ui.label(RichText::new(t("No relay address is known yet: the links appear as soon as a relay answers.")).small().color(p.mute));
            }
            if let Some((edit, view)) = &links {
                for (label, url) in [(t("Edit link"), edit), (t("Read-only link"), view)] {
                    ui.label(label);
                    ui.horizontal(|ui| {
                        if share.links_shown {
                            let mut shown = url.clone();
                            ui.add(egui::TextEdit::singleline(&mut shown).desired_width(270.0));
                        } else {
                            ui.label(RichText::new("••••••••••••••••••••").color(p.mute));
                        }
                        if ui.button(t("Copy")).clicked() {
                            acts.push(Act::Copy(url.clone()));
                        }
                    });
                }
                if ui.button(if share.links_shown { t("Hide") } else { t("Show") }).clicked() {
                    share.links_shown = !share.links_shown;
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
                participant_row(ui, share, following, m, acts);
            }
            ui.label(RichText::new(t("To cut off a removed person for good, generate new links: they can ask to join again as long as they hold the old link.")).small().color(p.mute));
            ui.separator();
            if ui.button(t("Stop sharing")).clicked() {
                acts.push(Act::Stop);
            }
        });
    if !open {
        app.share.open = false;
    }
}

fn participant_row(ui: &mut egui::Ui, share: &mut Share, following: Option<u32>, m: &MemberView, acts: &mut Vec<Act>) {
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
        let on = following == Some(m.id.0);
        if ui.small_button(if on { t("Stop following") } else { t("Follow") }).clicked() {
            acts.push(Act::Follow((!on).then_some(m.id)));
        }
        if ui.small_button(t("Remove")).on_hover_text(t("To cut someone off for good, generate new links.")).clicked() {
            acts.push(Act::Kick(m.id));
        }
    });
}

fn apply(app: &mut App, ctx: &egui::Context, act: Act) {
    match act {
        Act::Start => start(app),
        Act::CancelStart => cancel_start(app),
        Act::Stop => stop(app),
        Act::Relays(text) => set_relays(app, &text),
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
        Act::Follow(id) => crate::presence::follow(app, id.map(|i| i.0)),
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
