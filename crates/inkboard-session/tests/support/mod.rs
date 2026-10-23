//! Test harness: a minimal host (a `Sequencer` behind encrypted connections), guests, and a simulated network with a virtual clock.

use inkboard_core::geom::{Frame, Pt};
use inkboard_core::model::{Document, Id, Object, Page};
use inkboard_core::style::Style;
use inkboard_core::{BoxGeom, Label};
use inkboard_session::codec::read_hello;
use inkboard_session::crypto::{FrameCipher, HostKeys, SecureChannel, SeededEntropy, Side, Ticket, join_proof, verify_join_proof};
use inkboard_session::limits::{Limits, check_message};
use inkboard_session::proto::{DocMeta, Message, ParticipantId, RefuseReason, RejectReason, Role, Welcome};
use inkboard_session::sequencer::{CatchUp, Submitted};
use inkboard_session::sim::{NetConfig, SimConn, SimNet, SimTransport};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::{Client, ClientConfig, Error, JsonCodec, Phase, Sequencer, WireOp};
use std::collections::HashSet;

pub const HOST_ID: &str = "host";

struct Peer {
    conn: SimConn,
    chan: Option<SecureChannel>,
    role: Role,
    pid: Option<ParticipantId>,
}

pub struct Host {
    pub seqr: Sequencer,
    pub keys: HostKeys,
    pub limits: Limits,
    transport: SimTransport,
    peers: Vec<Peer>,
    known: HashSet<ParticipantId>,
    next_pid: u32,
    seed: u64,
    pub dropped_frames: u64,
}

impl Host {
    pub fn new(net: &SimNet, doc: Document, log_cap: usize, seed: u64) -> Self {
        Self {
            seqr: Sequencer::new(doc, log_cap),
            keys: HostKeys::generate(&mut SeededEntropy::new(seed)).unwrap(),
            limits: Limits::default(),
            transport: net.transport(HOST_ID),
            peers: Vec::new(),
            known: HashSet::new(),
            next_pid: 1,
            seed,
            dropped_frames: 0,
        }
    }

    pub fn ticket(&self, role: Role) -> Ticket {
        self.keys.ticket(role, HOST_ID, &[]).unwrap()
    }

    /// The host forgets a participant (its place expired).
    pub fn forget(&mut self, p: ParticipantId) {
        self.known.remove(&p);
        self.seqr.forget(p);
    }

    pub fn apply_local(&mut self, ops: Vec<WireOp>) {
        let a = self.seqr.apply_local(ParticipantId::HOST, ops);
        self.broadcast(&Message::Applied(a));
    }

    pub fn step(&mut self) {
        while let Some(conn) = self.transport.accept() {
            self.peers.push(Peer { conn, chan: None, role: Role::View, pid: None });
        }
        for i in 0..self.peers.len() {
            while let Some(frame) = self.peers[i].conn.recv() {
                self.on_frame(i, &frame);
            }
        }
    }

    fn send(&mut self, i: usize, msg: &Message) {
        let p = &mut self.peers[i];
        if let Some(ch) = p.chan.as_mut()
            && let Ok(f) = ch.seal(msg)
        {
            let _ = p.conn.send(&f);
        }
    }

    fn broadcast(&mut self, msg: &Message) {
        for i in 0..self.peers.len() {
            if self.peers[i].pid.is_some() {
                self.send(i, msg);
            }
        }
    }

    fn new_channel(&self, i: usize, cipher: FrameCipher) -> SecureChannel {
        let e = SeededEntropy::new(self.seed.wrapping_add(i as u64 * 7919 + 1));
        SecureChannel::new(cipher, Box::new(JsonCodec), Box::new(e), self.limits.max_message_bytes)
    }

    fn on_frame(&mut self, i: usize, frame: &[u8]) {
        let msg = if self.peers[i].chan.is_none() {
            let Ok((role, cipher, plain)) = self.keys.open_first(frame) else {
                self.dropped_frames += 1;
                return;
            };
            self.peers[i].role = role;
            let ch = self.new_channel(i, cipher);
            self.peers[i].chan = Some(ch);
            match read_hello(&JsonCodec, &plain) {
                Ok(h) => Message::Hello(h),
                Err(Error::IncompatibleProtocol { .. }) => {
                    self.send(i, &Message::Refused { reason: RefuseReason::Version { host_proto: inkboard_session::PROTO_VERSION } });
                    return;
                }
                Err(_) => {
                    self.dropped_frames += 1;
                    return;
                }
            }
        } else {
            match self.peers[i].chan.as_ref().map(|c| c.open(frame)) {
                Some(Ok(m)) => m,
                _ => {
                    self.dropped_frames += 1;
                    return;
                }
            }
        };
        self.on_message(i, msg);
    }

