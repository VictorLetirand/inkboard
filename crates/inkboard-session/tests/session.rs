//! End-to-end behavior of host + guests over the simulated, encrypted network. Everything is deterministic (seeded faults, virtual
//! clock); the loops are bounded by a number of rounds, never by a duration.
// Test code counts, casts and compares freely: these `clippy::pedantic` lints are for the code that ships.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::assert_is_empty,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::needless_raw_string_hashes,
    clippy::needless_pass_by_value,
    clippy::explicit_iter_loop,
    clippy::needless_for_each,
    clippy::collapsible_else_if,
    clippy::single_match_else,
    clippy::manual_assert,
    clippy::redundant_closure_for_method_calls,
    clippy::unnested_or_patterns,
    clippy::range_plus_one,
    clippy::clone_on_copy,
    clippy::match_same_arms,
    clippy::struct_excessive_bools,
    clippy::manual_assert_eq
)]

mod support;

use inkboard_session::crypto::{FrameCipher, SecureChannel, SeededEntropy, SessionKey, Side, join_proof};
use inkboard_session::proto::{Hello, Message, ParticipantId, RejectReason, Role, Submit, Welcome};
use inkboard_session::sequencer::{Sequencer, Submitted};
use inkboard_session::sim::{NetConfig, SimNet};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::{ClientEvent, Error, JsonCodec, Phase, WireOp};
use proptest::prelude::*;
use support::{Guest, HOST_ID, World, boxed};

fn snapshots(events: &[ClientEvent]) -> usize {
    events.iter().filter(|e| matches!(e, ClientEvent::SnapshotLoaded)).count()
}

fn assert_converged(w: &World) {
    for (i, g) in w.guests.iter().enumerate() {
        assert_eq!(g.client.doc(), w.host.seqr.doc(), "guest {i} differs from the host");
    }
}

#[test]
fn guests_join_and_start_from_the_hosts_document() {
    let mut w = World::new(1, 3, NetConfig::ideal(), 1000);
    w.connect_all();
    assert!(w.settle(100));
    assert_converged(&w);
    let mut ids: Vec<_> = w.guests.iter().map(|g| g.client.participant().unwrap()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "each guest gets its own participant id");
}

#[test]
fn an_edit_reaches_everybody_and_the_author_is_confirmed() {
    let mut w = World::new(2, 3, NetConfig::ideal(), 1000);
    w.connect_all();
    assert!(w.settle(100));
    let page = w.host.seqr.doc().pages[0].id.clone();
    let now = w.net.now();
    let cs = w.guests[0].client.edit(vec![WireOp::Put { page: page.clone(), object: Box::new(boxed("mine", 1.0, 2.0)) }], now);
    assert_eq!(cs, Some(1));
    assert_eq!(w.guests[0].client.pending_len(), 1, "optimistic until the host confirms");
    assert!(w.guests[0].client.doc().pages[0].get(&"mine".into()).is_some(), "visible at once");
    assert!(w.guests[1].client.doc().pages[0].get(&"mine".into()).is_none());
    assert!(w.settle(100));
    assert_eq!(w.guests[0].client.pending_len(), 0);
    assert!(w.guests[2].client.doc().pages[0].get(&"mine".into()).is_some());
    assert_converged(&w);
}

#[test]
fn the_host_state_wins_and_the_loser_is_told() {
    let mut w = World::new(3, 2, NetConfig::ideal(), 1000);
    w.connect_all();
    assert!(w.settle(100));
    let page = w.host.seqr.doc().pages[0].id.clone();
    let (mut a, mut b) = (boxed("seed-1", 10.0, 10.0), boxed("seed-1", 99.0, 99.0));
    a.locked = false;
    b.locked = false;
    let now = w.net.now();
    // both edit the same object from the same base: the later arrival at the host wins, for everybody
    w.guests[0].client.edit(vec![WireOp::Put { page: page.clone(), object: Box::new(a) }], now);
    w.guests[1].client.edit(vec![WireOp::Put { page: page.clone(), object: Box::new(b.clone()) }], now);
    w.guests[0].client.take_events();
    w.guests[1].client.take_events();
    assert!(w.settle(100));
    assert_converged(&w);
    assert_eq!(w.host.seqr.doc().pages[0].get(&"seed-1".into()), Some(&b), "guest 1 arrived last at the host");
    let flagged = |g: &mut Guest| g.client.take_events().iter().any(|e| matches!(e, ClientEvent::Superseded(ids) if ids.contains(&"seed-1".into())));
    assert!(flagged(&mut w.guests[1]), "the second writer is told that it overwrote a concurrent change");
    assert!(!flagged(&mut w.guests[0]));
}

