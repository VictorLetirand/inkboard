//! A simulated guest: the REAL `inkboard_session::Client` and secure channel on top of any connection (iroh, or the simulated network),
//! driven by a clock. This file is the protocol side (connecting, receiving, measuring, reconnecting, fetching and uploading images);
//! `activity` is what the person does (cursor, strokes, moves, texts, pastes...).

use crate::link::{Clock, Dialer, Impaired, LinkCounters, LinkSpec};
use crate::profile::Profile;
use crate::report::GuestReport;
use crate::rng::Rng;
use inkboard_core::geom::{Pt, Rect};
use inkboard_core::model::{Id, Kind};
use inkboard_session::assets::{AssetAssembler, hash_hex};
use inkboard_session::crypto::{Entropy, FrameCipher, OsEntropy, SecureChannel, SeededEntropy, Side, Ticket, join_proof};
use inkboard_session::proto::{AssetChunk, AssetInfo, Message, ParticipantId, Presence, Role};
use inkboard_session::transport::Connection;
use inkboard_session::{Client, ClientConfig, ClientEvent, JsonCodec, Phase};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

const MAX_FRAME: usize = 16 << 20;
/// Images fetched at once after the snapshot (the real client does the same).
const IMAGES_IN_FLIGHT: usize = 4;
/// Chunks of an upload (size and pace of the real client).
pub(crate) const UPLOAD_CHUNK: usize = 128 << 10;
pub(crate) const UPLOAD_WINDOW: u64 = 4 * UPLOAD_CHUNK as u64;
pub(crate) const UPLOAD_CHUNKS_PER_SEC: u64 = 30;
/// A quiet connection is kept alive with a ping this often.
const KEEPALIVE_MS: u64 = 10_000;

/// What every guest of a process shares.
#[allow(missing_debug_implementations)]
pub struct GuestEnv {
    pub clock: Arc<dyn Clock>,
    pub profile: Profile,
    /// Identifies the machine in the markers: latencies are only computed between guests of the same machine (the same clock).
    pub machine: u16,
    pub edit_ticket: Ticket,
    pub view_ticket: Option<Ticket>,
    /// Simulated network and seeded entropy: nothing depends on the machine.
    pub deterministic: bool,
    pub seed: u64,
}

#[derive(Clone, Debug)]
pub struct GuestPlan {
    pub index: usize,
    pub name: String,
    pub edit: bool,
    pub link: LinkSpec,
    /// Absolute time (clock ms) from which this guest reads nothing any more.
    pub stuck_at_ms: Option<u64>,
}

enum Stage {
    /// Not started yet.
    Idle,
    Dialing {
        rx: Receiver<Result<Box<dyn Connection + Send>, String>>,
        since: u64,
    },
    Live,
    /// Waiting to dial again (after a failure or a drop on purpose).
    Backoff {
        until: u64,
    },
    /// Left on purpose.
    Left,
    /// Refused, removed, host gone, or gave up.
    Dead,
}

/// An image being uploaded.
pub(crate) struct Upload {
    pub hash: String,
    pub data: Arc<Vec<u8>>,
    pub next: u64,
    pub acked: u64,
    pub last_ack_ms: u64,
    pub stored: bool,
    pub page: Id,
    pub frame_at: Pt,
    pub chunks_this_second: (u64, u64),
}

/// A fetch of an image of the board.
struct Fetch {
    asm: AssetAssembler,
}

#[allow(missing_debug_implementations)]
pub struct SimGuest {
    pub(crate) env: Arc<GuestEnv>,
    pub(crate) plan: GuestPlan,
    pub(crate) rng: Rng,
    pub(crate) client: Client,
    chan: SecureChannel,
    dialer: Arc<Mutex<Box<dyn Dialer>>>,
    conn: Option<Impaired>,
    counters: Option<Arc<LinkCounters>>,
    stage: Stage,
    pub rep: GuestReport,
    started_ms: u64,
    hello_ms: Option<u64>,
    lost_ms: Option<u64>,
    resumed_snapshot: bool,
    snapshot_seen: bool,
    snapshot_done_ms: Option<u64>,
    manifest: Vec<AssetInfo>,
    wanted: VecDeque<AssetInfo>,
    fetching: HashMap<String, Fetch>,
    pub(crate) known_images: HashSet<String>,
    icon_ids: Vec<Id>,
    images_total: u64,
    images_got: u64,
    seen_markers: HashSet<String>,
    pub(crate) sent_at: HashMap<u64, u64>,
    last_sent_ms: u64,
    pub(crate) quiet: bool,
    // activity state (see `activity`)
    pub(crate) act: crate::activity::State,
    pub(crate) upload: Option<Upload>,
}

