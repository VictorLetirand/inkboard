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
use inkboard_session::assets::hash_hex;
use inkboard_session::codec::{Codec, read_hello};
use inkboard_session::crypto::{Entropy, FrameCipher, HostKeys, Link, OsEntropy, SecureChannel, SeededEntropy, SessionKey, verify_join_proof};
use inkboard_session::limits::{Limits, Quota, RateLimiter, Violation, check_message, check_message_size, check_page_count};
use inkboard_session::proto::{
    AgentEvent as WireAgent, AssetChunk, AssetInfo, ByeReason, DocMeta, Message, PROTO_VERSION, ParticipantId, ParticipantInfo, Presence, RefuseReason,
    RejectReason, Role, Seq, Snapshot, Submit, UploadStatus, Welcome,
};
use inkboard_session::sequencer::{Sequencer, Submitted};
use inkboard_session::snapshot::{PART_BYTES, into_messages};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::wire::{WireOp, apply_wire_all, ops_to_wire};
use inkboard_session::{Error, JsonCodec};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

/* ------------------------------------------------------------------------------------------------------------------ seams */

/// What the host needs from a transport: incoming connections. Every `Transport` is one (the trait of `inkboard-session` has an
/// associated type and cannot be a trait object).
pub trait HostTransport {
    fn accept(&mut self) -> Option<Box<dyn Connection>>;

    /// The session stops: frees the sockets and threads. A network transport does it in the background (closing waits for the
    /// last frames to be delivered, which must never freeze the interface); the default just drops it.
    fn release(self: Box<Self>) {}
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

/// What a background start returns: the transport (it crosses threads) and the address for the links.
pub type StartResult = Result<(Box<dyn HostTransport + Send>, HostAddress), String>;
/// A background start; the flag tells it that the user cancelled (its result is then dropped).
pub type StartJob = Box<dyn FnOnce(&std::sync::atomic::AtomicBool) -> StartResult + Send>;

/// The network side of "Share", implemented by the integration lot with `iroh` (and by a fake in the tests). It is only called when the
/// user clicks "Share": the application stays offline otherwise.
pub trait SessionNet {
    /// Opens the network side of a session: starts listening for guests and tells how to reach this host.
    fn start(&mut self) -> Result<(Box<dyn HostTransport>, HostAddress), String>;

    /// A start that can run on a thread (binding sockets and waiting for the relay may take seconds): the job gets a flag set when
    /// the user cancels. `None` (the default): only the blocking [`SessionNet::start`] exists (simulated networks).
    fn start_job(&mut self) -> Option<StartJob> {
        None
    }

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
/// Cursors that move at once before the host starts relaying each of them less often, and by how much for each more (ms between two
/// relays of one cursor, at most `PRESENCE_MAX_INTERVAL_MS`): 30 active cursors are relayed every 120 ms, 60 every 240 ms.
const PRESENCE_FREE_CURSORS: u64 = 20;
const PRESENCE_MS_PER_CURSOR: u64 = 4;
const PRESENCE_MAX_INTERVAL_MS: u64 = 500;
/// A connection that has not sent its first message after this long is dropped.
const HANDSHAKE_MS: u64 = 10_000;
/// Connections that have not sent anything yet are counted apart: only this many are kept (the oldest go first), so that connections
/// opened by somebody who only knows the host's identity cannot take the places of real guests.
const MAX_UNAUTHENTICATED: usize = 16;
/// A connection younger than this is never cut for being over `MAX_UNAUTHENTICATED` (its first message may still be on its way).
const SILENT_GRACE_MS: u64 = 2_000;
/// Revoked key sets kept for the resumption of admitted guests (the oldest go first: a guest that stayed away through this many
/// regenerations has to ask to join again).
const MAX_REVOKED_KEYS: usize = 16;
/// Images announced to the guests in one message.
const ANNOUNCE_PER_MESSAGE: usize = 200;
const MAX_FRAMES_PER_POLL: usize = 1024;
/// Bytes of frames read per `poll`, all connections together (what is left waits for the next `poll`): the work of one frame is
/// proportional to its size, so this bounds how long one `poll` can take.
const POLL_BYTES_BUDGET: usize = 8 << 20;
const MAX_UPLOADS_PER_GUEST: usize = 2;
const UPLOAD_POOL: usize = 32;
/// Bytes of images kept for the modification that will use them, all guests together.
const UPLOAD_POOL_BYTES: usize = 64 << 20;
/// Bytes of image pieces waiting to be sent to one guest.
const MAX_QUEUED_ASSET_BYTES: usize = 32 << 20;
/// Bytes of image pieces handed to the transports per `poll`, all guests together; and the encoded pieces kept for the next guests.
const ASSET_BYTES_PER_POLL: usize = 4 << 20;
const PIECE_CACHE_BYTES: usize = 96 << 20;
/// A guest whose transport already holds this much is not given more for now (it reads slowly): the rest waits in the host's memory,
/// shared with the other guests.
const TRANSPORT_LOW_WATER: usize = 1 << 20;
/// Modifications and other messages that wait behind a snapshot for one guest (bytes) before it is considered too slow and dropped.
const MAX_BACKLOG_BYTES: usize = 32 << 20;
/// A guest whose transport queue is at least this big and has not shrunk for `HostConfig::stall_ms` is not reading.
const STALL_MIN_BYTES: usize = 1 << 20;
pub const MAX_NAME_CHARS: usize = crate::names::NAME_MAX;

#[derive(Clone, Debug)]
pub struct HostConfig {
    /// Shown to the guests ("Session of X"); never the Windows user name.
    pub host_name: String,
    /// Everybody in the session, the host included. Places held for a dropped guest count.
    pub max_participants: usize,
    /// Join requests waiting for the host's answer.
    pub max_pending: usize,
    /// A request nobody answered is refused after this long, whatever the guest keeps sending.
    pub pending_ms: u64,
    /// Bytes of images all the guests together may upload during the session.
    pub total_asset_quota_bytes: u64,
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
    /// Places one transport identity may hold at once (admitted guests, held places included): one more is refused with a message.
    /// The connections it may have open at once, admitted or not, are limited to twice that (the excess is closed unread).
    pub max_per_peer: usize,
    /// Objects the whole document may hold once guests have added theirs.
    pub max_total_objects: usize,
    /// Image chunks sent per connection and per `poll`.
    pub send_budget: usize,
    /// From this many objects, the snapshot of the board is encoded on a thread and kept for `snapshot_ttl_ms` for the guests that
    /// arrive meanwhile (a smaller board is encoded on the spot for each: that is cheap and exact).
    pub snapshot_thread_min_objects: usize,
    pub snapshot_ttl_ms: u64,
    /// Bytes of board (and of what follows it) the host seals and hands to the transports per `poll`, all guests together: a whole
    /// board is never encrypted in one frame time, however many people arrive at once.
    pub ordered_bytes_per_poll: usize,
    /// A connection whose queue holds at least a megabyte that has not shrunk for this long is closed (the guest does not read).
    pub stall_ms: u64,
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
            pending_ms: 120_000,
            total_asset_quota_bytes: 512 << 20,
            auto_accept_view: false,
            limits: Limits::default(),
            log_cap: 1_000,
            place_hold_ms: 60_000,
            idle_ms: 120_000,
            max_strikes: 20,
            strike_decay_ms: 30_000,
            max_per_peer: 3,
            max_total_objects: 50_000,
            send_budget: 32,
            snapshot_thread_min_objects: 1_500,
            snapshot_ttl_ms: 3_000,
            ordered_bytes_per_poll: 4 << 20,
            stall_ms: 30_000,
            link_base: "inkboard://join".to_owned(),
            entropy_seed: None,
        }
    }
}

