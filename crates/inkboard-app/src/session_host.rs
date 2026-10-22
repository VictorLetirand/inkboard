//! Host side of a shared session (lot C of `context/PLAN_REMOTE.md`).
//!
//! `SessionHost` sits on top of `inkboard_session::Sequencer` and an injected transport (anything that accepts connections; the real one
//! is `iroh`, wired by the integration lot, the tests use the simulated network). It does the handshake (key, version, role deduced from
//! the key), asks the host's consent for every newcomer, enforces rights, limits, quotas and rates on the host side, relays ephemeral
//! messages and presence, serves the snapshot and the images (by hash, in chunks), resumes dropped guests and undoes per author.
//!
//! It never opens anything by itself: nothing exists until `SessionHost::start`, which the "Share" window calls.
//!
//! The application owns the `Editor`: `poll` borrows it on every frame. Remote modifications go in through `Editor::apply_external`
//! (never in the host's undo stack); the host's own modifications are found by comparing the editor with the authoritative copy kept
//! by the sequencer, and are broadcast like any other (author = host).

use crate::i18n::t;
use inkboard_core::editor::Editor;
use inkboard_core::geom::Pt;
use inkboard_core::history::{Author, AuthorHistories};
use inkboard_core::model::{Asset, AssetId, Document, Id, Kind, Object, Page};
use inkboard_session::assets::{chunks, hash_hex};
use inkboard_session::codec::read_hello;
use inkboard_session::crypto::{Entropy, FrameCipher, HostKeys, Link, OsEntropy, SecureChannel, SeededEntropy, verify_join_proof};
use inkboard_session::limits::{Limits, Quota, RateLimiter, Violation, check_message, check_message_size, check_page_count};
use inkboard_session::proto::{
    AgentEvent as WireAgent, AssetChunk, AssetInfo, ByeReason, DocMeta, Message, PROTO_VERSION, ParticipantId, ParticipantInfo, Presence, RefuseReason,
    RejectReason, Role, Seq, Submit, Welcome,
};
use inkboard_session::sequencer::{CatchUp, Sequencer, Submitted};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::wire::{WireOp, apply_wire_all, ops_to_wire};
use inkboard_session::{Error, JsonCodec};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

/* ------------------------------------------------------------------------------------------------------------------ seams */

/// What the host needs from a transport: incoming connections. Every `Transport` is one (the trait of `inkboard-session` has an
/// associated type and cannot be a trait object).
pub trait HostTransport {
    fn accept(&mut self) -> Option<Box<dyn Connection>>;
}

impl<T: Transport> HostTransport for T
where
    T::Conn: 'static,
{
    fn accept(&mut self) -> Option<Box<dyn Connection>> {
        Transport::accept(self).map(|c| Box::new(c) as Box<dyn Connection>)
    }
}

/// How guests reach the host: the identity put in the links (an `iroh` node id, ...) and optional relay hints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostAddress {
    pub host: String,
    pub relays: Vec<String>,
}

/// The network side of "Share", implemented by the integration lot with `iroh` (and by a fake in the tests). It is only called when the
/// user clicks "Share": the application stays offline otherwise.
pub trait SessionNet {
    /// Opens the network side of a session: starts listening for guests and tells how to reach this host.
    fn start(&mut self) -> Result<(Box<dyn HostTransport>, HostAddress), String>;

    /// Called on every frame while the session runs: a better address than the one `start` gave (a relay that answered late, a new
    /// network interface), which the links should use from now on. `None` when nothing changed.
    fn refresh(&mut self) -> Option<HostAddress> {
        None
    }
}

/* --------------------------------------------------------------------------------------------------------------- settings */

pub const HOST_COLOR: (u8, u8, u8) = (0x4f, 0x6d, 0xf5);
/// Colors given to the guests, in order (the agent's orange is not among them).
const PALETTE: [(u8, u8, u8); 12] = [
    (0x1c, 0x7e, 0xd6),
    (0x2f, 0x9e, 0x44),
    (0xae, 0x3e, 0xc9),
    (0xe6, 0x49, 0x80),
    (0x0c, 0x99, 0x92),
    (0xf0, 0x8c, 0x00),
    (0x74, 0x5c, 0xe0),
    (0xc2, 0x25, 0x5c),
    (0x66, 0xa8, 0x0f),
    (0x09, 0x92, 0xcf),
    (0x86, 0x6a, 0x4f),
    (0x5c, 0x7c, 0xfa),
];
/// A closed connection stays readable this long (virtual or real milliseconds) so that the last message (refusal, removal) arrives.
const CLOSE_LINGER_MS: u64 = 2_000;
/// A connection that has not sent its first message after this long is dropped.
const HANDSHAKE_MS: u64 = 30_000;
const MAX_FRAMES_PER_POLL: usize = 1024;
const MAX_UPLOADS_PER_GUEST: usize = 2;
const UPLOAD_POOL: usize = 32;
const MAX_QUEUED_CHUNKS: usize = 512;
pub const MAX_NAME_CHARS: usize = 40;

#[derive(Clone, Debug)]
pub struct HostConfig {
    /// Shown to the guests ("Session of X"); never the Windows user name.
    pub host_name: String,
    /// Everybody in the session, the host included. Places held for a dropped guest count.
    pub max_participants: usize,
    /// Join requests waiting for the host's answer.
    pub max_pending: usize,
    /// Let people holding the read-only link in without asking.
    pub auto_accept_view: bool,
    pub limits: Limits,
    /// `Applied` kept to catch guests up.
    pub log_cap: usize,
    /// A dropped guest keeps its place this long.
    pub place_hold_ms: u64,
    /// A connection that sends nothing for this long is closed.
    pub idle_ms: u64,
    /// Violations (within `strike_decay_ms` of each other) after which a guest is excluded.
    pub max_strikes: u32,
    pub strike_decay_ms: u64,
    /// Objects the whole document may hold once guests have added theirs.
    pub max_total_objects: usize,
    /// Image chunks sent per connection and per `poll`.
    pub send_budget: usize,
    /// `<base>#<ticket>`: the web page, or `inkboard://join`.
    pub link_base: String,
    /// Tests only: deterministic randomness (keys, nonces). Never set in the application.
    pub entropy_seed: Option<u64>,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            host_name: t("Host"),
            max_participants: 10,
            max_pending: 8,
            auto_accept_view: false,
            limits: Limits::default(),
            log_cap: 1_000,
            place_hold_ms: 60_000,
            idle_ms: 120_000,
            max_strikes: 20,
            strike_decay_ms: 30_000,
            max_total_objects: 50_000,
            send_budget: 32,
            link_base: "inkboard://join".to_owned(),
            entropy_seed: None,
        }
    }
}