pub(crate) fn marker(machine: u16, index: usize, n: u64, us: u64) -> Id {
    Id(format!("m{machine}-{index}-{n}-{us:x}"))
}

/// Machine and microsecond epoch read back from a marker identifier.
pub fn parse_marker(id: &str) -> Option<(u16, u64)> {
    let rest = id.strip_prefix('m')?;
    let (machine, _) = rest.split_once('-')?;
    let us = u64::from_str_radix(rest.rsplit('-').next()?, 16).ok()?;
    Some((machine.parse().ok()?, us))
}

impl SimGuest {
    pub fn new(env: Arc<GuestEnv>, plan: GuestPlan, dialer: Box<dyn Dialer>) -> Self {
        let ticket = if plan.edit { &env.edit_ticket } else { env.view_ticket.as_ref().unwrap_or(&env.edit_ticket) };
        let role = ticket.role;
        let seed = env.seed ^ ((plan.index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut ccfg = ClientConfig::new(&plan.name, role, join_proof(&ticket.key, &plan.name, role));
        ccfg.resend_after_ms = 2_000;
        let entropy: Box<dyn Entropy> = if env.deterministic { Box::new(SeededEntropy::new(seed)) } else { Box::new(OsEntropy) };
        let chan = SecureChannel::new(FrameCipher::new(&ticket.key, Side::Guest), Box::new(JsonCodec), entropy, MAX_FRAME);
        let rep = GuestReport {
            index: plan.index,
            name: plan.name.clone(),
            edit: role == Role::Edit,
            link: plan.link,
            stuck: plan.stuck_at_ms.is_some(),
            ..Default::default()
        };
        let mut rng = Rng::new(seed);
        let act = crate::activity::State::new(&mut rng, &env.profile);
        Self {
            env,
            plan,
            rng,
            client: Client::new(ccfg),
            chan,
            dialer: Arc::new(Mutex::new(dialer)),
            conn: None,
            counters: None,
            stage: Stage::Idle,
            rep,
            started_ms: 0,
            hello_ms: None,
            lost_ms: None,
            resumed_snapshot: false,
            snapshot_seen: false,
            snapshot_done_ms: None,
            manifest: Vec::new(),
            wanted: VecDeque::new(),
            fetching: HashMap::new(),
            known_images: HashSet::new(),
            icon_ids: Vec::new(),
            images_total: 0,
            images_got: 0,
            seen_markers: HashSet::new(),
            sent_at: HashMap::new(),
            last_sent_ms: 0,
            quiet: false,
            act,
            upload: None,
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self.stage, Stage::Live) && self.client.phase() == Phase::Live
    }

    pub fn is_over(&self) -> bool {
        matches!(self.stage, Stage::Left | Stage::Dead)
    }

    pub fn participant(&self) -> Option<ParticipantId> {
        self.client.participant()
    }

    /// Stops all new activity (gestures in progress finish); used at the end of a run to let everything settle.
    pub fn set_quiet(&mut self) {
        self.quiet = true;
    }

    /// Works on this page only (the others are for people elsewhere on the board).
    pub fn pin_page(&mut self, page: usize) {
        self.act.pin_page(page);
    }

    /// Works around this point only.
    pub fn pin_area(&mut self, at: Pt) {
        self.act.pin_area(at);
    }

    /// Starts connecting.
    pub fn start(&mut self, now: u64) {
        self.started_ms = now;
        self.begin_dial(now);
    }

    fn begin_dial(&mut self, now: u64) {
        let (tx, rx) = channel();
        let dialer = Arc::clone(&self.dialer);
        if dialer.lock().is_ok_and(|d| d.instant()) {
            let r = dialer.lock().map_err(|_| "dialer poisoned".to_owned()).and_then(|mut d| d.dial());
            let _ = tx.send(r);
            self.stage = Stage::Dialing { rx, since: now };
            return;
        }
        let spawned = std::thread::Builder::new().name("crowd-dial".to_owned()).spawn(move || {
            let r = dialer.lock().map_err(|_| "dialer poisoned".to_owned()).and_then(|mut d| d.dial());
            let _ = tx.send(r);
        });
        if spawned.is_err() {
            self.stage = Stage::Backoff { until: now + 1_000 };
            return;
        }
        self.stage = Stage::Dialing { rx, since: now };
    }

    /// One turn of the guest. Cheap when nothing happens. Returns `true` when something was received or done.
    pub fn step(&mut self, now: u64) -> bool {
        let began = std::time::Instant::now();
        let busy = self.step_inner(now);
        let ms = began.elapsed().as_secs_f64() * 1000.0;
        if ms > self.rep.max_step_ms {
            self.rep.max_step_ms = ms;
        }
        busy
    }

    fn step_inner(&mut self, now: u64) -> bool {
        match &self.stage {
            Stage::Idle | Stage::Left | Stage::Dead => return false,
            Stage::Backoff { until } => {
                if now >= *until {
                    self.begin_dial(now);
                }
                return false;
            }
            Stage::Dialing { .. } => return self.poll_dial(now),
            Stage::Live => {}
        }
        let mut busy = self.receive(now);
        if self.client.phase() == Phase::Closed {
            return busy;
        }
        if self.conn.as_ref().is_some_and(|c| !c.is_open()) {
            self.on_connection_lost(now, "the connection closed");
            return true;
        }
        self.client.tick(now);
        if self.is_live() {
            busy |= crate::activity::act(self, now);
        }
        busy |= self.flush(now);
        if now.saturating_sub(self.last_sent_ms) >= KEEPALIVE_MS {
            self.send_message(&Message::Ping { nonce: now }, now);
        }
        busy
    }

    fn poll_dial(&mut self, now: u64) -> bool {
        let Stage::Dialing { rx, since } = &self.stage else { return false };
        let since = *since;
        match rx.try_recv() {
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.dial_failed(now, "the dial thread ended".to_owned());
                true
            }
            Ok(Err(e)) => {
                self.dial_failed(now, e);
                true
            }
            Ok(Ok(conn)) => {
                if self.rep.dial_ms.is_none() {
                    self.rep.dial_ms = Some(now.saturating_sub(since) as f64);
                }
                let seed = self.env.seed ^ self.plan.index as u64 ^ 0xC0FFEE;
                let imp = Impaired::new(conn, self.plan.link, seed, Arc::clone(&self.env.clock));
                if let Some(at) = self.plan.stuck_at_ms
                    && now >= at
                {
                    imp.frozen.store(true, Ordering::Relaxed);
                }
                self.counters = Some(Arc::clone(&imp.counters));
                self.conn = Some(imp);
                self.stage = Stage::Live;
                self.client.connect(now);
                self.hello_ms = Some(now);
                self.flush(now);
                true
            }
        }
    }

    fn dial_failed(&mut self, now: u64, why: String) {
        self.rep.dial_failures += 1;
        if self.rep.dial_errors.len() < 3 {
            self.rep.dial_errors.push(why);
        }
        // the first connection gives up after a few tries; a reconnection keeps trying
        if self.rep.joins == 0 && self.rep.dial_failures >= 3 {
            self.stage = Stage::Dead;
            self.rep.end = "dial failed".to_owned();
        } else {
            self.stage = Stage::Backoff { until: now + 2_000 };
        }
    }

    /// Reads everything that arrived and handles it.
    fn receive(&mut self, now: u64) -> bool {
        if let Some(at) = self.plan.stuck_at_ms
            && now >= at
            && let Some(c) = &self.conn
        {
            c.frozen.store(true, Ordering::Relaxed);
        }
        let mut frames = Vec::new();
        if let Some(c) = self.conn.as_mut() {
            while let Some(f) = c.recv() {
                frames.push(f);
                if frames.len() >= 2048 {
                    break;
                }
            }
        }
        if frames.is_empty() {
            return false;
        }
        self.rep.max_backlog = self.rep.max_backlog.max(frames.len() as u64);
        let began = std::time::Instant::now();
        for f in frames {
            match self.chan.open(&f) {
                Ok(msg) => self.on_message(msg, f.len(), now),
                Err(_) => self.rep.decode_errors += 1,
            }
            if self.client.phase() == Phase::Closed {
                break;
            }
        }
        self.rep.handle_ms_total += began.elapsed().as_secs_f64() * 1000.0;
        self.drain_events(now);
        true
    }

    fn on_message(&mut self, msg: Message, frame_len: usize, now: u64) {
        let epoch = self.env.clock.epoch_us();
        match &msg {
            Message::Snapshot(s) => {
                self.manifest.clone_from(&s.assets);
                self.rep.snapshot_kib += frame_len as f64 / 1024.0;
            }
            Message::SnapshotPart(p) => {
                if let Some(h) = &p.head {
                    self.manifest.clone_from(&h.assets);
                }
                self.rep.snapshot_kib += frame_len as f64 / 1024.0;
            }
            Message::Applied(a) => {
                if Some(a.author) == self.client.participant() {
                    if let Some(cs) = a.client_seq
                        && let Some(t) = self.sent_at.remove(&cs)
                    {
                        self.rep.rtt_ms.add(epoch.saturating_sub(t) as f64 / 1000.0);
                        self.rep.edits_confirmed += 1;
                    }
                } else {
                    for op in &a.ops {
                        if let inkboard_session::WireOp::Put { object, .. } = op {
                            if let Some((m, us)) = parse_marker(object.id.as_str())
                                && m == self.env.machine
                                && self.seen_markers.insert(object.id.0.clone())
                            {
                                self.rep.visible_ms.add(epoch.saturating_sub(us) as f64 / 1000.0);
                                if self.seen_markers.len() > 50_000 {
                                    self.seen_markers.clear();
                                }
                            }
                            if let Kind::Image(i) = &object.kind {
                                self.learn_image(i.asset.as_str());
                            }
                        }
                    }
                }
            }
            Message::Presence(p) => {
                if let Some(t) = &p.tool
                    && let Some(rest) = t.strip_prefix('L')
                    && let Some((m, us)) = rest.split_once(':')
                    && m.parse::<u16>().ok() == Some(self.env.machine)
                    && let Ok(us) = u64::from_str_radix(us, 16)
                {
                    self.rep.cursor_ms.add(epoch.saturating_sub(us) as f64 / 1000.0);
                }
            }
            Message::AssetChunk(c) => self.on_chunk(c, now),
            Message::Assets { list } => {
                for a in list {
                    self.learn_image(&a.hash);
                }
            }
            Message::AssetAck { hash, have, status } => crate::activity::on_ack(self, hash, *have, status, now),
            _ => {}
        }
        if let Err(e) = self.client.handle(msg, now) {
            self.rep.closed = Some(e.to_string());
        }
    }

    /// An image used by a modification of somebody else: fetched like the real client does (the identifier is the hash).
    fn learn_image(&mut self, hash: &str) {
        if self.env.profile.fetch_images && hash.len() == 64 && self.known_images.insert(hash.to_owned()) {
            self.wanted.push_back(AssetInfo { id: Id(hash.to_owned()), hash: hash.to_owned(), size: 0, mime: String::new() });
        }
    }

    fn on_chunk(&mut self, c: &AssetChunk, now: u64) {
        let Some(f) = self.fetching.get_mut(&c.hash) else { return };
        match f.asm.push(c) {
            Ok(Some(bytes)) => {
                self.images_got += bytes.len() as u64;
                self.fetching.remove(&c.hash);
                if self.fetching.is_empty() && self.wanted.is_empty() && self.snapshot_done_ms.is_some() && self.rep.images_ms.is_none() {
                    self.rep.images_ms = self.hello_ms.map(|h| now.saturating_sub(h) as f64);
                    self.rep.images_mib = self.images_got as f64 / (1024.0 * 1024.0);
                }
            }
            Ok(None) => {}
            Err(_) => {
                self.fetching.remove(&c.hash); // asked again later
                self.known_images.remove(&c.hash);
            }
        }
    }

    fn start_fetches(&mut self) {
        while self.fetching.len() < IMAGES_IN_FLIGHT {
            let Some(info) = self.wanted.pop_front() else { break };
            if self.fetching.contains_key(&info.hash) {
                continue;
            }
            let asm = if info.size > 0 {
                AssetAssembler::new(&info.hash, info.size, 16 << 20).ok()
            } else {
                Some(AssetAssembler::with_unknown_size(&info.hash, 16 << 20))
            };
            let Some(asm) = asm else { continue };
            self.images_total += info.size;
            self.fetching.insert(info.hash.clone(), Fetch { asm });
            self.client.send_ephemeral(Message::AssetRequest { hash: info.hash });
        }
    }

    fn drain_events(&mut self, now: u64) {
        for ev in self.client.take_events() {
            match ev {
                ClientEvent::Joined { .. } => {
                    self.rep.joins += 1;
                    let since = self.hello_ms.unwrap_or(now);
                    if self.rep.welcome_ms.is_none() {
                        self.rep.welcome_ms = Some(now.saturating_sub(since) as f64);
                    }
                    if let Some(lost) = self.lost_ms.take() {
                        self.rep.resume_ms.push(now.saturating_sub(lost) as f64);
                        self.resumed_snapshot = false;
                    }
                }
                ClientEvent::SnapshotLoaded => {
                    if self.snapshot_done_ms.is_none() {
                        self.snapshot_done_ms = Some(now);
                        self.rep.snapshot_ms = self.hello_ms.map(|h| now.saturating_sub(h) as f64);
                        self.icon_ids = self.manifest.iter().filter(|a| a.mime == "image/svg+xml").map(|a| a.id.clone()).collect();
                        if self.env.profile.fetch_images {
                            for a in std::mem::take(&mut self.manifest) {
                                self.known_images.insert(a.hash.clone());
                                self.wanted.push_back(a);
                            }
                        }
                        if self.wanted.is_empty() && self.rep.images_ms.is_none() {
                            self.rep.images_ms = self.rep.snapshot_ms;
                        }
                    } else if self.lost_ms.is_some() || self.rep.joins > 1 {
                        self.rep.resume_snapshots += 1;
                    }
                    self.snapshot_seen = true;
                }
                ClientEvent::Rejected { reason, .. } => {
                    let key = match &reason {
                        inkboard_session::proto::RejectReason::ReadOnly => "read_only".to_owned(),
                        inkboard_session::proto::RejectReason::Invalid { detail } => format!("invalid: {}", detail.chars().take(60).collect::<String>()),
                        inkboard_session::proto::RejectReason::Duplicate { .. } => "duplicate".to_owned(),
                        inkboard_session::proto::RejectReason::RateLimited => "rate_limited".to_owned(),
                        inkboard_session::proto::RejectReason::Quota => "quota".to_owned(),
                    };
                    *self.rep.rejected.entry(key).or_insert(0) += 1;
                }
                ClientEvent::Superseded(v) => self.rep.superseded += v.len() as u64,
                ClientEvent::PendingDiscarded(n) => self.rep.pending_discarded += n as u64,
                ClientEvent::Refused(r) => {
                    self.rep.closed = Some(format!("refused: {}", r.message()));
                }
                ClientEvent::Closed(reason) => {
                    self.rep.closed = Some(format!("closed: {reason:?}"));
                }
                ClientEvent::RoleChanged(_) | ClientEvent::Relay(_) => {}
            }
        }
        if self.client.phase() == Phase::Closed {
            self.stage = Stage::Dead;
            self.rep.end = self.rep.closed.clone().unwrap_or_else(|| "closed".to_owned());
            if let Some(c) = self.conn.as_mut() {
                c.close();
            }
        } else if matches!(self.stage, Stage::Live) {
            self.start_fetches();
        }
    }

    fn on_connection_lost(&mut self, now: u64, why: &str) {
        self.rep.drops += 1;
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.absorb_counters();
        self.conn = None;
        self.client.connection_lost();
        self.fetching.clear(); // asked again after the reconnection
        self.lost_ms.get_or_insert(now);
        let _ = why;
        self.act.on_disconnect();
        self.upload = None;
        self.stage = Stage::Backoff { until: now + 500 };
    }

    /// Cuts the connection on purpose, without a goodbye (a laptop lid, a network change): the guest comes back after `pause_ms`.
    pub fn drop_connection(&mut self, now: u64, pause_ms: u64) {
        if !matches!(self.stage, Stage::Live) {
            return;
        }
        self.on_connection_lost(now, "dropped on purpose");
        self.stage = Stage::Backoff { until: now + pause_ms };
    }

    /// Leaves for good, with a goodbye.
    pub fn leave(&mut self, now: u64) {
        if self.conn.is_some() {
            self.client.send_ephemeral(Message::Bye { reason: inkboard_session::proto::ByeReason::Left });
            self.flush(now);
            if let Some(c) = self.conn.as_mut() {
                c.close();
            }
        }
        self.absorb_counters();
        self.conn = None;
        self.stage = Stage::Left;
        self.rep.end = "left".to_owned();
    }

    fn absorb_counters(&mut self) {
        if let Some(c) = self.counters.take() {
            self.rep.frames_in += c.frames_in.load(Ordering::Relaxed);
            self.rep.bytes_in += c.bytes_in.load(Ordering::Relaxed);
            self.rep.frames_out += c.frames_out.load(Ordering::Relaxed);
            self.rep.bytes_out += c.bytes_out.load(Ordering::Relaxed);
            self.rep.link_dropped += c.dropped.load(Ordering::Relaxed);
        }
    }

    fn send_message(&mut self, m: &Message, now: u64) -> bool {
        let Some(conn) = self.conn.as_mut() else { return false };
        match self.chan.seal(m) {
            Ok(f) => {
                self.last_sent_ms = now;
                conn.send(&f).is_ok()
            }
            Err(_) => false,
        }
    }

    /// Sends what the client queued.
    pub(crate) fn flush(&mut self, now: u64) -> bool {
        let out = self.client.take_outbox();
        let any = !out.is_empty();
        for m in out {
            self.send_message(&m, now);
        }
        any
    }

    /// Applies an edit through the client and remembers when it was sent (for the round-trip time).
    pub(crate) fn edit(&mut self, ops: Vec<inkboard_session::WireOp>, now: u64) -> Option<u64> {
        let cs = self.client.edit(ops, now)?;
        self.sent_at.insert(cs, self.env.clock.epoch_us());
        if self.sent_at.len() > 2_000 {
            self.sent_at.clear();
        }
        self.rep.edits_sent += 1;
        Some(cs)
    }

    /// Sends an ephemeral message through the client's outbox.
    pub(crate) fn ephemeral(&mut self, m: Message) {
        self.client.send_ephemeral(m);
    }

    pub(crate) fn presence(&mut self, p: Presence) {
        self.rep.presence_sent += 1;
        self.client.send_ephemeral(Message::Presence(p));
    }

    pub(crate) fn raw_chunk(&mut self, c: AssetChunk) {
        self.rep.chunks_sent += 1;
        self.client.send_ephemeral(Message::AssetChunk(c));
    }

    /// A small vector image of the board (an icon) to put somewhere.
    pub(crate) fn manifest_icon(&mut self) -> Option<Id> {
        if self.icon_ids.is_empty() { None } else { Some(self.icon_ids[self.rng.below(self.icon_ids.len())].clone()) }
    }

    pub(crate) fn viewport(center: Pt) -> Rect {
        Rect::new(center.x - 700.0, center.y - 450.0, 1400.0, 900.0)
    }

    /// Final numbers: the digest of what this guest sees, and the last counters.
    pub fn finish(&mut self, now: u64) -> GuestReport {
        let _ = now;
        self.absorb_counters();
        let doc = self.client.doc();
        self.rep.digest = crate::digest::doc_digest(doc);
        self.rep.objects = doc.pages.iter().map(|p| p.objects.len()).sum();
        self.rep.last_seq = self.client.last_seq();
        self.rep.pending = self.client.pending_len();
        if self.rep.end.is_empty() {
            self.rep.end = format!("{:?}", self.client.phase());
        }
        let _ = hash_hex;
        let mut r = std::mem::take(&mut self.rep);
        // keep the (non-serialized) samples with the report
        r.rejected = r.rejected.into_iter().collect::<BTreeMap<_, _>>();
        r
    }
}
