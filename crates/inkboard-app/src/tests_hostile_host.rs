//! Hostile guests against the real `SessionHost` (test of robustness, see `context/mcp-remote/test-robustesse.md`).
//!
//! Attackers are raw peers: they speak the encrypted protocol by hand (valid keys, valid frames, hostile content), alone or at random,
//! next to honest guests. After every attack the host must be alive and its board intact (same document as the guests', no identifier
//! used twice), a refusal must be explicit (a typed answer or a removal), and an honest guest must still converge and edit. Everything
//! runs on the simulated network with a virtual clock and a fixed seed: nothing measures a duration. Memory is measured by a child
//! process (see `memory_stays_bounded_under_a_flood`): the allocation counter is global to a process.

use crate::session_host::{HostAddress, HostConfig, HostEvent, HostTransport, SessionHost};
use inkboard_core::connector::{Connector, End};
use inkboard_core::geom::{Frame, Pt};
use inkboard_core::model::{Document, Id, ImageObj, Kind, Layer, Object, Page, Table};
use inkboard_core::style::Style;
use inkboard_core::{BoxGeom, Editor, Label};
use inkboard_session::assets::hash_hex;
use inkboard_session::crypto::{FrameCipher, Link, SecureChannel, SeededEntropy, SessionKey, Side, Ticket, join_proof};
use inkboard_session::proto::{AssetChunk, Hello, Message, PROTO_VERSION, ParticipantId, Presence, RefuseReason, Role, Submit, UploadStatus};
use inkboard_session::sim::{NetConfig, SimConn, SimNet, SimTransport};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::{Client, ClientConfig, JsonCodec, WireOp};
use peak_alloc::PeakAlloc;
use serde_json::{Value, json};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const HOST_ID: &str = "host";

fn boxed(id: &str, x: f64) -> Object {
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(x, 0.0, 100.0, 60.0), Style::default(), Label::default());
    o.id = id.into();
    o
}

/// What must hold for any document: a page at least, no page or object identifier used twice.
fn doc_problem(doc: &Document) -> Option<String> {
    if doc.pages.is_empty() {
        return Some("no page".into());
    }
    let mut pages = std::collections::HashSet::new();
    for p in &doc.pages {
        if !pages.insert(&p.id) {
            return Some(format!("page {:?} twice", p.id));
        }
        let mut objects = std::collections::HashSet::new();
        if let Some(o) = p.objects.iter().find(|o| !objects.insert(&o.id)) {
            return Some(format!("object {:?} twice on page {:?}", o.id, p.id));
        }
    }
    None
}

/// Same pages and objects in the same order (the `view` of a page is local).
fn same(a: &Document, b: &Document) -> bool {
    a.name == b.name && a.pages.len() == b.pages.len() && a.pages.iter().zip(&b.pages).all(|(x, y)| x.id == y.id && x.name == y.name && x.objects == y.objects)
}

fn board_hash(doc: &Document) -> String {
    let pages: Vec<_> = doc.pages.iter().map(|p| (&p.id, &p.name, &p.layers, &p.objects)).collect();
    hash_hex(&serde_json::to_vec(&(&doc.name, &doc.background, pages)).expect("a document serializes"))
}

/// What the engine does with every object of the board (the host's interface does it on every frame): none of it may panic or loop.
fn exercise_engine(doc: &Document) {
    for page in &doc.pages {
        let index = page.index();
        for o in &page.objects {
            let _ = page.bounds(o);
            let _ = page.hit(o, Pt::new(1.0, 1.0), 4.0);
            if let Kind::Connector(c) = &o.kind {
                let _ = page.connector_path_with(c, &|id| index.get(id).copied());
            }
        }
        let _ = inkboard_core::svg::export_approx(doc, page);
    }
}

/* ------------------------------------------------------------------------------------------------ peers and laboratory */