/* ---------------------------------------------------------------------------------------------------------- display names */

/// Cleans a name typed by a person: control characters removed, white space squeezed, 1 to 40 characters (`fallback` when nothing
/// is left). The name is cosmetic, not an identity.
pub fn sanitize_name(raw: &str, fallback: &str) -> String {
    // tabs and line breaks become spaces; other control characters and invisible direction marks (which could fake another name) go
    let invisible = |c: char| matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}');
    let cleaned: String = raw.chars().filter(|c| c.is_whitespace() || !(c.is_control() || invisible(*c))).collect();
    let squeezed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = squeezed.chars().take(MAX_NAME_CHARS).collect();
    let name = name.trim_end().to_owned();
    if name.is_empty() { fallback.to_owned() } else { name }
}

/// Makes `name` different from every name in `taken` (case-insensitive): "Anna", "Anna (2)", "Anna (3)"... within 40 characters.
pub fn unique_name<'a>(name: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: HashSet<String> = taken.into_iter().map(str::to_lowercase).collect();
    if !taken.contains(&name.to_lowercase()) {
        return name.to_owned();
    }
    for n in 2u32.. {
        let suffix = format!(" ({n})");
        let room = MAX_NAME_CHARS - suffix.chars().count();
        let base: String = name.chars().take(room).collect();
        let candidate = format!("{}{suffix}", base.trim_end());
        if !taken.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!("the loop ends on the first free number")
}

fn name_path() -> std::path::PathBuf {
    inkboard_mcp::live::data_dir().join("display_name")
}

/// The host's name from the data folder (`display_name`, like `language`), when the user chose one.
pub fn load_display_name() -> Option<String> {
    let name = sanitize_name(&std::fs::read_to_string(name_path()).ok()?, "");
    (!name.is_empty()).then_some(name)
}

pub fn save_display_name(name: &str) {
    let path = name_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, sanitize_name(name, &t("Host")));
}

/* -------------------------------------------------------------------------------------------------------------- helpers */

fn color_for(pid: ParticipantId, used: &[(u8, u8, u8)]) -> (u8, u8, u8) {
    PALETTE.iter().copied().find(|c| !used.contains(c)).unwrap_or_else(|| {
        let n = pid.0;
        (((n * 67) % 190 + 40) as u8, ((n * 131) % 190 + 40) as u8, ((n * 29) % 190 + 40) as u8)
    })
}

/// Media type of an image from its first bytes (only images are accepted from guests).
fn sniff_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        let head = String::from_utf8_lossy(&data[..data.len().min(512)]).to_lowercase();
        (head.contains("<svg")).then_some("image/svg+xml")
    }
}

fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn image_assets(ops: &[WireOp]) -> Vec<AssetId> {
    let mut out: Vec<AssetId> = Vec::new();
    let mut add = |o: &Object| {
        if let Kind::Image(i) = &o.kind
            && !out.contains(&i.asset)
        {
            out.push(i.asset.clone());
        }
    };
    for op in ops {
        match op {
            WireOp::Put { object, .. } => add(object),
            WireOp::AddPage { page, .. } => page.objects.iter().for_each(&mut add),
            _ => {}
        }
    }
    out
}

fn object_count(doc: &Document) -> usize {
    doc.pages.iter().map(|p| p.objects.len()).sum()
}

/// Operations that turn `old` into `new` (what the host did on its own), index-free. Page order changes are not expressed (the
/// application never reorders pages); the `view` of a page is local and ignored.
pub fn diff_documents(old: &Document, new: &Document) -> Vec<WireOp> {
    let mut ops = Vec::new();
    if old.name != new.name {
        ops.push(WireOp::SetName { name: new.name.clone() });
    }
    if old.background != new.background {
        ops.push(WireOp::SetBackground { background: new.background });
    }
    let survivors = old.pages.iter().any(|p| new.page(&p.id).is_some());
    let removals =
        |ops: &mut Vec<WireOp>| ops.extend(old.pages.iter().filter(|p| new.page(&p.id).is_none()).map(|p| WireOp::RemovePage { page: p.id.clone() }));
    if survivors {
        removals(&mut ops); // a document always keeps one page: remove first while a survivor guarantees it
    }
    for (index, p) in new.pages.iter().enumerate() {
        match old.page(&p.id) {
            None => ops.push(WireOp::AddPage { index, page: Box::new(p.clone()) }),
            Some(o) => diff_page(o, p, &mut ops),
        }
    }
    if !survivors {
        removals(&mut ops);
    }
    ops
}

fn diff_page(old: &Page, new: &Page, ops: &mut Vec<WireOp>) {
    let id = &new.id;
    if old.name != new.name {
        ops.push(WireOp::RenamePage { page: id.clone(), name: new.name.clone() });
    }
    if old.layers != new.layers {
        ops.push(WireOp::SetLayers { page: id.clone(), layers: new.layers.clone() });
    }
    let before: HashMap<&Id, &Object> = old.objects.iter().map(|o| (&o.id, o)).collect();
    let after: HashSet<&Id> = new.objects.iter().map(|o| &o.id).collect();
    let deleted: Vec<Id> = old.objects.iter().filter(|o| !after.contains(&o.id)).map(|o| o.id.clone()).collect();
    if !deleted.is_empty() {
        ops.push(WireOp::Delete { page: id.clone(), ids: deleted });
    }
    for o in &new.objects {
        if before.get(&o.id).is_none_or(|b| *b != o) {
            ops.push(WireOp::Put { page: id.clone(), object: Box::new(o.clone()) });
        }
    }
    // order after the deletions and the insertions on top: the survivors in their old order, then the new ones
    let mut order: Vec<&Id> = old.objects.iter().map(|o| &o.id).filter(|i| after.contains(i)).collect();
    order.extend(new.objects.iter().map(|o| &o.id).filter(|i| !before.contains_key(i)));
    if order.iter().copied().ne(new.objects.iter().map(|o| &o.id)) {
        ops.push(WireOp::Order { page: id.clone(), ids: new.objects.iter().map(|o| o.id.clone()).collect() });
    }
}

/* ------------------------------------------------------------------------------------------------------------- data */

/// Cached manifest of the images of the authoritative document (an image never changes under its id, so a hash is computed once).
#[derive(Default)]
struct AssetIndex {
    info: BTreeMap<AssetId, AssetInfo>,
    by_hash: HashMap<String, AssetId>,
}

