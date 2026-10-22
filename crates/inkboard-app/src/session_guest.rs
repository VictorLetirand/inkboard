//! Guest side of a shared session (lot D): joining by link, the mirror of the host's board, reconnection, network-failure diagnosis.
//!
//! `GuestSession` is independent of the interface and of the real network: it speaks to the host through a [`GuestLink`] (any
//! `inkboard-session` [`Transport`]), with the optimistic `Client` of `inkboard-session` doing the protocol. The `impl App` at the
//! end connects it to the editor: the displayed board is a mirror of the host's, local modifications are captured from the editor,
//! sent as `WireOp`s and reconciled at every `Applied`; undo and redo are commands sent to the host and never applied locally.
//!
//! Contract with the host: it answers `Ping` with `Pong` (the guest probes a silent connection to show "Reconnecting…" within
//! seconds), and it sends `Welcome` (with its display name) then a `Snapshot` once it accepts the guest.

// The real network is plugged in by the integration lot (`GuestSession::with_defaults`, `join_url`…): until then only tests build a session.
#![allow(dead_code)]

use crate::app::App;
use crate::i18n::{t, tr};
use crate::presence::{self, RemotePerson, RemoteStroke};
use crate::tools::{Drag, Tool};
use eframe::egui::{self, Color32};
use inkboard_core::geom::Pt;
use inkboard_core::history::Op;
use inkboard_core::model::{Asset, Document, Id, Object};
use inkboard_session::assets::AssetAssembler;
use inkboard_session::crypto::{Entropy, FrameCipher, OsEntropy, SecureChannel, Side, Ticket, join_proof};
use inkboard_session::proto::{AssetInfo, ByeReason, ParticipantInfo, Presence, RefuseReason, RejectReason};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::wire::ops_to_wire;
use inkboard_session::{Client, ClientConfig, ClientEvent, Error, JsonCodec, Message, ParticipantId, Role};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Public relay of the default configuration (used for the diagnosis when the link carries no relay hint).
pub const DEFAULT_RELAY_URL: &str = "https://euc1-1.relay.n0.iroh.link./";
/// Longest display name, in characters.
pub const NAME_MAX: usize = 40;
/// Longest invitation link accepted (a real ticket is under 3 000 characters): anything longer is refused without decoding.
const MAX_LINK_CHARS: usize = 4096;
/// Largest frame a guest accepts from the host, and largest image it assembles.
const MAX_FRAME: usize = 16 << 20;
const MAX_IMAGE: u64 = 15 << 20;
/// Images asked for at the same time.
const MAX_IMAGES_IN_FLIGHT: usize = 4;
/// A transient state of somebody else's gesture is dropped (the board shows the host's state again) after this long without news.
const OVERLAY_IDLE: Duration = Duration::from_secs(3);

/* ------------------------------------------------------------------ display name */

/// Cleans a display name: control and invisible direction characters removed, runs of spaces collapsed, at most [`NAME_MAX`]
/// characters; "Guest" when nothing is left.
pub fn sanitize_name(raw: &str) -> String {
    let invisible = |c: char| matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}');
    let mut out = String::new();
    let mut space = false;
    for c in raw.chars().filter(|c| !c.is_control() && !invisible(*c)) {
        if c.is_whitespace() {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    let cut: String = out.chars().take(NAME_MAX).collect();
    let cut = cut.trim_end().to_owned();
    if cut.is_empty() { t("Guest") } else { cut }
}

fn name_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("display_name")
}

/// Last display name used, read from the file `display_name` of `dir` (cleaned again: the file can be edited by hand).
pub fn load_name_from(dir: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(name_path(dir)).ok()?;
    (!raw.trim().is_empty()).then(|| sanitize_name(&raw))
}

pub fn save_name_to(dir: &std::path::Path, name: &str) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(name_path(dir), sanitize_name(name));
}

pub fn load_name() -> Option<String> {
    load_name_from(&inkboard_mcp::live::data_dir())
}

pub fn save_name(name: &str) {
    save_name_to(&inkboard_mcp::live::data_dir(), name);
}

/* ------------------------------------------------------------------ invitation link */

/// Why a pasted or received link cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinLinkError {
    Empty,
    TooLong,
    /// Not an Inkboard invitation at all.
    NotInkboard,
    /// Made by a newer version of Inkboard.
    Unsupported,
    /// Truncated or altered (the checksum does not match).
    Damaged,
}

impl JoinLinkError {
    pub fn message(&self) -> String {
        match self {
            JoinLinkError::Empty => t("Paste an invitation link first."),
            JoinLinkError::TooLong => t("This text is too long to be an invitation link."),
            JoinLinkError::NotInkboard => t("This is not an Inkboard invitation link."),
            JoinLinkError::Unsupported => t("This link was made by a newer version of Inkboard: please update Inkboard."),
            JoinLinkError::Damaged => t("The link is damaged or incomplete: ask for a new one."),
        }
    }
}

/// Reads `inkboard://join/<ticket>` (also `inkboard://join#<ticket>`, a web link `https://…#<ticket>` and the bare ticket). Never
/// panics, whatever the text.
pub fn parse_join_link(raw: &str) -> Result<Ticket, JoinLinkError> {
    let text = raw.trim().trim_matches(|c| matches!(c, '"' | '\'' | '<' | '>')).trim();
    if text.is_empty() {
        return Err(JoinLinkError::Empty);
    }
    if text.chars().count() > MAX_LINK_CHARS {
        return Err(JoinLinkError::TooLong);
    }
    let scheme = "inkboard://join";
    let ticket = if text.len() >= scheme.len() && text.is_char_boundary(scheme.len()) && text[..scheme.len()].eq_ignore_ascii_case(scheme) {
        // the operating system may add a trailing slash or a query
        let rest = text[scheme.len()..].split('?').next().unwrap_or("");
        rest.trim_start_matches(['/', '#']).trim_end_matches('/').to_owned()
    } else if let Some((_, frag)) = text.split_once('#') {
        frag.to_owned()
    } else {
        text.to_owned()
    };
    Ticket::decode(&ticket).map_err(|e| match e {
        Error::BadTicket(why) if why.contains("supported by this build") => JoinLinkError::Unsupported,
        Error::BadTicket(why) if why.contains("not an Inkboard link") || why.contains("bad encoding") => JoinLinkError::NotInkboard,
        _ => JoinLinkError::Damaged,
    })
}

