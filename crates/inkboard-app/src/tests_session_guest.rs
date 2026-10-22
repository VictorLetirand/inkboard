//! Guest mode (lot D): joining, mirror, optimistic edits, read-only guests, reconnection, end of session, display names, link
//! parsing and the classification of network failures. Everything runs on the simulated network of `inkboard-session` with a virtual
//! clock: no socket, no sleep, no assertion about durations. The small host below is a copy of the one in
//! `crates/inkboard-session/tests/support/mod.rs` (plus `Pong`, a mute switch and counters); the real host is lot C.

use crate::app::App;
use crate::i18n::{Lang, MISSING, set_for_thread};
use crate::keymap::Action;
use crate::session_guest::*;
use crate::tests::T;
use crate::tools::Tool;
use inkboard_core::geom::{Frame, Pt};
use inkboard_core::model::{Document, Id, Object};
use inkboard_core::style::Style;
use inkboard_core::{BoxGeom, Label};
use inkboard_session::codec::read_hello;
use inkboard_session::crypto::{HostKeys, SecureChannel, SeededEntropy, Ticket, verify_join_proof};
use inkboard_session::limits::check_message;
use inkboard_session::proto::{AssetChunk, AssetInfo, ByeReason, DocMeta, ParticipantInfo, Presence, RefuseReason, RejectReason, Welcome};
use inkboard_session::sequencer::{CatchUp, Submitted};
use inkboard_session::sim::{NetConfig, SimConn, SimNet, SimTransport};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::{Error, JsonCodec, Message, ParticipantId, Role, Sequencer, WireOp};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const HOST_ID: &str = "host";

/* ------------------------------------------------------------------ a small host */

struct Peer {
    conn: SimConn,
    chan: Option<SecureChannel>,
    role: Role,
    pid: Option<ParticipantId>,
}

struct Host {
    seqr: Sequencer,
    keys: HostKeys,
    transport: SimTransport,
    peers: Vec<Peer>,
    known: HashSet<ParticipantId>,
    next_pid: u32,
    /// The host neither reads nor sends (an outage seen from the guest).
    down: bool,
    submits: usize,
    undos: usize,
    snapshots_after_resync: usize,
    hello_names: Vec<String>,
    /// Images listed in the snapshot, and the pieces served for each hash (tests of the images the guest receives).
    manifest: Vec<AssetInfo>,
    blobs: HashMap<String, Vec<AssetChunk>>,
}

impl Host {
    fn new(net: &SimNet, doc: Document, log_cap: usize) -> Self {
        Self {
            seqr: Sequencer::new(doc, log_cap),
            keys: HostKeys::generate(&mut SeededEntropy::new(99)).unwrap(),
            transport: net.transport(HOST_ID),
            peers: Vec::new(),
            known: HashSet::new(),
            next_pid: 1,
            down: false,
            submits: 0,
            undos: 0,
            snapshots_after_resync: 0,
            hello_names: Vec::new(),
            manifest: Vec::new(),
            blobs: HashMap::new(),
        }
    }

    fn ticket(&self, role: Role) -> Ticket {
        self.keys.ticket(role, HOST_ID, &[]).unwrap()
    }

    fn apply_local(&mut self, ops: Vec<WireOp>) {
        let a = self.seqr.apply_local(ParticipantId::HOST, ops);
        self.broadcast(&Message::Applied(a));
    }

    /// Closes every connection (a drop of the network the guest notices at once).
    fn cut(&mut self) {
        self.peers.iter_mut().for_each(|p| p.conn.close());
    }