/* ---------------------------------------------------------------------------------------------------------- display names */

/// Cleans a name typed by a person or sent by a guest (`names::clean_name`, shared with the guest side): 1 to 40 visible characters,
/// `fallback` when nothing visible is left. The name is cosmetic, not an identity.
pub fn sanitize_name(raw: &str, fallback: &str) -> String {
    crate::names::clean_name(raw).unwrap_or_else(|| fallback.to_owned())
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
    crate::platform::data_dir().join("display_name")
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
        let channel = |k: u32| u8::try_from((n * k) % 190 + 40).unwrap_or(u8::MAX);
        (channel(67), channel(131), channel(29))
    })
}

/// Media type of an image from its first bytes (only images are accepted from guests).
pub(crate) fn sniff_mime(data: &[u8]) -> Option<&'static str> {
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

pub(crate) fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Longest side and number of pixels a bitmap received from somebody else may declare (read from the header, before any decoding).
const MAX_IMAGE_SIDE: u32 = inkboard_formats::raster::MAX_BITMAP_SIDE;
const MAX_IMAGE_PIXELS: u64 = inkboard_formats::raster::MAX_BITMAP_PIXELS;

/// Checks the dimensions a bitmap declares, without decoding it (a tiny file can claim gigabytes of pixels). Vector images are only
/// required to be text; a bitmap whose header cannot be read passes (it cannot be decoded either). `Err` is a short English reason.
pub(crate) fn check_image_bounds(data: &[u8], mime: &str) -> Result<(), &'static str> {
    if mime == "image/svg+xml" {
        return std::str::from_utf8(data).map(|_| ()).map_err(|_| "the vector image is not valid text");
    }
    // a header that cannot be read declares nothing: the decoder fails on it the same way and the image is drawn as unreadable
    // (a truncated file whose first bytes do declare huge dimensions is still caught by the plain header reader)
    let dims = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok());
    let Some((w, h)) = dims.or_else(|| inkboard_formats::raster::declared_dimensions(data)) else { return Ok(()) };
    if w == 0 || h == 0 || w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE || u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS {
        return Err("image dimensions out of bounds");
    }
    Ok(())
}

/// What the host accepts from a guest's upload: [`check_image_bounds`], and for a vector image the white list of
/// `inkboard_formats::raster::check_untrusted_svg` (no filter, pattern, mask, script, external link; bounded complexity), because every
/// participant will draw it. `Err` is a short English reason (shown to the guest).
pub(crate) fn check_guest_upload(data: &[u8], mime: &str) -> Result<(), &'static str> {
    check_image_bounds(data, mime)?;
    if mime == "image/svg+xml" {
        // `check_image_bounds` made sure it is text
        inkboard_formats::raster::check_untrusted_svg(&String::from_utf8_lossy(data))
            .map_err(|_| "vector image with features that are not allowed (filter, pattern, mask, script, external link) or too complex")?;
    }
    Ok(())
}

pub(crate) fn image_assets(ops: &[WireOp]) -> Vec<AssetId> {
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

/// Why a guest may not apply `ops` to `doc` (short English text), if they touch an object that is locked (by its own flag or by its
/// layer), change a lock, or remove something locked. Only guests go through here: the host's own changes are not checked.
fn lock_conflict(doc: &Document, ops: &[WireOp]) -> Option<&'static str> {
    const TOUCH: &str = "the object or its layer is locked";
    const CHANGE: &str = "only the host can lock or unlock";
    // the locked objects of each page the operations name, found once (a board has thousands of objects, a submit up to 200 operations)
    let mut locked: HashMap<&Id, HashSet<&Id>> = HashMap::new();
    for op in ops {
        if let WireOp::Put { page, .. } | WireOp::Delete { page, .. } | WireOp::Order { page, .. } = op
            && let Some(p) = doc.page(page)
        {
            locked.entry(&p.id).or_insert_with(|| p.objects.iter().filter(|o| p.is_locked(o)).map(|o| &o.id).collect());
        }
    }
    for op in ops {
        match op {
            WireOp::Put { page, object } => {
                let Some(p) = doc.page(page) else { continue };
                if object.locked {
                    return Some(CHANGE); // a guest never makes an object locked (one already locked is refused just below)
                }
                if p.layer(&object.layer).is_some_and(|l| l.locked) || locked[&p.id].contains(&object.id) {
                    return Some(TOUCH);
                }
            }
            WireOp::Delete { page, ids } => {
                let Some(p) = doc.page(page) else { continue };
                if ids.iter().any(|i| locked[&p.id].contains(i)) {
                    return Some(TOUCH);
                }
            }
            WireOp::Order { page, ids } => {
                let Some(p) = doc.page(page) else { continue };
                let set = &locked[&p.id];
                if !set.is_empty() && !p.objects.iter().map(|o| &o.id).filter(|i| set.contains(i)).eq(ids.iter().filter(|i| set.contains(i))) {
                    return Some(TOUCH); // the locked objects keep their order (and none may be left out)
                }
            }
            WireOp::SetLayers { page, layers } => {
                let Some(p) = doc.page(page) else { continue };
                let moved = layers.iter().any(|n| n.locked != p.layer(&n.id).is_some_and(|o| o.locked));
                let lost = p.layers.iter().any(|o| o.locked && !layers.iter().any(|n| n.id == o.id));
                if moved || lost {
                    return Some(CHANGE);
                }
            }
            WireOp::RemovePage { page } => {
                let Some(p) = doc.page(page) else { continue };
                if p.layers.iter().any(|l| l.locked) || p.objects.iter().any(|o| o.locked) {
                    return Some(TOUCH);
                }
            }
            WireOp::AddPage { .. } | WireOp::RenamePage { .. } | WireOp::SetBackground { .. } | WireOp::SetName { .. } => {}
        }
    }
    None
}

fn object_count(doc: &Document) -> usize {
    doc.pages.iter().map(|p| p.objects.len()).sum()
}