#[test]
fn reconnection_catches_up_the_missing_modifications() {
    let mut w = World::new(4, 2, NetConfig::ideal(), 1000);
    w.connect_all();
    assert!(w.settle(100));
    let before = w.guests[0].client.participant();
    assert_eq!(snapshots(&w.guests[0].client.take_events()), 1);
    w.guests[0].drop_connection();
    assert_eq!(w.guests[0].client.phase(), Phase::Disconnected);
    for k in 0..3u16 {
        let doc = w.guests[1].client.doc().clone();
        let ops = w.random_edit(&doc, 0, k * 7, k * 13);
        let now = w.net.now();
        w.guests[1].client.edit(ops, now);
        w.rounds(3, 10);
    }
    // an edit made while offline is queued and survives the reconnection
    let doc = w.guests[0].client.doc().clone();
    let ops = w.random_edit(&doc, 0, 5, 5);
    let WireOp::Put { object: offline, .. } = &ops[0] else { panic!("expected a Put") };
    let offline_id = offline.id.clone();
    let now = w.net.now();
    assert!(w.guests[0].client.edit(ops, now).is_some());
    assert_eq!(w.guests[0].client.pending_len(), 1);
    let now = w.net.now();
    w.guests[0].connect(now);
    assert!(w.settle(200));
    assert_converged(&w);
    assert_eq!(w.guests[0].client.participant(), before, "same place after the reconnection");
    let events = w.guests[0].client.take_events();
    assert_eq!(snapshots(&events), 0, "the missing modifications were replayed, no snapshot needed");
    assert!(events.iter().any(|e| matches!(e, ClientEvent::Joined { .. })));
    assert!(w.host.seqr.doc().pages[0].get(&offline_id).is_some(), "the offline edit reached the host");
}

#[test]
fn reconnection_beyond_the_retained_window_gets_a_snapshot() {
    let mut w = World::new(5, 2, NetConfig::ideal(), 3);
    w.connect_all();
    assert!(w.settle(100));
    w.guests[0].client.take_events();
    w.guests[0].drop_connection();
    for k in 0..10u16 {
        let doc = w.guests[1].client.doc().clone();
        let ops = w.random_edit(&doc, 0, k * 11, k * 3);
        let now = w.net.now();
        w.guests[1].client.edit(ops, now);
        w.rounds(2, 10);
    }
    let doc = w.guests[0].client.doc().clone();
    let ops = w.random_edit(&doc, 0, 9, 9);
    let now = w.net.now();
    w.guests[0].client.edit(ops, now);
    let now = w.net.now();
    w.guests[0].connect(now);
    assert!(w.settle(200));
    assert_converged(&w);
    assert_eq!(snapshots(&w.guests[0].client.take_events()), 1, "too far behind: a snapshot replaces the log");
}

#[test]
fn a_host_that_forgot_the_guest_drops_its_unconfirmed_edits_and_resyncs() {
    let mut w = World::new(6, 1, NetConfig::ideal(), 1000);
    w.connect_all();
    assert!(w.settle(100));
    let pid = w.guests[0].client.participant().unwrap();
    w.guests[0].drop_connection();
    let doc = w.guests[0].client.doc().clone();
    let ops = w.random_edit(&doc, 0, 1, 2);
    let now = w.net.now();
    w.guests[0].client.edit(ops, now);
    w.host.forget(pid);
    let now = w.net.now();
    w.guests[0].connect(now);
    assert!(w.settle(200));
    assert_converged(&w);
    assert_ne!(w.guests[0].client.participant(), Some(pid));
    assert!(w.guests[0].client.take_events().iter().any(|e| matches!(e, ClientEvent::PendingDiscarded(1))));
}