impl AssetIndex {
    fn refresh(&mut self, doc: &Document) {
        if self.info.len() == doc.assets.len() && doc.assets.keys().all(|k| self.info.contains_key(k)) {
            return;
        }
        for (id, a) in &doc.assets {
            if !self.info.contains_key(id) {
                let hash = hash_hex(&a.data);
                self.by_hash.entry(hash.clone()).or_insert_with(|| id.clone());
                self.info.insert(id.clone(), AssetInfo { id: id.clone(), hash, size: a.data.len() as u64, mime: a.mime.clone() });
            }
        }
    }
    fn manifest(&self) -> Vec<AssetInfo> {
        self.info.values().cloned().collect()
    }
}

struct Uploaded {
    hash: String,
    mime: &'static str,
    data: Arc<Vec<u8>>,
}

enum ConnState {
    /// Nothing received yet.
    New,
    /// The guest waits for the host's answer (request id).
    Pending(u64),
    Member(ParticipantId),
    /// Refused, removed or closed: kept readable until `until`, then dropped.
    Closing {
        until: u64,
    },
}

struct Conn {
    id: u64,
    conn: Box<dyn Connection>,
    chan: Option<SecureChannel>,
    key_role: Role,
    state: ConnState,
    opened: u64,
    last_rx: u64,
    /// Image chunks waiting to be sent (a few per `poll`, so that one big image does not block everything).
    out: VecDeque<Message>,
}

struct Member {
    id: ParticipantId,
    name: String,
    color: (u8, u8, u8),
    role: Role,
    /// Transport identity of the guest: only the same identity can resume the place.
    peer: String,
    conn: Option<u64>,
    away_since: Option<u64>,
    ops_rl: RateLimiter,
    transient_rl: RateLimiter,
    quota: Quota,
    strikes: u32,
    last_strike: u64,
    uploads: HashMap<String, Vec<u8>>,
    presence: Option<Presence>,
}

struct Request {
    id: u64,
    conn: u64,
    name: String,
    key_role: Role,
    peer: String,
}

/// A join request waiting for the host's answer ("Anna wants to join").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingView {
    pub request: u64,
    pub name: String,
    /// What the link they used allows: a read-only link cannot be turned into edit access.
    pub key_role: Role,
}

/// A participant as the interface shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberView {
    pub id: ParticipantId,
    pub name: String,
    pub color: (u8, u8, u8),
    pub role: Role,
    /// `false` while the place is held for a guest whose connection dropped.
    pub connected: bool,
    pub editing: Option<Id>,
}

/// What the interface may want to react to (toasts, the consent window, remote cursors).
#[derive(Clone, Debug, PartialEq)]
pub enum HostEvent {
    JoinRequest {
        request: u64,
        name: String,
    },
    Joined {
        id: ParticipantId,
        name: String,
    },
    Left {
        id: ParticipantId,
        name: String,
    },
    Excluded {
        id: ParticipantId,
        name: String,
        reason: String,
    },
    /// A guest went over a limit (it was told; repeated violations get it excluded).
    Warned {
        id: ParticipantId,
        name: String,
        reason: String,
    },
    /// An undo or redo left objects alone because somebody else changed them: `(who, how many)`.
    UndoSkipped {
        id: ParticipantId,
        by: Vec<(String, usize)>,
    },
    Presence {
        id: ParticipantId,
        presence: Box<Presence>,
    },
    /// An editor is moving or resizing `object`: this is its state for now (relayed to the guests; not part of the board).
    Transient {
        id: ParticipantId,
        object: Id,
        state: Box<inkboard_core::Object>,
    },
    /// New points of a stroke an editor is drawing.
    Stroke {
        id: ParticipantId,
        stroke: Id,
        points: Vec<Pt>,
    },
    /// The gesture of this guest is over (its result was submitted) or its connection dropped: whatever was shown of it goes away.
    Settled {
        id: ParticipantId,
    },
}

/* ----------------------------------------------------------------------------------------------------------- the host */

pub struct SessionHost {
    cfg: HostConfig,
    transport: Option<Box<dyn HostTransport>>,
    keys: HostKeys,
    seqr: Sequencer,
    synced_rev: u64,
    now: u64,
    conns: Vec<Conn>,
    next_conn: u64,
    members: BTreeMap<ParticipantId, Member>,
    next_pid: u32,
    requests: Vec<Request>,
    next_request: u64,
    decisions: Vec<(u64, Option<Role>)>,
    banned: HashSet<String>,
    histories: AuthorHistories,
    assets: AssetIndex,
    uploaded: VecDeque<Uploaded>,
    events: Vec<HostEvent>,
    roster_dirty: bool,
    entropy_counter: u64,
    /// Frames that could not be opened before any handshake (wrong key, garbage).
    dropped_frames: u64,
}

fn describe(v: &Violation) -> RejectReason {
    match v {
        Violation::QuotaExceeded { .. } => RejectReason::Quota,
        Violation::RateExceeded { .. } => RejectReason::RateLimited,
        other => RejectReason::Invalid { detail: other.to_string() },
    }
}

impl SessionHost {
    /// Starts a session on `ed`'s current document. Fresh keys are generated: the links of an earlier session never work again.
    pub fn start(cfg: HostConfig, ed: &Editor, transport: Box<dyn HostTransport>) -> Result<Self, Error> {
        let mut entropy: Box<dyn Entropy> = match cfg.entropy_seed {
            Some(s) => Box::new(SeededEntropy::new(s)),
            None => Box::new(OsEntropy),
        };
        let keys = HostKeys::generate(entropy.as_mut())?;
        let mut assets = AssetIndex::default();
        assets.refresh(&ed.doc);
        Ok(Self {
            seqr: Sequencer::new(ed.doc.clone(), cfg.log_cap),
            cfg,
            transport: Some(transport),
            keys,
            synced_rev: ed.revision(),
            now: 0,
            conns: Vec::new(),
            next_conn: 1,
            members: BTreeMap::new(),
            next_pid: 1,
            requests: Vec::new(),
            next_request: 1,
            decisions: Vec::new(),
            banned: HashSet::new(),
            histories: AuthorHistories::default(),
            assets,
            uploaded: VecDeque::new(),
            events: Vec::new(),
            roster_dirty: false,
            entropy_counter: 0,
            dropped_frames: 0,
        })
    }