/// The board as encoded frames (see `inkboard_session::snapshot`): done once, then sealed for each guest.
fn encode_parts(s: Snapshot) -> Vec<Arc<Vec<u8>>> {
    into_messages(s, PART_BYTES).into_iter().map(|m| Arc::new(JsonCodec.encode(&inkboard_session::pack::pack(&m).unwrap_or(m)))).collect()
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
    /// Indexes the images of `doc` not seen before; returns them.
    fn refresh(&mut self, doc: &Document) -> Vec<AssetInfo> {
        let mut added = Vec::new();
        if self.info.len() == doc.assets.len() && doc.assets.keys().all(|k| self.info.contains_key(k)) {
            return added;
        }
        for (id, a) in &doc.assets {
            if !self.info.contains_key(id) {
                let hash = hash_hex(&a.data);
                self.by_hash.entry(hash.clone()).or_insert_with(|| id.clone());
                let info = AssetInfo { id: id.clone(), hash, size: a.data.len() as u64, mime: a.mime.clone() };
                self.info.insert(id.clone(), info.clone());
                added.push(info);
            }
        }
        added
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

#[allow(clippy::struct_field_names, reason = "`conn` is the transport connection of a `Conn`")]
struct Conn {
    id: u64,
    conn: Box<dyn Connection>,
    chan: Option<SecureChannel>,
    key_role: Role,
    /// The first frame was opened with a key of a revoked link (see `regenerate_links`): only a guest already admitted may use it,
    /// to resume its place.
    revoked_key: Option<SessionKey>,
    state: ConnState,
    opened: u64,
    last_rx: u64,
    /// The connection was found closed by an earlier `housekeeping`: it goes at the next one, after a whole `poll` read what it had
    /// received before closing (the final `Bye` of a guest who leaves; otherwise a departure would be taken for a network cut).
    closed_seen: bool,
    /// Image chunks waiting to be sent (a few per `poll`, so that one big image does not block everything).
    out: VecDeque<Piece>,
    /// Frames (already encoded, shared between guests) that must arrive in order: the board being sent and everything said after it.
    /// While it is not empty, whatever is sent to this guest goes behind it (a guest never sees a modification before its board).
    ordered: VecDeque<Ordered>,
    /// Bytes of the `Ordered` entries that are not snapshot.
    backlog: usize,
    /// The snapshot is being encoded on a thread: the entries of `ordered` go behind it.
    awaiting_snapshot: bool,
    /// Smallest size the transport queue had since `queue_since` (stall detection).
    queue_floor: usize,
    queue_since: u64,
    /// Frames per second before the guest is admitted (afterwards its `Member` limiters apply).
    rl: RateLimiter,
}

/// A piece of an image waiting to be sent to a guest: which image (by hash), which piece, how many bytes of the image it carries.
struct Piece {
    hash: String,
    idx: usize,
    raw: usize,
}

/// An encoded message waiting for its turn (see `Conn::ordered`).
struct Ordered {
    plain: Arc<Vec<u8>>,
    /// Part of the board (not counted in the backlog bound).
    snapshot: bool,
}

/// The board encoded as frames, ready for the guests that arrive (see `HostConfig::snapshot_thread_min_objects`).
#[derive(Clone)]
struct Built {
    seq: Seq,
    /// Images known when it was made: a newer list makes it obsolete (the guests would not find the images).
    assets: usize,
    at: u64,
    parts: Arc<Vec<Arc<Vec<u8>>>>,
}

enum SnapState {
    Empty,
    /// Being encoded on a thread: the guests that arrive wait for it.
    Building(Receiver<Built>),
    Ready(Built),
}

/// What `poll` read from a connection: a frame to decode, or one dropped unread because its sender is over its rate.
enum Inbound {
    Frame(Vec<u8>),
    Flood,
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
    /// `Resync` and repeated `Hello` (each can make the host send the whole document), for readers as well.
    sync_rl: RateLimiter,
    /// Frames and bytes received, checked before a frame is decoded.
    rx_frames: RateLimiter,
    rx_bytes: RateLimiter,
    /// Bytes of ephemeral messages (relayed to everybody).
    eph_bytes: RateLimiter,
    /// The guest has shown that it holds the document (it submitted, undid or resynchronized): a `Hello` from it is never answered.
    confirmed: bool,
    strikes: u32,
    last_strike: u64,
    uploads: HashMap<String, Vec<u8>>,
    presence: Option<Presence>,
    /// When this guest's cursor was last relayed, and the newer one waiting for its turn (see `SessionHost::presence_interval_ms`).
    presence_at: u64,
    held_presence: Option<Presence>,
}

struct Request {
    id: u64,
    conn: u64,
    name: String,
    key_role: Role,
    peer: String,
    /// When the request was made (it expires, see `HostConfig::pending_ms`).
    at: u64,
}

/// A join request waiting for the host's answer ("Anna wants to join").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingView {
    pub request: u64,
    pub name: String,
    /// What the link they used allows: a read-only link cannot be turned into edit access.
    pub key_role: Role,
    /// Start of the guest's transport identity: lets the host tell two "Anna" apart in the consent window.
    pub short_id: String,
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

/// Sizes of what a running host keeps (see [`SessionHost::stats`]); for the load tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostStats {
    pub seq: Seq,
    /// Modifications kept to catch guests up, and their estimated bytes.
    pub log_entries: usize,
    pub log_bytes: usize,
    /// Objects whose last author is remembered.
    pub tracked_objects: usize,
    pub connections: usize,
    pub members: usize,
    /// Image pieces waiting to be sent, all connections together.
    pub queued_messages: usize,
    pub dropped_frames: u64,
    /// Big boards encoded so far (one serves all the guests who arrive within `HostConfig::snapshot_ttl_ms`).
    pub snapshots_built: u64,
}

/* ----------------------------------------------------------------------------------------------------------- the host */

pub struct SessionHost {
    cfg: HostConfig,
    transport: Option<Box<dyn HostTransport>>,
    keys: HostKeys,
    /// Keys of the links the host revoked, most recent last: they only let a guest that was already admitted resume its place.
    revoked_keys: Vec<HostKeys>,
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
    /// Image bytes charged to each transport identity: kept when a guest leaves and comes back, so that the quota cannot be reset.
    quotas: HashMap<String, Quota>,
    /// Image bytes charged to everybody during the session.
    uploaded_total: u64,
    events: Vec<HostEvent>,
    roster_dirty: bool,
    entropy_counter: u64,
    /// Frames that could not be opened before any handshake (wrong key, garbage).
    dropped_frames: u64,
    /// Size of the frame being handled (for the byte limiters that need the message first).
    frame_len: usize,
    /// Where the next `poll` starts reading, so that no connection is always served last.
    rr: usize,
    snap: SnapState,
    /// Encoded pieces of images (shared by the guests who fetch the same image), oldest first, and their total size.
    piece_cache: HashMap<(String, usize), Arc<Vec<u8>>>,
    piece_order: VecDeque<(String, usize)>,
    piece_bytes: usize,
    /// Boards encoded for the big-board path (the cache of `HostConfig::snapshot_ttl_ms` makes it far fewer than the arrivals).
    snapshots_built: u64,
    /// Least delay between two relays of one guest's cursor (0 up to a dozen active cursors).
    presence_interval_ms: u64,
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
            revoked_keys: Vec::new(),
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
            quotas: HashMap::new(),
            uploaded_total: 0,
            events: Vec::new(),
            roster_dirty: false,
            entropy_counter: 0,
            dropped_frames: 0,
            frame_len: 0,
            rr: 0,
            snap: SnapState::Empty,
            piece_cache: HashMap::new(),
            piece_order: VecDeque::new(),
            piece_bytes: 0,
            snapshots_built: 0,
            presence_interval_ms: 0,
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
    /// Connections that exist, admitted or not.
    #[cfg(test)]
    pub fn connection_count(&self) -> usize {
        self.conns.len()
    }

    pub fn take_events(&mut self) -> Vec<HostEvent> {
        std::mem::take(&mut self.events)
    }

    /// Sizes of what the host keeps (load tests).
    #[cfg(test)]
    pub fn stats(&self) -> HostStats {
        let (log_entries, log_bytes) = self.seqr.log_stats();
        HostStats {
            seq: self.seqr.seq(),
            log_entries,
            log_bytes,
            tracked_objects: self.seqr.tracked_objects(),
            connections: self.conns.len(),
            members: self.members.len(),
            queued_messages: self.conns.iter().map(|c| c.out.len()).sum(),
            dropped_frames: self.dropped_frames,
            snapshots_built: self.snapshots_built,
        }
    }

    pub fn pending(&self) -> Vec<PendingView> {
        self.requests
            .iter()
            .map(|r| PendingView { request: r.id, name: r.name.clone(), key_role: r.key_role, short_id: r.peer.chars().take(8).collect() })
            .collect()
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

    /// New keys: the previous links stop working at once for everybody who is not in yet. Requests waiting for an answer are refused;
    /// a newcomer with an old link gets no answer at all. Guests already admitted keep their connection and, if it drops, can
    /// resume their place with the old key (their transport identity must match that of the place); to cut one of them off, remove
    /// them (their identity is then refused for the rest of the session and no link, old or new, gives them a place back).
    pub fn regenerate_links(&mut self) -> Result<(), Error> {
        let mut entropy: Box<dyn Entropy> = match self.cfg.entropy_seed {
            Some(s) => Box::new(SeededEntropy::new(s ^ 0x5eed ^ self.next_request)),
            None => Box::new(OsEntropy),
        };
        let fresh = HostKeys::generate(entropy.as_mut())?;
        self.revoked_keys.push(std::mem::replace(&mut self.keys, fresh));
        if self.revoked_keys.len() > MAX_REVOKED_KEYS {
            self.revoked_keys.remove(0);
        }
        for r in std::mem::take(&mut self.requests) {
            if let Some(i) = self.conn_index(r.conn) {
                self.refuse(i, RefuseReason::BadProof);
            }
        }
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
                self.deliver(i, &JsonCodec.encode(&bye));
            }
            self.conns[i].conn.close();
        }
        self.conns.clear();
        self.requests.clear();
        self.decisions.clear();
        self.members.clear();
        if let Some(t) = self.transport.take() {
            t.release();
        }
    }

    /// Relays an action of the host's MCP agent to the guests (they see the agent act).
    pub fn forward_agent(&mut self, ev: &crate::presence::AgentEvent) {
        let targets: Vec<Id> = ev.targets().map(|t| t.id.clone()).collect();
        let at = ev.targets().map(|t| t.frame).reduce(|a, b| a.union(&b)).map(|r| r.center());
        let action = if ev.action == crate::presence::Action::Look { "look".to_owned() } else { ev.tool.clone() };
        self.broadcast(&Message::Agent(WireAgent { name: ev.name.clone(), action, targets, at }), None);
    }

    /// The state of an object the host is moving, resizing or editing right now (shown by the guests, never written to their board).
    pub fn send_host_transient(&mut self, object: Id, state: Object) {
        self.broadcast(&Message::Transient { object, state: Box::new(state), author: Some(ParticipantId::HOST) }, None);
    }

    /// New points of a stroke the host is drawing right now.
    pub fn send_host_stroke(&mut self, stroke: Id, points: Vec<Pt>) {
        self.broadcast(&Message::StrokePoints { stroke, points, author: Some(ParticipantId::HOST) }, None);
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
        self.poll_build();
        self.flush_host(ed);
        self.process_decisions();
        // Frames are counted (rate, bytes) before anything is decoded, and only a budget of bytes is read per poll: what is over
        // the budget stays queued for the next poll, so a flood cannot freeze the interface.
        let mut inbound: Vec<(u64, Inbound)> = Vec::new();
        let mut spent = 0usize;
        let n = self.conns.len();
        let first = if n == 0 { 0 } else { self.rr % n };
        self.rr = self.rr.wrapping_add(1);
        for k in 0..n {
            let c = &mut self.conns[(first + k) % n];
            if matches!(c.state, ConnState::Closing { .. }) {
                continue;
            }
            for _ in 0..MAX_FRAMES_PER_POLL {
                if spent >= POLL_BYTES_BUDGET {
                    break;
                }
                let Some(f) = c.conn.recv() else { break };
                c.last_rx = now;
                spent += f.len();
                let within = match c.state {
                    ConnState::Member(pid) => self.members.get_mut(&pid).is_none_or(|m| m.rx_frames.allow(now) && m.rx_bytes.allow_n(now, f.len() as u64)),
                    _ => c.rl.allow(now),
                };
                inbound.push((c.id, if within { Inbound::Frame(f) } else { Inbound::Flood }));
            }
        }
        inbound.sort_by_key(|(cid, _)| *cid); // stable: the order of arrival on each connection is kept; the rotation only decides who is read first
        for (cid, item) in inbound {
            match item {
                Inbound::Frame(frame) => self.on_frame(cid, &frame, ed),
                Inbound::Flood => self.on_flood(cid),
            }
        }
        self.relay_held_presence();
        self.drain_ordered();
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
            // connections that have not said anything yet are limited apart (see `housekeeping`): they do not use up this ceiling
            if self.conns.iter().filter(|c| !matches!(c.state, ConnState::New)).count() >= cap {
                conn.close();
                continue;
            }
            // one transport identity cannot fill the host with connections (nothing is known of it yet, so nothing can be said to it)
            let open_from_peer = self.conns.iter().filter(|c| !matches!(c.state, ConnState::Closing { .. }) && c.conn.peer() == conn.peer()).count();
            if open_from_peer >= self.cfg.max_per_peer.max(1) * 2 {
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
                revoked_key: None,
                state: ConnState::New,
                opened: self.now,
                last_rx: self.now,
                closed_seen: false,
                out: VecDeque::new(),
                ordered: VecDeque::new(),
                backlog: 0,
                awaiting_snapshot: false,
                queue_floor: usize::MAX,
                queue_since: self.now,
                rl: RateLimiter::new(10, 20),
            });
        }
    }

    /// A frame dropped unread: the guest is over its rate. Somebody not admitted yet is simply disconnected.
    fn on_flood(&mut self, cid: u64) {
        let Some(i) = self.conn_index(cid) else { return };
        match self.conns[i].state {
            ConnState::Member(pid) => self.violation(pid, &Violation::RateExceeded { what: "frames" }, 0),
            ConnState::Closing { .. } => {}
            _ => self.fail_conn(i, "sending too fast".to_owned()),
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

    /// Sends `msg` on connection `i`. `false` when it could not be sent (frame too large for the transport, a guest that does not read,
    /// a dead link): the connection is then closed (see `fail_conn`), never left half working.
    fn send(&mut self, i: usize, msg: &Message) -> bool {
        if self.conns[i].ordered.is_empty() && !self.conns[i].awaiting_snapshot {
            return self.deliver(i, &JsonCodec.encode(msg));
        }
        let plain = Arc::new(JsonCodec.encode(msg));
        self.send_plain(i, &plain)
    }

    /// Like `send` for a message already encoded (a broadcast is encoded once, then sealed for each guest): it goes behind the board
    /// being sent to this guest, if any, so that a guest never sees a modification before the document it applies to.
    fn send_plain(&mut self, i: usize, plain: &Arc<Vec<u8>>) -> bool {
        let c = &mut self.conns[i];
        if c.ordered.is_empty() && !c.awaiting_snapshot {
            return self.deliver(i, plain);
        }
        if matches!(c.state, ConnState::Closing { .. }) {
            return false;
        }
        c.backlog += plain.len();
        c.ordered.push_back(Ordered { plain: Arc::clone(plain), snapshot: false });
        if c.backlog > MAX_BACKLOG_BYTES {
            self.fail_conn(i, "the guest is too slow to receive the board and what follows".to_owned());
            return false;
        }
        true
    }

    /// Seals and hands the encoded message to the transport.
    fn deliver(&mut self, i: usize, plain: &[u8]) -> bool {
        let c = &mut self.conns[i];
        let Some(ch) = c.chan.as_mut() else { return true };
        let failure = match ch.seal_encoded(plain).and_then(|frame| c.conn.send(&frame)) {
            Ok(()) => return true,
            Err(e) => e.to_string(),
        };
        if !matches!(c.state, ConnState::Closing { .. }) {
            self.fail_conn(i, failure);
        }
        false
    }

    /// The connection cannot be written to: it is closed at once. A guest keeps its place for a while, as after any lost connection
    /// (it can come back and resume); the host is told.
    #[allow(clippy::needless_pass_by_value, reason = "owned: the reason is moved into the event of a caller that has built it")]
    fn fail_conn(&mut self, i: usize, why: String) {
        let id = self.conns[i].id;
        match self.conns[i].state {
            ConnState::Member(pid) => {
                if let Some(m) = self.members.get_mut(&pid)
                    && m.conn == Some(id)
                {
                    m.conn = None;
                    m.away_since = Some(self.now);
                    let name = m.name.clone();
                    self.events.push(HostEvent::Settled { id: pid });
                    self.events.push(HostEvent::Warned { id: pid, name, reason: format!("connection closed: {why}") });
                }
            }
            ConnState::Pending(r) => self.requests.retain(|q| q.id != r),
            _ => {}
        }
        self.drop_conn(i);
    }

    /// The board for connection `i`, in as many frames as the transport needs (see `inkboard_session::snapshot`). The frames wait in
    /// the connection's ordered queue and leave a few megabytes per `poll` (see `drain_ordered`), so that arrivals never freeze the
    /// host; a big board is encoded on a thread, once for all the guests who arrive within a few seconds.
    fn queue_snapshot(&mut self, i: usize) {
        let objects = object_count(self.seqr.doc());
        if objects < self.cfg.snapshot_thread_min_objects {
            let snap = self.seqr.snapshot(self.assets.manifest());
            let parts: Vec<Ordered> = encode_parts(snap).into_iter().map(|plain| Ordered { plain, snapshot: true }).collect();
            self.conns[i].ordered.extend(parts);
            return;
        }
        if let SnapState::Ready(b) = &self.snap
            && self.now.saturating_sub(b.at) <= self.cfg.snapshot_ttl_ms
            && b.assets == self.assets.info.len()
        {
            let b = b.clone();
            if self.attach_built(i, &b, false) {
                return;
            }
        }
        self.conns[i].awaiting_snapshot = true;
        if !matches!(self.snap, SnapState::Building(_)) {
            self.start_build();
        }
    }

    /// Starts encoding the board for the guests that wait for it.
    fn start_build(&mut self) {
        self.snapshots_built += 1;
        let snap = self.seqr.snapshot(self.assets.manifest());
        let (seq, assets, at) = (snap.seq, self.assets.info.len(), self.now);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new().name("inkboard-snapshot".to_owned()).spawn({
                let snap = snap.clone();
                move || {
                    let _ = tx.send(Built { seq, assets, at, parts: Arc::new(encode_parts(snap)) });
                }
            });
            if spawned.is_ok() {
                self.snap = SnapState::Building(rx);
                return;
            }
        }
        let b = Built { seq, assets, at, parts: Arc::new(encode_parts(snap)) };
        self.snap = SnapState::Ready(b.clone());
        self.fulfil(&b);
    }

    /// Tests drive the host with a virtual clock while a big board is encoded on a thread in real time: they wait here for the thread
    /// before letting virtual time go on (on a loaded machine hundreds of virtual seconds would pass, the guests would be taken for
    /// idle and dropped, and the encoding would start again, for ever).
    #[cfg(test)]
    pub fn wait_for_build(&mut self) {
        let began = std::time::Instant::now();
        while matches!(self.snap, SnapState::Building(_)) && began.elapsed() < std::time::Duration::from_secs(600) {
            self.poll_build();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Gives the board that was being encoded to the guests that waited for it.
    fn poll_build(&mut self) {
        let SnapState::Building(rx) = &self.snap else { return };
        match rx.try_recv() {
            Ok(mut b) => {
                b.at = self.now; // the time to live counts from the moment it is ready, not from the start of a long encoding
                self.snap = SnapState::Ready(b.clone());
                self.fulfil(&b);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // the thread died: encode it here
                self.snap = SnapState::Empty;
                if self.conns.iter().any(|c| c.awaiting_snapshot) {
                    self.start_build_inline();
                }
            }
        }
    }

    fn start_build_inline(&mut self) {
        self.snapshots_built += 1;
        let snap = self.seqr.snapshot(self.assets.manifest());
        let b = Built { seq: snap.seq, assets: self.assets.info.len(), at: self.now, parts: Arc::new(encode_parts(snap)) };
        self.snap = SnapState::Ready(b.clone());
        self.fulfil(&b);
    }

    fn fulfil(&mut self, b: &Built) {
        for i in 0..self.conns.len() {
            if !self.conns[i].awaiting_snapshot {
                continue;
            }
            self.conns[i].awaiting_snapshot = false;
            if !self.attach_built(i, b, true) {
                // the log no longer reaches back to this board (a very slow build): encode a fresh one here
                self.conns[i].awaiting_snapshot = true;
                self.start_build_inline();
                return;
            }
        }
    }

    /// Queues an encoded board and the modifications made since for connection `i` (`front`: before what already waits there, for a
    /// guest that arrived while the board was being encoded). `false` when the log cannot bridge the gap.
    fn attach_built(&mut self, i: usize, b: &Built, front: bool) -> bool {
        let Some(applied) = self.seqr.applied_since(b.seq) else { return false };
        let mut items: Vec<Ordered> = b.parts.iter().map(|p| Ordered { plain: Arc::clone(p), snapshot: true }).collect();
        let c = &mut self.conns[i];
        for a in applied {
            let plain = Arc::new(JsonCodec.encode(&Message::Applied(a)));
            c.backlog += plain.len();
            items.push(Ordered { plain, snapshot: false });
        }
        if front {
            for it in items.into_iter().rev() {
                c.ordered.push_front(it);
            }
        } else {
            c.ordered.extend(items);
        }
        true
    }

    /// Sets how often each cursor is relayed from the number of cursors that move (no delay up to `PRESENCE_FREE_CURSORS`, then a few
    /// milliseconds more for each), and relays the cursors that waited for their turn.
    fn relay_held_presence(&mut self) {
        let now = self.now;
        let active = self.members.values().filter(|m| m.held_presence.is_some() || now.saturating_sub(m.presence_at) < 1_000).count() as u64;
        self.presence_interval_ms = if active <= PRESENCE_FREE_CURSORS { 0 } else { (active * PRESENCE_MS_PER_CURSOR).min(PRESENCE_MAX_INTERVAL_MS) };
        let interval = self.presence_interval_ms;
        let due: Vec<ParticipantId> =
            self.members.values().filter(|m| m.held_presence.is_some() && now.saturating_sub(m.presence_at) >= interval).map(|m| m.id).collect();
        for pid in due {
            if let Some(m) = self.members.get_mut(&pid) {
                m.presence_at = now;
                if let Some(p) = m.held_presence.take() {
                    self.broadcast(&Message::Presence(p), Some(pid));
                }
            }
        }
    }

    /// Seals and hands to the transports a few megabytes of what waits behind boards, all guests together; a guest whose transport
    /// still holds a lot is skipped (it reads slowly).
    fn drain_ordered(&mut self) {
        let n = self.conns.len();
        let mut budget = self.cfg.ordered_bytes_per_poll;
        for k in 0..n {
            let i = (self.rr.wrapping_add(k)) % n;
            while budget > 0 {
                let c = &mut self.conns[i];
                if matches!(c.state, ConnState::Closing { .. }) || c.awaiting_snapshot || c.conn.queued_bytes() >= TRANSPORT_LOW_WATER {
                    break;
                }
                let Some(item) = c.ordered.pop_front() else { break };
                if !item.snapshot {
                    c.backlog = c.backlog.saturating_sub(item.plain.len());
                }
                budget = budget.saturating_sub(item.plain.len());
                if !self.deliver(i, &item.plain) {
                    break;
                }
            }
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
        // encoded once: only the encryption is done for each guest
        let plain = Arc::new(JsonCodec.encode(msg));
        for i in 0..self.conns.len() {
            if let ConnState::Member(pid) = self.conns[i].state
                && Some(pid) != except
            {
                self.send_plain(i, &plain);
            }
        }
    }

    fn close_later(&mut self, i: usize) {
        self.conns[i].state = ConnState::Closing { until: self.now + CLOSE_LINGER_MS };
    }

    fn drop_conn(&mut self, i: usize) {
        self.conns[i].conn.close();
        self.conns[i].ordered.clear();
        self.conns[i].backlog = 0;
        self.conns[i].awaiting_snapshot = false;
        self.conns[i].state = ConnState::Closing { until: self.now };
    }

    fn refuse(&mut self, i: usize, reason: RefuseReason) {
        self.deliver(i, &JsonCodec.encode(&Message::Refused { reason }));
        self.close_later(i);
    }

    /// Hands to the transports pieces of images: a few per guest, a few megabytes in all per `poll`, and nothing to a guest whose
    /// transport still holds a lot. The text of a piece is made once and shared by all the guests who fetch that image.
    fn drain_outgoing(&mut self) {
        let n = self.conns.len();
        let mut budget = ASSET_BYTES_PER_POLL;
        for k in 0..n {
            let i = self.rr.wrapping_add(k) % n;
            for _ in 0..self.cfg.send_budget {
                let c = &self.conns[i];
                if budget == 0 {
                    return;
                }
                if c.out.is_empty()
                    || !c.ordered.is_empty()
                    || c.awaiting_snapshot
                    || matches!(c.state, ConnState::Closing { .. })
                    || c.conn.queued_bytes() >= TRANSPORT_LOW_WATER
                {
                    break;
                }
                let Some(p) = self.conns[i].out.pop_front() else { break };
                let Some(plain) = self.piece_plain(&p.hash, p.idx) else { continue };
                budget = budget.saturating_sub(p.raw);
                if !self.deliver(i, &plain) {
                    break;
                }
            }
        }
    }

    /// The encoded message of piece `idx` of the image with this hash (cached: 100 guests fetching the same photo cost one encoding).
    fn piece_plain(&mut self, hash: &str, idx: usize) -> Option<Arc<Vec<u8>>> {
        let key = (hash.to_owned(), idx);
        if let Some(p) = self.piece_cache.get(&key) {
            return Some(Arc::clone(p));
        }
        let size = self.cfg.limits.max_asset_chunk_bytes.max(1);
        let plain = {
            let id = self.assets.by_hash.get(hash)?;
            let data = &self.seqr.doc().assets.get(id)?.data;
            let from = idx.checked_mul(size)?;
            if from > data.len() || (from == data.len() && idx > 0) {
                return None;
            }
            let to = (from + size).min(data.len());
            let chunk = AssetChunk { hash: hash.to_owned(), offset: from as u64, data: data[from..to].to_vec(), last: to >= data.len() };
            Arc::new(JsonCodec.encode(&Message::AssetChunk(chunk)))
        };
        self.piece_bytes += plain.len();
        self.piece_cache.insert(key.clone(), Arc::clone(&plain));
        self.piece_order.push_back(key);
        while self.piece_bytes > PIECE_CACHE_BYTES {
            let Some(old) = self.piece_order.pop_front() else { break };
            if let Some(p) = self.piece_cache.remove(&old) {
                self.piece_bytes -= p.len();
            }
        }
        Some(plain)
    }

    /* --------------------------------------------------------------------------------------------------- the host's changes */

    /// Broadcasts what the host changed in the editor since the last call (found by comparing with the authoritative copy).
    fn flush_host(&mut self, ed: &Editor) {
        for (id, a) in &ed.doc.assets {
            if !self.seqr.doc().assets.contains_key(id) {
                self.seqr.insert_asset(id.clone(), a.clone());
            }
        }
        // images the host gained since the guests got the manifest (their identifiers are not hashes): announced before the
        // modification that uses them, so that the guests already in can fetch them
        let added = self.assets.refresh(self.seqr.doc());
        for list in added.chunks(ANNOUNCE_PER_MESSAGE) {
            self.broadcast(&Message::Assets { list: list.to_vec() }, None);
        }
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
        self.frame_len = frame.len();
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
            // the current keys first, then those of revoked links (they only serve the resumption of a place, see `on_hello`)
            let current = self.keys.open_first(frame).map(|(r, c, p)| (r, c, p, None));
            let opened = current
                .or_else(|e| self.revoked_keys.iter().rev().find_map(|k| k.open_first(frame).ok().map(|(r, c, p)| (r, c, p, Some(k.key(r).clone())))).ok_or(e));
            let Ok((role, cipher, plain, revoked_key)) = opened else {
                self.dropped_frames += 1;
                return self.drop_conn(i);
            };
            self.conns[i].key_role = role;
            self.conns[i].revoked_key = revoked_key;
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
        match self.conns[i].state {
            // A guest already in repeats its `Hello` only when it lost our answer, and only until it has shown that it holds the
            // document: answered within a small budget, then ignored (a guest that insists is punished like any other violation).
            ConnState::Member(pid) => {
                let now = self.now;
                let Some(m) = self.members.get_mut(&pid) else { return };
                if !m.sync_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "repeated hellos" }, 0);
                }
                if m.confirmed {
                    return;
                }
                return self.welcome(i, pid, h.resume.map(|r| r.last_seq));
            }
            ConnState::Pending(_) | ConnState::Closing { .. } => return,
            ConnState::New => {}
        }
        let proof_key = self.conns[i].revoked_key.as_ref().unwrap_or_else(|| self.keys.key(key_role));
        if !verify_join_proof(proof_key, &h.name, h.role_requested, &h.join_proof) {
            return self.refuse(i, RefuseReason::BadProof);
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
        if self.conns[i].revoked_key.is_some() {
            // a revoked link never lets a newcomer in (nor a removed guest back): it is not even answered
            self.dropped_frames += 1;
            return self.drop_conn(i);
        }
        // A guest that starts again without resuming (it never got the whole board, or it lost its state) does not need the places it
        // left held: they are freed, so that a guest on a bad link that keeps retrying does not use up the places of the session.
        let held: Vec<ParticipantId> = self.members.values().filter(|m| m.peer == peer && m.conn.is_none()).map(|m| m.id).collect();
        for pid in held {
            self.leave(pid);
        }
        if self.members.values().filter(|m| m.peer == peer).count() >= self.cfg.max_per_peer.max(1) {
            return self.refuse(i, RefuseReason::TooManyFromPeer);
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
        self.requests.push(Request { id: request, conn: self.conns[i].id, name: name.clone(), key_role, peer, at: self.now });
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

    #[allow(clippy::needless_pass_by_value, reason = "owned: the strings come out of a request that is consumed")]
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
            sync_rl: RateLimiter::new(l.sync_per_sec, l.sync_burst),
            rx_frames: RateLimiter::new(l.frames_per_sec, l.frames_burst),
            rx_bytes: RateLimiter::new(l.bytes_per_sec, l.bytes_burst),
            eph_bytes: RateLimiter::new(l.ephemeral_bytes_per_sec, l.ephemeral_bytes_burst),
            confirmed: false,
            strikes: 0,
            last_strike: 0,
            uploads: HashMap::new(),
            presence: None,
            presence_at: 0,
            held_presence: None,
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
        match last_seq.and_then(|s| self.seqr.applied_since(s)) {
            Some(v) => {
                for a in v {
                    if !self.send(i, &Message::Applied(a)) {
                        break;
                    }
                }
            }
            None => self.queue_snapshot(i),
        }
    }

    /* ------------------------------------------------------------------------------------------------------ exclusion */

    fn exclude(&mut self, pid: ParticipantId, reason: String) {
        let Some(m) = self.members.remove(&pid) else { return };
        self.banned.insert(m.peer.clone());
        if let Some(i) = m.conn.and_then(|c| self.conn_index(c)) {
            self.deliver(i, &JsonCodec.encode(&Message::Kick { participant: pid }));
            self.close_later(i);
        }
        self.seqr.forget(pid);
        self.histories.forget(pid.0);
        self.roster_dirty = true;
        self.events.push(HostEvent::Excluded { id: pid, name: m.name, reason });
    }

    /// Display name of the guest who modified `id` last, when it was a guest (and not the host) and the session still remembers it.
    /// Used to mark text that reaches the host's agent as written by somebody else (S-14).
    pub fn guest_author_of(&self, id: &Id) -> Option<String> {
        let author = self.histories.last_author(id).filter(|a| *a != 0)?;
        Some(self.members.get(&ParticipantId(author)).map_or_else(|| t("a guest who left"), |m| m.name.clone()))
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

    #[allow(clippy::too_many_lines, reason = "the message table of the host")]
    fn on_message(&mut self, pid: ParticipantId, msg: Message, ed: &mut Editor) {
        let now = self.now;
        let len = self.frame_len as u64;
        let Some(m) = self.members.get_mut(&pid) else { return };
        let role = m.role;
        if let Err(v) = check_message(&self.cfg.limits, &msg) {
            let seq = if let Message::Submit(s) = &msg { s.client_seq } else { 0 };
            self.violation(pid, &v, seq);
            if let Message::AssetChunk(c) = &msg {
                self.ack_upload(pid, &c.hash, 0, UploadStatus::Refused { reason: describe(&v) });
            }
            return;
        }
        match msg {
            Message::Submit(s) => {
                if !m.ops_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "modifications" }, s.client_seq);
                }
                if role == Role::View {
                    return self.send_to(pid, &Message::Rejected { client_seq: s.client_seq, reason: RejectReason::ReadOnly });
                }
                m.confirmed = true;
                self.on_submit(pid, s, ed);
            }
            Message::Undo | Message::Redo => {
                if !m.ops_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "modifications" }, 0);
                }
                m.confirmed = true;
                if role == Role::View {
                    return self.send_to(pid, &Message::Rejected { client_seq: 0, reason: RejectReason::ReadOnly });
                }
                self.on_undo(pid, matches!(msg, Message::Redo), ed);
            }
            Message::Transient { object, state, .. } => {
                if !m.transient_rl.allow(now) || !m.eph_bytes.allow_n(now, len) {
                    return self.violation(pid, &Violation::RateExceeded { what: "ephemeral messages" }, 0);
                }
                if role == Role::Edit {
                    self.events.push(HostEvent::Transient { id: pid, object: object.clone(), state: state.clone() });
                    self.broadcast(&Message::Transient { object, state, author: Some(pid) }, Some(pid));
                }
            }
            Message::StrokePoints { stroke, points, .. } => {
                if !m.transient_rl.allow(now) || !m.eph_bytes.allow_n(now, len) {
                    return self.violation(pid, &Violation::RateExceeded { what: "ephemeral messages" }, 0);
                }
                if role == Role::Edit {
                    self.events.push(HostEvent::Stroke { id: pid, stroke: stroke.clone(), points: points.clone() });
                    self.broadcast(&Message::StrokePoints { stroke, points, author: Some(pid) }, Some(pid));
                }
            }
            Message::Presence(mut p) => {
                if !m.transient_rl.allow(now) || !m.eph_bytes.allow_n(now, len) {
                    return self.violation(pid, &Violation::RateExceeded { what: "ephemeral messages" }, 0);
                }
                p.participant = Some(pid);
                m.presence = Some(p.clone());
                self.events.push(HostEvent::Presence { id: pid, presence: Box::new(p.clone()) });
                // With many people each cursor is relayed less often (the latest position always gets there): the work of the host
                // and the traffic of every guest would otherwise grow with the square of the number of participants.
                if now.saturating_sub(m.presence_at) >= self.presence_interval_ms {
                    m.presence_at = now;
                    m.held_presence = None;
                    self.broadcast(&Message::Presence(p), Some(pid));
                } else {
                    m.held_presence = Some(p);
                }
            }
            Message::Resync { last_seq } => {
                // one of the few requests that makes the host send a lot: its own budget, for readers as well
                if !m.sync_rl.allow(now) {
                    return self.violation(pid, &Violation::RateExceeded { what: "resynchronizations" }, 0);
                }
                m.confirmed = true;
                let Some(i) = self.members.get(&pid).and_then(|m| m.conn).and_then(|c| self.conn_index(c)) else { return };
                match self.seqr.applied_since(last_seq) {
                    Some(v) => {
                        for a in v {
                            if !self.send(i, &Message::Applied(a)) {
                                break;
                            }
                        }
                    }
                    None => self.queue_snapshot(i),
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
        // locks are the host's: a guest cannot touch a locked object or layer, nor lock or unlock anything
        if let Some(why) = lock_conflict(&ed.doc, &s.ops) {
            return self.violation(pid, &Violation::Locked(why), client_seq);
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
        let queued: usize = self.conns[i].out.iter().map(|p| p.raw).sum();
        if queued + data.len() > MAX_QUEUED_ASSET_BYTES {
            return; // the guest asked for more than it can take: it asks again later
        }
        let size = self.cfg.limits.max_asset_chunk_bytes.max(1);
        let n = data.len().div_ceil(size).max(1);
        let pieces = (0..n).map(|idx| Piece { hash: hash.to_owned(), idx, raw: size.min(data.len().saturating_sub(idx * size)) });
        self.conns[i].out.extend(pieces);
    }

    /// An image uploaded by a guest, chunk by chunk, before the modification that uses it.
    #[allow(clippy::needless_pass_by_value, reason = "owned: the chunk is a message taken out of the frame")]
    fn on_chunk(&mut self, pid: ParticipantId, c: AssetChunk) {
        let limits = self.cfg.limits.clone();
        if !self.members.contains_key(&pid) {
            return;
        }
        if !is_hash(&c.hash) {
            return self.violation(pid, &Violation::Malformed("image identifier is not a SHA-256 hash"), 0);
        }
        // already on the board or uploaded by somebody: nothing to transfer, nothing to charge
        if self.assets.by_hash.contains_key(&c.hash) || self.uploaded.iter().any(|u| u.hash == c.hash) {
            return self.ack_upload(pid, &c.hash, 0, UploadStatus::Stored);
        }
        let Some(m) = self.members.get_mut(&pid) else { return };
        if !m.uploads.contains_key(&c.hash) && m.uploads.len() >= MAX_UPLOADS_PER_GUEST {
            return; // too many uploads at once: the guest starts this one again later
        }
        let buf = m.uploads.entry(c.hash.clone()).or_default();
        let have = buf.len() as u64;
        if c.offset != have {
            // a chunk that arrived twice or after a gap: the guest goes on from what is held
            return self.ack_upload(pid, &c.hash, have, UploadStatus::Receiving);
        }
        buf.extend_from_slice(&c.data);
        let held = buf.len() as u64;
        if held > limits.max_image_bytes {
            m.uploads.remove(&c.hash);
            let v = Violation::ImageTooLarge { size: held, max: limits.max_image_bytes };
            return self.refuse_upload(pid, &c.hash, &v);
        }
        if !c.last {
            return self.ack_upload(pid, &c.hash, held, UploadStatus::Receiving);
        }
        let data = m.uploads.remove(&c.hash).unwrap_or_default();
        if hash_hex(&data) != c.hash {
            return self.refuse_upload(pid, &c.hash, &Violation::Malformed("image does not match its hash"));
        }
        let Some(mime) = sniff_mime(&data) else {
            return self.refuse_upload(pid, &c.hash, &Violation::Malformed("not an image (png, jpeg, gif, webp or svg)"));
        };
        if let Err(why) = check_guest_upload(&data, mime) {
            return self.refuse_upload(pid, &c.hash, &Violation::Malformed(why));
        }
        // the quota follows the transport identity (a guest that leaves and comes back keeps what it used), and the host has a total
        let size = data.len() as u64;
        if self.uploaded_total.saturating_add(size) > self.cfg.total_asset_quota_bytes {
            let v = Violation::QuotaExceeded { used: self.uploaded_total, size, max: self.cfg.total_asset_quota_bytes };
            return self.refuse_upload(pid, &c.hash, &v);
        }
        if let Err(v) = self.quotas.entry(m.peer.clone()).or_default().charge(&limits, size) {
            return self.refuse_upload(pid, &c.hash, &v);
        }
        self.uploaded_total += size;
        self.uploaded.push_back(Uploaded { hash: c.hash.clone(), mime, data: Arc::new(data) });
        let mut pool_bytes: usize = self.uploaded.iter().map(|u| u.data.len()).sum();
        while self.uploaded.len() > UPLOAD_POOL || pool_bytes > UPLOAD_POOL_BYTES {
            let Some(old) = self.uploaded.pop_front() else { break };
            pool_bytes -= old.data.len();
        }
        self.ack_upload(pid, &c.hash, held, UploadStatus::Stored);
    }

    fn ack_upload(&mut self, pid: ParticipantId, hash: &str, have: u64, status: UploadStatus) {
        self.send_to(pid, &Message::AssetAck { hash: hash.to_owned(), have, status });
    }

    /// The upload ends here: the guest is told why (it cancels its modification), then the violation counts like any other.
    fn refuse_upload(&mut self, pid: ParticipantId, hash: &str, v: &Violation) {
        // the warning first: the guest shows the last thing it was told, which is the precise one
        self.violation(pid, v, 0);
        self.ack_upload(pid, hash, 0, UploadStatus::Refused { reason: describe(v) });
    }

    /* ------------------------------------------------------------------------------------------------------ upkeep */

    fn housekeeping(&mut self) {
        let now = self.now;
        let (idle, hold) = (self.cfg.idle_ms, self.cfg.place_hold_ms);
        // a guest that does not read: its transport queue holds a lot and has not shrunk for a long time
        let mut stalled: Vec<usize> = Vec::new();
        for (i, c) in self.conns.iter_mut().enumerate() {
            if !matches!(c.state, ConnState::Member(_)) {
                continue;
            }
            let q = c.conn.queued_bytes();
            if q < STALL_MIN_BYTES {
                c.queue_floor = usize::MAX;
                c.queue_since = now;
            } else if q < c.queue_floor {
                c.queue_floor = q;
                c.queue_since = now;
            } else if now.saturating_sub(c.queue_since) >= self.cfg.stall_ms {
                stalled.push(i);
            }
        }
        for i in stalled {
            self.fail_conn(i, "the guest does not read what is sent to it".to_owned());
        }
        // a request nobody answered expires, even if the guest keeps sending `Ping`s
        let stale: Vec<(u64, u64)> = self.requests.iter().filter(|r| now.saturating_sub(r.at) >= self.cfg.pending_ms).map(|r| (r.id, r.conn)).collect();
        for (rid, cid) in stale {
            self.requests.retain(|r| r.id != rid);
            if let Some(i) = self.conn_index(cid) {
                self.refuse(i, RefuseReason::Declined);
            }
        }
        // connections that have said nothing are limited apart: the oldest beyond the ceiling go
        // (a connection gets a moment to send its first message: a burst of real guests is not cut short)
        let mut silent: Vec<(u64, u64)> = self.conns.iter().filter(|c| matches!(c.state, ConnState::New)).map(|c| (c.opened, c.id)).collect();
        silent.sort_unstable();
        let excess = silent.len().saturating_sub(MAX_UNAUTHENTICATED);
        let evict: HashSet<u64> = silent.iter().take(excess).filter(|e| now.saturating_sub(e.0) >= SILENT_GRACE_MS).map(|e| e.1).collect();
        let mut gone: Vec<usize> = Vec::new();
        for (i, c) in self.conns.iter_mut().enumerate() {
            // a closed connection goes at the housekeeping after the one that first saw it closed: a whole `poll` has read in between
            // what it had received before closing
            let open = c.conn.is_open();
            let closed = !open && c.closed_seen;
            if !open {
                c.closed_seen = true;
            }
            let dead = match c.state {
                ConnState::Closing { until } => now >= until,
                ConnState::New => closed || now.saturating_sub(c.opened) > HANDSHAKE_MS || evict.contains(&c.id),
                ConnState::Pending(_) | ConnState::Member(_) => closed || now.saturating_sub(c.last_rx) > idle,
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