#[test]
fn sequencer_applies_a_client_seq_once() {
    let mut s = Sequencer::new(inkboard_core::Document::new("d"), 10);
    let page = s.doc().pages[0].id.clone();
    let submit = Submit { client_seq: 1, base_seq: 0, ops: vec![WireOp::Put { page, object: Box::new(boxed("x", 0.0, 0.0)) }] };
    let p = ParticipantId(7);
    assert!(matches!(s.submit(p, submit.clone()), Submitted::Applied(a) if a.seq == 1 && a.client_seq == Some(1)));
    assert_eq!(s.submit(p, submit.clone()), Submitted::Duplicate);
    // another participant may use the same client_seq; and out-of-order arrival is not mistaken for a duplicate
    assert!(matches!(s.submit(ParticipantId(8), submit.clone()), Submitted::Applied(_)));
    let later = Submit { client_seq: 3, ..submit.clone() };
    let earlier = Submit { client_seq: 2, ..submit };
    assert!(matches!(s.submit(p, later.clone()), Submitted::Applied(_)));
    assert!(matches!(s.submit(p, earlier.clone()), Submitted::Applied(_)));
    assert_eq!(s.submit(p, later), Submitted::Duplicate);
    assert_eq!(s.submit(p, earlier), Submitted::Duplicate);
    assert_eq!(s.seq(), 4);
}

#[test]
fn the_log_and_the_change_table_are_bounded_and_a_guest_too_far_behind_gets_a_snapshot() {
    use inkboard_core::model::Id;
    use inkboard_session::sequencer::CatchUp;
    // many large modifications: 400 x ~25 kB would be 10 MB without the byte budget, 800 000 remembered objects without the cap
    let mut s = Sequencer::new(inkboard_core::Document::new("d"), 10_000);
    s.set_budgets(200_000, 5_000);
    let page = s.doc().pages[0].id.clone();
    for round in 0..400 {
        let ids = (0..2_000).map(|i| Id(format!("r{round}-{i}"))).collect();
        s.apply_local(ParticipantId::HOST, vec![WireOp::Delete { page: page.clone(), ids }]);
        let (entries, bytes) = s.log_stats();
        assert!(bytes <= 200_000 && entries < 20, "round {round}: {entries} entries, {bytes} bytes");
        assert!(s.tracked_objects() <= 5_000, "round {round}: {} objects", s.tracked_objects());
    }
    assert_eq!(s.seq(), 400);
    assert!(matches!(s.catch_up(0, Vec::new()), CatchUp::Snapshot(_)), "what was evicted is replaced by a snapshot");
    assert!(matches!(s.catch_up(398, Vec::new()), CatchUp::Applied(v) if v.len() == 2), "recent entries are still served as modifications");
}

/// A raw connection to the host with a given key, to play the role of a hostile or buggy peer.
struct Raw {
    conn: inkboard_session::sim::SimConn,
    chan: SecureChannel,
}

impl Raw {
    fn new(net: &SimNet, name: &str, key: &SessionKey) -> Self {
        let conn = net.transport(name).connect(HOST_ID).unwrap();
        let chan = SecureChannel::new(FrameCipher::new(key, Side::Guest), Box::new(JsonCodec), Box::new(SeededEntropy::new(5)), 1 << 20);
        Self { conn, chan }
    }
    fn send(&mut self, m: &Message) {
        let f = self.chan.seal(m).unwrap();
        self.conn.send(&f).unwrap();
    }
    fn received(&mut self) -> Vec<Message> {
        std::iter::from_fn(|| self.conn.recv()).filter_map(|f| self.chan.open(&f).ok()).collect()
    }
}

fn hello(name: &str, key: &SessionKey, requested: Role, proto: u32) -> Message {
    Message::Hello(Hello { proto, name: name.into(), join_proof: join_proof(key, name, requested), role_requested: requested, resume: None })
}

#[test]
fn a_reader_is_refused_by_the_host_even_when_it_claims_to_edit() {
    let mut w = World::new(7, 0, NetConfig::ideal(), 1000);
    let key = w.host.ticket(Role::View).key;
    let mut raw = Raw::new(&w.net, "reader", &key);
    raw.send(&hello("reader", &key, Role::Edit, inkboard_session::PROTO_VERSION)); // claims Edit with the read-only key
    w.rounds(3, 10);
    let msgs = raw.received();
    let granted = msgs.iter().find_map(|m| if let Message::Welcome(Welcome { role, .. }) = m { Some(*role) } else { None });
    assert_eq!(granted, Some(Role::View), "the role comes from the key, not from the request");
    let page = w.host.seqr.doc().pages[0].id.clone();
    raw.send(&Message::Submit(Submit { client_seq: 1, base_seq: 0, ops: vec![WireOp::Put { page, object: Box::new(boxed("evil", 0.0, 0.0)) }] }));
    w.rounds(3, 10);
    assert!(raw.received().iter().any(|m| matches!(m, Message::Rejected { client_seq: 1, reason: RejectReason::ReadOnly })));
    assert_eq!(w.host.seqr.seq(), 0);
    assert!(w.host.seqr.doc().pages[0].get(&"evil".into()).is_none());
}

