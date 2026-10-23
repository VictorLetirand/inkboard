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
use crate::platform::{Instant, check_relay_url, is_known_relay};
use crate::presence::{self, RemotePerson};
use crate::session_host::{check_guest_upload, check_image_bounds, image_assets, is_hash, sniff_mime};
use crate::tools::{Drag, Tool};
use eframe::egui::{self, Color32};
use inkboard_core::geom::Pt;
use inkboard_core::history::Op;
use inkboard_core::model::{Asset, AssetId, Document, Id, ImageObj, Kind, Object};
use inkboard_session::assets::{AssetAssembler, hash_hex};
use inkboard_session::crypto::{Entropy, FrameCipher, OsEntropy, SecureChannel, Side, Ticket, join_proof};
use inkboard_session::limits::{Limits, RateLimiter};
use inkboard_session::proto::{AssetChunk, AssetInfo, ByeReason, ParticipantInfo, Presence, RefuseReason, RejectReason, Submit, UploadStatus};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::wire::{WireOp, ops_to_wire};
use inkboard_session::{Client, ClientConfig, ClientEvent, Error, JsonCodec, Message, ParticipantId, Role};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

/// Public relay of the default configuration (used for the diagnosis when the link carries no relay hint).
pub const DEFAULT_RELAY_URL: &str = "https://euc1-1.relay.n0.iroh.link./";
/// Longest display name, in characters.
pub const NAME_MAX: usize = crate::names::NAME_MAX;
/// Longest invitation link accepted (a real ticket is under 3 000 characters): anything longer is refused without decoding.
const MAX_LINK_CHARS: usize = 4096;
/// Largest frame a guest accepts from the host, and largest image it assembles.
const MAX_FRAME: usize = 16 << 20;
pub(crate) const MAX_IMAGE: u64 = 15 << 20;
/// Images asked for at the same time.
const MAX_IMAGES_IN_FLIGHT: usize = 4;
/// Images learnt after the snapshot (announced by the host or used by others' modifications) that a guest keeps.
const MAX_LEARNT: usize = 10_000;
/// Size of the pieces of an image sent to the host (the host accepts up to 256 KiB).
const UPLOAD_CHUNK: usize = 128 << 10;
/// Bytes sent but not yet acknowledged by the host at most (the transfer is paced by the acknowledgements).
const UPLOAD_WINDOW: u64 = 4 * UPLOAD_CHUNK as u64;
/// Without any acknowledgement for this long, the transfer goes on again from what the host holds.
const UPLOAD_RESEND_MS: u64 = 2_000;
/// Pieces sent per second at most (the host's rate limit also serves the modifications).
const UPLOAD_CHUNKS_PER_SEC: u32 = 30;
/// An image whose content does not match its hash is given up after this many tries.
const IMAGE_FAILURES_MAX: u32 = 3;
/// Shown in place of an image that cannot be received or read: a crossed frame.
const UNREADABLE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 48"><rect x="2" y="2" width="60" height="44" rx="4" fill="#e9ecef" stroke="#868e96" stroke-width="2"/><path d="M2 2L62 46M62 2L2 46" stroke="#868e96" stroke-width="2"/></svg>"##;

/// The marker that stands for an image that cannot be read.
pub(crate) fn unreadable_asset() -> Asset {
    Asset { mime: "image/svg+xml".to_owned(), data: Arc::new(UNREADABLE_SVG.as_bytes().to_vec()) }
}

/// Runs `f` on every image object of `ops`.
fn for_each_image(ops: &mut [WireOp], f: &mut impl FnMut(&mut ImageObj)) {
    let mut visit = |o: &mut Object| {
        if let Kind::Image(i) = &mut o.kind {
            f(i);
        }
    };
    for op in ops {
        match op {
            WireOp::Put { object, .. } => visit(object),
            WireOp::AddPage { page, .. } => page.objects.iter_mut().for_each(&mut visit),
            _ => {}
        }
    }
}

/// An image being sent to the host.
struct Upload {
    hash: String,
    data: Arc<Vec<u8>>,
    /// Offset of the next piece to send.
    sent: u64,
    /// Bytes the host says it holds, in order.
    acked: u64,
    last_progress: u64,
}

/// Progress of the images being sent, for a discreet indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadProgress {
    pub percent: u32,
    pub images: usize,
}
/// A transient state of somebody else's gesture is dropped (the board shows the host's state again) after this long without news.
const OVERLAY_IDLE: Duration = Duration::from_secs(3);
/// The detection of the network coming back is looked at this often.
const NET_CHECK_MS: u64 = 1_000;
/// Objects of a gesture sent per tick of 50 ms (at most 20 a second, the others take turns): with the cursor that is 40 ephemeral messages a second, under the host limit of 60.
const MAX_TRANSIENTS_PER_TICK: usize = 1;

/* ------------------------------------------------------------------ display name */

/// Cleans a display name: control and invisible direction characters removed, runs of spaces collapsed, at most [`NAME_MAX`]
/// characters; "Guest" when nothing is left.
pub fn sanitize_name(raw: &str) -> String {
    crate::names::clean_name(raw).unwrap_or_else(|| t("Guest"))
}