    fn on_message(&mut self, i: usize, msg: Message) {
        match msg {
            Message::Hello(h) => {
                let role = self.peers[i].role;
                if !verify_join_proof(self.keys.key(role), &h.name, h.role_requested, &h.join_proof) {
                    self.send(i, &Message::Refused { reason: RefuseReason::BadProof });
                    return;
                }
                let resumed = h.resume.filter(|r| self.known.contains(&r.participant));
                let pid = match (self.peers[i].pid, &resumed) {
                    (Some(p), _) => p,
                    (None, Some(r)) => r.participant,
                    (None, None) => {
                        self.next_pid += 1;
                        ParticipantId(self.next_pid - 1)
                    }
                };
                self.known.insert(pid);
                self.peers[i].pid = Some(pid);
                let doc = self.seqr.doc();
                let welcome = Welcome {
                    participant: pid,
                    color: (pid.0 as u8, 0, 0),
                    role,
                    doc_meta: DocMeta { name: doc.name.clone(), seq: self.seqr.seq(), pages: doc.pages.len() },
                    host_proto: inkboard_session::PROTO_VERSION,
                    host_name: String::new(),
                    participants: Vec::new(),
                };
                self.send(i, &Message::Welcome(welcome));
                match resumed {
                    Some(r) if self.peers[i].pid == Some(r.participant) => self.send_catch_up(i, r.last_seq),
                    _ => {
                        let s = self.seqr.snapshot(Vec::new());
                        self.send(i, &Message::Snapshot(s));
                    }
                }
            }
            Message::Submit(s) => {
                let (Some(pid), role) = (self.peers[i].pid, self.peers[i].role) else { return };
                let client_seq = s.client_seq;
                if role == Role::View {
                    self.send(i, &Message::Rejected { client_seq, reason: RejectReason::ReadOnly });
                } else if let Err(v) = check_message(&self.limits, &Message::Submit(s.clone())) {
                    self.send(i, &Message::Rejected { client_seq, reason: RejectReason::Invalid { detail: v.to_string() } });
                } else {
                    match self.seqr.submit(pid, s) {
                        Submitted::Applied(a) => self.broadcast(&Message::Applied(a)),
                        Submitted::Duplicate => {
                            let host_seq = self.seqr.seq();
                            self.send(i, &Message::Rejected { client_seq, reason: RejectReason::Duplicate { host_seq } });
                        }
                    }
                }
            }
            Message::Resync { last_seq } if self.peers[i].pid.is_some() => self.send_catch_up(i, last_seq),
            _ => {}
        }
    }

    fn send_catch_up(&mut self, i: usize, last_seq: u64) {
        match self.seqr.catch_up(last_seq, Vec::new()) {
            CatchUp::Applied(v) => v.into_iter().for_each(|a| self.send(i, &Message::Applied(a))),
            CatchUp::Snapshot(s) => self.send(i, &Message::Snapshot(s)),
        }
    }
}

pub struct Guest {
    pub client: Client,
    pub conn: Option<SimConn>,
    chan: SecureChannel,
    transport: SimTransport,
}

impl Guest {
    pub fn new(net: &SimNet, name: &str, ticket: &Ticket, seed: u64) -> Self {
        let mut cfg = ClientConfig::new(name, ticket.role, join_proof(&ticket.key, name, ticket.role));
        cfg.resend_after_ms = 200;
        cfg.sync_interval_ms = 500;
        let chan = SecureChannel::new(FrameCipher::new(&ticket.key, Side::Guest), Box::new(JsonCodec), Box::new(SeededEntropy::new(seed)), 1 << 20);
        Self { client: Client::new(cfg), conn: None, chan, transport: net.transport(&format!("guest-{name}")) }
    }

    pub fn connect(&mut self, now: u64) {
        self.conn = Some(self.transport.connect(HOST_ID).unwrap());
        self.client.connect(now);
    }

    pub fn drop_connection(&mut self) {
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.conn = None;
        self.client.connection_lost();
    }

    pub fn pump(&mut self, now: u64) {
        if let Some(conn) = self.conn.as_mut() {
            while let Some(frame) = conn.recv() {
                if let Ok(msg) = self.chan.open(&frame) {
                    let _ = self.client.handle(msg, now);
                }
            }
        }
        self.client.tick(now);
        for m in self.client.take_outbox() {
            if let (Some(conn), Ok(f)) = (self.conn.as_mut(), self.chan.seal(&m)) {
                let _ = conn.send(&f);
            }
        }
    }
}

pub struct World {
    pub net: SimNet,
    pub host: Host,
    pub guests: Vec<Guest>,
    counter: u64,
}

impl World {
    pub fn new(seed: u64, n: usize, cfg: NetConfig, log_cap: usize) -> Self {
        let net = SimNet::new(seed, cfg);
        let mut doc = Document::new("board");
        doc.pages[0].objects.push(boxed("seed-1", 0.0, 0.0));
        doc.pages[0].objects.push(boxed("seed-2", 50.0, 50.0));
        let host = Host::new(&net, doc, log_cap, seed ^ 0xA5A5);
        let ticket = host.ticket(Role::Edit);
        let guests = (0..n).map(|i| Guest::new(&net, &format!("g{i}"), &ticket, seed.wrapping_add(1 + i as u64))).collect();
        Self { net, host, guests, counter: 0 }
    }