    pub fn is_running(&self) -> bool {
        self.transport.is_some()
    }
    pub fn config_mut(&mut self) -> &mut HostConfig {
        &mut self.cfg
    }
    pub fn host_name(&self) -> &str {
        &self.cfg.host_name
    }
    /// Everybody in the session, the host included.
    pub fn participant_count(&self) -> usize {
        1 + self.members.len()
    }
    /// Sequence number of the last modification broadcast.
    #[cfg(test)]
    pub fn seq(&self) -> Seq {
        self.seqr.seq()
    }
    #[cfg(test)]
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    pub fn take_events(&mut self) -> Vec<HostEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn pending(&self) -> Vec<PendingView> {
        self.requests.iter().map(|r| PendingView { request: r.id, name: r.name.clone(), key_role: r.key_role }).collect()
    }

    pub fn members(&self) -> Vec<MemberView> {
        self.members
            .values()
            .map(|m| MemberView {
                id: m.id,
                name: m.name.clone(),
                color: m.color,
                role: m.role,
                connected: m.conn.is_some(),
                editing: m.presence.as_ref().and_then(|p| p.editing.clone()),
            })
            .collect()
    }

    /// The link to give for `role` (edit or read-only), for the host reachable at `addr`.
    pub fn link(&self, role: Role, addr: &HostAddress) -> Result<String, Error> {
        let ticket = self.keys.ticket(role, &addr.host, &addr.relays)?;
        Ok(Link { base: self.cfg.link_base.clone(), ticket }.to_url())
    }

    /// New keys: the previous links stop working for newcomers (guests already in keep their connection).
    pub fn regenerate_links(&mut self) -> Result<(), Error> {
        let mut entropy: Box<dyn Entropy> = match self.cfg.entropy_seed {
            Some(s) => Box::new(SeededEntropy::new(s ^ 0x5eed ^ self.next_request)),
            None => Box::new(OsEntropy),
        };
        self.keys = HostKeys::generate(entropy.as_mut())?;
        Ok(())
    }

    /* ------------------------------------------------------------------------------------------------ decisions of the host */

    /// Accepts a join request with `role` (a read-only link cannot give more than read-only). Takes effect at the next `poll`.
    pub fn accept(&mut self, request: u64, role: Role) {
        self.decisions.push((request, Some(role)));
    }

    pub fn decline(&mut self, request: u64) {
        self.decisions.push((request, None));
    }

    /// Removes a participant for good (its identity is refused if it comes back during this session).
    pub fn kick(&mut self, pid: ParticipantId) {
        self.exclude(pid, "removed by the host".to_owned());
    }

    pub fn set_role(&mut self, pid: ParticipantId, role: Role) {
        let Some(m) = self.members.get_mut(&pid) else { return };
        if m.role == role {
            return;
        }
        m.role = role;
        self.send_to(pid, &Message::RoleChange { participant: pid, role });
        self.roster_dirty = true;
    }

    pub fn rename(&mut self, pid: ParticipantId, name: &str) {
        let name = sanitize_name(name, &t("Guest"));
        let host = self.cfg.host_name.clone();
        let others: Vec<&str> = self.members.values().filter(|m| m.id != pid).map(|m| m.name.as_str()).chain(std::iter::once(host.as_str())).collect();
        let unique = unique_name(&name, others);
        if let Some(m) = self.members.get_mut(&pid) {
            m.name = unique;
            self.roster_dirty = true;
        }
    }

    pub fn set_host_name(&mut self, name: &str) {
        self.cfg.host_name = sanitize_name(name, &t("Host"));
        self.roster_dirty = true;
    }

    /// Ends the session: tells everybody, closes every connection and stops listening.
    pub fn stop(&mut self) {
        let bye = Message::Bye { reason: ByeReason::HostLeft };
        for i in 0..self.conns.len() {
            if matches!(self.conns[i].state, ConnState::Member(_)) {
                self.send(i, &bye);
            }
            self.conns[i].conn.close();
        }
        self.conns.clear();
        self.requests.clear();
        self.decisions.clear();
        self.members.clear();
        self.transport = None;
    }

    /// Relays an action of the host's MCP agent to the guests (they see the agent act).
    pub fn forward_agent(&mut self, ev: &crate::presence::AgentEvent) {
        let targets: Vec<Id> = ev.targets().map(|t| t.id.clone()).collect();
        let at = ev.targets().map(|t| t.frame).reduce(|a, b| a.union(&b)).map(|r| r.center());
        let action = if ev.action == crate::presence::Action::Look { "look".to_owned() } else { ev.tool.clone() };
        self.broadcast(&Message::Agent(WireAgent { name: ev.name.clone(), action, targets, at }), None);
    }

    /// Relays the host's own cursor and selection.
    pub fn send_host_presence(&mut self, mut p: Presence) {
        p.participant = Some(ParticipantId::HOST);
        self.broadcast(&Message::Presence(p), None);
    }

    /* -------------------------------------------------------------------------------------------------------------- poll */

    /// Call on every frame with a clock in milliseconds (any origin, monotonic): accepts connections, reads what the guests sent,
    /// applies it to `ed`, broadcasts what changed on the host side, serves images, expires places.
    pub fn poll(&mut self, ed: &mut Editor, now: u64) {
        if self.transport.is_none() {
            return;
        }
        self.now = now;
        self.accept_connections();
        self.flush_host(ed);
        self.process_decisions();
        let mut inbound: Vec<(u64, Vec<u8>)> = Vec::new();
        for c in &mut self.conns {
            if matches!(c.state, ConnState::Closing { .. }) {
                continue;
            }
            for _ in 0..MAX_FRAMES_PER_POLL {
                let Some(f) = c.conn.recv() else { break };
                c.last_rx = now;
                inbound.push((c.id, f));
            }
        }
        for (cid, frame) in inbound {
            self.on_frame(cid, &frame, ed);
        }
        self.drain_outgoing();
        self.housekeeping();
        if self.roster_dirty {
            self.roster_dirty = false;
            let roster = Message::Participants { list: self.roster() };
            self.broadcast(&roster, None);
        }
    }

    fn accept_connections(&mut self) {
        let cap = (self.cfg.max_participants + self.cfg.max_pending) * 2;
        while let Some(conn) = self.transport.as_mut().and_then(|t| t.accept()) {
            let mut conn = conn;
            if self.conns.len() >= cap {
                conn.close();
                continue;
            }
            let id = self.next_conn;
            self.next_conn += 1;
            self.conns.push(Conn {
                id,
                conn,
                chan: None,
                key_role: Role::View,
                state: ConnState::New,
                opened: self.now,
                last_rx: self.now,
                out: VecDeque::new(),
            });
        }
    }