/// Most participants and most agent targets a guest takes from the host's messages.
const MAX_ROSTER: usize = 256;
const MAX_AGENT_TARGETS: usize = 100;

/// The list of participants as the host sent it, bounded and with every name cleaned.
fn clean_roster(list: &[ParticipantInfo]) -> Vec<ParticipantInfo> {
    list.iter().take(MAX_ROSTER).map(|p| ParticipantInfo { name: sanitize_name(&p.name), ..p.clone() }).collect()
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

/// The last name used: the file `display_name` of the data folder, or the local storage of the browser.
pub fn load_name() -> Option<String> {
    let raw = crate::platform::load_pref("display_name")?;
    (!raw.trim().is_empty()).then(|| sanitize_name(&raw))
}

pub fn save_name(name: &str) {
    crate::platform::save_pref("display_name", &sanitize_name(name));
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

    /// The computer's network came back or changed (or the user asked to retry): the transport should probe its paths again now
    /// instead of waiting for its own timers. Must not block.
    fn network_changed(&mut self) {}
}

/// Notices that the computer's network came back or changed (a new local address). Polled about once a second by the session.
pub trait NetWatch {
    /// `true` once for each change to a usable network (offline then online again, or another local address).
    fn changed(&mut self) -> bool;
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
    /// The relay comes from the link and is neither one of n0 nor a configured one: it is the sender's, not ours. Nothing is then
    /// suggested about it (no "open this address", no "ask IT to allow this domain").
    pub unknown_relay: bool,
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
    NetIssue {
        class,
        domain: relay_domain(relay_url),
        relay_url: relay_url.to_owned(),
        detail: detail.to_owned(),
        diagnosis: diagnosis.cloned(),
        unknown_relay: false,
    }
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
            // a domain chosen by the author of the link: never advise to open it or to have it allowed
            _ if self.unknown_relay => vec![t("Try another network, for example your phone's connection sharing.")],
            _ => vec![
                t("Try another network, for example your phone's connection sharing."),
                tr!("Open the address of the service in your browser (https://{}) and accept the warning if there is one, then try again.", self.domain),
                tr!("Ask your IT department to allow this domain: {}", self.domain),
            ],
        }
    }

    /// Shown with the failure when the link uses a relay that is not ours (see [`NetIssue::unknown_relay`]).
    pub fn warning(&self) -> Option<String> {
        self.unknown_relay.then(|| tr!("This link uses an unknown relay: {}. Avoid opening it unless you trust the sender.", self.domain))
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
    /// Pause between two attempts to reconnect: `retry_ms` for the first `fast_retries` attempts (close together), then it doubles
    /// up to `retry_max_ms`.
    pub retry_ms: u64,
    pub retry_max_ms: u64,
    pub fast_retries: u32,
    /// A connection that is not open after this long is a failure.
    pub connect_timeout_ms: u64,
    /// While reconnecting, an attempt that is not answered after this long is abandoned (really cut) and the next one starts.
    pub reconnect_attempt_ms: u64,
    /// Reconnection is abandoned after this long (the host is probably gone).
    pub give_up_ms: u64,
    /// Connection service to diagnose when the link carries no relay hint.
    pub relay_url: String,
    /// Relays configured by the user (host names or URLs). With the public relays of n0 they are the *known* relays: a link that
    /// names any other relay must be confirmed before joining, and a failure on it gives no advice about that domain.
    pub trusted_relays: Vec<String>,
}