struct Rng(SeededEntropy);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(SeededEntropy::new(seed))
    }
    fn below(&mut self, n: usize) -> usize {
        (self.0.next_u64() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// An honest guest: a real `Client`.
struct Honest {
    client: Client,
    conn: Option<SimConn>,
    chan: SecureChannel,
    transport: SimTransport,
}

impl Honest {
    fn new(net: &SimNet, name: &str, ticket: &Ticket, seed: u64) -> Self {
        let cfg = ClientConfig::new(name, ticket.role, join_proof(&ticket.key, name, ticket.role));
        // the application's own timers (a faster resynchronization would trip the host's limiter, as it should)
        let chan = SecureChannel::new(FrameCipher::new(&ticket.key, Side::Guest), Box::new(JsonCodec), Box::new(SeededEntropy::new(seed)), 1 << 22);
        Self { client: Client::new(cfg), conn: None, chan, transport: net.transport(&format!("honest-{name}")) }
    }
    fn pump(&mut self, now: u64) {
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

/// A peer that speaks the protocol by hand.
struct Atk {
    conn: SimConn,
    chan: SecureChannel,
    cipher: FrameCipher,
    entropy: SeededEntropy,
    key: SessionKey,
    name: String,
    /// The host told it that it is out (a `Kick`, a `Refused`, a `Bye`): it stops sending.
    out: bool,
}

impl Atk {
    fn new(net: &SimNet, name: &str, ticket: &Ticket) -> Self {
        let conn = net.transport(name).connect(HOST_ID).unwrap();
        let chan = SecureChannel::new(FrameCipher::new(&ticket.key, Side::Guest), Box::new(JsonCodec), Box::new(SeededEntropy::new(99)), 1 << 24);
        Self {
            conn,
            chan,
            cipher: FrameCipher::new(&ticket.key, Side::Guest),
            entropy: SeededEntropy::new(7),
            key: ticket.key.clone(),
            name: name.to_owned(),
            out: false,
        }
    }
    fn send(&mut self, m: &Message) -> bool {
        match self.chan.seal(m) {
            Ok(f) => self.conn.send(&f).is_ok(),
            Err(_) => false,
        }
    }
    /// Plaintext of any shape, correctly encrypted: the host opens it and meets the hostile content.
    fn send_plain(&mut self, plain: &[u8]) -> bool {
        match self.cipher.seal(plain, &mut self.entropy) {
            Ok(f) => self.conn.send(&f).is_ok(),
            Err(_) => false,
        }
    }
    fn send_frame(&mut self, frame: &[u8]) -> bool {
        self.conn.send(frame).is_ok()
    }
    fn hello(&mut self, role: Role) {
        let h =
            Hello { proto: PROTO_VERSION, name: self.name.clone(), join_proof: join_proof(&self.key, &self.name, role), role_requested: role, resume: None };
        self.send(&Message::Hello(h));
    }
    fn received(&mut self) -> Vec<Message> {
        let got: Vec<Message> = std::iter::from_fn(|| self.conn.recv()).filter_map(|f| self.chan.open(&f).ok()).collect();
        if got.iter().any(|m| matches!(m, Message::Kick { .. } | Message::Refused { .. } | Message::Bye { .. })) {
            self.out = true;
        }
        got
    }
}

struct Lab {
    net: SimNet,
    ed: Editor,
    host: SessionHost,
    addr: HostAddress,
    honest: Vec<Honest>,
    seed: u64,
    spawned: usize,
    events: Vec<HostEvent>,
}

impl Lab {
    fn new(seed: u64, tweak: impl FnOnce(&mut HostConfig)) -> Self {
        let net = SimNet::new(seed, NetConfig::ideal());
        let mut ed = Editor::new(Document::new("board"));
        ed.insert(boxed("seed-1", 0.0));
        ed.insert(boxed("seed-2", 120.0));
        let mut cfg = HostConfig { entropy_seed: Some(seed ^ 0xA5A5), ..HostConfig::default() };
        tweak(&mut cfg);
        let host = SessionHost::start(cfg, &ed, Box::new(net.transport(HOST_ID)) as Box<dyn HostTransport>).unwrap();
        let addr = HostAddress { host: HOST_ID.to_owned(), relays: Vec::new() };
        Self { net, ed, host, addr, honest: Vec::new(), seed, spawned: 0, events: Vec::new() }
    }

    fn ticket(&self, role: Role) -> Ticket {
        Link::parse(&self.host.link(role, &self.addr).unwrap()).unwrap().ticket
    }

    fn page(&self) -> Id {
        self.ed.doc.pages[0].id.clone()
    }

    /// One tick of 10 virtual milliseconds. Join requests are accepted (edit if the link allows it).
    fn step(&mut self) {
        self.net.advance(10);
        let now = self.net.now();
        self.host.poll(&mut self.ed, now);
        for p in self.host.pending() {
            self.host.accept(p.request, Role::Edit);
        }
        for g in &mut self.honest {
            g.pump(now);
        }
        // what the interface would react to: only the removals are kept (a host that nobody drains would keep every event)
        self.events.extend(self.host.take_events().into_iter().filter(|e| matches!(e, HostEvent::Excluded { .. })));
    }

    fn rounds(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }

    fn honest_guest(&mut self, name: &str) -> usize {
        let ticket = self.ticket(Role::Edit);
        let mut g = Honest::new(&self.net, name, &ticket, self.seed.wrapping_add(self.honest.len() as u64 + 1));
        g.conn = Some(g.transport.connect(HOST_ID).unwrap());
        g.client.connect(self.net.now());
        self.honest.push(g);
        self.rounds(8);
        self.honest.len() - 1
    }

    /// A hostile peer already admitted (it answers like a guest that holds the document).
    fn attacker(&mut self, role: Role) -> Atk {
        self.spawned += 1;
        let ticket = self.ticket(role);
        let mut a = Atk::new(&self.net, &format!("atk{}", self.spawned), &ticket);
        a.hello(role);
        self.rounds(4);
        a.received();
        a
    }

    fn honest_edit(&mut self, guest: usize, o: Object) {
        let page = self.honest[guest].client.doc().pages[0].id.clone();
        let now = self.net.now();
        let c = &mut self.honest[guest].client;
        let edited = c.edit(vec![WireOp::Put { page, object: Box::new(o) }], now);
        let why: Vec<_> =
            c.take_events().into_iter().filter(|e| matches!(e, inkboard_session::ClientEvent::Closed(_) | inkboard_session::ClientEvent::Refused(_))).collect();
        assert!(
            edited.is_some(),
            "honest edit refused locally: phase {:?} role {:?} seq {} {why:?} objects on the host board {}",
            c.phase(),
            c.role(),
            c.last_seq(),
            self.ed.doc.pages.iter().map(|p| p.objects.len()).sum::<usize>()
        );
    }

    /// The board is intact and the honest guests have it.
    fn assert_healthy(&mut self, what: &str) {
        self.rounds(60);
        assert_eq!(doc_problem(&self.ed.doc), None, "{what}: the host's board is corrupted");
        for (i, g) in self.honest.iter().enumerate() {
            assert!(same(g.client.doc(), &self.ed.doc), "{what}: honest guest {i} does not have the host's board");
        }
        exercise_engine(&self.ed.doc);
    }
}

/* ------------------------------------------------------------------------------------------------------ hostile payloads */

fn seed_messages() -> Vec<Value> {
    let page = Id("p".into());
    let put = |o: Object| WireOp::Put { page: page.clone(), object: Box::new(o) };
    let mut pen = Object::new_pen(vec![Pt::new(0.0, 0.0), Pt::new(5.0, 9.0)], false, Style::default());
    pen.id = "pen".into();
    let mut table = Object::new(Kind::Table(Table::new(Pt::new(1.0, 2.0), 2, 3)), Style::default());
    table.id = "tbl".into();
    let mut conn = Object::new(Kind::Connector(Connector::new(End::bound("conn".into()), End::bound("seed-1".into()))), Style::default());
    conn.id = "conn".into();
    let mut img =
        Object::new(Kind::Image(ImageObj { frame: Frame::new(0.0, 0.0, 10.0, 10.0), asset: Id("0".repeat(64)), latex: None, mono: false }), Style::default());
    img.id = "img".into();
    let ops = vec![
        put(boxed("n0", 3.0)),
        put(pen),
        put(table),
        put(conn),
        put(img),
        WireOp::Delete { page: page.clone(), ids: vec!["seed-1".into()] },
        WireOp::Order { page: page.clone(), ids: vec!["seed-2".into(), "seed-1".into()] },
        WireOp::SetLayers { page: page.clone(), layers: vec![Layer::main()] },
        WireOp::AddPage { index: 1, page: Box::new(Page::new("two")) },
        WireOp::RenamePage { page: page.clone(), name: "x".into() },
        WireOp::RemovePage { page: "zz".into() },
        WireOp::SetName { name: "new".into() },
    ];
    let msgs = [
        Message::Submit(Submit { client_seq: 1, base_seq: 0, ops }),
        Message::AssetChunk(AssetChunk { hash: "0".repeat(64), offset: 0, data: vec![1, 2, 3], last: true }),
        Message::AssetRequest { hash: "0".repeat(64) },
        Message::Resync { last_seq: 0 },
        Message::Transient { object: "seed-1".into(), state: Box::new(boxed("seed-1", 5.0)), author: None },
        Message::StrokePoints { stroke: "s".into(), points: vec![Pt::new(1.0, 2.0)], author: None },
        Message::Presence(Presence {
            participant: None,
            page: page.clone(),
            cursor: Some(Pt::new(1.0, 1.0)),
            selection: vec!["seed-1".into()],
            tool: Some("pen".into()),
            editing: Some("seed-1".into()),
            viewport: None,
        }),
        Message::Undo,
        Message::Redo,
        Message::Follow { participant: Some(ParticipantId(2)) },
        Message::Kick { participant: ParticipantId(1) },
        Message::Bye { reason: inkboard_session::proto::ByeReason::HostLeft },
        Message::Ping { nonce: u64::MAX },
        Message::Pong { nonce: 1 },
    ];
    msgs.iter().map(|m| serde_json::to_value(m).unwrap()).collect()
}

/// The values a hostile peer puts in place of the right ones. The last `HEAVY` are expensive to build and to parse: rarely picked.
const HEAVY: usize = 2;

fn nasty_values() -> Vec<Value> {
    vec![
        Value::Null,
        json!(true),
        json!(-1),
        json!(0),
        json!(u64::MAX),
        json!(i64::MIN),
        json!(1.797_693_134_862_315_7e308),
        json!(5e-324),
        json!(1e7),
        json!(10_000_001.0),
        json!(""),
        json!("a".repeat(70)),
        json!("\u{0}\u{202e}\u{3164}"),
        json!([]),
        json!({}),
        (0..100).fold(json!(0), |acc, _| json!([acc])),
        json!("é".repeat(120_000)),
        Value::Array(vec![json!(0.5); 60_000]),
    ]
}

fn mutate(rng: &mut Rng, v: &mut Value, nasty: &[Value]) {
    let pick = if rng.chance(4) { rng.below(nasty.len()) } else { rng.below(nasty.len() - HEAVY) };
    let with = nasty[pick].clone();
    match v {
        Value::Object(map) if !map.is_empty() && rng.chance(85) => {
            let key = map.keys().nth(rng.below(map.len())).cloned().unwrap_or_default();
            if rng.chance(15) {
                map.remove(&key);
            } else if let Some(c) = map.get_mut(&key) {
                mutate(rng, c, nasty);
            }
        }
        Value::Array(a) if !a.is_empty() && rng.chance(85) => {
            let i = rng.below(a.len());
            mutate(rng, &mut a[i], nasty);
        }
        _ => *v = with,
    }
}

/// Bytes an attacker puts in an encrypted frame.
fn hostile_plain(rng: &mut Rng, seeds: &[Value], nasty: &[Value]) -> Vec<u8> {
    match rng.below(12) {
        0 => (0..rng.below(1500)).map(|_| rng.below(256) as u8).collect(),
        1 => {
            let mut b = serde_json::to_vec(&seeds[rng.below(seeds.len())]).unwrap();
            let cut = rng.below(b.len() + 1);
            b.truncate(cut);
            b
        }
        2 => format!("{}{}", "[".repeat(5_000), "]".repeat(5_000)).into_bytes(),
        3 => br#"{"t":"zzz"}"#.to_vec(),
        4 => vec![0xff, 0xfe, 0xc0, 0x80, b'{'],
        _ => {
            let mut v = seeds[rng.below(seeds.len())].clone();
            for _ in 0..1 + rng.below(3) {
                mutate(rng, &mut v, nasty);
            }
            serde_json::to_vec(&v).unwrap()
        }
    }
}

/* ----------------------------------------------------------------------------------------------------------- scenarios */

#[test]
fn nothing_a_stranger_sends_before_being_let_in_reaches_the_board() {
    let mut w = Lab::new(1, |_| {});
    let honest = w.honest_guest("Anna");
    let (seq, before) = (w.host.seq(), board_hash(&w.ed.doc));
    let ticket = w.ticket(Role::Edit);
    let page = w.page();
    let evil = Message::Submit(Submit {
        client_seq: 1,
        base_seq: 0,
        ops: vec![WireOp::SetName { name: "pwned".into() }, WireOp::Delete { page, ids: vec!["seed-1".into()] }],
    });
    // 1. a submit as the very first frame: the connection is dropped, no request is made
    let mut a = Atk::new(&w.net, "first-submit", &ticket);
    assert!(a.send(&evil));
    w.rounds(3);
    assert!(w.host.pending().is_empty());
    // 2. a hello, then everything else while the request waits (within the rate of a stranger: one request, nothing applied)
    let mut b = Atk::new(&w.net, "waiting", &ticket);
    b.hello(Role::Edit);
    for _ in 0..3 {
        b.hello(Role::Edit);
        b.send(&evil);
        b.send(&Message::Resync { last_seq: 0 });
        b.send(&Message::AssetChunk(AssetChunk { hash: "f".repeat(64), offset: 0, data: vec![1; 100], last: true }));
        b.send(&Message::Undo);
    }
    // the lab's own `step` accepts every request: look at the state right after the poll that read the frames
    w.net.advance(10);
    let now = w.net.now();
    w.host.poll(&mut w.ed, now);
    assert_eq!(w.host.pending().len(), 1, "one request, however many hellos");
    assert_eq!((w.host.seq(), board_hash(&w.ed.doc)), (seq, before.clone()), "nothing reached the board");
    // a stranger that sends faster than that is disconnected, and its request goes with it
    let mut f = Atk::new(&w.net, "faster", &ticket);
    f.hello(Role::Edit);
    for _ in 0..60 {
        f.send(&Message::Ping { nonce: 1 });
    }
    w.net.advance(10);
    let now = w.net.now();
    w.host.poll(&mut w.ed, now);
    assert_eq!(w.host.pending().len(), 1, "the fast one is not waiting any more");
    // 3. garbage frames: wrong key, empty, one byte, truncated, oversized
    let mut c = Atk::new(&w.net, "garbage", &ticket);
    let good = c.cipher.seal(b"{}", &mut c.entropy).unwrap();
    for frame in [vec![], vec![1], vec![1; 20], vec![1; 40], good[..good.len() - 1].to_vec(), vec![7; 3 << 20], vec![0; 100]] {
        c.send_frame(&frame);
    }
    w.rounds(15);
    assert!(w.host.connection_count() <= 4, "{} connections", w.host.connection_count());
    assert_eq!((w.host.seq(), board_hash(&w.ed.doc)), (seq, before));
    w.assert_healthy("stranger");
    w.honest_edit(honest, boxed("after", 500.0));
    w.rounds(10);
    assert!(w.ed.doc.pages[0].get(&"after".into()).is_some(), "an honest guest still edits");
}

#[test]
fn a_reader_stays_a_reader_and_an_old_identity_cannot_take_a_place() {
    let mut w = Lab::new(2, |_| {});
    let anna = w.honest_guest("Anna");
    let view = w.ticket(Role::View);
    let mut r = Atk::new(&w.net, "reader", &view);
    // asks for edit access with the read-only key
    r.hello(Role::Edit);
    w.rounds(3);
    let page = w.page();
    r.send(&Message::Submit(Submit { client_seq: 1, base_seq: 0, ops: vec![WireOp::Delete { page: page.clone(), ids: vec!["seed-1".into()] }] }));
    r.send(&Message::Transient { object: "seed-1".into(), state: Box::new(boxed("seed-1", 9.0)), author: None });
    w.rounds(4);
    assert!(w.ed.doc.pages[0].get(&"seed-1".into()).is_some_and(|o| o.frame().unwrap().x == 0.0));
    // another transport identity claims Anna's place with a resume
    let edit = w.ticket(Role::Edit);
    let mut thief = Atk::new(&w.net, "thief", &edit);
    let h = Hello {
        proto: PROTO_VERSION,
        name: "thief".into(),
        join_proof: join_proof(&edit.key, "thief", Role::Edit),
        role_requested: Role::Edit,
        resume: Some(inkboard_session::proto::Resume { participant: ParticipantId(1), last_seq: 0 }),
    };
    thief.send(&Message::Hello(h));
    w.net.advance(10);
    let now = w.net.now();
    w.host.poll(&mut w.ed, now);
    assert_eq!(w.host.pending().len(), 1, "a new request, not Anna's place");
    assert!(w.host.members().iter().any(|m| m.id == ParticipantId(1) && m.name == "Anna" && m.connected));
    w.assert_healthy("identity");
    w.honest_edit(anna, boxed("anna", 700.0));
    w.rounds(10);
    assert!(w.ed.doc.pages[0].get(&"anna".into()).is_some());
}

#[test]
fn hostile_operations_on_missing_things_change_nothing_and_are_answered() {
    let mut w = Lab::new(3, |_| {});
    w.honest_guest("Anna");
    let mut a = w.attacker(Role::Edit);
    let before = board_hash(&w.ed.doc);
    let page = w.page();
    let ghost = Id("ghost".into());
    let ops = [
        vec![WireOp::Put { page: ghost.clone(), object: Box::new(boxed("x", 1.0)) }],
        vec![WireOp::Delete { page: page.clone(), ids: vec![ghost.clone(), "nobody".into(), ghost.clone()] }],
        vec![WireOp::Order { page: page.clone(), ids: vec![ghost.clone(), "nobody".into(), ghost.clone()] }],
        vec![WireOp::SetLayers { page: ghost.clone(), layers: vec![Layer::main()] }],
        vec![WireOp::RenamePage { page: ghost.clone(), name: "x".into() }],
        vec![WireOp::RemovePage { page: ghost.clone() }],
    ];
    for (i, o) in ops.into_iter().enumerate() {
        a.send(&Message::Submit(Submit { client_seq: i as u64 + 1, base_seq: 9_999, ops: o }));
    }
    w.rounds(6);
    // a modification that matches nothing leaves the board as it was
    assert_eq!(board_hash(&w.ed.doc), before);
    assert!(!w.host.members().is_empty());
    w.assert_healthy("missing things");
}

#[test]
fn operations_on_a_page_the_host_just_deleted_are_harmless() {
    let mut w = Lab::new(4, |_| {});
    let anna = w.honest_guest("Anna");
    let second = w.ed.add_page("second");
    w.rounds(6);
    let mut a = w.attacker(Role::Edit);
    // the attacker still draws on the page the host removes in the same instant
    w.ed.remove_page(&second);
    for i in 0..20 {
        a.send(&Message::Submit(Submit {
            client_seq: i + 1,
            base_seq: 0,
            ops: vec![WireOp::Put { page: second.clone(), object: Box::new(boxed(&format!("late{i}"), 1.0)) }, WireOp::RemovePage { page: second.clone() }],
        }));
    }
    w.rounds(10);
    assert!(w.ed.doc.page(&second).is_none());
    assert_eq!(w.ed.doc.pages.len(), 1);
    w.assert_healthy("deleted page");
    w.honest_edit(anna, boxed("ok", 800.0));
    w.rounds(10);
    assert!(w.ed.doc.pages[0].get(&"ok".into()).is_some());
}

#[test]
fn cycles_of_connectors_and_groups_cannot_hang_the_host_or_the_engine() {
    let mut w = Lab::new(5, |_| {});
    w.honest_guest("Anna");
    let mut a = w.attacker(Role::Edit);
    let page = w.page();
    let conn = |id: &str, from: &str, to: &str, route: inkboard_core::Route| {
        let mut c = Connector::new(End::bound(from.into()), End::bound(to.into()));
        c.route = route;
        c.via = vec![Pt::new(1e6, -1e6), Pt::new(-1e6, 1e6)];
        let mut o = Object::new(Kind::Connector(c), Style::default());
        o.id = id.into();
        o
    };
    let mut grouped = boxed("g1", 10.0);
    grouped.group = Some("g1".into());
    let mut ops = vec![
        WireOp::Put { page: page.clone(), object: Box::new(conn("c1", "c1", "c1", inkboard_core::Route::Elbow)) },
        WireOp::Put { page: page.clone(), object: Box::new(conn("c2", "c3", "c3", inkboard_core::Route::Curve)) },
        WireOp::Put { page: page.clone(), object: Box::new(conn("c3", "c2", "c2", inkboard_core::Route::Elbow)) },
        WireOp::Put { page: page.clone(), object: Box::new(conn("c4", "ghost", "seed-1", inkboard_core::Route::Straight)) },
        WireOp::Put { page: page.clone(), object: Box::new(grouped) },
    ];
    // a few hundred elbow connectors around the same obstacles
    ops.extend((0..150).map(|i| WireOp::Put { page: page.clone(), object: Box::new(conn(&format!("e{i}"), "seed-1", "seed-2", inkboard_core::Route::Elbow)) }));
    a.send(&Message::Submit(Submit { client_seq: 1, base_seq: 0, ops }));
    w.rounds(10);
    assert!(w.ed.doc.pages[0].get(&"c1".into()).is_some(), "valid but strange: it is on the board");
    w.assert_healthy("cycles");
    // the host's own editing on top of them
    let ids: Vec<Id> = w.ed.doc.pages[0].objects.iter().map(|o| o.id.clone()).collect();
    w.ed.group(&ids[..4]);
    w.ed.duplicate(&ids[..6], Pt::new(5.0, 5.0));
    w.ed.remove(&["c1".into(), "seed-1".into()]);
    w.ed.undo();
    w.assert_healthy("cycles, then the host edits");
}

#[test]
fn a_replayed_frame_is_applied_once_and_extreme_sequence_numbers_do_not_overflow() {
    let mut w = Lab::new(6, |_| {});
    w.honest_guest("Anna");
    let mut a = w.attacker(Role::Edit);
    let page = w.page();
    let seq0 = w.host.seq();
    let frame = a
        .chan
        .seal(&Message::Submit(Submit { client_seq: 1, base_seq: 0, ops: vec![WireOp::Put { page: page.clone(), object: Box::new(boxed("once", 1.0)) }] }))
        .unwrap();
    for _ in 0..30 {
        a.send_frame(&frame);
        w.step();
    }
    assert_eq!(w.host.seq(), seq0 + 1, "30 identical frames, one modification");
    let rejected = a.received().iter().filter(|m| matches!(m, Message::Rejected { .. })).count();
    assert!(rejected >= 1, "and the repetitions were answered");
    // client_seq at the edges, in every order
    for (i, cs) in [u64::MAX, 0, 1, u64::MAX - 1, 2, u64::MAX, 1 << 63].into_iter().enumerate() {
        let o = boxed(&format!("edge{i}"), i as f64);
        a.send(&Message::Submit(Submit { client_seq: cs, base_seq: u64::MAX, ops: vec![WireOp::Put { page: page.clone(), object: Box::new(o) }] }));
        a.send(&Message::Resync { last_seq: u64::MAX - i as u64 });
        w.rounds(2);
    }
    w.assert_healthy("sequence numbers");
}

#[test]
fn image_uploads_with_forged_identifiers_gaps_and_duplicates_are_refused_without_effect() {
    let mut w = Lab::new(7, |_| {});
    w.honest_guest("Anna");
    let mut a = w.attacker(Role::Edit);
    let before = board_hash(&w.ed.doc);
    let real = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4];
    let hash = hash_hex(&real);
    let bad_ids = ["", "x", &"g".repeat(64), &"a".repeat(63), &"A".repeat(65), "../../etc/passwd", &"é".repeat(40), "C:\\secret.png"];
    for id in bad_ids {
        a.send(&Message::AssetChunk(AssetChunk { hash: id.to_owned(), offset: 0, data: vec![1], last: true }));
    }
    // right shape, wrong bytes: the claimed hash is not the hash of what follows
    a.send(&Message::AssetChunk(AssetChunk { hash: "ab".repeat(32), offset: 0, data: real.clone(), last: true }));
    // gap, repeated chunk, offset at the end of the world, a chunk longer than a chunk may be
    a.send(&Message::AssetChunk(AssetChunk { hash: hash.clone(), offset: 5, data: vec![1; 4], last: false }));
    a.send(&Message::AssetChunk(AssetChunk { hash: hash.clone(), offset: u64::MAX, data: vec![1; 4], last: false }));
    a.send(&Message::AssetChunk(AssetChunk { hash: hash.clone(), offset: 0, data: real[..6].to_vec(), last: false }));
    a.send(&Message::AssetChunk(AssetChunk { hash: hash.clone(), offset: 0, data: real[..6].to_vec(), last: false }));
    a.send(&Message::AssetChunk(AssetChunk { hash: hash.clone(), offset: 0, data: vec![0; 300 << 10], last: false }));
    // a thousand different unfinished uploads, and requests for images nobody has
    for i in 0..1000u32 {
        a.send(&Message::AssetChunk(AssetChunk { hash: hash_hex(&i.to_le_bytes()), offset: 0, data: vec![9; 10], last: false }));
        a.send(&Message::AssetRequest { hash: hash_hex(&(i + 5000).to_le_bytes()) });
    }
    w.rounds(30);
    let answers = a.received();
    assert!(
        answers.iter().any(
            |m| matches!(m, Message::AssetAck { status: UploadStatus::Refused { .. }, .. }) || matches!(m, Message::Rejected { .. } | Message::Kick { .. })
        ),
        "refusals are explicit"
    );
    assert_eq!(board_hash(&w.ed.doc), before);
    // a modification that uses an image nobody uploaded is refused too
    let mut img =
        Object::new(Kind::Image(ImageObj { frame: Frame::new(0.0, 0.0, 10.0, 10.0), asset: Id("ab".repeat(32)), latex: None, mono: false }), Style::default());
    img.id = "img".into();
    let page = w.page();
    let mut b = w.attacker(Role::Edit);
    b.send(&Message::Submit(Submit { client_seq: 1, base_seq: 0, ops: vec![WireOp::Put { page, object: Box::new(img) }] }));
    w.rounds(4);
    assert!(b.received().iter().any(|m| matches!(m, Message::Rejected { client_seq: 1, .. })));
    assert_eq!(board_hash(&w.ed.doc), before);
    w.assert_healthy("uploads");
}

#[test]
fn a_clock_that_jumps_or_runs_backwards_does_not_break_the_host() {
    let mut w = Lab::new(8, |_| {});
    w.honest_guest("Anna");
    let mut a = w.attacker(Role::Edit);
    for now in [5_000u64, 10, 10, 0, u64::MAX / 4, 3, u64::MAX / 2, 100, 1 << 40, 0] {
        a.send(&Message::Ping { nonce: now });
        a.send(&Message::Presence(Presence { participant: None, page: w.page(), cursor: None, selection: vec![], tool: None, editing: None, viewport: None }));
        w.host.poll(&mut w.ed, now);
    }
    w.host.poll(&mut w.ed, 12);
    assert_eq!(doc_problem(&w.ed.doc), None);
}

#[test]
fn the_board_stays_under_its_ceiling_whatever_is_submitted() {
    let mut w = Lab::new(9, |c| c.max_total_objects = 60);
    w.honest_guest("Anna");
    let mut a = w.attacker(Role::Edit);
    let page = w.page();
    for round in 0..40u64 {
        let ops: Vec<WireOp> = (0..10).map(|i| WireOp::Put { page: page.clone(), object: Box::new(boxed(&format!("f{round}-{i}"), i as f64)) }).collect();
        a.send(&Message::Submit(Submit { client_seq: round + 1, base_seq: 0, ops }));
        w.step();
    }
    w.rounds(10);
    let n: usize = w.ed.doc.pages.iter().map(|p| p.objects.len()).sum();
    assert!(n <= 60, "{n} objects");
    w.assert_healthy("ceiling");
}

/* ------------------------------------------------------------------------------------------- randomized attack (model) */

/// Several attackers of both roles send random hostile content for a few thousand ticks, next to honest guests who edit and a host who
/// edits. Afterwards: the host did not panic, its board is well formed, every honest guest has exactly the host's board, an honest edit
/// still goes through, and every attacker that was removed was told.
fn random_attack(seed: u64, ticks: usize) {
    let mut w = Lab::new(seed, |c| c.max_total_objects = 2_000);
    let mut rng = Rng::new(seed ^ 0xBAD);
    let (seeds, nasty) = (seed_messages(), nasty_values());
    let anna = w.honest_guest("Anna");
    w.honest_guest("Marc");
    let mut attackers: Vec<Atk> = (0..3).map(|i| w.attacker(if i == 0 { Role::View } else { Role::Edit })).collect();
    let mut removed_and_told = 0;
    let mut counter = 0u32;
    for t in 0..ticks {
        for a in &mut attackers {
            if a.out {
                continue;
            }
            for _ in 0..rng.below(5) {
                let plain = hostile_plain(&mut rng, &seeds, &nasty);
                a.send_plain(&plain);
            }
            if rng.chance(8) {
                a.hello(Role::Edit);
            }
            a.received();
        }
        // replace the attackers that were thrown out
        for a in &mut attackers {
            if a.out {
                removed_and_told += 1;
                *a = w.attacker(if rng.chance(30) { Role::View } else { Role::Edit });
            }
        }
        if t % 7 == 0 {
            counter += 1;
            w.honest_edit(anna, boxed(&format!("h{counter}"), f64::from(counter)));
        }
        if t % 11 == 0 {
            w.ed.insert(boxed(&format!("host{t}"), 1.0));
        }
        if t % 97 == 0 && w.ed.can_undo() {
            w.ed.undo();
        }
        w.step();
    }
    assert!(removed_and_told > 0, "the attackers were never thrown out: the attack was too gentle to prove anything");
    w.assert_healthy("random attack");
    w.honest_edit(anna, boxed("final", 12_345.0));
    w.rounds(20);
    assert!(w.ed.doc.pages[0].get(&"final".into()).is_some(), "an honest guest can still edit after the attack");
    assert!(w.host.members().len() <= 6, "{} members", w.host.members().len());
    assert!(w.host.connection_count() < 60, "{} connections", w.host.connection_count());
}

#[test]
fn a_random_hostile_crowd_cannot_break_the_host() {
    random_attack(100, 150);
}

#[test]
#[ignore = "long bench: see context/mcp-remote/test-robustesse.md"]
fn long_random_hostile_crowd() {
    let n: usize = std::env::var("INKBOARD_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    let base: u64 = std::env::var("INKBOARD_FUZZ_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    for k in 0..n as u64 {
        random_attack(base + k, 1500);
    }
}

/* ----------------------------------------------------------------------------------------------- liveness and memory */

#[test]
fn an_honest_guest_is_served_in_a_bounded_number_of_ticks_during_a_flood() {
    // "How long can a guest keep the host busy?" is answered in virtual time: every tick is 10 ms of the simulated clock and the host reads
    // at most a fixed number of bytes per tick, so the work a flood causes per tick is bounded, and an honest modification has to be
    // applied within a fixed number of ticks whatever the flood.
    let mut w = Lab::new(10, |_| {});
    let anna = w.honest_guest("Anna");
    let mut flood: Vec<Atk> = (0..3).map(|_| w.attacker(Role::Edit)).collect();
    let (seeds, nasty) = (seed_messages(), nasty_values());
    let mut rng = Rng::new(10);
    let mut worst = 0usize;
    for k in 0..8u32 {
        let id = format!("served{k}");
        w.honest_edit(anna, boxed(&id, f64::from(k)));
        let mut waited = 0;
        while w.ed.doc.pages[0].get(&id.as_str().into()).is_none() {
            for a in flood.iter_mut().filter(|a| !a.out) {
                for _ in 0..12 {
                    let plain = hostile_plain(&mut rng, &seeds, &nasty);
                    a.send_plain(&plain);
                }
                a.received();
            }
            w.step();
            waited += 1;
            assert!(waited <= 40, "the honest modification {k} waited {waited} ticks");
        }
        worst = worst.max(waited);
        for a in flood.iter_mut().filter(|a| a.out) {
            *a = w.attacker(Role::Edit);
        }
    }
    assert!(worst <= 40, "worst wait: {worst} ticks");
    w.assert_healthy("flood");
}

/// One wave of hostile traffic of every kind from four attackers (replaced when thrown out), while an honest guest works; then the
/// attackers go away and their places expire. Returns the hostile bytes sent.
fn flood(w: &mut Lab, anna: usize, rng: &mut Rng, ticks: u32) -> usize {
    let (seeds, nasty) = (seed_messages(), nasty_values());
    let mut attackers: Vec<Atk> = (0..4).map(|_| w.attacker(Role::Edit)).collect();
    let mut sent = 0usize;
    for t in 0..ticks {
        for a in attackers.iter_mut() {
            a.received();
            if a.out {
                continue;
            }
            // each wave stays well under what the host reads in one tick, so that what is measured is the host and not the simulated wire
            for _ in 0..3 {
                let plain = match rng.below(5) {
                    0 => vec![b'x'; 100 << 10],
                    1 => serde_json::to_vec(&json!({"t": "presence", "page": "p", "selection": vec!["a"; 4000]})).unwrap(),
                    2 => serde_json::to_vec(&Message::AssetChunk(AssetChunk {
                        hash: hash_hex(&t.to_le_bytes()),
                        offset: 0,
                        data: vec![3; 100 << 10],
                        last: false,
                    }))
                    .unwrap(),
                    _ => hostile_plain(rng, &seeds, &nasty),
                };
                sent += plain.len();
                a.send_plain(&plain);
            }
        }
        for a in attackers.iter_mut() {
            if a.out {
                *a = w.attacker(Role::Edit);
            }
        }
        if t % 5 == 0 {
            w.honest_edit(anna, boxed(&format!("m{}-{t}", w.net.now()), f64::from(t)));
        }
        w.step();
    }
    drop(attackers);
    // the silent connections time out, then the places they held expire
    for _ in 0..2 {
        w.net.advance(200_000);
        w.rounds(10);
    }
    w.net.forget_closed();
    sent
}

/// The scenario measured by the child process. A first wave warms up everything that is allocated once (tables, caches) and gives the
/// reference; the second must leave the host holding next to nothing more, although it receives as much hostile data again.
fn memory_probe() {
    let ticks: u32 = std::env::var("INKBOARD_MEMORY_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(24);
    let mut w = Lab::new(11, |c| c.max_total_objects = 5_000);
    let anna = w.honest_guest("Anna");
    let mut rng = Rng::new(11);
    let before = ALLOC.current_usage();
    flood(&mut w, anna, &mut rng, ticks);
    let baseline = ALLOC.current_usage();
    let sent = flood(&mut w, anna, &mut rng, ticks);
    let grown = ALLOC.current_usage().saturating_sub(baseline);
    eprintln!(
        "hostile bytes sent: {} MiB, memory held after the first wave: {} KiB, added by the second: {} KiB",
        sent >> 20,
        baseline.saturating_sub(before) >> 10,
        grown >> 10
    );
    assert!(sent > (8 << 20) * (ticks as usize).max(24) / 24, "the flood was too small to mean anything ({} MiB)", sent >> 20);
    // what the honest work legitimately adds (a few objects, the log of modifications) is some KiB
    assert!(grown < (2 << 20), "the host side kept {} KiB more after a second flood", grown >> 10);
    assert_eq!(doc_problem(&w.ed.doc), None);
}

const PROBE: &str = "INKBOARD_MEMORY_PROBE";

/// Runs `memory_probe` alone in a child process: the allocation counter is global to a process, and the other tests run in parallel
/// threads of this one.
#[test]
fn memory_stays_bounded_under_a_flood() {
    if std::env::var_os(PROBE).is_some() {
        return memory_probe();
    }
    let exe = std::env::current_exe().expect("test executable");
    let out = std::process::Command::new(exe)
        .args(["--exact", "tests_hostile_host::memory_stays_bounded_under_a_flood", "--test-threads=1", "--nocapture"])
        .env(PROBE, "1")
        .output()
        .expect("child process");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the memory probe failed:\n{text}");
    assert!(text.contains("hostile bytes sent"), "the probe did not run:\n{text}");
}

#[test]
fn refusing_is_explicit_the_reasons_are_typed() {
    // a wrong link gets no answer at all (nothing to learn from it); a right link with a wrong proof gets a typed refusal
    let mut w = Lab::new(12, |_| {});
    let ticket = w.ticket(Role::Edit);
    let mut a = Atk::new(&w.net, "badproof", &ticket);
    let h = Hello { proto: PROTO_VERSION, name: "x".into(), join_proof: vec![0; 32], role_requested: Role::Edit, resume: None };
    a.send(&Message::Hello(h));
    w.rounds(3);
    assert!(a.received().iter().any(|m| matches!(m, Message::Refused { reason: RefuseReason::BadProof })));
    let mut old = Atk::new(&w.net, "oldversion", &ticket);
    old.send_plain(br#"{"t":"hello","proto":1,"name":"x","join_proof":"","role_requested":"edit"}"#);
    w.rounds(3);
    assert!(old.received().iter().any(|m| matches!(m, Message::Refused { reason: RefuseReason::Version { .. } })));
}