    fn conn_index(&self, id: u64) -> Option<usize> {
        self.conns.iter().position(|c| c.id == id)
    }

    fn new_channel(&mut self, cipher: FrameCipher) -> SecureChannel {
        self.entropy_counter += 1;
        let entropy: Box<dyn Entropy> = match self.cfg.entropy_seed {
            Some(s) => Box::new(SeededEntropy::new(s.wrapping_add(self.entropy_counter.wrapping_mul(7919)))),
            None => Box::new(OsEntropy),
        };
        SecureChannel::new(cipher, Box::new(JsonCodec), entropy, self.cfg.limits.max_message_bytes)
    }

    /* ------------------------------------------------------------------------------------------------------------- sending */

    fn send(&mut self, i: usize, msg: &Message) {
        let c = &mut self.conns[i];
        if let Some(ch) = c.chan.as_mut()
            && let Ok(frame) = ch.seal(msg)
        {
            let _ = c.conn.send(&frame);
        }
    }

    fn send_to(&mut self, pid: ParticipantId, msg: &Message) {
        let Some(cid) = self.members.get(&pid).and_then(|m| m.conn) else { return };
        if let Some(i) = self.conn_index(cid) {
            self.send(i, msg);
        }
    }

    /// To every guest (all of them have been admitted), except `except`.
    fn broadcast(&mut self, msg: &Message, except: Option<ParticipantId>) {
        for i in 0..self.conns.len() {
            if let ConnState::Member(pid) = self.conns[i].state
                && Some(pid) != except
            {
                self.send(i, msg);
            }
        }
    }

    fn close_later(&mut self, i: usize) {
        self.conns[i].state = ConnState::Closing { until: self.now + CLOSE_LINGER_MS };
    }

    fn drop_conn(&mut self, i: usize) {
        self.conns[i].conn.close();
        self.conns[i].state = ConnState::Closing { until: self.now };
    }

    fn refuse(&mut self, i: usize, reason: RefuseReason) {
        self.send(i, &Message::Refused { reason });
        self.close_later(i);
    }

    fn drain_outgoing(&mut self) {
        for i in 0..self.conns.len() {
            for _ in 0..self.cfg.send_budget {
                let Some(m) = self.conns[i].out.pop_front() else { break };
                self.send(i, &m);
            }
        }
    }

    /* --------------------------------------------------------------------------------------------------- the host's changes */

    /// Broadcasts what the host changed in the editor since the last call (found by comparing with the authoritative copy).
    fn flush_host(&mut self, ed: &Editor) {
        for (id, a) in &ed.doc.assets {
            if !self.seqr.doc().assets.contains_key(id) {
                self.seqr.insert_asset(id.clone(), a.clone());
            }
        }
        self.assets.refresh(self.seqr.doc());
        if ed.revision() == self.synced_rev {
            return;
        }
        self.synced_rev = ed.revision();
        let ops = diff_documents(self.seqr.doc(), &ed.doc);
        self.histories.note(0, ops.iter().flat_map(WireOp::object_ids));
        for part in ops.chunks(self.cfg.limits.max_ops_per_message.max(1)) {
            let applied = self.seqr.apply_local(ParticipantId::HOST, part.to_vec());
            self.broadcast(&Message::Applied(applied), None);
        }
    }

    /* ----------------------------------------------------------------------------------------------------------- frames */

    fn on_frame(&mut self, cid: u64, frame: &[u8], ed: &mut Editor) {
        let Some(i) = self.conn_index(cid) else { return };
        if matches!(self.conns[i].state, ConnState::Closing { .. }) {
            return;
        }
        if self.conns[i].chan.is_none() {
            // first frame: the key that opens it gives the role
            if check_message_size(&self.cfg.limits, frame.len()).is_err() {
                self.dropped_frames += 1;
                return self.drop_conn(i);
            }
            let Ok((role, cipher, plain)) = self.keys.open_first(frame) else {
                self.dropped_frames += 1;
                return self.drop_conn(i);
            };
            self.conns[i].key_role = role;
            let channel = self.new_channel(cipher);
            self.conns[i].chan = Some(channel);
            match read_hello(&JsonCodec, &plain) {
                Ok(h) => self.on_hello(i, h),
                Err(Error::IncompatibleProtocol { .. }) => self.refuse(i, RefuseReason::Version { host_proto: PROTO_VERSION }),
                Err(_) => {
                    self.dropped_frames += 1;
                    self.drop_conn(i);
                }
            }
            return;
        }
        let opened = self.conns[i].chan.as_ref().map(|c| c.open(frame));
        match (opened, self.conns[i].state_member()) {
            (Some(Ok(Message::Hello(h))), _) => self.on_hello(i, h),
            (Some(Ok(msg)), Some(pid)) => self.on_message(pid, msg, ed),
            (Some(Err(Error::Invalid(v))), Some(pid)) => self.violation(pid, &v, 0),
            (Some(Err(_)), Some(pid)) => {
                let v = Violation::Malformed("frame that cannot be read");
                self.violation(pid, &v, 0);
            }
            // nothing is accepted from somebody who has not been let in
            _ => self.dropped_frames += 1,
        }
    }

    /* ---------------------------------------------------------------------------------------------------------- handshake */

    fn capacity_left(&self) -> bool {
        self.participant_count() < self.cfg.max_participants
    }

    fn on_hello(&mut self, i: usize, h: inkboard_session::proto::Hello) {
        let key_role = self.conns[i].key_role;
        if !verify_join_proof(self.keys.key(key_role), &h.name, h.role_requested, &h.join_proof) {
            return self.refuse(i, RefuseReason::BadProof);
        }
        match self.conns[i].state {
            // the guest repeats its `Hello` (it lost our answer): answer again
            ConnState::Member(pid) => return self.welcome(i, pid, h.resume.map(|r| r.last_seq)),
            ConnState::Pending(_) => return,
            ConnState::Closing { .. } => return,
            ConnState::New => {}
        }
        let peer = self.conns[i].conn.peer().to_owned();
        if self.banned.contains(&peer) {
            return self.refuse(i, RefuseReason::Banned);
        }
        // reconnection: same identity, same place
        if let Some(r) = h.resume.as_ref()
            && let Some(m) = self.members.get_mut(&r.participant)
            && m.peer == peer
        {
            let (pid, old) = (r.participant, m.conn.replace(self.conns[i].id));
            m.away_since = None;
            if let Some(oi) = old.and_then(|c| self.conn_index(c)) {
                self.drop_conn(oi);
            }
            self.conns[i].state = ConnState::Member(pid);
            return self.welcome(i, pid, Some(r.last_seq));
        }
        if !self.capacity_left() || self.requests.len() >= self.cfg.max_pending {
            return self.refuse(i, RefuseReason::Full);
        }
        let name = sanitize_name(&h.name, &t("Guest"));
        if key_role == Role::View && self.cfg.auto_accept_view {
            return self.admit(i, name, Role::View, peer);
        }
        // one request per identity: a newer one replaces the older
        if let Some(pos) = self.requests.iter().position(|r| r.peer == peer) {
            let old = self.requests.remove(pos);
            if let Some(oi) = self.conn_index(old.conn) {
                self.drop_conn(oi);
            }
        }
        let request = self.next_request;
        self.next_request += 1;
        self.conns[i].state = ConnState::Pending(request);
        self.requests.push(Request { id: request, conn: self.conns[i].id, name: name.clone(), key_role, peer });
        self.events.push(HostEvent::JoinRequest { request, name });
    }