    fn step(&mut self) {
        if self.down {
            return;
        }
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
        if self.down {
            return;
        }
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

    fn roster(&self) -> Vec<ParticipantInfo> {
        let mut v = vec![ParticipantInfo { id: ParticipantId::HOST, name: "Anna".into(), color: (0x2b, 0x8a, 0x3e), role: Role::Edit }];
        v.extend(
            self.peers.iter().filter_map(|p| p.pid.map(|id| ParticipantInfo { id, name: format!("Guest {}", id.0), color: (0xd9, 0x77, 0x57), role: p.role })),
        );
        v
    }

    fn on_frame(&mut self, i: usize, frame: &[u8]) {
        let msg = if self.peers[i].chan.is_none() {
            let Ok((role, cipher, plain)) = self.keys.open_first(frame) else { return };
            self.peers[i].role = role;
            let e = SeededEntropy::new(1000 + i as u64);
            self.peers[i].chan = Some(SecureChannel::new(cipher, Box::new(JsonCodec), Box::new(e), 1 << 20));
            match read_hello(&JsonCodec, &plain) {
                Ok(h) => Message::Hello(h),
                Err(Error::IncompatibleProtocol { .. }) => {
                    self.send(i, &Message::Refused { reason: RefuseReason::Version { host_proto: inkboard_session::PROTO_VERSION } });
                    return;
                }
                Err(_) => return,
            }
        } else {
            match self.peers[i].chan.as_ref().map(|c| c.open(frame)) {
                Some(Ok(m)) => m,
                _ => return,
            }
        };
        self.on_message(i, msg);
    }

    fn on_message(&mut self, i: usize, msg: Message) {
        match msg {
            Message::Hello(h) => {
                self.hello_names.push(h.name.clone());
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
                    color: (0xd9, 0x77, 0x57),
                    role,
                    doc_meta: DocMeta { name: doc.name.clone(), seq: self.seqr.seq(), pages: doc.pages.len() },
                    host_proto: inkboard_session::PROTO_VERSION,
                    host_name: "Anna".into(),
                    participants: self.roster(),
                };
                self.send(i, &Message::Welcome(welcome));
                match resumed {
                    Some(r) => self.send_catch_up(i, r.last_seq, false),
                    None => {
                        let s = self.seqr.snapshot(self.manifest.clone());
                        self.send(i, &Message::Snapshot(s));
                    }
                }
            }
            Message::Submit(s) => {
                self.submits += 1;
                let (Some(pid), role) = (self.peers[i].pid, self.peers[i].role) else { return };
                let client_seq = s.client_seq;
                if role == Role::View {
                    self.send(i, &Message::Rejected { client_seq, reason: RejectReason::ReadOnly });
                } else if let Err(v) = check_message(&inkboard_session::Limits::default(), &Message::Submit(s.clone())) {
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
            Message::Resync { last_seq } if self.peers[i].pid.is_some() => self.send_catch_up(i, last_seq, true),
            Message::Ping { nonce } => self.send(i, &Message::Pong { nonce }),
            Message::Undo => self.undos += 1,
            Message::AssetRequest { hash } => {
                for c in self.blobs.get(&hash).cloned().unwrap_or_default() {
                    self.send(i, &Message::AssetChunk(c));
                }
            }
            _ => {}
        }
    }

    fn send_catch_up(&mut self, i: usize, last_seq: u64, count: bool) {
        match self.seqr.catch_up(last_seq, Vec::new()) {
            CatchUp::Applied(v) => v.into_iter().for_each(|a| self.send(i, &Message::Applied(a))),
            CatchUp::Snapshot(s) => {
                self.snapshots_after_resync += usize::from(count);
                self.send(i, &Message::Snapshot(s));
            }
        }
    }
}

/* ------------------------------------------------------------------ fakes */

struct FakeDiagnoser(Diagnosis);

impl RelayDiagnoser for FakeDiagnoser {
    fn diagnose(&self, _relay_url: &str) -> Diagnosis {
        self.0.clone()
    }
}

struct FailingLink(ConnectErrorKind);

impl GuestLink for FailingLink {
    fn connect(&mut self, _host: &str) -> Result<Box<dyn Connection>, ConnectError> {
        Err(ConnectError::new(self.0, "connection attempt failed"))
    }
}

fn zscaler() -> Diagnosis {
    Diagnosis {
        reachable: true,
        http_status: Some(403),
        server_header: Some("Zscaler/6.2".into()),
        gateway: Some("Zscaler".into()),
        summary: "HTTP 403 with a Zscaler block page".into(),
    }
}

fn boxed(id: &str, x: f64, y: f64) -> Object {
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(x, y, 80.0, 40.0), Style::default(), Label::default());
    o.id = Id(id.to_owned());
    o
}

/// Objects of every page (the view of a page is the user's own and may differ).
fn same_objects(a: &Document, b: &Document) -> bool {
    a.pages.len() == b.pages.len() && a.pages.iter().zip(&b.pages).all(|(p, q)| p.id == q.id && p.objects == q.objects)
}

/* ------------------------------------------------------------------ a world: the application as a guest + the host */

struct World {
    net: SimNet,
    host: Host,
    clock: Arc<AtomicU64>,
    t: T,
    dir: std::path::PathBuf,
}

impl World {
    fn new(log_cap: usize) -> Self {
        set_for_thread(Some(Lang::En));
        let net = SimNet::new(11, NetConfig::ideal());
        let mut doc = Document::new("board");
        doc.pages[0].objects.push(boxed("seed-1", 0.0, 0.0));
        doc.pages[0].objects.push(boxed("seed-2", 200.0, 100.0));
        let host = Host::new(&net, doc, log_cap);
        let clock = Arc::new(AtomicU64::new(0));
        let dir = std::env::temp_dir().join(format!("inkboard-guest-{}", Id::new()));
        let mut t = T::new();
        let (n, c) = (net.clone(), clock.clone());
        t.app().join.factory = Some(Box::new(move |ticket, name| {
            let link = Box::new(n.transport("guest"));
            let diag = Arc::new(FakeDiagnoser(zscaler()));
            GuestSession::new(link, diag, ticket, name, Box::new(SeededEntropy::new(3)), Clock::Virtual(c.clone()), GuestConfig::default())
        }));
        t.app().join.name_dir = Some(dir.clone());
        World { net, host, clock, t, dir }
    }

    fn app(&mut self) -> &mut App {
        self.t.app()
    }

    /// Rounds of: host, one frame of the application, 50 ms of virtual time.
    fn pump(&mut self, rounds: usize) {
        for _ in 0..rounds {
            self.host.step();
            self.t.h.step();
            self.net.advance(50);
            self.clock.store(self.net.now(), Ordering::Relaxed);
        }
    }

    fn join(&mut self, role: Role, name: &str) {
        let link = join_url(&self.host.ticket(role));
        self.app().open_join_window(Some(link));
        if let Some(w) = self.app().join.window.as_mut() {
            w.name = name.to_owned();
        }
        self.app().submit_join();
        self.pump(12);
    }

    fn mirrors_host(&mut self) -> bool {
        same_objects(&self.t.app().ed.doc, self.host.seqr.doc())
    }

    fn guest_state(&mut self) -> GuestState {
        self.app().guest.as_ref().map(|g| g.state()).expect("a session")
    }