impl Default for GuestConfig {
    fn default() -> Self {
        Self {
            silence_ms: 2_500,
            probe_ms: 1_000,
            stale_ms: 20_000,
            retry_ms: 1_000,
            retry_max_ms: 8_000,
            fast_retries: 4,
            connect_timeout_ms: 20_000,
            reconnect_attempt_ms: 6_000,
            give_up_ms: 15 * 60_000,
            relay_url: DEFAULT_RELAY_URL.to_owned(),
            trusted_relays: Vec::new(),
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
    /// An image is over the size limit: the modification was cancelled.
    ImageTooLarge {
        size: u64,
        max: u64,
    },
    /// An image cannot be read (or sent): the modification was cancelled, or a received image is replaced by a marker.
    ImageUnreadable,
    /// A vector image uses features the host would refuse in a shared session (filter, mask, external link...): the modification was cancelled.
    ImageNotAllowed,
    /// The host refused an image being uploaded (quota, size, content): the modification was cancelled.
    ImageRefused(RejectReason),
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
    unknown_relay: bool,
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
    /// Connections started since the connection was lost (shown as "attempt N"); back to 0 once the host answered.
    reconnect_try: u32,
    net_watch: Option<Box<dyn NetWatch>>,
    next_net_check: u64,
    lost_since: Option<u64>,
    changes: Changes,
    manifest: Vec<AssetInfo>,
    manifest_seq: u64,
    images: HashMap<String, Asset>,
    assemblers: HashMap<String, AssetAssembler>,
    in_flight: HashSet<String>,
    /// Images the host holds that the snapshot's manifest does not list: ours (uploaded) and those learnt from modifications.
    extra: Vec<AssetInfo>,
    uploads: VecDeque<Upload>,
    /// Hashes the host confirmed it holds.
    stored: HashSet<String>,
    /// Modifications that use an image still being sent wait here (in order) until the host has it.
    held: Vec<Submit>,
    /// `client_seq` of the modifications that use an image not yet stored, with the hashes they wait for.
    image_submits: Vec<(u64, Vec<String>)>,
    uploaded_bytes: u64,
    batch_total: u64,
    batch_done: u64,
    upload_rl: RateLimiter,
    failures: HashMap<String, u32>,
    /// Pieces of images sent so far, repeats included.
    pub chunks_sent: u64,
    edits: u32,
    undone: u32,
    last_presence: Option<(Presence, u64)>,
    nonce: u64,
    diagnosis: Option<PendingDiagnosis>,
    pub dropped_frames: u64,
    /// Tests: every ephemeral message sent, with the time of the session's clock (rate measurements).
    #[cfg(test)]
    pub sent_log: Vec<(u64, &'static str)>,
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
            reconnect_try: 0,
            net_watch: None,
            next_net_check: 0,
            lost_since: None,
            changes: Changes::default(),
            manifest: Vec::new(),
            manifest_seq: 0,
            images: HashMap::new(),
            assemblers: HashMap::new(),
            in_flight: HashSet::new(),
            extra: Vec::new(),
            uploads: VecDeque::new(),
            stored: HashSet::new(),
            held: Vec::new(),
            image_submits: Vec::new(),
            uploaded_bytes: 0,
            batch_total: 0,
            batch_done: 0,
            upload_rl: RateLimiter::new(UPLOAD_CHUNKS_PER_SEC, 8),
            failures: HashMap::new(),
            chunks_sent: 0,
            edits: 0,
            undone: 0,
            last_presence: None,
            nonce: 0,
            diagnosis: None,
            dropped_frames: 0,
            #[cfg(test)]
            sent_log: Vec::new(),
        }
    }

    /// Session of the real application: operating system randomness and real time.
    pub fn with_defaults(link: Box<dyn GuestLink>, diagnoser: Arc<dyn RelayDiagnoser>, ticket: Ticket, name: &str) -> Self {
        Self::new(link, diagnoser, ticket, name, Box::new(OsEntropy), Clock::real(), GuestConfig::default())
    }

    /// Plugs in the detection of the network coming back (the real application: `session_net::LocalAddrWatch`).
    pub fn set_net_watch(&mut self, watch: Box<dyn NetWatch>) {
        self.net_watch = Some(watch);
    }

    /// Connections started since the link was lost (0 while the first one is still being waited for).
    pub fn reconnect_attempt(&self) -> u32 {
        self.reconnect_try
    }

    /// "Retry now" (button, or the network came back): the transport probes its paths again, the attempt in progress is abandoned
    /// and a new one starts at once, with the same participant identifier (the host gives the place back without asking again).
    pub fn retry_now(&mut self) {
        let now = self.clock.now_ms();
        self.link.network_changed();
        match self.stage {
            Stage::Reconnecting => {
                if let Some(c) = self.conn.as_mut() {
                    c.close();
                }
                self.conn = None;
                self.next_attempt = now;
                self.attempts = 0;
            }
            Stage::Live if self.silent => self.connection_lost(now, ConnectErrorKind::Other, "retry requested"),
            _ => {}
        }
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
    ///
    /// `assets` are the image bytes of the user's editor: an image the host does not have yet is sent to it first, piece by piece,
    /// and the modification that uses it follows once the host holds it. An image that cannot be sent (too large, unreadable, over
    /// the quota) cancels the modification: `false` and a notice.
    pub fn submit_local(&mut self, ops: &[Op], assets: &BTreeMap<AssetId, Asset>) -> bool {
        if ops.is_empty() {
            return true;
        }
        let mut wire = ops_to_wire(ops, self.client.doc());
        if wire.is_empty() {
            return true;
        }
        let fresh = match self.plan_images(&mut wire, assets) {
            Ok(f) => f,
            Err(notice) => {
                self.changes.notices.push(notice);
                return false;
            }
        };
        let now = self.clock.now_ms();
        let waiting: Vec<String> = image_assets(&wire).into_iter().map(|a| a.0).filter(|a| is_hash(a)).collect();
        let Some(client_seq) = self.client.edit(wire, now) else { return false };
        self.edits += 1;
        self.undone = 0;
        for (hash, asset) in fresh {
            self.queue_upload(hash, asset, now);
        }
        let waiting: Vec<String> = waiting.into_iter().filter(|h| self.uploads.iter().any(|u| u.hash == *h)).collect();
        if !waiting.is_empty() {
            self.image_submits.push((client_seq, waiting));
        }
        true
    }

    /// Does the host hold the image `id` (listed in its manifest, or learnt since)?
    fn host_knows(&self, id: &AssetId) -> bool {
        self.manifest.iter().chain(&self.extra).any(|a| a.id == *id)
    }

    /// Points the images of `wire` that the host does not have at their hash (the identifier the host accepts for an uploaded image),
    /// or at the identifier it already uses for the same bytes. Returns the images to send; nothing is changed in the session yet.
    fn plan_images(&self, wire: &mut [WireOp], local: &BTreeMap<AssetId, Asset>) -> Result<Vec<(String, Asset)>, Notice> {
        let limits = Limits::default();
        let mut fresh: Vec<(String, Asset)> = Vec::new();
        let mut rename: HashMap<AssetId, AssetId> = HashMap::new();
        for id in image_assets(wire) {
            if self.host_knows(&id) {
                continue;
            }
            let Some(asset) = local.get(&id) else { continue };
            let hash = hash_hex(&asset.data);
            if let Some(known) = self.manifest.iter().chain(&self.extra).find(|a| a.hash == hash) {
                rename.insert(id, known.id.clone());
                continue;
            }
            if fresh.iter().any(|(h, _)| *h == hash) {
                rename.insert(id, Id::from(hash.as_str()));
                continue;
            }
            let size = asset.data.len() as u64;
            if size > MAX_IMAGE {
                return Err(Notice::ImageTooLarge { size, max: MAX_IMAGE });
            }
            let Some(mime) = sniff_mime(&asset.data) else { return Err(Notice::ImageUnreadable) };
            if check_image_bounds(&asset.data, mime).is_err() {
                return Err(Notice::ImageUnreadable);
            }
            if check_guest_upload(&asset.data, mime).is_err() {
                return Err(Notice::ImageNotAllowed);
            }
            let planned: u64 = fresh.iter().map(|(_, a)| a.data.len() as u64).sum();
            if self.uploaded_bytes + planned + size > limits.guest_asset_quota_bytes {
                return Err(Notice::Rejected(RejectReason::Quota));
            }
            rename.insert(id, Id::from(hash.as_str()));
            fresh.push((hash, Asset { mime: mime.to_owned(), data: Arc::clone(&asset.data) }));
        }
        for_each_image(wire, &mut |i| {
            if let Some(n) = rename.get(&i.asset) {
                i.asset = n.clone();
            }
        });
        Ok(fresh)
    }

    fn queue_upload(&mut self, hash: String, asset: Asset, now: u64) {
        let size = asset.data.len() as u64;
        self.uploaded_bytes += size;
        self.batch_total += size;
        self.extra.push(AssetInfo { id: Id::from(hash.as_str()), hash: hash.clone(), size, mime: asset.mime.clone() });
        self.images.insert(hash.clone(), asset.clone());
        self.uploads.push_back(Upload { hash, data: asset.data, sent: 0, acked: 0, last_progress: now });
    }

    /// Is an image being sent to the host?
    pub fn is_uploading(&self) -> bool {
        !self.uploads.is_empty()
    }

    /// Progress of the images being sent (`None` when nothing is).
    pub fn upload_progress(&self) -> Option<UploadProgress> {
        let front = self.uploads.front()?;
        let done = (self.batch_done + front.acked).min(self.batch_total);
        let percent = (done * 100).checked_div(self.batch_total).unwrap_or(0) as u32;
        Some(UploadProgress { percent, images: self.uploads.len() })
    }

    /// Sends the next pieces of the image at the head of the queue (a window at a time, a few per second), or again from what the
    /// host holds when it has not answered for a while.
    fn pump_uploads(&mut self, now: u64) {
        if self.conn.is_none() || !self.client.is_live() {
            return;
        }
        let Some(u) = self.uploads.front_mut() else { return };
        let total = u.data.len() as u64;
        if u.sent > u.acked && now.saturating_sub(u.last_progress) >= UPLOAD_RESEND_MS {
            u.sent = u.acked;
            u.last_progress = now;
        }
        if u.sent == u.acked {
            u.last_progress = now;
        }
        let mut out = Vec::new();
        while u.sent < total && u.sent - u.acked < UPLOAD_WINDOW && self.upload_rl.allow(now) {
            let end = (u.sent + UPLOAD_CHUNK as u64).min(total);
            out.push(AssetChunk { hash: u.hash.clone(), offset: u.sent, data: u.data[u.sent as usize..end as usize].to_vec(), last: end == total });
            u.sent = end;
        }
        self.chunks_sent += out.len() as u64;
        for c in out {
            self.client.send_ephemeral(Message::AssetChunk(c));
        }
    }

    /// The host's answer to a piece: progress, completion (the waiting modifications go out) or refusal (they are cancelled).
    fn on_ack(&mut self, hash: &str, have: u64, status: UploadStatus, now: u64) {
        match status {
            UploadStatus::Receiving => {
                if let Some(u) = self.uploads.iter_mut().find(|u| u.hash == hash) {
                    if have < u.acked {
                        u.sent = have; // the host lost what it held: start again from what it has
                    } else if have > u.acked {
                        u.last_progress = now;
                    }
                    u.acked = have.min(u.data.len() as u64);
                    u.sent = u.sent.max(u.acked);
                }
            }
            UploadStatus::Stored => {
                self.stored.insert(hash.to_owned());
                if let Some(i) = self.uploads.iter().position(|u| u.hash == hash)
                    && let Some(u) = self.uploads.remove(i)
                {
                    self.batch_done += u.data.len() as u64;
                }
                for (_, hashes) in &mut self.image_submits {
                    hashes.retain(|h| h != hash);
                }
                self.image_submits.retain(|(_, h)| !h.is_empty());
                self.upload_finished();
            }
            UploadStatus::Refused { reason } => self.fail_upload(hash, reason, now),
        }
    }

    /// When nothing is left to send, the modifications that waited are sent, in order.
    fn upload_finished(&mut self) {
        if !self.uploads.is_empty() {
            return;
        }
        self.batch_total = 0;
        self.batch_done = 0;
        let held = std::mem::take(&mut self.held);
        // not connected: the client sends its pending modifications again by itself once it is
        if self.conn.is_some() && self.client.is_live() {
            for s in held {
                self.send_now(&Message::Submit(s));
            }
        }
    }

    /// The host will not take this image: the modifications that need it are rolled back and the user is told.
    fn fail_upload(&mut self, hash: &str, reason: RejectReason, now: u64) {
        if let Some(u) = self.uploads.iter().find(|u| u.hash == hash) {
            self.batch_total = self.batch_total.saturating_sub(u.data.len() as u64);
        }
        self.uploads.retain(|u| u.hash != hash);
        self.extra.retain(|i| i.hash != hash);
        self.images.remove(hash);
        self.stored.remove(hash);
        let seqs: Vec<u64> = self.image_submits.iter().filter(|(_, h)| h.iter().any(|x| x == hash)).map(|(s, _)| *s).collect();
        self.image_submits.retain(|(s, _)| !seqs.contains(s));
        self.held.retain(|s| !seqs.contains(&s.client_seq));
        for client_seq in seqs {
            let _ = self.client.handle(Message::Rejected { client_seq, reason: reason.clone() }, now);
        }
        // the notice below says why; the client's own "rejected" event would only repeat it less clearly
        for ev in self.client.take_events() {
            if !matches!(ev, ClientEvent::Rejected { .. }) {
                self.on_event(ev);
            }
        }
        self.changes.doc = true;
        self.changes.notices.push(Notice::ImageRefused(reason));
        self.upload_finished();
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
        #[cfg(test)]
        self.sent_log.push((now, "presence"));
        self.client.send_ephemeral(Message::Presence(p));
    }
    /// State of an object being moved, resized or edited by a gesture still in progress.
    pub fn send_transient(&mut self, object: Id, state: Object) {
        #[cfg(test)]
        self.sent_log.push((self.clock.now_ms(), "transient"));
        self.client.send_ephemeral(Message::Transient { object, state: Box::new(state), author: None });
    }
    /// New points of a stroke being drawn.
    pub fn send_stroke(&mut self, stroke: Id, points: Vec<Pt>) {
        #[cfg(test)]
        self.sent_log.push((self.clock.now_ms(), "stroke"));
        self.client.send_ephemeral(Message::StrokePoints { stroke, points, author: None });
    }

    /// The other participants (the host first) with their colors: who can be followed.
    pub fn others(&self) -> Vec<(u32, String, Color32)> {
        let me = self.participant();
        let mut v: Vec<(u32, String, Color32)> = Vec::new();
        if !self.host_name.is_empty() && me != Some(ParticipantId::HOST) && self.roster.iter().all(|p| p.id != ParticipantId::HOST) {
            v.push((ParticipantId::HOST.0, self.host_name.clone(), Color32::from_rgb(0x5b, 0x7c, 0xfa)));
        }
        v.extend(self.roster.iter().filter(|p| Some(p.id) != me).map(|p| (p.id.0, p.name.clone(), color32(p.color))));
        v
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
        for info in self.manifest.iter().chain(&self.extra) {
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
        self.watch_network(now);
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
        self.pump_uploads(now);
        self.flush();
    }

    /// The relay of the link to diagnose (the hints also hold direct addresses, which are not a connection service), and whether it
    /// is unknown. A relay address of the link that is not acceptable (not https, private address...) is never contacted, not even
    /// to diagnose: the configured one is looked at instead.
    fn relay_target(&self) -> (String, bool) {
        let hint = self.ticket.relays.iter().find(|h| h.starts_with("http://") || h.starts_with("https://"));
        match hint {
            Some(h) if is_known_relay(h, &self.cfg.trusted_relays) => (h.clone(), false),
            Some(h) if check_relay_url(h, false).is_ok() => (h.clone(), true),
            _ => (self.cfg.relay_url.clone(), false),
        }
    }

    /// Looks for the network coming back about once a second; a change after the first welcome is a reason to retry at once.
    fn watch_network(&mut self, now: u64) {
        if now < self.next_net_check {
            return;
        }
        self.next_net_check = now + NET_CHECK_MS;
        if self.net_watch.as_mut().is_some_and(|w| w.changed()) && self.welcomed {
            self.retry_now();
        }
    }

    fn try_connect(&mut self, now: u64) {
        if self.welcomed {
            self.reconnect_try += 1;
        }
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
            self.next_attempt = now + self.retry_pause();
            if now.saturating_sub(self.lost_since.unwrap_or(now)) >= self.cfg.give_up_ms {
                self.stage = Stage::Ended(EndReason::ConnectionLost);
            }
            return;
        }
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.conn = None;
        let (url, unknown_relay) = self.relay_target();
        self.stage = Stage::Failed(NetIssue { unknown_relay, ..classify_failure(kind, detail, &url, None) });
        if kind != ConnectErrorKind::NoNetwork {
            let (tx, rx) = channel();
            let (d, u) = (Arc::clone(&self.diagnoser), url.clone());
            // a browser has no thread: its diagnoser cannot look at the network and answers at once
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::spawn(move || {
                let _ = tx.send(d.diagnose(&u));
            });
            #[cfg(target_arch = "wasm32")]
            let _ = tx.send(d.diagnose(&u));
            self.diagnosis = Some(PendingDiagnosis { rx, kind, detail: detail.to_owned(), relay_url: url, unknown_relay });
        }
    }

    /// Pause after the `attempts`-th failed attempt: close together at first, then spaced (doubling up to `retry_max_ms`).
    fn retry_pause(&self) -> u64 {
        let slow = self.attempts.saturating_sub(self.cfg.fast_retries).min(16);
        (self.cfg.retry_ms << slow).min(self.cfg.retry_max_ms.max(self.cfg.retry_ms))
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
            self.stage = Stage::Failed(NetIssue { unknown_relay: p.unknown_relay, ..classify_failure(p.kind, &p.detail, &p.relay_url, found.as_ref()) });
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
            // a reconnection is given less time than the first connection: the next attempt may find a better path
            let limit = if self.welcomed { self.cfg.reconnect_attempt_ms } else { self.cfg.connect_timeout_ms };
            if now.saturating_sub(self.connecting_since) >= limit {
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
        let stale = if matches!(self.stage, Stage::Reconnecting) { self.cfg.reconnect_attempt_ms } else { self.cfg.stale_ms };
        if idle >= stale {
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
        self.held.clear(); // sent again with the pending modifications once back
        if self.welcomed && matches!(self.stage, Stage::Reconnecting) {
            // an attempt made while reconnecting failed: the next one waits a little (close together first, then spaced)
            self.connect_failed(kind, detail, now);
        } else if self.welcomed {
            self.stage = Stage::Reconnecting;
            self.lost_since.get_or_insert(now);
            self.next_attempt = now;
            self.attempts = 0;
            self.reconnect_try = 0;
        } else {
            self.connect_failed(kind, detail, now);
        }
    }

    fn flush(&mut self) {
        let msgs = self.client.take_outbox();
        let mut broke = false;
        for m in msgs {
            // a modification that uses an image waits (behind the earlier ones) until the host holds the image
            if let Message::Submit(s) = &m
                && !self.uploads.is_empty()
            {
                match self.held.iter_mut().find(|h| h.client_seq == s.client_seq) {
                    Some(h) => *h = s.clone(),
                    None => self.held.push(s.clone()),
                }
                continue;
            }
            broke |= !self.send_now(&m);
        }
        if broke {
            let now = self.clock.now_ms();
            self.connection_lost(now, ConnectErrorKind::Other, "sending failed");
        }
    }

    /// Sends at once; `false` when the connection broke.
    fn send_now(&mut self, m: &Message) -> bool {
        match (self.conn.as_mut(), self.chan.seal(m)) {
            (Some(c), Ok(f)) => c.send(&f).is_ok(),
            _ => true,
        }
    }

    fn on_message(&mut self, msg: Message, now: u64) {
        // the board may come compressed (protocol v3): expanded here, under a ceiling, so that the list of images it carries is read
        let msg = match msg {
            Message::Packed { data } => match inkboard_session::pack::unpack(&data, MAX_FRAME) {
                Ok(inner) => inner,
                Err(e) => return self.end(EndReason::Protocol(e.to_string())),
            },
            other => other,
        };
        match &msg {
            Message::Welcome(w) => {
                // what the host says about people is cleaned like what a person types (the host may be hostile)
                self.host_name = crate::names::clean_name(&w.host_name).unwrap_or_default();
                if !w.participants.is_empty() {
                    self.roster = clean_roster(&w.participants);
                }
                self.welcomed = true;
            }
            Message::Snapshot(s) if s.seq >= self.manifest_seq => {
                self.manifest = s.assets.clone();
                self.manifest_seq = s.seq;
                self.changes.doc = true;
            }
            Message::SnapshotPart(p) if p.seq >= self.manifest_seq && p.head.is_some() => {
                self.manifest = p.head.as_ref().map(|h| h.assets.clone()).unwrap_or_default();
                self.manifest_seq = p.seq;
                self.changes.doc = true;
            }
            Message::Applied(a) => {
                self.changes.doc = true;
                self.changes.authors.push(a.author);
                self.learn_images(&a.ops);
            }
            Message::Rejected { .. } => self.changes.doc = true,
            Message::Participants { list } => self.roster = clean_roster(list),
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
                self.reconnect_try = 0;
                // an image being sent goes on from what the host holds (it keeps it for a guest that comes back)
                for u in &mut self.uploads {
                    u.sent = u.acked;
                    u.last_progress = self.clock.now_ms();
                }
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
            ClientEvent::PendingDiscarded(n) => {
                // the host forgot this guest: what it held of the images is gone with the modifications that needed them
                let gone: Vec<String> = self.uploads.drain(..).map(|u| u.hash).collect();
                self.extra.retain(|i| !gone.contains(&i.hash));
                gone.iter().for_each(|h| {
                    self.images.remove(h);
                });
                self.held.clear();
                self.image_submits.clear();
                self.batch_total = 0;
                self.batch_done = 0;
                self.changes.notices.push(Notice::PendingDiscarded(n));
            }
            ClientEvent::RoleChanged(r) => self.changes.notices.push(Notice::RoleChanged(r)),
            ClientEvent::Relay(m) => match *m {
                Message::AssetChunk(c) => self.on_chunk(&c),
                Message::AssetAck { hash, have, status } => {
                    let now = self.clock.now_ms();
                    self.on_ack(&hash, have, status, now);
                }
                Message::Assets { list } => self.learn_announced(&list),
                Message::Agent(mut ev) => {
                    ev.name = crate::names::clean_name(&ev.name).unwrap_or_else(|| t("Agent"));
                    ev.targets.truncate(MAX_AGENT_TARGETS);
                    self.changes.ephemeral.push(Message::Agent(ev));
                }
                m @ (Message::Presence(_) | Message::Transient { .. } | Message::StrokePoints { .. }) => self.changes.ephemeral.push(m),
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
            .chain(&self.extra)
            .filter(|a| !self.images.contains_key(&a.hash) && !self.in_flight.contains(&a.hash))
            .take(MAX_IMAGES_IN_FLIGHT)
            .cloned()
            .collect();
        for info in missing {
            if self.in_flight.len() >= MAX_IMAGES_IN_FLIGHT {
                break;
            }
            if self.in_flight.contains(&info.hash) {
                continue;
            }
            // a size of 0 is "not known": the image was learnt from a modification, the hash alone vouches for it
            let asm = if info.size == 0 {
                AssetAssembler::with_unknown_size(&info.hash, MAX_IMAGE)
            } else {
                match AssetAssembler::new(&info.hash, info.size, MAX_IMAGE) {
                    Ok(a) => a,
                    Err(_) => {
                        // declared larger than any image we take: shown as unreadable, never asked for
                        self.images.insert(info.hash.clone(), unreadable_asset());
                        self.changes.doc = true;
                        continue;
                    }
                }
            };
            self.assemblers.insert(info.hash.clone(), asm);
            self.in_flight.insert(info.hash.clone());
            self.client.send_ephemeral(Message::AssetRequest { hash: info.hash });
        }
    }

    fn on_chunk(&mut self, c: &AssetChunk) {
        let Some(asm) = self.assemblers.get_mut(&c.hash) else { return };
        match asm.push(c) {
            Ok(None) => {}
            Ok(Some(data)) => {
                self.assemblers.remove(&c.hash);
                self.in_flight.remove(&c.hash);
                self.finish_image(&c.hash, data);
                self.request_images();
            }
            Err(e) => {
                // dropped and asked for again; an image whose content keeps not matching its hash is given up (marker shown)
                self.assemblers.remove(&c.hash);
                self.in_flight.remove(&c.hash);
                if !matches!(e, Error::Unexpected(_)) {
                    let n = self.failures.entry(c.hash.clone()).or_insert(0);
                    *n += 1;
                    if *n >= IMAGE_FAILURES_MAX {
                        self.images.insert(c.hash.clone(), unreadable_asset());
                        self.changes.notices.push(Notice::ImageUnreadable);
                        self.changes.doc = true;
                    }
                }
                self.request_images();
            }
        }
    }

    /// A complete image arrived (its hash was checked): kept if its type and declared dimensions are sane, else the marker is kept.
    fn finish_image(&mut self, hash: &str, data: Vec<u8>) {
        let declared = self.manifest.iter().chain(&self.extra).find(|a| a.hash == hash && !a.mime.is_empty()).map(|a| a.mime.clone());
        let mime = declared.or_else(|| sniff_mime(&data).map(str::to_owned));
        let asset = match mime {
            Some(m) if check_image_bounds(&data, &m).is_ok() => Asset { mime: m, data: Arc::new(data) },
            _ => {
                self.changes.notices.push(Notice::ImageUnreadable);
                unreadable_asset()
            }
        };
        self.images.insert(hash.to_owned(), asset);
        self.changes.doc = true;
    }

    /// Images the host announces after the snapshot (its own, whose identifier is not a hash): fetched by hash like the others. What
    /// the host says is bounded: a real hash, a size we would take, and no more than `MAX_LEARNT` images kept in all.
    fn learn_announced(&mut self, list: &[AssetInfo]) {
        for info in list {
            if self.extra.len() >= MAX_LEARNT || !is_hash(&info.hash) || self.host_knows(&info.id) {
                continue;
            }
            self.extra.push(info.clone());
        }
    }

    /// Images that modifications of others use and that the manifest of the snapshot does not list: when the identifier is a hash
    /// (guests upload under their hash), the image can be asked for by it.
    fn learn_images(&mut self, ops: &[WireOp]) {
        for id in image_assets(ops) {
            if is_hash(id.as_str()) && !self.host_knows(&id) && self.extra.len() < MAX_LEARNT {
                self.extra.push(AssetInfo { hash: id.0.clone(), id, size: 0, mime: String::new() });
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
    /// Where the next round of transient states starts among the objects of a gesture.
    turn: usize,
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
            let now = self.agent.remote.now();
            let ended = matches!(g.state(), GuestState::Ended(_));
            // modifications made on the previous frame go first: the refresh below replaces the board
            let ops = self.ed.take_recorded_ops();
            if !ops.is_empty() && !g.submit_local(&ops, &self.ed.doc.assets) {
                self.mirror.refresh = true;
            }
            self.mirror.refresh |= changes.doc;
            for m in changes.ephemeral {
                self.apply_ephemeral(&g, m, now);
            }
            for a in &changes.authors {
                self.agent.remote.settle(a.0);
            }
            if self.mirror.overlays.values().any(|t| now.saturating_duration_since(*t) >= OVERLAY_IDLE) {
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
            Notice::ImageTooLarge { max, .. } => tr!("Image too large ({} MB maximum): this change was cancelled.", max >> 20),
            Notice::ImageUnreadable => t("Unreadable image"),
            Notice::ImageNotAllowed => {
                t("This vector image uses features that are not allowed in a shared session (filter, mask, link...): this change was cancelled.")
            }
            Notice::ImageRefused(RejectReason::Quota) => t("Upload quota exceeded: this change was refused."),
            Notice::ImageRefused(_) => t("The host refused this image: this change was cancelled."),
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
                let person = self.agent.remote.people.entry(id.0).or_insert_with(|| {
                    RemotePerson::new(
                        p.page.clone(),
                        presence::PresenceState::new(presence::Participant {
                            id: format!("guest-{}", id.0),
                            name: name.clone(),
                            color,
                            kind: presence::ParticipantKind::Human,
                        }),
                    )
                });
                person.editing = p.editing.clone().map(|e| (e, now));
                person.viewport = p.viewport;
                person.page = p.page.clone();
                person.state.participant.name = name;
                person.state.participant.color = color;
                person.state.selection = frames;
                match p.cursor {
                    Some(c) => person.state.glide_to(c, now),
                    None => person.state.last_activity = None,
                }
                self.agent.remote.touch(id.0, now);
            }
            Message::Transient { object, state, author } => {
                // a state replaces the object of the same identifier, never another one
                if state.id != object || author == me || (self.objects_moving() && self.sel.contains(&object)) {
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
                self.agent.remote.add_stroke(stroke, author, points, now);
            }
            Message::Agent(ev) => {
                // targets are looked up in the session's board, which is ahead of the mirror (it is refreshed after the messages)
                let page = g.doc().page(&self.ed.current).unwrap_or_else(|| self.ed.page());
                let steps: Vec<presence::Target> =
                    ev.targets.iter().filter_map(|id| page.get(id).map(|o| presence::Target { id: id.clone(), frame: page.bounds(o) })).collect();
                let view = self.ed.page().view;
                let z = view.zoom.max(1e-6);
                let anchor =
                    ev.at.unwrap_or_else(|| Pt::new((f64::from(self.canvas_size.0) / 2.0 - view.x) / z, (f64::from(self.canvas_size.1) / 2.0 - view.y) / z));
                let look = ev.action == "look" || presence::is_look_tool(&ev.action);
                let action = if look { presence::Action::Look } else { presence::Action::Act };
                let steps = if steps.is_empty() { Vec::new() } else { vec![steps] };
                // S-14: the name comes from the host's side (whatever its agent calls itself): it is always shown as an agent's, so
                // that it cannot pass for a person of the session
                self.agent.state.participant.name = tr!("{} (agent)", ev.name);
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
            viewport: Some(inkboard_core::geom::Rect::new(-view.x / z, -view.y / z, f64::from(self.canvas_size.0) / z, f64::from(self.canvas_size.1) / z)),
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
            // a few objects per tick (the host limits ephemeral messages per second): the others take turns on the next ticks
            let n = current.len();
            let mut budget = MAX_TRANSIENTS_PER_TICK;
            let mut last = None;
            for k in 0..n {
                let i = (self.mirror.turn + k) % n;
                if budget == 0 {
                    break;
                }
                let o = &current[i];
                if self.mirror.last_sent.iter().any(|l| l == o) {
                    continue;
                }
                g.send_transient(o.id.clone(), o.clone());
                self.mirror.last_sent.retain(|l| l.id != o.id);
                self.mirror.last_sent.push(o.clone());
                budget -= 1;
                last = Some(i);
            }
            if let Some(i) = last {
                self.mirror.turn = i + 1;
                self.mirror.last_gesture_ms = now;
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