#[test]
fn a_wrong_key_gets_nothing_and_a_wrong_proof_is_refused() {
    let mut w = World::new(8, 0, NetConfig::ideal(), 1000);
    let wrong = SessionKey::generate(&mut SeededEntropy::new(99)).unwrap();
    let mut raw = Raw::new(&w.net, "intruder", &wrong);
    raw.send(&hello("intruder", &wrong, Role::Edit, inkboard_session::PROTO_VERSION));
    w.rounds(3, 10);
    assert!(raw.received().is_empty(), "nothing is answered to a peer that does not hold a session key");
    assert_eq!(w.host.dropped_frames, 1);

    let key = w.host.ticket(Role::Edit).key;
    let mut raw = Raw::new(&w.net, "liar", &key);
    raw.send(&hello("someone else", &key, Role::Edit, inkboard_session::PROTO_VERSION).clone_with_name("liar"));
    w.rounds(3, 10);
    assert!(raw.received().iter().any(|m| matches!(m, Message::Refused { .. })));
}

#[test]
fn an_incompatible_protocol_gets_a_clear_refusal() {
    let mut w = World::new(9, 0, NetConfig::ideal(), 1000);
    let key = w.host.ticket(Role::Edit).key;
    let mut raw = Raw::new(&w.net, "old", &key);
    raw.send(&hello("old", &key, Role::Edit, 99));
    w.rounds(3, 10);
    let refused = raw.received().into_iter().find_map(|m| if let Message::Refused { reason } = m { Some(reason) } else { None });
    let text = refused.expect("refused").message();
    assert!(text.contains("update Inkboard"), "{text}");

    // and the guest side: a host that speaks another version
    let mut g = Guest::new(&w.net, "g", &w.host.ticket(Role::Edit), 1);
    let w2 = Welcome {
        participant: ParticipantId(1),
        color: (0, 0, 0),
        role: Role::Edit,
        doc_meta: inkboard_session::proto::DocMeta { name: "x".into(), seq: 0, pages: 1 },
        host_proto: 99,
        host_name: String::new(),
        participants: vec![],
    };
    let err = g.client.handle(Message::Welcome(w2), 0).unwrap_err();
    assert!(matches!(err, Error::IncompatibleProtocol { theirs: 99, .. }));
    assert!(err.to_string().contains("update Inkboard"));
    assert_eq!(g.client.phase(), Phase::Closed);
}

trait HelloExt {
    fn clone_with_name(self, name: &str) -> Message;
}
impl HelloExt for Message {
    /// Same proof, other name: the proof is bound to the name.
    fn clone_with_name(self, name: &str) -> Message {
        match self {
            Message::Hello(h) => Message::Hello(Hello { name: name.into(), ..h }),
            m => m,
        }
    }
}

#[derive(Debug, Clone)]
struct Faults {
    loss_ppm: u32,
    dup_ppm: u32,
    reorder_ppm: u32,
    max_latency: u64,
}

fn faults() -> impl Strategy<Value = Faults> {
    (0u32..350_000, 0u32..350_000, 0u32..500_000, 0u64..400).prop_map(|(loss_ppm, dup_ppm, reorder_ppm, max_latency)| Faults {
        loss_ppm,
        dup_ppm,
        reorder_ppm,
        max_latency,
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 40, failure_persistence: None, ..ProptestConfig::default() })]

    /// Property: whatever N guests (2 to 20) edit, with loss, duplication, reordering and latency injected, once the network
    /// delivers everything every guest has exactly the host's document.
    #[test]
    fn all_guests_converge_to_the_host_state(
        seed in any::<u64>(),
        n in 2usize..=20,
        f in faults(),
        steps in prop::collection::vec((0usize..64, 0u8..20, any::<u16>(), any::<u16>(), 0u8..4), 1..50),
    ) {
        let cfg = NetConfig { latency_ms: (0, f.max_latency), loss_ppm: f.loss_ppm, dup_ppm: f.dup_ppm, reorder_ppm: f.reorder_ppm, reorder_extra_ms: 300 };
        let mut w = World::new(seed, n, cfg, 1000);
        w.connect_all();
        w.rounds(2, 20);
        for (who, code, a, b, gap) in steps {
            let idx = who % (n + 1);
            if idx == n {
                let doc = w.host.seqr.doc().clone();
                let ops = w.random_edit(&doc, code, a, b);
                w.host.apply_local(ops);
            } else {
                let doc = w.guests[idx].client.doc().clone();
                let ops = w.random_edit(&doc, code, a, b);
                let now = w.net.now();
                w.guests[idx].client.edit(ops, now);
            }
            w.rounds(usize::from(gap), 25);
        }
        // the network heals: from now on everything is delivered
        w.net.set_config(NetConfig::ideal());
        prop_assert!(w.settle(3000), "the system did not settle: {}", w.describe());
        for (i, g) in w.guests.iter().enumerate() {
            prop_assert_eq!(g.client.doc(), w.host.seqr.doc(), "guest {} differs", i);
        }
    }
}