    fn process_decisions(&mut self) {
        for (rid, role) in std::mem::take(&mut self.decisions) {
            let Some(pos) = self.requests.iter().position(|r| r.id == rid) else { continue };
            let req = self.requests.remove(pos);
            let Some(i) = self.conn_index(req.conn) else { continue };
            match role {
                None => self.refuse(i, RefuseReason::Declined),
                Some(_) if !self.capacity_left() => self.refuse(i, RefuseReason::Full),
                Some(role) => {
                    let role = if req.key_role == Role::View { Role::View } else { role };
                    self.admit(i, req.name, role, req.peer);
                }
            }
        }
    }

    fn admit(&mut self, i: usize, name: String, role: Role, peer: String) {
        let pid = ParticipantId(self.next_pid);
        self.next_pid += 1;
        let host = self.cfg.host_name.clone();
        let name = unique_name(&name, self.members.values().map(|m| m.name.as_str()).chain(std::iter::once(host.as_str())));
        let used: Vec<(u8, u8, u8)> = self.members.values().map(|m| m.color).chain(std::iter::once(HOST_COLOR)).collect();
        let l = &self.cfg.limits;
        let member = Member {
            id: pid,
            name: name.clone(),
            color: color_for(pid, &used),
            role,
            peer,
            conn: Some(self.conns[i].id),
            away_since: None,
            ops_rl: RateLimiter::new(l.ops_per_sec, l.ops_burst),
            transient_rl: RateLimiter::new(l.transient_per_sec, l.transient_burst),
            quota: Quota::default(),
            strikes: 0,
            last_strike: 0,
            uploads: HashMap::new(),
            presence: None,
        };
        self.members.insert(pid, member);
        self.conns[i].state = ConnState::Member(pid);
        self.roster_dirty = true;
        self.events.push(HostEvent::Joined { id: pid, name });
        self.welcome(i, pid, None);
    }

    fn roster(&self) -> Vec<ParticipantInfo> {
        std::iter::once(ParticipantInfo { id: ParticipantId::HOST, name: self.cfg.host_name.clone(), color: HOST_COLOR, role: Role::Edit })
            .chain(self.members.values().map(|m| ParticipantInfo { id: m.id, name: m.name.clone(), color: m.color, role: m.role }))
            .collect()
    }

    /// `Welcome`, then the missing modifications (`last_seq` known) or the whole document.
    fn welcome(&mut self, i: usize, pid: ParticipantId, last_seq: Option<Seq>) {
        let Some(m) = self.members.get(&pid) else { return };
        let doc = self.seqr.doc();
        let welcome = Welcome {
            participant: pid,
            color: m.color,
            role: m.role,
            doc_meta: DocMeta { name: doc.name.clone(), seq: self.seqr.seq(), pages: doc.pages.len() },
            host_proto: PROTO_VERSION,
            host_name: self.cfg.host_name.clone(),
            participants: self.roster(),
        };
        self.send(i, &Message::Welcome(welcome));
        let manifest = self.assets.manifest();
        match last_seq.map(|s| self.seqr.catch_up(s, manifest.clone())) {
            Some(CatchUp::Applied(v)) => v.into_iter().for_each(|a| self.send(i, &Message::Applied(a))),
            Some(CatchUp::Snapshot(s)) => self.send(i, &Message::Snapshot(s)),
            None => {
                let s = self.seqr.snapshot(manifest);
                self.send(i, &Message::Snapshot(s));
            }
        }
    }

    /* ------------------------------------------------------------------------------------------------------ exclusion */

    fn exclude(&mut self, pid: ParticipantId, reason: String) {
        let Some(m) = self.members.remove(&pid) else { return };
        self.banned.insert(m.peer.clone());
        if let Some(i) = m.conn.and_then(|c| self.conn_index(c)) {
            self.send(i, &Message::Kick { participant: pid });
            self.close_later(i);
        }
        self.seqr.forget(pid);
        self.histories.forget(pid.0);
        self.roster_dirty = true;
        self.events.push(HostEvent::Excluded { id: pid, name: m.name, reason });
    }

    fn name_of(&self, author: Author) -> String {
        if author == 0 {
            return self.cfg.host_name.clone();
        }
        self.members.get(&ParticipantId(author)).map_or_else(|| t("someone"), |m| m.name.clone())
    }

    /// A guest broke a limit: it is told (always for a modification, which expects an answer; now and then otherwise) and, after
    /// `max_strikes` violations in a short time, excluded.
    fn violation(&mut self, pid: ParticipantId, v: &Violation, client_seq: u64) {
        let (now, decay, max) = (self.now, self.cfg.strike_decay_ms, self.cfg.max_strikes);
        let Some(m) = self.members.get_mut(&pid) else { return };
        if now.saturating_sub(m.last_strike) > decay {
            m.strikes = 0;
        }
        m.strikes += 1;
        m.last_strike = now;
        let (strikes, name) = (m.strikes, m.name.clone());
        if strikes >= max {
            return self.exclude(pid, v.to_string());
        }
        if client_seq != 0 || strikes == 1 || strikes % 5 == 0 {
            self.send_to(pid, &Message::Rejected { client_seq, reason: describe(v) });
        }
        self.events.push(HostEvent::Warned { id: pid, name, reason: v.to_string() });
    }

    /* ------------------------------------------------------------------------------------------------------- messages */