/// The link the host gives to invite somebody natively.
pub fn join_url(ticket: &Ticket) -> String {
    format!("inkboard://join/{}", ticket.encode())
}

/* ------------------------------------------------------------------ network failure diagnosis */

/// What was found out about the connection service (the relay), by the lot that owns the real network (same shape as `inkboard-net`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnosis {
    pub reachable: bool,
    pub http_status: Option<u16>,
    pub server_header: Option<String>,
    /// Security gateway recognized from the answer (e.g. "Zscaler").
    pub gateway: Option<String>,
    pub summary: String,
}

/// Looks at the connection service when joining fails. It may take seconds (a network request): the session runs it on a thread.
pub trait RelayDiagnoser: Send + Sync {
    fn diagnose(&self, relay_url: &str) -> Diagnosis;
}

/// Diagnoser that finds out nothing (until the real one is plugged in): the failure is then reported generically.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDiagnosis;

impl RelayDiagnoser for NoDiagnosis {
    fn diagnose(&self, _relay_url: &str) -> Diagnosis {
        Diagnosis { summary: "no diagnosis available".into(), ..Diagnosis::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectErrorKind {
    /// The computer has no network at all.
    NoNetwork,
    TimedOut,
    Other,
}

/// A failed attempt to reach the host, as reported by the transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectError {
    pub kind: ConnectErrorKind,
    pub detail: String,
}

impl ConnectError {
    pub fn new(kind: ConnectErrorKind, detail: &str) -> Self {
        Self { kind, detail: detail.to_owned() }
    }
}

/// What a guest needs from the network: open a connection to the host named in the ticket. Any `Transport` is one.
pub trait GuestLink {
    fn connect(&mut self, host: &str) -> Result<Box<dyn Connection>, ConnectError>;

    /// For links that connect in the background (the application must not freeze while a network handshake runs): `connect` then
    /// returns at once with a connection that is not open yet, and the reason why it never opened is taken here.
    fn take_failure(&mut self) -> Option<ConnectError> {
        None
    }
}

impl<T: Transport> GuestLink for T
where
    T::Conn: 'static,
{
    fn connect(&mut self, host: &str) -> Result<Box<dyn Connection>, ConnectError> {
        Transport::connect(self, host).map(|c| Box::new(c) as Box<dyn Connection>).map_err(|e| ConnectError::new(ConnectErrorKind::Other, &e.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetClass {
    /// No network on this computer.
    Offline,
    /// The service answers like a firewall or gateway would (status 403, 407…), or a gateway was recognized.
    Blocked { gateway: Option<String>, status: Option<u16> },
    /// The service answers normally: the host cannot be reached (closed session, host offline).
    HostUnreachable,
    /// Nothing answers, and nothing more is known: a firewall is the usual cause.
    Unreachable,
}

/// A failure to reach the host, classified, with what is needed to explain it and to copy a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetIssue {
    pub class: NetClass,
    /// Exact domain name of the connection service, to give to the IT department.
    pub domain: String,
    pub relay_url: String,
    /// What the transport said.
    pub detail: String,
    pub diagnosis: Option<Diagnosis>,
}

/// Domain name of a relay address (`https://euc1-1.relay.n0.iroh.link./` gives `euc1-1.relay.n0.iroh.link`).
pub fn relay_domain(url: &str) -> String {
    let s = url.trim();
    let s = s.split_once("://").map_or(s, |(_, rest)| rest);
    let host = s.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = if host.starts_with('[') { host } else { host.split(':').next().unwrap_or(host) };
    host.trim_end_matches('.').to_owned()
}

/// Decides what to tell the user. `diagnosis` is what was found out about the connection service, if anything.
pub fn classify_failure(kind: ConnectErrorKind, detail: &str, relay_url: &str, diagnosis: Option<&Diagnosis>) -> NetIssue {
    let class = if kind == ConnectErrorKind::NoNetwork {
        NetClass::Offline
    } else if let Some(d) = diagnosis {
        // 401/403/407/451/511: the answers of proxies and gateways that stop or put a page in front of a connection
        if d.gateway.is_some() || matches!(d.http_status, Some(401 | 403 | 407 | 451 | 511)) {
            NetClass::Blocked { gateway: d.gateway.clone(), status: d.http_status }
        } else if d.reachable && d.http_status.is_none_or(|s| s < 400) {
            NetClass::HostUnreachable
        } else {
            NetClass::Unreachable
        }
    } else {
        NetClass::Unreachable
    };
    NetIssue { class, domain: relay_domain(relay_url), relay_url: relay_url.to_owned(), detail: detail.to_owned(), diagnosis: diagnosis.cloned() }
}

impl NetIssue {
    /// The sentence that says what happened.
    pub fn headline(&self) -> String {
        match &self.class {
            NetClass::Offline => t("No connection: this computer does not seem to be connected to the Internet. Check your network and try again."),
            NetClass::Blocked { gateway: Some(g), .. } => {
                tr!("Cannot reach the connection service. A firewall or a security gateway ({}) is blocking it.", g)
            }
            NetClass::HostUnreachable => {
                t("The connection service answers, but the host cannot be reached. The host may have closed the session or be offline.")
            }
            NetClass::Blocked { gateway: None, .. } | NetClass::Unreachable => {
                t("Cannot reach the connection service. A firewall or a security gateway (often at work or at school) is probably blocking it.")
            }
        }
    }

    /// What the user can try (empty when nothing in the network is to blame).
    pub fn tips(&self) -> Vec<String> {
        match self.class {
            NetClass::Offline | NetClass::HostUnreachable => Vec::new(),
            _ => vec![
                t("Try another network, for example your phone's connection sharing."),
                tr!("Open the address of the service in your browser (https://{}) and accept the warning if there is one, then try again.", self.domain),
                tr!("Ask your IT department to allow this domain: {}", self.domain),
            ],
        }
    }

    /// Plain text to paste in a message to the IT department or to support (English on purpose).
    pub fn diagnostic_text(&self) -> String {
        let d = self.diagnosis.clone().unwrap_or_default();
        let yes_no = |b: bool| if b { "yes" } else { "no" };
        let mut s = format!("Inkboard {}\nConnection service: {} ({})\n", env!("CARGO_PKG_VERSION"), self.domain, self.relay_url);
        s += &format!("Reachable: {}\n", if self.diagnosis.is_some() { yes_no(d.reachable) } else { "unknown" });
        s += &format!("HTTP status: {}\n", d.http_status.map_or("none".to_owned(), |c| c.to_string()));
        s += &format!("Server header: {}\n", d.server_header.as_deref().unwrap_or("none"));
        s += &format!("Security gateway: {}\n", d.gateway.as_deref().unwrap_or("not detected"));
        s += &format!("Diagnosis: {}\n", if d.summary.is_empty() { "none" } else { &d.summary });
        s += &format!("Error: {}\n", if self.detail.is_empty() { "none" } else { &self.detail });
        s
    }
}

/* ------------------------------------------------------------------ clock */

/// Milliseconds since the session started. Virtual in tests (driven by the simulated network), real otherwise.
#[derive(Clone, Debug)]
pub enum Clock {
    Real(Instant),
    Virtual(Arc<AtomicU64>),
}

impl Clock {
    pub fn real() -> Self {
        Clock::Real(Instant::now())
    }
    pub fn now_ms(&self) -> u64 {
        match self {
            Clock::Real(t0) => t0.elapsed().as_millis() as u64,
            Clock::Virtual(v) => v.load(Ordering::Relaxed),
        }
    }
}

/* ------------------------------------------------------------------ session */

#[derive(Clone, Debug)]
pub struct GuestConfig {
    /// Silence after which "Reconnecting…" is shown while the connection is kept (the host is probed meanwhile).
    pub silence_ms: u64,
    /// A silent connection is probed with a `Ping` this often.
    pub probe_ms: u64,
    /// Silence after which the connection is considered dead and a new one is opened.
    pub stale_ms: u64,
    /// Pause between two attempts to reconnect (it grows to `retry_max_ms`).
    pub retry_ms: u64,
    pub retry_max_ms: u64,
    /// A connection that is not open after this long is a failure.
    pub connect_timeout_ms: u64,
    /// Reconnection is abandoned after this long (the host is probably gone).
    pub give_up_ms: u64,
    /// Connection service to diagnose when the link carries no relay hint.
    pub relay_url: String,
}

impl Default for GuestConfig {
    fn default() -> Self {
        Self {
            silence_ms: 2_500,
            probe_ms: 1_000,
            stale_ms: 20_000,
            retry_ms: 1_000,
            retry_max_ms: 4_000,
            connect_timeout_ms: 20_000,
            give_up_ms: 15 * 60_000,
            relay_url: DEFAULT_RELAY_URL.to_owned(),
        }
    }
}

/// Why a session ended for this guest. The mirror stays on screen: a copy can still be saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EndReason {
    HostLeft,
    Kicked,
    Refused(RefuseReason),
    /// This guest left.
    Left,
    ConnectionLost,
    Protocol(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuestState {
    /// Connecting, or waiting for the host to accept.
    Joining,
    Live,
    /// The connection dropped or went silent: modifications are kept and sent when it is back.
    Reconnecting,
    /// The host could not be reached (before the first join).
    Failed(NetIssue),
    Ended(EndReason),
}

/// Something to tell the user once.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Rejected(RejectReason),
    /// Objects were changed by somebody else at the same moment (the guest's version won).
    Superseded(usize),
    /// Modifications made while disconnected could not be kept (the host had forgotten this guest).
    PendingDiscarded(usize),
    RoleChanged(Role),
}

/// What happened since the last call of [`GuestSession::take_changes`].
#[derive(Debug, Default)]
pub struct Changes {
    /// The mirror must be refreshed from [`GuestSession::doc`].
    pub doc: bool,
    pub notices: Vec<Notice>,
    /// Presence, transient states, strokes in progress and agent events of the others.
    pub ephemeral: Vec<Message>,
    /// Authors of the modifications applied (their strokes in progress are over).
    pub authors: Vec<ParticipantId>,
}

enum Stage {
    Joining,
    Live,
    Reconnecting,
    Failed(NetIssue),
    Ended(EndReason),
}

struct PendingDiagnosis {
    rx: Receiver<Diagnosis>,
    kind: ConnectErrorKind,
    detail: String,
    relay_url: String,
}

pub struct GuestSession {
    link: Box<dyn GuestLink>,
    diagnoser: Arc<dyn RelayDiagnoser>,
    ticket: Ticket,
    cfg: GuestConfig,
    clock: Clock,
    chan: SecureChannel,
    client: Client,
    conn: Option<Box<dyn Connection>>,
    conn_opened: bool,
    stage: Stage,
    host_name: String,
    roster: Vec<ParticipantInfo>,
    welcomed: bool,
    mirror_ready: bool,
    silent: bool,
    last_rx: u64,
    last_probe: u64,
    connecting_since: u64,
    next_attempt: u64,
    attempts: u32,
    lost_since: Option<u64>,
    changes: Changes,
    manifest: Vec<AssetInfo>,
    manifest_seq: u64,
    images: HashMap<String, Asset>,
    assemblers: HashMap<String, AssetAssembler>,
    in_flight: HashSet<String>,
    edits: u32,
    undone: u32,
    last_presence: Option<(Presence, u64)>,
    nonce: u64,
    diagnosis: Option<PendingDiagnosis>,
    pub dropped_frames: u64,
}

impl std::fmt::Debug for GuestSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuestSession").field("host_name", &self.host_name).field("state", &self.state()).finish_non_exhaustive()
    }
}

impl GuestSession {
    /// `name` is cleaned here. `entropy`: `OsEntropy` in the real application, a seeded generator in tests.
    pub fn new(
        link: Box<dyn GuestLink>,
        diagnoser: Arc<dyn RelayDiagnoser>,
        ticket: Ticket,
        name: &str,
        entropy: Box<dyn Entropy>,
        clock: Clock,
        cfg: GuestConfig,
    ) -> Self {
        let name = sanitize_name(name);
        let mut ccfg = ClientConfig::new(&name, ticket.role, join_proof(&ticket.key, &name, ticket.role));
        // a reply is waited for a little longer than on a local network
        ccfg.resend_after_ms = 2_000;
        let chan = SecureChannel::new(FrameCipher::new(&ticket.key, Side::Guest), Box::new(JsonCodec), entropy, MAX_FRAME);
        Self {
            link,
            diagnoser,
            ticket,
            cfg,
            chan,
            client: Client::new(ccfg),
            clock,
            conn: None,
            conn_opened: false,
            stage: Stage::Joining,
            host_name: String::new(),
            roster: Vec::new(),
            welcomed: false,
            mirror_ready: false,
            silent: false,
            last_rx: 0,
            last_probe: 0,
            connecting_since: 0,
            next_attempt: 0,
            attempts: 0,
            lost_since: None,
            changes: Changes::default(),
            manifest: Vec::new(),
            manifest_seq: 0,
            images: HashMap::new(),
            assemblers: HashMap::new(),
            in_flight: HashSet::new(),
            edits: 0,
            undone: 0,
            last_presence: None,
            nonce: 0,
            diagnosis: None,
            dropped_frames: 0,
        }
    }

    /// Session of the real application: operating system randomness and real time.
    pub fn with_defaults(link: Box<dyn GuestLink>, diagnoser: Arc<dyn RelayDiagnoser>, ticket: Ticket, name: &str) -> Self {
        Self::new(link, diagnoser, ticket, name, Box::new(OsEntropy), Clock::real(), GuestConfig::default())
    }

    pub fn state(&self) -> GuestState {
        match &self.stage {
            Stage::Joining => GuestState::Joining,
            Stage::Live if self.silent => GuestState::Reconnecting,
            Stage::Live => GuestState::Live,
            Stage::Reconnecting => GuestState::Reconnecting,
            Stage::Failed(i) => GuestState::Failed(i.clone()),
            Stage::Ended(r) => GuestState::Ended(r.clone()),
        }
    }
    /// The host's view of the board plus this guest's unconfirmed modifications.
    pub fn doc(&self) -> &Document {
        self.client.doc()
    }
    pub fn role(&self) -> Role {
        self.client.role()
    }
    pub fn participant(&self) -> Option<ParticipantId> {
        self.client.participant()
    }
    /// Display name of the host, once it has said (empty before).
    pub fn host_name(&self) -> &str {
        &self.host_name
    }
    pub fn roster(&self) -> &[ParticipantInfo] {
        &self.roster
    }
    /// Is the board of the host available (the snapshot was received)?
    pub fn mirror_ready(&self) -> bool {
        self.mirror_ready
    }
    /// Modifications waiting for the host's confirmation.
    pub fn pending_len(&self) -> usize {
        self.client.pending_len()
    }
    pub fn ticket(&self) -> &Ticket {
        &self.ticket
    }
    pub fn is_diagnosing(&self) -> bool {
        self.diagnosis.is_some()
    }
    pub fn clock_ms(&self) -> u64 {
        self.clock.now_ms()
    }
    pub fn can_undo(&self) -> bool {
        self.role() == Role::Edit && self.state() == GuestState::Live && self.edits > self.undone
    }
    pub fn can_redo(&self) -> bool {
        self.role() == Role::Edit && self.state() == GuestState::Live && self.undone > 0
    }

    /// Asks the host to undo this guest's last modification. Nothing changes locally: the answer comes as an ordinary `Applied`.
    pub fn request_undo(&mut self) -> bool {
        let ok = self.can_undo() && self.client.request_undo(false);
        if ok {
            self.undone += 1;
        }
        ok
    }
    pub fn request_redo(&mut self) -> bool {
        let ok = self.can_redo() && self.client.request_undo(true);
        if ok {
            self.undone -= 1;
        }
        ok
    }

    /// Sends the modifications the editor has just made. `false` when they cannot be sent (read-only, no board yet, session over):
    /// the caller then refreshes its mirror, which drops them.
    pub fn submit_local(&mut self, ops: &[Op]) -> bool {
        if ops.is_empty() {
            return true;
        }
        let wire = ops_to_wire(ops, self.client.doc());
        if wire.is_empty() {
            return true;
        }
        let now = self.clock.now_ms();
        let ok = self.client.edit(wire, now).is_some();
        if ok {
            self.edits += 1;
            self.undone = 0;
        }
        ok
    }

    /// Own presence (cursor, selection, tool), at most 20 times a second and only when it changed (or every 2 s to stay visible).
    pub fn send_presence(&mut self, p: Presence) {
        let now = self.clock.now_ms();
        if let Some((last, at)) = &self.last_presence
            && (now.saturating_sub(*at) < 50 || (*last == p && now.saturating_sub(*at) < 2_000))
        {
            return;
        }
        self.last_presence = Some((p.clone(), now));
        self.client.send_ephemeral(Message::Presence(p));
    }
    /// State of an object being moved, resized or edited by a gesture still in progress.
    pub fn send_transient(&mut self, object: Id, state: Object) {
        self.client.send_ephemeral(Message::Transient { object, state: Box::new(state), author: None });
    }
    /// New points of a stroke being drawn.
    pub fn send_stroke(&mut self, stroke: Id, points: Vec<Pt>) {
        self.client.send_ephemeral(Message::StrokePoints { stroke, points, author: None });
    }

    pub fn take_changes(&mut self) -> Changes {
        std::mem::take(&mut self.changes)
    }

    /// Tests: the network drops under the session (the connection closes without the session having decided anything).
    #[cfg(test)]
    pub fn cut_connection(&mut self) {
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
    }

    /// The user leaves: the host is told, the connection closed. The mirror remains available.
    pub fn leave(&mut self) {
        if matches!(self.stage, Stage::Ended(_)) {
            return;
        }
        self.client.send_ephemeral(Message::Bye { reason: ByeReason::Left });
        self.flush();
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.conn = None;
        self.stage = Stage::Ended(EndReason::Left);
    }

    /// Image bytes received so far are put back into `doc` (the mirror is rebuilt from the client's board, which has none).
    pub fn attach_images(&self, doc: &mut Document) {
        for info in &self.manifest {
            if let Some(a) = self.images.get(&info.hash) {
                doc.assets.insert(info.id.clone(), a.clone());
            }
        }
    }

    /// Everything that depends on time and on the network. Call it on every frame of the application.
    pub fn poll(&mut self) {
        let now = self.clock.now_ms();
        self.poll_diagnosis();
        if matches!(self.stage, Stage::Failed(_) | Stage::Ended(_)) {
            return;
        }
        if self.conn.is_none() && now >= self.next_attempt {
            self.try_connect(now);
        }
        self.receive(now);
        if self.conn.is_some() {
            self.watch(now);
        }
        if matches!(self.stage, Stage::Failed(_) | Stage::Ended(_)) {
            return;
        }
        self.client.tick(now);
        self.flush();
    }

    /// The relay of the link to diagnose (the hints also hold direct addresses, which are not a connection service).
    fn relay_url(&self) -> String {
        self.ticket.relays.iter().find(|h| h.starts_with("http://") || h.starts_with("https://")).cloned().unwrap_or_else(|| self.cfg.relay_url.clone())
    }

    fn try_connect(&mut self, now: u64) {
        match self.link.connect(&self.ticket.host) {
            Ok(conn) => {
                self.conn = Some(conn);
                self.conn_opened = false;
                self.connecting_since = now;
                self.last_rx = now;
                self.last_probe = now;
                self.client.connect(now);
            }
            Err(e) => self.connect_failed(e.kind, &e.detail, now),
        }
    }

    /// A connection attempt failed: before the first welcome it is reported with a diagnosis, afterwards it is retried.
    fn connect_failed(&mut self, kind: ConnectErrorKind, detail: &str, now: u64) {
        if self.welcomed {
            self.attempts += 1;
            let pause = (self.cfg.retry_ms * u64::from(self.attempts)).min(self.cfg.retry_max_ms);
            self.next_attempt = now + pause;
            if now.saturating_sub(self.lost_since.unwrap_or(now)) >= self.cfg.give_up_ms {
                self.stage = Stage::Ended(EndReason::ConnectionLost);
            }
            return;
        }
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.conn = None;
        let url = self.relay_url();
        self.stage = Stage::Failed(classify_failure(kind, detail, &url, None));
        if kind != ConnectErrorKind::NoNetwork {
            let (tx, rx) = channel();
            let (d, u) = (Arc::clone(&self.diagnoser), url.clone());
            std::thread::spawn(move || {
                let _ = tx.send(d.diagnose(&u));
            });
            self.diagnosis = Some(PendingDiagnosis { rx, kind, detail: detail.to_owned(), relay_url: url });
        }
    }

    fn poll_diagnosis(&mut self) {
        let Some(p) = self.diagnosis.as_ref() else { return };
        let found = match p.rx.try_recv() {
            Ok(d) => Some(d),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => None,
        };
        if let Some(p) = self.diagnosis.take()
            && matches!(self.stage, Stage::Failed(_))
        {
            self.stage = Stage::Failed(classify_failure(p.kind, &p.detail, &p.relay_url, found.as_ref()));
        }
    }

    fn receive(&mut self, now: u64) {
        let mut frames = Vec::new();
        if let Some(conn) = self.conn.as_mut() {
            while let Some(f) = conn.recv() {
                frames.push(f);
            }
        }
        for f in frames {
            match self.chan.open(&f) {
                Ok(msg) => {
                    self.last_rx = now;
                    self.silent = false;
                    self.on_message(msg, now);
                }
                Err(_) => self.dropped_frames += 1,
            }
            if matches!(self.stage, Stage::Failed(_) | Stage::Ended(_)) {
                return;
            }
        }
    }

    /// Connection health: lost, silent, or never opened.
    fn watch(&mut self, now: u64) {
        let open = self.conn.as_ref().is_some_and(|c| c.is_open());
        self.conn_opened |= open;
        if self.conn_opened && !open {
            return self.connection_lost(now, ConnectErrorKind::Other, "the connection was closed");
        }
        if !self.conn_opened {
            if let Some(e) = self.link.take_failure() {
                // same as an attempt that fails at once: reported before the first welcome, retried with a pause afterwards
                self.conn = None;
                return self.connect_failed(e.kind, &e.detail, now);
            }
            if now.saturating_sub(self.connecting_since) >= self.cfg.connect_timeout_ms {
                self.connection_lost(now, ConnectErrorKind::TimedOut, "the connection could not be established in time");
            }
            return;
        }
        if !self.welcomed {
            return; // waiting for the host's approval can take as long as the host needs
        }
        let idle = now.saturating_sub(self.last_rx);
        if idle >= self.cfg.probe_ms && now.saturating_sub(self.last_probe) >= self.cfg.probe_ms {
            self.last_probe = now;
            self.nonce += 1;
            let nonce = self.nonce;
            // `send_ephemeral` only sends when live; a probe is needed precisely when it may not be, so it goes straight out
            if let (Some(c), Ok(f)) = (self.conn.as_mut(), self.chan.seal(&Message::Ping { nonce })) {
                let _ = c.send(&f);
            }
        }
        if idle >= self.cfg.stale_ms {
            self.connection_lost(now, ConnectErrorKind::TimedOut, "no news from the host");
        } else if idle >= self.cfg.silence_ms {
            self.silent = true;
        }
    }

    fn connection_lost(&mut self, now: u64, kind: ConnectErrorKind, detail: &str) {
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.conn = None;
        self.silent = false;
        self.client.connection_lost();
        if self.welcomed {
            self.stage = Stage::Reconnecting;
            self.lost_since.get_or_insert(now);
            self.next_attempt = now;
            self.attempts = 0;
        } else {
            self.connect_failed(kind, detail, now);
        }
    }

    fn flush(&mut self) {
        let msgs = self.client.take_outbox();
        let mut broke = false;
        for m in msgs {
            if let (Some(c), Ok(f)) = (self.conn.as_mut(), self.chan.seal(&m)) {
                broke |= c.send(&f).is_err();
            }
        }
        if broke {
            let now = self.clock.now_ms();
            self.connection_lost(now, ConnectErrorKind::Other, "sending failed");
        }
    }

    fn on_message(&mut self, msg: Message, now: u64) {
        match &msg {
            Message::Welcome(w) => {
                self.host_name = w.host_name.clone();
                if !w.participants.is_empty() {
                    self.roster = w.participants.clone();
                }
                self.welcomed = true;
            }
            Message::Snapshot(s) if s.seq >= self.manifest_seq => {
                self.manifest = s.assets.clone();
                self.manifest_seq = s.seq;
                self.changes.doc = true;
            }
            Message::Applied(a) => {
                self.changes.doc = true;
                self.changes.authors.push(a.author);
            }
            Message::Rejected { .. } => self.changes.doc = true,
            Message::Participants { list } => self.roster = list.clone(),
            _ => {}
        }
        if let Err(e) = self.client.handle(msg, now) {
            let reason = match e {
                Error::IncompatibleProtocol { .. } => EndReason::Refused(RefuseReason::Version { host_proto: 0 }),
                other => EndReason::Protocol(other.to_string()),
            };
            return self.end(reason);
        }
        for ev in self.client.take_events() {
            self.on_event(ev);
        }
        self.request_images();
    }

    fn on_event(&mut self, ev: ClientEvent) {
        match ev {
            ClientEvent::Joined { .. } => {
                self.lost_since = None;
                self.attempts = 0;
                if matches!(self.stage, Stage::Reconnecting) {
                    self.stage = Stage::Live;
                }
            }
            ClientEvent::SnapshotLoaded => {
                self.mirror_ready = true;
                self.changes.doc = true;
                if matches!(self.stage, Stage::Joining) {
                    self.stage = Stage::Live;
                }
            }
            ClientEvent::Refused(r) => self.end(EndReason::Refused(r)),
            ClientEvent::Closed(r) => self.end(match r {
                ByeReason::HostLeft => EndReason::HostLeft,
                ByeReason::Kicked => EndReason::Kicked,
                ByeReason::Left => EndReason::Left,
                ByeReason::ProtocolError { detail } => EndReason::Protocol(detail),
            }),
            ClientEvent::Rejected { reason, .. } => self.changes.notices.push(Notice::Rejected(reason)),
            ClientEvent::Superseded(ids) => self.changes.notices.push(Notice::Superseded(ids.len())),
            ClientEvent::PendingDiscarded(n) => self.changes.notices.push(Notice::PendingDiscarded(n)),
            ClientEvent::RoleChanged(r) => self.changes.notices.push(Notice::RoleChanged(r)),
            ClientEvent::Relay(m) => match *m {
                Message::AssetChunk(c) => self.on_chunk(&c),
                m @ (Message::Presence(_) | Message::Transient { .. } | Message::StrokePoints { .. } | Message::Agent(_)) => self.changes.ephemeral.push(m),
                _ => {}
            },
        }
    }

    fn end(&mut self, reason: EndReason) {
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.conn = None;
        self.silent = false;
        self.stage = Stage::Ended(reason);
    }

    /* ---- images, by hash and on demand */

    fn request_images(&mut self) {
        if !self.mirror_ready {
            return;
        }
        let missing: Vec<AssetInfo> = self
            .manifest
            .iter()
            .filter(|a| !self.images.contains_key(&a.hash) && !self.in_flight.contains(&a.hash))
            .take(MAX_IMAGES_IN_FLIGHT)
            .cloned()
            .collect();
        for info in missing {
            if self.in_flight.len() >= MAX_IMAGES_IN_FLIGHT {
                break;
            }
            let Ok(asm) = AssetAssembler::new(&info.hash, info.size, MAX_IMAGE) else { continue };
            self.assemblers.insert(info.hash.clone(), asm);
            self.in_flight.insert(info.hash.clone());
            self.client.send_ephemeral(Message::AssetRequest { hash: info.hash });
        }
    }

    fn on_chunk(&mut self, c: &inkboard_session::proto::AssetChunk) {
        let Some(asm) = self.assemblers.get_mut(&c.hash) else { return };
        match asm.push(c) {
            Ok(None) => {}
            Ok(Some(data)) => {
                self.assemblers.remove(&c.hash);
                self.in_flight.remove(&c.hash);
                let mime = self.manifest.iter().find(|a| a.hash == c.hash).map(|a| a.mime.clone()).unwrap_or_default();
                self.images.insert(c.hash.clone(), Asset { mime, data: Arc::new(data) });
                self.changes.doc = true;
                self.request_images();
            }
            Err(_) => {
                // damaged or out of order: dropped, asked for again with the next snapshot
                self.assemblers.remove(&c.hash);
                self.in_flight.remove(&c.hash);
            }
        }
    }
}

/* ------------------------------------------------------------------ the application side */

/// The board the user had before joining, kept to be given back when leaving.
pub struct LocalBoard {
    ed: inkboard_core::Editor,
    path: Option<std::path::PathBuf>,
    saved_rev: u64,
    sel: Vec<Id>,
    disk_mtime: Option<std::time::SystemTime>,
    system_image: fn() -> Option<crate::app::RawImage>,
}

impl std::fmt::Debug for LocalBoard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalBoard").field("path", &self.path).finish_non_exhaustive()
    }
}

/// Per-application state of the guest mode that is not the session itself.
#[derive(Debug, Default)]
pub struct MirrorState {
    pub stash: Option<LocalBoard>,
    /// The mirror must be refreshed as soon as no gesture is in progress.
    pub refresh: bool,
    overlays: HashMap<Id, Instant>,
    last_gesture_ms: u64,
    last_sent: Vec<Object>,
    stroke: Option<(Id, usize)>,
}

fn color32(c: (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

fn tool_name(tool: Tool) -> String {
    format!("{tool:?}").to_lowercase()
}

impl App {
    /// Is the board on screen the mirror of a host's board?
    pub fn guest_active(&self) -> bool {
        self.guest.is_some() && self.mirror.stash.is_some()
    }

    /// Read-only guest: editing tools are disabled (and the editor refuses every modification).
    pub fn view_only(&self) -> bool {
        self.guest_active() && self.guest.as_ref().is_some_and(|g| g.role() == Role::View)
    }

    pub fn can_undo(&self) -> bool {
        if self.guest_active() { self.guest.as_ref().is_some_and(|g| g.can_undo()) } else { self.ed.can_undo() }
    }

    pub fn can_redo(&self) -> bool {
        if self.guest_active() { self.guest.as_ref().is_some_and(|g| g.can_redo()) } else { self.ed.can_redo() }
    }

    /// Undo: local for the host's own board, a command to the host for a guest (the mirror is never undone locally).
    pub fn undo(&mut self) {
        match self.guest.as_mut().filter(|_| self.mirror.stash.is_some()) {
            Some(g) => {
                g.request_undo();
            }
            None => {
                self.ed.undo();
            }
        }
    }

    pub fn redo(&mut self) {
        match self.guest.as_mut().filter(|_| self.mirror.stash.is_some()) {
            Some(g) => {
                g.request_redo();
            }
            None => {
                self.ed.redo();
            }
        }
    }

    /// Starts a session as a guest. The board on screen is replaced by the host's once it accepts.
    pub fn start_guest(&mut self, session: GuestSession) {
        self.leave_guest();
        self.guest = Some(Box::new(session));
    }

    /// Leaves the session (if any) and gives back the board the user had before.
    pub fn leave_guest(&mut self) {
        if let Some(mut g) = self.guest.take() {
            g.leave();
        }
        self.agent.remote.clear();
        self.mirror.refresh = false;
        self.mirror.overlays.clear();
        if let Some(l) = self.mirror.stash.take() {
            self.commit_editing();
            self.ed = l.ed;
            self.path = l.path;
            self.saved_rev = l.saved_rev;
            self.sel = l.sel;
            self.disk_mtime = l.disk_mtime;
            self.system_image = l.system_image;
            self.drag = None;
            self.draft = None;
            self.textures.clear();
            self.caches = Default::default();
        }
    }

    fn enter_mirror(&mut self, doc: Document) {
        self.commit_editing();
        let mirror = inkboard_core::Editor::new(doc);
        let local = std::mem::replace(&mut self.ed, mirror);
        self.mirror.stash = Some(LocalBoard {
            ed: local,
            path: self.path.take(),
            saved_rev: self.saved_rev,
            sel: std::mem::take(&mut self.sel),
            disk_mtime: self.disk_mtime.take(),
            system_image: self.system_image,
        });
        // images cannot be uploaded to the host yet: pasting a system image does nothing
        self.system_image = || None;
        self.ed.record_ops(true);
        self.drag = None;
        self.draft = None;
        self.textures.clear();
        self.caches = Default::default();
        self.fit_pending = true;
        self.saved_rev = self.ed.revision();
        self.dialog = None;
    }

    /// Refreshes the mirror from the session's board. Each page keeps the view (position and zoom) of this user.
    fn refresh_mirror(&mut self, g: &GuestSession) {
        let mut doc = g.doc().clone();
        for p in &mut doc.pages {
            if let Some(old) = self.ed.doc.pages.iter().find(|o| o.id == p.id) {
                p.view = old.view;
            }
        }
        g.attach_images(&mut doc);
        if doc.assets.len() != self.ed.doc.assets.len() {
            self.textures.clear();
        }
        self.ed.replace_document(doc);
        self.mirror.refresh = false;
        self.mirror.overlays.clear();
        let page = self.ed.page().clone();
        self.sel.retain(|id| page.get(id).is_some());
    }

    /// Call on every frame before the interface: network, mirror, capture of the local modifications, presence of the others.
    pub fn guest_frame(&mut self, ctx: &egui::Context) {
        let Some(mut g) = self.guest.take() else { return };
        g.poll();
        let changes = g.take_changes();
        for n in &changes.notices {
            self.notice(n);
        }
        if g.mirror_ready() && self.mirror.stash.is_none() {
            let mut doc = g.doc().clone();
            g.attach_images(&mut doc);
            self.enter_mirror(doc);
        }
        if self.mirror.stash.is_some() {
            let now = std::time::Instant::now();
            let ended = matches!(g.state(), GuestState::Ended(_));
            // modifications made on the previous frame go first: the refresh below replaces the board
            let ops = self.ed.take_recorded_ops();
            if !ops.is_empty() && !g.submit_local(&ops) {
                self.mirror.refresh = true;
            }
            self.mirror.refresh |= changes.doc;
            for m in changes.ephemeral {
                self.apply_ephemeral(&g, m, now);
            }
            for a in &changes.authors {
                self.agent.remote.strokes.retain(|_, s| s.author != a.0);
            }
            if self.mirror.overlays.values().any(|t| t.elapsed() >= OVERLAY_IDLE) {
                self.mirror.refresh = true;
            }
            self.agent.remote.prune(now);
            if self.mirror.refresh && !self.objects_moving() {
                self.refresh_mirror(&g);
            }
            self.ed.set_read_only(g.role() == Role::View || ended);
            if g.role() == Role::View && !matches!(self.tool, Tool::Hand | Tool::Laser) {
                self.tool = Tool::Hand;
            }
            if g.role() == Role::View && !matches!(self.drag, None | Some(Drag::Pan { .. })) {
                self.drag = None;
            }
            self.editing_guard(&g);
            if !ended {
                self.send_local_state(&mut g, ctx);
            }
            self.agent.remote.page = Some(self.ed.current.clone());
            self.saved_rev = self.ed.revision(); // the mirror is not the user's file: never "unsaved"
        }
        self.guest = Some(g);
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    /// A text being typed in a mirror that can no longer be modified is dropped.
    fn editing_guard(&mut self, g: &GuestSession) {
        if (g.role() == Role::View || matches!(g.state(), GuestState::Ended(_))) && self.editing.is_some() {
            self.editing = None;
        }
    }

    fn notice(&mut self, n: &Notice) {
        let msg = match n {
            Notice::Rejected(RejectReason::ReadOnly) => t("You have read-only access: this change was refused."),
            Notice::Rejected(RejectReason::RateLimited) => t("You are sending changes too fast: this change was refused."),
            Notice::Rejected(RejectReason::Quota) => t("Upload quota exceeded: this change was refused."),
            Notice::Rejected(_) => t("The host refused this change."),
            Notice::Superseded(_) => t("Somebody changed the same objects at the same time: your version was kept."),
            Notice::PendingDiscarded(_) => t("Changes made while you were disconnected could not be kept."),
            Notice::RoleChanged(Role::Edit) => t("The host lets you edit now."),
            Notice::RoleChanged(Role::View) => t("The host changed your access to read-only."),
        };
        self.toast(&msg);
    }

    fn person_name(g: &GuestSession, id: ParticipantId) -> (String, Color32) {
        match g.roster().iter().find(|p| p.id == id) {
            Some(p) => (p.name.clone(), color32(p.color)),
            None if id == ParticipantId::HOST && !g.host_name().is_empty() => (g.host_name().to_owned(), Color32::from_rgb(0x5b, 0x7c, 0xfa)),
            None => (tr!("Participant {}", id.0), Color32::from_rgb(0x80, 0x80, 0x80)),
        }
    }

    fn apply_ephemeral(&mut self, g: &GuestSession, msg: Message, now: Instant) {
        let me = g.participant();
        match msg {
            Message::Presence(p) => {
                let Some(id) = p.participant.filter(|id| Some(*id) != me) else { return };
                let (name, color) = Self::person_name(g, id);
                let page = g.doc().page(&p.page);
                let frames = page.map(|pg| p.selection.iter().filter_map(|i| pg.get(i).map(|o| pg.bounds(o))).collect::<Vec<_>>()).unwrap_or_default();
                let person = self.agent.remote.people.entry(id.0).or_insert_with(|| RemotePerson {
                    page: p.page.clone(),
                    state: presence::PresenceState::new(presence::Participant {
                        id: format!("guest-{}", id.0),
                        name: name.clone(),
                        color,
                        kind: presence::ParticipantKind::Human,
                    }),
                });
                person.page = p.page.clone();
                person.state.participant.name = name;
                person.state.participant.color = color;
                person.state.selection = frames;
                match p.cursor {
                    Some(c) => person.state.glide_to(c, now),
                    None => person.state.last_activity = None,
                }
            }
            Message::Transient { object, state, author } => {
                if author == me || (self.objects_moving() && self.sel.contains(&object)) {
                    return;
                }
                let Some(page) = self.ed.doc.pages.iter_mut().find(|p| p.index_of(&object).is_some()) else { return };
                if let Some(i) = page.index_of(&object) {
                    page.objects[i] = *state;
                    self.mirror.overlays.insert(object, now);
                    self.ed.bump_revision();
                }
            }
            Message::StrokePoints { stroke, points, author } => {
                let author = author.map_or(u32::MAX, |a| a.0);
                if Some(ParticipantId(author)) == me {
                    return;
                }
                let s = self.agent.remote.strokes.entry(stroke).or_insert_with(|| RemoteStroke { author, pts: Vec::new(), last: now });
                if s.pts.len() < 20_000 {
                    s.pts.extend(points);
                }
                s.last = now;
            }
            Message::Agent(ev) => {
                let page = self.ed.page();
                let steps: Vec<presence::Target> =
                    ev.targets.iter().filter_map(|id| page.get(id).map(|o| presence::Target { id: id.clone(), frame: page.bounds(o) })).collect();
                let view = page.view;
                let z = view.zoom.max(1e-6);
                let anchor =
                    ev.at.unwrap_or_else(|| Pt::new((f64::from(self.canvas_size.0) / 2.0 - view.x) / z, (f64::from(self.canvas_size.1) / 2.0 - view.y) / z));
                let look = ev.action == "look" || presence::is_look_tool(&ev.action);
                let action = if look { presence::Action::Look } else { presence::Action::Act };
                let steps = if steps.is_empty() { Vec::new() } else { vec![steps] };
                self.agent.state.participant.name = ev.name.clone();
                self.agent.record(presence::AgentEvent { name: ev.name, tool: ev.action, action, steps, at: now }, anchor, now);
            }
            _ => {}
        }
    }

    /// Own cursor, selection and tool, and the intermediate state of a gesture in progress.
    fn send_local_state(&mut self, g: &mut GuestSession, ctx: &egui::Context) {
        if g.state() != GuestState::Live {
            return;
        }
        let view = self.ed.page().view;
        let z = view.zoom.max(1e-6);
        let rect = self.canvas_rect;
        let cursor = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .map(|p| Pt::new((f64::from(p.x - rect.min.x) - view.x) / z, (f64::from(p.y - rect.min.y) - view.y) / z));
        g.send_presence(Presence {
            participant: None,
            page: self.ed.current.clone(),
            cursor,
            selection: self.sel.clone(),
            tool: Some(tool_name(self.tool)),
            editing: self.editing.as_ref().map(|e| e.id.clone()),
            viewport: None,
        });
        if g.role() == Role::View {
            return;
        }
        let now = g.clock_ms();
        if now.saturating_sub(self.mirror.last_gesture_ms) < 50 {
            return;
        }
        let ids: Vec<Id> = match &self.drag {
            Some(Drag::Move { orig, .. } | Drag::Resize { orig, .. } | Drag::Rotate { orig, .. }) => orig.iter().map(|o| o.id.clone()).collect(),
            Some(Drag::Endpoint { orig, .. } | Drag::Via { orig, .. }) => vec![orig.id.clone()],
            _ => self.prop_edit.as_ref().map(|v| v.iter().map(|o| o.id.clone()).collect()).unwrap_or_default(),
        };
        if ids.is_empty() {
            self.mirror.last_sent.clear();
        } else {
            let page = self.ed.page();
            let current: Vec<Object> = ids.iter().filter_map(|i| page.get(i).cloned()).collect();
            if current != self.mirror.last_sent {
                self.mirror.last_gesture_ms = now;
                for o in &current {
                    g.send_transient(o.id.clone(), o.clone());
                }
                self.mirror.last_sent = current;
            }
        }
        match &self.drag {
            Some(Drag::Draw { pts, .. }) => {
                let (id, sent) = self.mirror.stroke.get_or_insert_with(|| (Id::new(), 0));
                if pts.len() > *sent {
                    g.send_stroke(id.clone(), pts[*sent..].to_vec());
                    *sent = pts.len();
                    self.mirror.last_gesture_ms = now;
                }
            }
            _ => self.mirror.stroke = None,
        }
    }
}