/// Load check beyond the design target (20): 50 and 100 guests, 300 edits, strong faults. Everything converges to the host state.
/// (No assertion on durations: they are printed by `--nocapture` only.)
#[test]
fn fifty_and_a_hundred_guests_converge_under_heavy_faults() {
    for (n, seed) in [(50usize, 21u64), (100, 11)] {
        let started = std::time::Instant::now();
        let cfg = NetConfig { latency_ms: (0, 300), loss_ppm: 200_000, dup_ppm: 100_000, reorder_ppm: 300_000, reorder_extra_ms: 300 };
        let mut w = World::new(seed, n, cfg, 1000);
        w.connect_all();
        w.rounds(2, 20);
        let mut rng = seed;
        for _ in 0..300u32 {
            rng = rng.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let idx = (rng >> 33) as usize % (n + 1);
            let (code, a, b) = (((rng >> 20) % 20) as u8, (rng >> 8) as u16, (rng >> 40) as u16);
            if idx == n {
                let doc = w.host.seqr.doc().clone();
                let ops = w.random_edit(&doc, code, a, b);
                w.host.apply_local(ops);
            } else {
                let doc = w.guests[idx].client.doc().clone();
                let ops = w.random_edit(&doc, code, a, b);
                let now = w.net.now();
                w.guests[idx].client.edit(ops, now);
            }
            w.rounds(usize::from((rng >> 50) as u8 % 3), 25);
        }
        w.net.set_config(NetConfig::ideal());
        assert!(w.settle(6000), "{n} guests did not settle: {}", w.describe());
        for (i, g) in w.guests.iter().enumerate() {
            assert_eq!(g.client.doc(), w.host.seqr.doc(), "{n} guests: guest {i} differs");
        }
        let total: usize = w.host.seqr.doc().pages.iter().map(|p| p.objects.len()).sum();
        println!("{n} guests converged ({total} objects on {} pages) in {:?}", w.host.seqr.doc().pages.len(), started.elapsed());
    }
}

/// A case that the property above found on a CI run (Windows, 2026-10-26): with 13 % of loss and 15 % of duplication, the snapshot of the join was
/// lost for one guest, which stayed on the empty document while the harness reported the system as settled: on a document that was never changed
/// (host `seq` 0) a guest that has received nothing has `last_seq` 0 too, so `last_seq` could not tell. The product was right (the guest asks
/// again every `resend_after_ms`); the harness now also waits for the snapshot.
#[test]
fn regression_a_guest_that_lost_its_join_messages_still_converges() {
    let cfg = NetConfig { latency_ms: (0, 29), loss_ppm: 133_723, dup_ppm: 147_276, reorder_ppm: 1, reorder_extra_ms: 300 };
    let mut w = World::new(18_068_962_631_262_724_944, 3, cfg, 1000);
    w.connect_all();
    w.rounds(2, 20);
    let doc = w.guests[0].client.doc().clone();
    let ops = w.random_edit(&doc, 0, 0, 57);
    let now = w.net.now();
    w.guests[0].client.edit(ops, now);
    w.rounds(0, 25);
    w.net.set_config(NetConfig::ideal());
    assert!(w.settle(3000), "the system did not settle: {}", w.describe());
    for (i, g) in w.guests.iter().enumerate() {
        assert_eq!(g.client.doc(), w.host.seqr.doc(), "guest {i} differs");
    }
}