    fn on_message(&mut self, pid: ParticipantId, msg: Message, ed: &mut Editor) {
        let now = self.now;
        let Some(m) = self.members.get_mut(&pid) else { return };
        let role = m.role;
        if let Err(v) = check_message(&self.cfg.limits, &msg) {
            let seq = if let Message::Submit(s) = &msg { s.client_seq } else { 0 };
            return self.violation(pid, &v, seq);
        }
        match msg {
            Message::Submit(s) => {
                if !m.ops_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "modifications" }, s.client_seq);
                }
                if role == Role::View {
                    return self.send_to(pid, &Message::Rejected { client_seq: s.client_seq, reason: RejectReason::ReadOnly });
                }
                self.on_submit(pid, s, ed);
            }
            Message::Undo | Message::Redo => {
                if !m.ops_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "modifications" }, 0);
                }
                if role == Role::View {
                    return self.send_to(pid, &Message::Rejected { client_seq: 0, reason: RejectReason::ReadOnly });
                }
                self.on_undo(pid, matches!(msg, Message::Redo), ed);
            }
            Message::Transient { object, state, .. } => {
                if !m.transient_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "ephemeral messages" }, 0);
                }
                if role == Role::Edit {
                    self.events.push(HostEvent::Transient { id: pid, object: object.clone(), state: state.clone() });
                    self.broadcast(&Message::Transient { object, state, author: Some(pid) }, Some(pid));
                }
            }
            Message::StrokePoints { stroke, points, .. } => {
                if !m.transient_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "ephemeral messages" }, 0);
                }
                if role == Role::Edit {
                    self.events.push(HostEvent::Stroke { id: pid, stroke: stroke.clone(), points: points.clone() });
                    self.broadcast(&Message::StrokePoints { stroke, points, author: Some(pid) }, Some(pid));
                }
            }
            Message::Presence(mut p) => {
                if !m.transient_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "ephemeral messages" }, 0);
                }
                p.participant = Some(pid);
                m.presence = Some(p.clone());
                self.events.push(HostEvent::Presence { id: pid, presence: Box::new(p.clone()) });
                self.broadcast(&Message::Presence(p), Some(pid));
            }
            Message::Resync { last_seq } => {
                let manifest = self.assets.manifest();
                let Some(i) = self.members.get(&pid).and_then(|m| m.conn).and_then(|c| self.conn_index(c)) else { return };
                match self.seqr.catch_up(last_seq, manifest) {
                    CatchUp::Applied(v) => v.into_iter().for_each(|a| self.send(i, &Message::Applied(a))),
                    CatchUp::Snapshot(s) => self.send(i, &Message::Snapshot(s)),
                }
            }
            Message::AssetRequest { hash } => {
                if !m.transient_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "image requests" }, 0);
                }
                self.serve_asset(pid, &hash);
            }
            Message::AssetChunk(c) => {
                if !m.ops_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "image data" }, 0);
                }
                if role == Role::View {
                    return self.send_to(pid, &Message::Rejected { client_seq: 0, reason: RejectReason::ReadOnly });
                }
                self.on_chunk(pid, c);
            }
            Message::Ping { nonce } => self.send_to(pid, &Message::Pong { nonce }),
            Message::Bye { .. } => self.leave(pid),
            // `Follow` is between the guest's own interface and the presence it already receives; the rest only the host sends
            _ => {}
        }
    }

    /// A guest leaves on purpose: its place is released at once.
    fn leave(&mut self, pid: ParticipantId) {
        let Some(m) = self.members.remove(&pid) else { return };
        if let Some(i) = m.conn.and_then(|c| self.conn_index(c)) {
            self.drop_conn(i);
        }
        self.seqr.forget(pid);
        self.histories.forget(pid.0);
        self.roster_dirty = true;
        self.events.push(HostEvent::Left { id: pid, name: m.name });
    }

    fn on_submit(&mut self, pid: ParticipantId, s: Submit, ed: &mut Editor) {
        self.events.push(HostEvent::Settled { id: pid });
        let l = &self.cfg.limits;
        let client_seq = s.client_seq;
        let pages = ed.doc.pages.len() + s.ops.iter().filter(|o| matches!(o, WireOp::AddPage { .. })).count();
        let added: usize = s
            .ops
            .iter()
            .map(|o| match o {
                WireOp::Put { .. } => 1,
                WireOp::AddPage { page, .. } => page.objects.len(),
                _ => 0,
            })
            .sum();
        let check = check_page_count(l, pages).and_then(|()| {
            if object_count(&ed.doc) + added > self.cfg.max_total_objects {
                Err(Violation::TooMany { what: "objects in the board", count: object_count(&ed.doc) + added, max: self.cfg.max_total_objects })
            } else {
                Ok(())
            }
        });
        if let Err(v) = check {
            return self.violation(pid, &v, client_seq);
        }
        // an image must be one the board already has or one the guest has just uploaded (its asset id is the hash of its bytes)
        let wanted = image_assets(&s.ops);
        for id in &wanted {
            if !ed.doc.assets.contains_key(id) && !self.seqr.doc().assets.contains_key(id) && !self.uploaded.iter().any(|u| u.hash == id.as_str()) {
                let reason = RejectReason::Invalid { detail: "an image of this modification was not uploaded".to_owned() };
                return self.send_to(pid, &Message::Rejected { client_seq, reason });
            }
        }
        for id in wanted {
            if self.seqr.doc().assets.contains_key(&id) && ed.doc.assets.contains_key(&id) {
                continue;
            }
            let found = self
                .seqr
                .doc()
                .assets
                .get(&id)
                .cloned()
                .or_else(|| self.uploaded.iter().find(|u| u.hash == id.as_str()).map(|u| Asset { mime: u.mime.to_owned(), data: Arc::clone(&u.data) }));
            if let Some(asset) = found {
                if !self.seqr.doc().assets.contains_key(&id) {
                    self.seqr.insert_asset(id.clone(), asset.clone());
                }
                ed.doc.assets.entry(id).or_insert(asset);
            }
        }
        match self.seqr.submit(pid, s) {
            Submitted::Applied(applied) => {
                let core = ed.apply_external_with(|d| apply_wire_all(d, &applied.ops));
                self.histories.record(pid.0, core);
                self.synced_rev = ed.revision();
                self.assets.refresh(self.seqr.doc());
                self.broadcast(&Message::Applied(applied), None);
            }
            Submitted::Duplicate => {
                let host_seq = self.seqr.seq();
                self.send_to(pid, &Message::Rejected { client_seq, reason: RejectReason::Duplicate { host_seq } });
            }
        }
    }

    fn on_undo(&mut self, pid: ParticipantId, redo: bool, ed: &mut Editor) {
        let outcome = if redo { self.histories.redo(pid.0, &ed.doc) } else { self.histories.undo(pid.0, &ed.doc) };
        let Some(out) = outcome else {
            let detail = if redo { "nothing to redo" } else { "nothing to undo" };
            return self.send_to(pid, &Message::Rejected { client_seq: 0, reason: RejectReason::Invalid { detail: detail.to_owned() } });
        };
        if !out.ops.is_empty() {
            let wire = ops_to_wire(&out.ops, self.seqr.doc());
            let applied = self.seqr.apply_local(pid, wire);
            ed.apply_external_with(|d| apply_wire_all(d, &applied.ops));
            self.synced_rev = ed.revision();
            self.broadcast(&Message::Applied(applied), None);
        }
        let by: Vec<(String, usize)> = out.skipped_by().into_iter().map(|(a, n)| (a.map_or_else(|| t("someone"), |a| self.name_of(a)), n)).collect();
        if !by.is_empty() {
            let text = by.iter().map(|(who, n)| format!("{n} objects modified by {who}")).collect::<Vec<_>>().join(", ");
            let detail = format!("{text}, not {}", if redo { "redone" } else { "undone" });
            self.send_to(pid, &Message::Rejected { client_seq: 0, reason: RejectReason::Invalid { detail } });
            self.events.push(HostEvent::UndoSkipped { id: pid, by });
        }
    }

    /* ----------------------------------------------------------------------------------------------------------- images */

    fn serve_asset(&mut self, pid: ParticipantId, hash: &str) {
        let Some(id) = self.assets.by_hash.get(hash).cloned() else { return };
        let Some(data) = self.seqr.doc().assets.get(&id).map(|a| Arc::clone(&a.data)) else { return };
        let Some(i) = self.members.get(&pid).and_then(|m| m.conn).and_then(|c| self.conn_index(c)) else { return };
        if self.conns[i].out.len() + data.len() / self.cfg.limits.max_asset_chunk_bytes.max(1) > MAX_QUEUED_CHUNKS {
            return; // the guest asked for more than it can take: it asks again later
        }
        let chunk = self.cfg.limits.max_asset_chunk_bytes;
        self.conns[i].out.extend(chunks(&data, hash, chunk).into_iter().map(Message::AssetChunk));
    }

    /// An image uploaded by a guest, chunk by chunk, before the modification that uses it.
    fn on_chunk(&mut self, pid: ParticipantId, c: AssetChunk) {
        let limits = self.cfg.limits.clone();
        let Some(m) = self.members.get_mut(&pid) else { return };
        if !is_hash(&c.hash) {
            return self.violation(pid, &Violation::Malformed("image identifier is not a SHA-256 hash"), 0);
        }
        if !m.uploads.contains_key(&c.hash) && m.uploads.len() >= MAX_UPLOADS_PER_GUEST {
            return; // too many uploads at once: the guest starts this one again later
        }
        let buf = m.uploads.entry(c.hash.clone()).or_default();
        let have = buf.len() as u64;
        if c.offset < have {
            return; // a chunk that arrived twice
        }
        if c.offset > have {
            m.uploads.remove(&c.hash); // a gap: the guest must start again
            return;
        }
        buf.extend_from_slice(&c.data);
        if buf.len() as u64 > limits.max_image_bytes {
            m.uploads.remove(&c.hash);
            return self.violation(pid, &Violation::ImageTooLarge { size: have + c.data.len() as u64, max: limits.max_image_bytes }, 0);
        }
        if !c.last {
            return;
        }
        let data = m.uploads.remove(&c.hash).unwrap_or_default();
        if hash_hex(&data) != c.hash {
            return self.violation(pid, &Violation::Malformed("image does not match its hash"), 0);
        }
        let Some(mime) = sniff_mime(&data) else {
            return self.violation(pid, &Violation::Malformed("not an image (png, jpeg, gif, webp or svg)"), 0);
        };
        if let Err(v) = m.quota.charge(&limits, data.len() as u64) {
            return self.violation(pid, &v, 0);
        }
        if self.uploaded.iter().all(|u| u.hash != c.hash) {
            self.uploaded.push_back(Uploaded { hash: c.hash, mime, data: Arc::new(data) });
            while self.uploaded.len() > UPLOAD_POOL {
                self.uploaded.pop_front();
            }
        }
    }

    /* ------------------------------------------------------------------------------------------------------ upkeep */

    fn housekeeping(&mut self) {
        let now = self.now;
        let (idle, hold) = (self.cfg.idle_ms, self.cfg.place_hold_ms);
        let mut gone: Vec<usize> = Vec::new();
        for (i, c) in self.conns.iter().enumerate() {
            let dead = match c.state {
                ConnState::Closing { until } => now >= until,
                ConnState::New => !c.conn.is_open() || now.saturating_sub(c.opened) > HANDSHAKE_MS,
                ConnState::Pending(_) | ConnState::Member(_) => !c.conn.is_open() || now.saturating_sub(c.last_rx) > idle,
            };
            if dead {
                gone.push(i);
            }
        }
        for &i in gone.iter().rev() {
            let c = self.conns.remove(i);
            let mut conn = c.conn;
            conn.close();
            match c.state {
                ConnState::Pending(r) => self.requests.retain(|q| q.id != r),
                // the place is held for a while: the guest may come back
                ConnState::Member(pid) => {
                    if let Some(m) = self.members.get_mut(&pid)
                        && m.conn == Some(c.id)
                    {
                        m.conn = None;
                        m.away_since = Some(now);
                        self.events.push(HostEvent::Settled { id: pid });
                    }
                }
                _ => {}
            }
        }
        let expired: Vec<ParticipantId> = self.members.values().filter(|m| m.away_since.is_some_and(|t| now.saturating_sub(t) >= hold)).map(|m| m.id).collect();
        for pid in expired {
            self.leave(pid);
        }
    }
}

impl Conn {
    fn state_member(&self) -> Option<ParticipantId> {
        match self.state {
            ConnState::Member(p) => Some(p),
            _ => None,
        }
    }
}

/// Point of the world under a screen position (used to send the host's cursor).
pub fn world_point(screen: eframe::egui::Pos2, canvas_min: eframe::egui::Pos2, view: inkboard_core::View) -> Pt {
    let z = view.zoom.max(1e-6);
    Pt::new((f64::from(screen.x - canvas_min.x) - view.x) / z, (f64::from(screen.y - canvas_min.y) - view.y) / z)
}