    pub fn connect_all(&mut self) {
        let now = self.net.now();
        self.guests.iter_mut().for_each(|g| g.connect(now));
    }

    /// One round: everybody handles what arrived and sends, then the virtual clock moves by `dt` milliseconds.
    pub fn round(&mut self, dt: u64) {
        let now = self.net.now();
        self.host.step();
        for g in &mut self.guests {
            g.pump(now);
        }
        self.host.step();
        self.net.advance(dt);
    }

    pub fn rounds(&mut self, n: usize, dt: u64) {
        for _ in 0..n {
            self.round(dt);
        }
    }

    pub fn is_idle(&self) -> bool {
        self.net.pending_frames() == 0
            && self
                .guests
                .iter()
                .all(|g| g.conn.is_some() && g.client.phase() == Phase::Live && g.client.pending_len() == 0 && g.client.last_seq() == self.host.seqr.seq())
    }

    /// State of everybody, for failure messages.
    pub fn describe(&self) -> String {
        let mut s = format!("host seq {}, frames pending {}", self.host.seqr.seq(), self.net.pending_frames());
        for (i, g) in self.guests.iter().enumerate() {
            s += &format!("\n guest {i}: {:?} pending {} last_seq {} conn {}", g.client.phase(), g.client.pending_len(), g.client.last_seq(), g.conn.is_some());
        }
        s
    }

    /// Runs rounds until the system is quiescent (bounded by a number of rounds, never by a duration). Returns whether it settled.
    pub fn settle(&mut self, max_rounds: usize) -> bool {
        for _ in 0..max_rounds {
            self.round(50);
            if self.is_idle() {
                return true;
            }
        }
        false
    }

    pub fn fresh_id(&mut self, prefix: &str) -> Id {
        self.counter += 1;
        Id(format!("{prefix}{}", self.counter))
    }

    /// A random edit made from the view `doc` (or the host's document): `(code, a, b)` are the random choices.
    pub fn random_edit(&mut self, doc: &Document, code: u8, a: u16, b: u16) -> Vec<WireOp> {
        let page = &doc.pages[0];
        let n = page.objects.len();
        let pick = |k: u16| page.objects.get(usize::from(k) % n.max(1));
        let (x, y) = (f64::from(a % 400), f64::from(b % 400));
        match code % 10 {
            0..=2 => vec![WireOp::Put { page: page.id.clone(), object: Box::new(boxed(&self.fresh_id("o").0, x, y)) }],
            3 => match pick(a) {
                Some(o) => {
                    let mut o = o.clone();
                    o.translate(Pt::new(x, y));
                    vec![WireOp::Put { page: page.id.clone(), object: Box::new(o) }]
                }
                None => vec![],
            },
            4 => {
                let ids: Vec<Id> = [pick(a), pick(b)].into_iter().flatten().map(|o| o.id.clone()).collect();
                vec![WireOp::Delete { page: page.id.clone(), ids }]
            }
            5 => {
                let mut ids: Vec<Id> = page.objects.iter().map(|o| o.id.clone()).collect();
                if ids.len() > 1 {
                    let (i, j) = (usize::from(a) % ids.len(), usize::from(b) % ids.len());
                    ids.swap(i, j);
                    let k = usize::from(a) % ids.len();
                    ids.rotate_left(k);
                }
                vec![WireOp::Order { page: page.id.clone(), ids }]
            }
            6 => {
                let pen = Object::new_pen(vec![Pt::new(x, y), Pt::new(y, x), Pt::new(x + 5.0, y + 9.0)], a.is_multiple_of(2), Style::default());
                let mut pen = pen;
                pen.id = self.fresh_id("p");
                vec![WireOp::Put { page: page.id.clone(), object: Box::new(pen) }]
            }
            7 => vec![WireOp::RenamePage { page: page.id.clone(), name: format!("page {a}") }, WireOp::SetName { name: format!("board {b}") }],
            8 => {
                if a.is_multiple_of(2) || doc.pages.len() < 2 {
                    let mut p = Page::new(&format!("extra {b}"));
                    p.id = self.fresh_id("pg");
                    vec![WireOp::AddPage { index: usize::from(b) % 4, page: Box::new(p) }]
                } else {
                    let victim = doc.pages[usize::from(b) % doc.pages.len()].id.clone();
                    vec![WireOp::RemovePage { page: victim }]
                }
            }
            _ => {
                // a composite edit: a new object and an explicit stacking position
                let o = boxed(&self.fresh_id("c").0, x, y);
                let mut ids: Vec<Id> = page.objects.iter().map(|o| o.id.clone()).collect();
                ids.insert(usize::from(a) % (ids.len() + 1), o.id.clone());
                vec![WireOp::Put { page: page.id.clone(), object: Box::new(o) }, WireOp::Order { page: page.id.clone(), ids }]
            }
        }
    }
}

pub fn boxed(id: &str, x: f64, y: f64) -> Object {
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(x, y, 80.0, 40.0), Style::default(), Label::default());
    o.id = Id(id.to_owned());
    o
}