    fn host_obj(&self, id: &str) -> Object {
        self.host.seqr.doc().pages[0].get(&Id(id.into())).cloned().unwrap()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
        set_for_thread(None);
    }
}

// Lot P: presence finishing touches (strokes and moves in progress, typing badge, follow), in their own file.
#[path = "tests_presence.rs"]
mod presence_lot;

fn put_on_first_page(w: &World, o: Object) -> WireOp {
    WireOp::Put { page: w.host.seqr.doc().pages[0].id.clone(), object: Box::new(o) }
}

/* ------------------------------------------------------------------ joining */

#[test]
fn joining_shows_a_mirror_identical_to_the_hosts_and_leaving_gives_the_local_board_back() {
    let mut w = World::new(100);
    let mine = w.app().ed.insert(boxed("local-only", 5.0, 5.0));
    w.join(Role::Edit, "Bob");
    assert!(w.app().guest_active(), "the board of the host is on screen");
    assert_eq!(w.guest_state(), GuestState::Live);
    assert!(w.mirrors_host());
    assert!(w.app().ed.page().get(&mine).is_none(), "the local board is put aside, not mixed in");
    let g = w.app().guest.as_ref().unwrap();
    assert_eq!(g.host_name(), "Anna", "the name of the host comes from Welcome");
    assert_eq!(g.roster().len(), 2);
    // a change of the host reaches the mirror
    let seed = w.host_obj("seed-2");
    let op = put_on_first_page(&w, {
        let mut o = seed;
        o.translate(Pt::new(30.0, 0.0));
        o
    });
    w.host.apply_local(vec![op]);
    w.pump(4);
    assert!(w.mirrors_host());
    // leaving
    w.app().leave_guest();
    assert!(!w.app().guest_active() && w.app().guest.is_none());
    assert!(w.app().ed.page().get(&mine).is_some(), "the local board is back");
    assert!(!w.app().ed.is_read_only());
}

#[test]
fn the_waiting_window_shows_until_the_host_answers() {
    let mut w = World::new(100);
    w.host.down = true;
    w.join(Role::Edit, "Bob");
    assert_eq!(w.guest_state(), GuestState::Joining);
    assert!(!w.app().guest_active(), "no board yet");
    w.host.down = false;
    w.pump(60);
    assert_eq!(w.guest_state(), GuestState::Live);
    assert!(w.app().guest_active());
}

/* ------------------------------------------------------------------ editing */

#[test]
fn a_local_edit_is_captured_sent_confirmed_and_the_hosts_later_edit_wins() {
    let mut w = World::new(100);
    w.join(Role::Edit, "Bob");
    let seed = Id("seed-1".into());
    let mine = w.app().ed.insert(boxed("mine", 0.0, 300.0));
    w.app().ed.update(&seed, |o| o.translate(Pt::new(10.0, 0.0)));
    assert!(w.app().ed.page().get(&mine).is_some(), "applied at once, before the host knows");
    w.t.h.step(); // captured and handed to the optimistic client
    assert_eq!(w.app().guest.as_ref().unwrap().pending_len(), 1);
    w.t.h.step(); // sent
    w.net.advance(1);
    w.host.step(); // the host applies the guest's modification…
    assert!(w.host.seqr.doc().pages[0].get(&mine).is_some());
    let page = w.host.seqr.doc().pages[0].id.clone();
    let mut theirs = w.host_obj("seed-1");
    theirs.translate(Pt::new(500.0 - theirs.frame().unwrap().x, 0.0));
    w.host.apply_local(vec![WireOp::Put { page, object: Box::new(theirs) }]); // …then changes the same object itself
    w.pump(10);
    assert_eq!(w.app().guest.as_ref().unwrap().pending_len(), 0, "confirmed");
    assert!(w.mirrors_host());
    assert_eq!(w.app().ed.page().get(&seed).unwrap().frame().unwrap().x, 500.0, "the host's state wins");
    assert_eq!(w.host.submits, 1, "one gesture, one submission");
}

#[test]
fn undo_and_redo_are_commands_for_the_host_and_never_change_the_mirror_locally() {
    let mut w = World::new(100);
    w.join(Role::Edit, "Bob");
    assert!(!w.app().can_undo(), "nothing to undo yet");
    w.app().run_action(Action::Undo);
    w.pump(3);
    assert_eq!(w.host.undos, 0);
    w.app().ed.insert(boxed("mine", 0.0, 300.0));
    w.pump(6);
    assert!(w.app().can_undo());
    let before = w.app().ed.doc.clone();
    w.app().run_action(Action::Undo);
    w.pump(4);
    assert_eq!(w.host.undos, 1, "the host received the command");
    assert!(same_objects(&w.app().ed.doc, &before), "the mirror only changes when the host answers");
    assert!(w.app().can_redo());
}

#[test]
fn a_reader_has_no_editing_tools_and_never_submits() {
    let mut w = World::new(100);
    w.join(Role::View, "Rita");
    assert!(w.app().view_only());
    for tool in [Tool::Pen, Tool::Text, Tool::Select, Tool::Eraser, Tool::Sticky] {
        w.app().set_tool(tool);
        assert!(matches!(w.app().tool, Tool::Hand | Tool::Laser), "{tool:?} is disabled");
    }
    let rev = w.app().ed.revision();
    w.app().ed.insert(boxed("sneaky", 0.0, 0.0));
    w.app().delete_selection();
    w.app().select_all();
    w.app().delete_selection();
    w.app().run_action(Action::Undo);
    w.app().run_action(Action::Duplicate);
    assert_eq!(w.app().ed.revision(), rev, "the editor itself refuses");
    w.pump(10);
    assert_eq!(w.host.submits, 0, "no Submit was ever sent");
    assert_eq!(w.host.undos, 0);
    assert!(w.mirrors_host());
    assert!(!w.app().guest.as_mut().unwrap().request_undo());
}

/* ------------------------------------------------------------------ others' presence */

#[test]
fn cursors_names_strokes_and_the_agent_of_the_others_are_shown() {
    let mut w = World::new(100);
    w.join(Role::Edit, "Bob");
    let page = w.host.seqr.doc().pages[0].id.clone();
    w.host.broadcast(&Message::Presence(Presence {
        participant: Some(ParticipantId::HOST),
        page: page.clone(),
        cursor: Some(Pt::new(120.0, 80.0)),
        selection: vec![Id("seed-1".into())],
        tool: Some("select".into()),
        editing: None,
        viewport: None,
    }));
    w.host.broadcast(&Message::StrokePoints { stroke: Id("s".into()), points: vec![Pt::new(0.0, 0.0), Pt::new(10.0, 10.0)], author: Some(ParticipantId(7)) });
    w.host.broadcast(&Message::Agent(inkboard_session::proto::AgentEvent {
        name: "Claude".into(),
        action: "add_shape".into(),
        targets: vec![Id("seed-2".into())],
        at: None,
    }));
    let mut moving = w.host_obj("seed-2");
    moving.translate(Pt::new(111.0, 0.0));
    w.host.broadcast(&Message::Transient { object: Id("seed-2".into()), state: Box::new(moving.clone()), author: Some(ParticipantId(7)) });
    w.pump(4);
    let app = w.app();
    let p = app.agent.remote.people.get(&0).expect("the host's cursor");
    assert_eq!(p.state.participant.name, "Anna");
    assert_eq!(p.state.selection.len(), 1);
    assert_eq!(app.agent.remote.strokes.len(), 1);
    assert_eq!(app.agent.outbox.len(), 1, "the agent's event was recorded");
    assert_eq!(app.ed.page().get(&Id("seed-2".into())).unwrap().frame().unwrap().x, moving.frame().unwrap().x, "the gesture of the other is shown");
    // the final state of the host replaces the intermediate one
    let page_id = page;
    let original = {
        let mut o = moving;
        o.translate(Pt::new(-111.0, 0.0));
        o
    };
    w.host.apply_local(vec![WireOp::Put { page: page_id, object: Box::new(original) }]);
    w.pump(4);
    assert!(w.mirrors_host());
}

/* ------------------------------------------------------------------ reconnection */

#[test]
fn a_silent_host_shows_reconnecting_changes_are_kept_and_missed_updates_are_caught_up() {
    let mut w = World::new(100);
    w.join(Role::Edit, "Bob");
    let pid = w.app().guest.as_ref().unwrap().participant();
    w.host.down = true;
    w.pump(60); // 3 s of virtual silence
    assert_eq!(w.guest_state(), GuestState::Reconnecting);
    // the board stays usable: the edit is kept
    let mine = w.app().ed.insert(boxed("offline", 0.0, 400.0));
    w.pump(2);
    assert!(w.app().ed.page().get(&mine).is_some());
    assert_eq!(w.app().guest.as_ref().unwrap().pending_len(), 1);
    let (p, extra) = (w.host.seqr.doc().pages[0].id.clone(), boxed("host-new", 400.0, 0.0));
    w.host.down = false;
    w.host.apply_local(vec![WireOp::Put { page: p, object: Box::new(extra) }]);
    w.pump(160);
    assert_eq!(w.guest_state(), GuestState::Live);
    assert_eq!(w.app().guest.as_ref().unwrap().participant(), pid);
    assert_eq!(w.app().guest.as_ref().unwrap().pending_len(), 0);
    assert!(w.mirrors_host());
    assert!(w.host.seqr.doc().pages[0].get(&mine).is_some(), "the offline edit reached the host");
    assert_eq!(w.host.snapshots_after_resync, 0, "the missing updates were enough");
}

#[test]
fn a_dropped_connection_resumes_with_the_same_identity_and_a_gap_beyond_the_window_gets_a_snapshot() {
    let mut w = World::new(3);
    w.join(Role::Edit, "Bob");
    let pid = w.app().guest.as_ref().unwrap().participant();
    w.host.cut();
    w.host.down = true;
    for i in 0..10 {
        // more than the host remembers
        let op = put_on_first_page(&w, boxed(&format!("n{i}"), f64::from(i) * 90.0, 600.0));
        w.host.seqr.apply_local(ParticipantId::HOST, vec![op]);
    }
    w.pump(40);
    assert_eq!(w.guest_state(), GuestState::Reconnecting);
    w.host.down = false;
    w.pump(60);
    assert_eq!(w.guest_state(), GuestState::Live);
    assert_eq!(w.app().guest.as_ref().unwrap().participant(), pid, "same identifier of participant");
    assert!(w.mirrors_host());
}

#[test]
fn when_the_host_leaves_the_mirror_stays_and_a_copy_can_be_saved() {
    let mut w = World::new(100);
    w.join(Role::Edit, "Bob");
    w.host.broadcast(&Message::Bye { reason: ByeReason::HostLeft });
    w.pump(4);
    assert_eq!(w.guest_state(), GuestState::Ended(EndReason::HostLeft));
    assert!(w.app().guest_active(), "the board is still on screen");
    assert!(w.app().ed.is_read_only());
    let file = w.dir.join("copy.inkboard");
    std::fs::create_dir_all(&w.dir).unwrap();
    w.app().save_to(&file);
    let saved = inkboard_core::serial::load(&file).unwrap();
    assert!(same_objects(&saved, w.host.seqr.doc()), "the copy holds the board");
    w.app().leave_guest();
    assert!(!w.app().guest_active());
}

/* ------------------------------------------------------------------ names */

#[test]
fn display_names_are_cleaned_remembered_and_sent_clean() {
    set_for_thread(Some(Lang::En));
    assert_eq!(sanitize_name("  Anna   Bob  "), "Anna Bob");
    assert_eq!(sanitize_name("An\u{0007}na\n\tBob"), "AnnaBob", "control characters are removed, not turned into spaces");
    assert_eq!(sanitize_name("a\u{202E}b\u{200B}c"), "abc");
    assert_eq!(sanitize_name(""), "Guest");
    assert_eq!(sanitize_name(" \n\t "), "Guest");
    assert_eq!(sanitize_name(&"a".repeat(50)).chars().count(), NAME_MAX);
    assert_eq!(sanitize_name(&"€".repeat(45)).chars().count(), NAME_MAX);
    assert_eq!(sanitize_name(&format!("{} b", "a".repeat(39))), "a".repeat(39), "no space left at the cut");
    set_for_thread(None);

    let mut w = World::new(100);
    assert_eq!(load_name_from(&w.dir), None);
    w.join(Role::Edit, "  Zed\u{0007}   Q  ");
    assert_eq!(w.host.hello_names, vec!["Zed Q".to_owned()]);
    assert_eq!(load_name_from(&w.dir).as_deref(), Some("Zed Q"), "remembered in the file display_name");
    // the next window starts with it
    w.app().leave_guest();
    w.app().open_join_window(None);
    assert_eq!(w.app().join.window.as_ref().unwrap().name, "Zed Q");
}

/* ------------------------------------------------------------------ links */

fn sample_ticket() -> Ticket {
    HostKeys::generate(&mut SeededEntropy::new(1)).unwrap().ticket(Role::Edit, "host-node-id", &["https://relay.example/".to_owned()]).unwrap()
}

#[test]
fn links_are_read_in_every_form_and_bad_ones_are_refused_without_panic() {
    let ticket = sample_ticket();
    let url = join_url(&ticket);
    assert!(url.starts_with("inkboard://join/"));
    let enc = ticket.encode();
    for ok in [
        url.clone(),
        format!("{url}/"),
        format!("  \"{url}\"  "),
        format!("INKBOARD://JOIN/{enc}"),
        format!("inkboard://join#{enc}"),
        format!("https://page.example/join#{enc}"),
        enc.clone(),
        format!("{url}?utm=1"),
    ] {
        assert_eq!(parse_join_link(&ok).as_ref(), Ok(&ticket), "{ok}");
    }
    // truncated
    for cut in [1, 5, 20, enc.len() - 1] {
        let bad = format!("inkboard://join/{}", &enc[..enc.len() - cut]);
        assert!(parse_join_link(&bad).is_err(), "{bad}");
    }
    // altered
    let mut chars: Vec<char> = enc.chars().collect();
    chars[30] = if chars[30] == 'A' { 'B' } else { 'A' };
    let altered: String = chars.into_iter().collect();
    assert_eq!(parse_join_link(&format!("inkboard://join/{altered}")), Err(JoinLinkError::Damaged));
    // too long, empty, other things
    assert_eq!(parse_join_link(&format!("inkboard://join/{}", "A".repeat(5000))), Err(JoinLinkError::TooLong));
    assert_eq!(parse_join_link("   "), Err(JoinLinkError::Empty));
    assert_eq!(parse_join_link("https://example.com/page"), Err(JoinLinkError::NotInkboard));
    // hostile garbage, including multi-byte text cut anywhere: never a panic
    let mut seed = SeededEntropy::new(5);
    let alphabet: Vec<char> = "inkboard://join/#?-_=AbZ09€é🙂\u{0}\u{7f}%".chars().collect();
    for _ in 0..3000 {
        let n = (seed.next_u64() % 60) as usize;
        let s: String = (0..n).map(|_| alphabet[(seed.next_u64() % alphabet.len() as u64) as usize]).collect();
        let _ = parse_join_link(&s);
        let _ = parse_join_link(&format!("inkboard://join/{s}"));
    }
    for (i, _) in "inkboard://join/€é🙂".char_indices() {
        let _ = parse_join_link(&"inkboard://join/€é🙂"[..i]);
    }
    assert!(parse_join_link("inkboard://jo").is_err());
}

#[test]
fn the_join_window_reports_a_bad_link_and_does_not_start_a_session() {
    let mut w = World::new(100);
    w.app().open_join_window(None);
    w.app().join.window.as_mut().unwrap().link = "inkboard://join/not-a-ticket".into();
    w.app().submit_join();
    assert!(w.app().guest.is_none());
    assert!(w.app().join.window.as_ref().unwrap().error.is_some());
    w.pump(2);
}

/* ------------------------------------------------------------------ network failures */

#[test]
fn a_403_from_a_known_gateway_gives_the_security_gateway_message() {
    set_for_thread(Some(Lang::En));
    let issue = classify_failure(ConnectErrorKind::TimedOut, "timed out", "https://euc1-1.relay.n0.iroh.link./", Some(&zscaler()));
    assert_eq!(issue.class, NetClass::Blocked { gateway: Some("Zscaler".into()), status: Some(403) });
    assert_eq!(issue.domain, "euc1-1.relay.n0.iroh.link", "the exact name to give to the IT department");
    assert!(issue.headline().contains("security gateway (Zscaler)"), "{}", issue.headline());
    let tips = issue.tips();
    assert_eq!(tips.len(), 3);
    assert!(tips[0].contains("another network") && tips[1].contains("browser") && tips[2].contains("euc1-1.relay.n0.iroh.link"));
    let text = issue.diagnostic_text();
    for needle in ["euc1-1.relay.n0.iroh.link", "403", "Zscaler", env!("CARGO_PKG_VERSION")] {
        assert!(text.contains(needle), "{needle} in {text}");
    }
    // a 403 without a recognized gateway is still a block
    let plain = Diagnosis { reachable: true, http_status: Some(403), ..Diagnosis::default() };
    assert!(matches!(classify_failure(ConnectErrorKind::Other, "", DEFAULT_RELAY_URL, Some(&plain)).class, NetClass::Blocked { gateway: None, .. }));
    set_for_thread(None);
}

#[test]
fn a_timeout_without_diagnosis_gives_the_generic_message_and_no_network_a_distinct_one() {
    set_for_thread(Some(Lang::En));
    let generic = classify_failure(ConnectErrorKind::TimedOut, "timed out", DEFAULT_RELAY_URL, None);
    assert_eq!(generic.class, NetClass::Unreachable);
    assert!(generic.headline().contains("firewall or a security gateway (often at work or at school)"));
    assert!(!generic.headline().contains("Zscaler"));
    assert_eq!(generic.tips().len(), 3);
    let offline = classify_failure(ConnectErrorKind::NoNetwork, "no route", DEFAULT_RELAY_URL, None);
    assert_eq!(offline.class, NetClass::Offline);
    assert!(offline.headline().starts_with("No connection"));
    assert_ne!(offline.headline(), generic.headline());
    assert!(offline.tips().is_empty(), "nothing to ask the IT department");
    // the service answers fine: the host is the problem
    let ok = Diagnosis { reachable: true, http_status: Some(200), ..Diagnosis::default() };
    assert_eq!(classify_failure(ConnectErrorKind::TimedOut, "", DEFAULT_RELAY_URL, Some(&ok)).class, NetClass::HostUnreachable);
    // nothing answers: generic
    let dead = Diagnosis { reachable: false, ..Diagnosis::default() };
    assert_eq!(classify_failure(ConnectErrorKind::TimedOut, "", DEFAULT_RELAY_URL, Some(&dead)).class, NetClass::Unreachable);
    for (url, domain) in [("https://a.b.example:8443/x?y#z", "a.b.example"), ("a.example.", "a.example"), ("https://user@h.example/", "h.example"), ("", "")] {
        assert_eq!(relay_domain(url), domain);
    }
    set_for_thread(None);
}

#[test]
fn the_failure_messages_are_translated() {
    MISSING.with(|m| m.borrow_mut().clear());
    let en = {
        set_for_thread(Some(Lang::En));
        classify_failure(ConnectErrorKind::TimedOut, "", "https://r.example/", Some(&zscaler()))
    };
    set_for_thread(Some(Lang::Fr));
    let fr_issue = en.clone();
    assert_ne!(fr_issue.headline(), {
        set_for_thread(Some(Lang::En));
        let h = en.headline();
        set_for_thread(Some(Lang::Fr));
        h
    });
    assert!(fr_issue.headline().contains("(Zscaler)") && fr_issue.tips().len() == 3);
    for issue in [
        classify_failure(ConnectErrorKind::NoNetwork, "", "https://r.example/", None),
        classify_failure(ConnectErrorKind::TimedOut, "", "https://r.example/", None),
        classify_failure(
            ConnectErrorKind::TimedOut,
            "",
            "https://r.example/",
            Some(&Diagnosis { reachable: true, http_status: Some(200), ..Diagnosis::default() }),
        ),
    ] {
        let _ = (issue.headline(), issue.tips());
    }
    for e in [JoinLinkError::Empty, JoinLinkError::TooLong, JoinLinkError::NotInkboard, JoinLinkError::Unsupported, JoinLinkError::Damaged] {
        let _ = e.message();
    }
    let missing = MISSING.with(|m| m.borrow().clone());
    assert!(missing.is_empty(), "texts without a French translation: {missing:?}");
    set_for_thread(None);
}

#[test]
fn a_failed_connection_is_classified_with_the_diagnosis_run_on_a_thread() {
    set_for_thread(Some(Lang::En));
    let clock = Arc::new(AtomicU64::new(0));
    let mut g = GuestSession::new(
        Box::new(FailingLink(ConnectErrorKind::TimedOut)),
        Arc::new(FakeDiagnoser(zscaler())),
        sample_ticket(),
        "Bob",
        Box::new(SeededEntropy::new(1)),
        Clock::Virtual(clock),
        GuestConfig::default(),
    );
    g.poll();
    assert!(matches!(g.state(), GuestState::Failed(_)), "reported at once, with what is known");
    for _ in 0..10_000 {
        g.poll();
        if !g.is_diagnosing() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!g.is_diagnosing());
    // the ticket names a relay: that is the one diagnosed
    let GuestState::Failed(issue) = g.state() else { panic!("failed") };
    assert_eq!(issue.class, NetClass::Blocked { gateway: Some("Zscaler".into()), status: Some(403) });
    assert_eq!(issue.domain, "relay.example");
    // no network at all: no diagnosis is even started
    let mut off = GuestSession::new(
        Box::new(FailingLink(ConnectErrorKind::NoNetwork)),
        Arc::new(FakeDiagnoser(zscaler())),
        sample_ticket(),
        "Bob",
        Box::new(SeededEntropy::new(1)),
        Clock::Virtual(Arc::new(AtomicU64::new(0))),
        GuestConfig::default(),
    );
    off.poll();
    assert!(!off.is_diagnosing());
    assert!(matches!(off.state(), GuestState::Failed(i) if i.class == NetClass::Offline));
    set_for_thread(None);
}

#[test]
fn an_unknown_host_on_the_simulated_network_fails_instead_of_waiting_forever() {
    let net = SimNet::new(1, NetConfig::ideal());
    let mut t = {
        let mut g = GuestSession::new(
            Box::new(net.transport("guest")),
            Arc::new(NoDiagnosis),
            sample_ticket(), // names a host that does not exist here
            "Bob",
            Box::new(SeededEntropy::new(1)),
            Clock::Virtual(Arc::new(AtomicU64::new(0))),
            GuestConfig::default(),
        );
        g.poll();
        g
    };
    for _ in 0..1000 {
        t.poll();
        if !t.is_diagnosing() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(matches!(t.state(), GuestState::Failed(i) if i.class == NetClass::Unreachable));
}

#[test]
fn an_inkboard_link_on_the_command_line_opens_the_join_window_not_a_file() {
    let l = crate::parse_args(&["inkboard://join/abc".to_owned()]);
    assert_eq!(l.join.as_deref(), Some("inkboard://join/abc"));
    assert!(l.open.is_none());
    let l = crate::parse_args(&["INKBOARD://JOIN/abc".to_owned(), "board.inkboard".to_owned()]);
    assert!(l.join.is_some() && l.open.is_none(), "a link is the whole command line: no other argument is read");
    let l = crate::parse_args(&["board.inkboard".to_owned(), "inkb".to_owned(), "é".to_owned()]);
    assert!(l.join.is_none() && l.open.is_some(), "short and multi-byte arguments do not panic");
}

#[test]
fn a_link_on_the_command_line_hides_every_other_argument_from_the_option_parser() {
    let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let l = crate::parse_args(&args(&["inkboard://join#x", "--screenshot", "y"]));
    assert_eq!(l.join.as_deref(), Some("inkboard://join#x"));
    assert!(l.screenshot.is_none() && l.open.is_none() && !l.bench, "--screenshot is not an option after a link");
    let l = crate::parse_args(&args(&["inkboard://join#x", "--bench", "--pen-test", "C:\\Users\\me\\file.inkboard"]));
    assert!(l.join.is_some() && !l.bench && !l.pen_test && l.open.is_none());
    // a link with a quote, white space, a line break or an absurd length is refused, and still nothing after it is read
    let long = format!("inkboard://join#{}", "a".repeat(5000));
    for bad in ["inkboard://join#x\"", "inkboard://join#x y", "inkboard://join#x\r\n", "inkboard://join#x\"--screenshot", long.as_str()] {
        let l = crate::parse_args(&args(&[bad, "--screenshot", "y"]));
        assert!(l.join.is_none() && l.bad_link, "{bad:?} is refused");
        assert!(l.screenshot.is_none() && l.open.is_none(), "nothing after a refused link is read");
    }
}

#[test]
fn host_and_guest_clean_names_with_the_same_single_function() {
    use crate::session_host::sanitize_name as host;
    let long = "é".repeat(60);
    let cases = [
        "Anna",
        "  Anna \t Smith\n",
        "\u{3164}",
        "\u{2800}Anna",
        "An\u{061C}na",
        "\u{115F}\u{1160}",
        "a\u{034F}b",
        "\u{202E}evil\u{202C}",
        "x\u{200D}y\u{2066}z",
        "\u{00AD}",
        "€€€",
        long.as_str(),
    ];
    for c in cases {
        let g = sanitize_name(c);
        assert_eq!(host(c, &g), g, "{c:?}: both sides give the same name");
        assert_eq!(crate::names::clean_name(c).unwrap_or_else(|| "Guest".to_owned()), g);
        assert!(g.chars().count() <= NAME_MAX && !g.chars().any(|ch| ch.is_control() || ('\u{200B}'..='\u{200F}').contains(&ch)));
    }
    assert_eq!(sanitize_name("\u{3164}"), "Guest", "an invisible name is refused");
    assert_eq!(host("\u{2800}", "Host"), "Host");
    assert_eq!(sanitize_name("\u{2800}Anna\u{3164}"), "Anna");
}

#[test]
fn the_installers_register_the_inkboard_scheme() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pk = std::fs::read_to_string(root.join("Packager.toml")).unwrap();
    assert!(pk.contains("[[deep-link-protocols]]") && pk.contains("schemes = [\"inkboard\"]"));
    // the corrected NSIS and WiX templates keep the registration of the schemes (and the uninstaller removes it)
    let nsi = std::fs::read_to_string(root.join("packaging/nsis/installer.nsi")).unwrap();
    assert!(nsi.matches("{{#each deep_link_protocols as |protocol|").count() >= 2 && nsi.contains("URL Protocol"));
    let wxs = std::fs::read_to_string(root.join("packaging/wix/main.wxs")).unwrap();
    assert!(wxs.contains("{{#each deep_link_protocols as |protocol|") && wxs.contains("URL Protocol"));
}

/* ------------------------------------------------------------------ captures (run by hand: --ignored) */

fn capture(t: &mut T, name: &str) {
    for _ in 0..8 {
        t.h.step();
    }
    t.h.remove_cursor();
    t.h.step();
    let image = t.h.render().expect("render");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../context/mcp-remote").join(name);
    image.save(&path).expect("save the capture");
}

/// `cargo +1.99.1 test -p inkboard-app capture_lot_d -- --ignored` writes `context/mcp-remote/lot-d-*.png`.
#[test]
#[ignore = "writes the screenshots of context/mcp-remote"]
fn capture_lot_d_screenshots() {
    // the "Join" window
    let mut w = World::new(100);
    let link = join_url(&w.host.ticket(Role::Edit));
    w.app().open_join_window(Some(link));
    capture(&mut w.t, "lot-d-join-window.png");
    w.host.down = true;
    w.app().submit_join();
    w.pump(8);
    capture(&mut w.t, "lot-d-waiting.png");
    w.host.down = false;
    w.pump(40);
    // reconnecting banner over the mirror
    w.app().ed.insert(boxed("offline", 0.0, 400.0));
    w.host.down = true;
    w.pump(70);
    assert_eq!(w.guest_state(), GuestState::Reconnecting);
    capture(&mut w.t, "lot-d-reconnecting.png");
    drop(w);
    // the corporate network message
    let mut w = World::new(100);
    w.app().join.factory = Some(Box::new(|ticket, name| {
        GuestSession::new(
            Box::new(FailingLink(ConnectErrorKind::TimedOut)),
            Arc::new(FakeDiagnoser(zscaler())),
            ticket,
            name,
            Box::new(SeededEntropy::new(1)),
            Clock::Virtual(Arc::new(AtomicU64::new(0))),
            GuestConfig::default(),
        )
    }));
    let link = join_url(&sample_ticket());
    w.app().open_join_window(Some(link));
    w.app().submit_join();
    for _ in 0..200 {
        w.t.h.step();
        if !w.app().guest.as_ref().is_some_and(|g| g.is_diagnosing()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    capture(&mut w.t, "lot-d-enterprise-error.png");
}

/* ------------------------------------------------------------------ relay of the link (S-06, S-07) */

fn link_with_relays(w: &World, relays: &[&str]) -> String {
    let relays: Vec<String> = relays.iter().map(|r| (*r).to_owned()).collect();
    join_url(&w.host.keys.ticket(Role::Edit, HOST_ID, &relays).unwrap())
}

#[test]
fn a_link_with_an_unknown_relay_asks_before_connecting() {
    use egui_kittest::kittest::Queryable;
    let mut w = World::new(100);
    let link = link_with_relays(&w, &["https://relay.evil.example/"]);
    w.app().open_join_window(Some(link));
    w.app().submit_join();
    assert!(w.app().guest.is_none(), "nothing connects before the user confirms");
    assert_eq!(w.app().join.window.as_ref().unwrap().unknown_relay.as_deref(), Some("relay.evil.example"));
    w.t.h.step();
    w.t.h.step();
    w.t.h.step();
    assert!(w.t.h.query_by_label_contains("This link uses an unknown relay: relay.evil.example").is_some());
    assert!(w.t.h.query_by_label("Cancel").is_some() && w.t.h.query_by_label("Continue").is_some());
    // Cancel: the window closes and nothing was started
    w.t.h.get_by_label("Cancel").click();
    w.t.h.step();
    w.t.h.step();
    assert!(w.app().join.window.is_none(), "window {:?}", w.app().join.window);
    assert!(w.app().guest.is_none());
    // Continue: the session starts
    let link = link_with_relays(&w, &["https://relay.evil.example/"]);
    w.app().open_join_window(Some(link));
    w.app().submit_join();
    w.t.h.step();
    w.t.h.get_by_label("Continue").click();
    w.t.h.step();
    w.t.h.step();
    assert!(w.app().guest.is_some(), "confirmed: the session starts");
}

#[test]
fn relays_that_are_refused_outright_never_reach_the_network() {
    let mut w = World::new(101);
    for relay in [
        "http://127.0.0.1:3340/",
        "https://10.0.0.1/",
        "https://192.168.1.1:8443/",
        "file:///etc/passwd",
        "https://relay.example.org:22/",
        "http://relay.example.org/",
    ] {
        let link = link_with_relays(&w, &[relay]);
        w.app().open_join_window(Some(link));
        w.app().submit_join();
        assert!(w.app().guest.is_none(), "{relay}");
        let win = w.app().join.window.as_ref().unwrap();
        assert!(win.error.is_some() && win.unknown_relay.is_none(), "{relay}: refused, not offered");
    }
}

#[test]
fn n0_relays_direct_addresses_and_configured_relays_join_without_asking() {
    let mut w = World::new(102);
    let n0 = inkboard_net::default_relay_urls().remove(0);
    let link = link_with_relays(&w, &[&n0, "127.0.0.1:4000"]);
    w.app().open_join_window(Some(link));
    w.app().submit_join();
    assert!(w.app().guest.is_some(), "a relay of n0 needs no confirmation");
    w.app().leave_guest();
    w.app().join.trusted_relays = vec!["https://relay.mycorp.example/".to_owned()];
    let link = link_with_relays(&w, &["https://relay.mycorp.example/"]);
    w.app().open_join_window(Some(link));
    w.app().submit_join();
    assert!(w.app().guest.is_some(), "a configured relay needs no confirmation");
}

#[test]
fn a_failure_on_an_unknown_relay_gives_no_advice_about_its_domain() {
    set_for_thread(Some(Lang::En));
    let mut g = GuestSession::new(
        Box::new(FailingLink(ConnectErrorKind::TimedOut)),
        Arc::new(FakeDiagnoser(zscaler())),
        w_ticket_with(&["https://relay.evil.example/"]),
        "Bob",
        Box::new(SeededEntropy::new(1)),
        Clock::Virtual(Arc::new(AtomicU64::new(0))),
        GuestConfig::default(),
    );
    for _ in 0..10_000 {
        g.poll();
        if matches!(g.state(), GuestState::Failed(_)) && !g.is_diagnosing() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let GuestState::Failed(issue) = g.state() else { panic!("failed") };
    assert!(issue.unknown_relay);
    let shown: String = issue.tips().join(" ") + &issue.headline() + &issue.warning().unwrap();
    assert!(issue.warning().unwrap().contains("unknown relay: relay.evil.example"));
    for forbidden in ["browser", "accept the warning", "IT department", "allow this domain", "https://relay.evil.example"] {
        assert!(!shown.contains(forbidden), "advice about an unknown domain: {forbidden}");
    }
    // a known relay keeps the three suggestions
    let known = classify_failure(ConnectErrorKind::TimedOut, "", DEFAULT_RELAY_URL, Some(&zscaler()));
    assert!(!known.unknown_relay && known.tips().len() == 3 && known.warning().is_none());
    set_for_thread(None);
}

fn w_ticket_with(relays: &[&str]) -> Ticket {
    let keys = HostKeys::generate(&mut SeededEntropy::new(8)).unwrap();
    let relays: Vec<String> = relays.iter().map(|r| (*r).to_owned()).collect();
    keys.ticket(Role::Edit, HOST_ID, &relays).unwrap()
}

/* ------------------------------------------------------------------ images of a session (lot G) */

#[path = "tests_session_images.rs"]
mod images;
