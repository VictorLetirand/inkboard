//! Hostile-input bench of the protocol layer (decoding, validation, sequencing, the guest state machine).
//!
//! Every property runs from a fixed seed (`ChaCha`), so a CI run is reproducible: same bytes, same verdict. The same properties exist in a
//! long `#[ignore]` form (`INKBOARD_FUZZ_CASES`, `INKBOARD_FUZZ_SEED`), see `context/mcp-remote/test-robustesse.md`.
//! Nothing here measures a duration: sizes are bounded by construction, loops by a number of iterations.
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
    clippy::struct_excessive_bools
)]

use inkboard_core::connector::{Connector, End};
use inkboard_core::geom::{Frame, Pt};
use inkboard_core::model::{Document, Id, ImageObj, Kind, Layer, Object, Page, Table};
use inkboard_core::style::Style;
use inkboard_core::{BoxGeom, Label, RichText};
use inkboard_session::limits::{Limits, check_message, check_ops};
use inkboard_session::proto::{
    Applied, AssetChunk, Hello, Message, ParticipantId, Presence, RejectReason, Role, Snapshot, SnapshotHead, SnapshotPart, Submit, Welcome,
};
use inkboard_session::sequencer::{CatchUp, Sequencer, Submitted};
use inkboard_session::snapshot::{SnapshotAssembler, into_messages};
use inkboard_session::wire::{apply_wire_all, rollback};
use inkboard_session::{Client, ClientConfig, Codec, JsonCodec, Phase, WireOp};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/* ------------------------------------------------------------------------------------------------------------ plumbing */

fn runner(cases: u32, seed: u64) -> TestRunner {
    let mut s = [0u8; 32];
    s[..8].copy_from_slice(&seed.to_le_bytes());
    TestRunner::new_with_rng(Config { cases, failure_persistence: None, ..Config::default() }, TestRng::from_seed(RngAlgorithm::ChaCha, &s))
}

/// `ci` cases in a normal run; the long bench passes its own default and may be overridden by `INKBOARD_FUZZ_CASES`.
fn cases(long_default: Option<u32>, ci: u32) -> u32 {
    match long_default {
        None => ci,
        Some(d) => std::env::var("INKBOARD_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(d),
    }
}

fn seed(base: u64, long: bool) -> u64 {
    if long { std::env::var("INKBOARD_FUZZ_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(base) } else { base }
}

/// What must hold for any document, whatever was applied to it: a page at least, no identifier used twice.
fn doc_problem(doc: &Document) -> Option<String> {
    if doc.pages.is_empty() {
        return Some("no page".into());
    }
    let mut pages = std::collections::HashSet::new();
    for p in &doc.pages {
        if !pages.insert(&p.id) {
            return Some(format!("page id {:?} used twice", p.id));
        }
        let mut objects = std::collections::HashSet::new();
        if let Some(o) = p.objects.iter().find(|o| !objects.insert(&o.id)) {
            return Some(format!("object id {:?} used twice on page {:?}", o.id, p.id));
        }
    }
    None
}

fn doc_hash(doc: &Document) -> [u8; 32] {
    Sha256::digest(serde_json::to_vec(doc).expect("a document serializes")).into()
}

fn boxed(id: &str, x: f64) -> Object {
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(x, 0.0, 80.0, 40.0), Style::default(), Label::default());
    o.id = Id(id.to_owned());
    o
}

fn base_doc() -> Document {
    let mut d = Document::new("board");
    d.pages[0].id = Id("p".into());
    d.pages[0].objects = vec![boxed("a", 0.0), boxed("b", 100.0)];
    d
}

fn joined_client() -> Client {
    let mut c = Client::new(ClientConfig::new("g", Role::Edit, vec![]));
    c.connect(0);
    c.take_outbox();
    let w = Welcome {
        participant: ParticipantId(1),
        color: (1, 2, 3),
        role: Role::Edit,
        doc_meta: inkboard_session::proto::DocMeta { name: "board".into(), seq: 0, pages: 1 },
        host_proto: inkboard_session::PROTO_VERSION,
        host_name: String::new(),
        participants: vec![],
    };
    c.handle(Message::Welcome(w), 0).unwrap();
    c.handle(Message::Snapshot(Snapshot::new(&base_doc(), 0, vec![])), 0).unwrap();
    c
}

/* ------------------------------------------------------------------------------------------------- JSON-level mutation */

fn nasty() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        Just(json!(true)),
        Just(json!(-1)),
        Just(json!(0)),
        Just(json!(u64::MAX)),
        Just(json!(i64::MIN)),
        Just(json!(1.797_693_134_862_315_7e308)),
        Just(json!(-1.797_693_134_862_315_7e308)),
        Just(json!(5e-324)),
        Just(json!(1e7)),
        Just(json!(10_000_001.0)),
        Just(json!(1_000_001.0)),
        Just(json!("")),
        Just(json!("\u{0}\u{202e}\u{fffd}\u{3164}")),
        Just(json!([])),
        Just(json!({})),
        Just(json!("a".repeat(70))),
        (1usize..130_000).prop_map(|n| json!("é".repeat(n))),
        (1usize..120).prop_map(|d| (0..d).fold(json!(0), |acc, _| json!([acc]))),
        (1usize..60_000).prop_map(|n| Value::Array(vec![json!(0.5); n])),
        any::<f64>().prop_filter("finite", |x| x.is_finite()).prop_map(|x| json!(x)),
        any::<i64>().prop_map(|x| json!(x)),
        "\\PC{0,24}".prop_map(Value::String),
    ]
}

#[derive(Clone, Debug)]
struct Mutation {
    path: Vec<u32>,
    kind: u8,
    with: Value,
}

fn mutation() -> impl Strategy<Value = Mutation> {
    (prop::collection::vec(any::<u32>(), 0..9), 0u8..4, nasty()).prop_map(|(path, kind, with)| Mutation { path, kind, with })
}

/// Replaces (kinds 0-2), removes (kind 3) or duplicates (kind 2 on an array) the node `path` leads to; a path that runs out of
/// containers replaces the node it ended on.
fn mutate(v: &mut Value, path: &[u32], kind: u8, with: &Value) {
    if let Some((&p, rest)) = path.split_first() {
        if let Value::Object(map) = v
            && !map.is_empty()
        {
            let key = map.keys().nth(p as usize % map.len()).cloned().unwrap_or_default();
            if rest.is_empty() && kind == 3 {
                map.remove(&key);
            } else if let Some(c) = map.get_mut(&key) {
                mutate(c, rest, kind, with);
            }
            return;
        }
        if let Value::Array(a) = v
            && !a.is_empty()
        {
            let i = p as usize % a.len();
            if rest.is_empty() && kind == 3 {
                a.remove(i);
            } else if rest.is_empty() && kind == 2 {
                let copy = a[i].clone();
                a.push(copy);
            } else {
                mutate(&mut a[i], rest, kind, with);
            }
            return;
        }
    }
    *v = with.clone();
}

fn rich_object(kind: u8, id: &str) -> Object {
    let mut o = match kind % 6 {
        0 => boxed(id, 3.0),
        1 => Object::new_pen(vec![Pt::new(0.0, 0.0), Pt::new(5.0, 9.0), Pt::new(7.0, 1.0)], false, Style::default()),
        2 => Object::new(Kind::Table(Table::new(Pt::new(1.0, 2.0), 2, 3)), Style::default()),
        3 => Object::new(Kind::Connector(Connector::new(End::bound(Id("a".into())), End::free(Pt::new(50.0, 50.0)))), Style::default()),
        4 => Object::new(
            Kind::Image(ImageObj { frame: Frame::new(0.0, 0.0, 10.0, 10.0), asset: Id("0".repeat(64)), latex: None, mono: false }),
            Style::default(),
        ),
        _ => {
            let mut b = boxed(id, 0.0);
            if let Kind::Box(x) = &mut b.kind {
                x.label.text = RichText::plain("hello");
            }
            b
        }
    };
    o.id = Id(id.to_owned());
    o
}

/// One valid message of every kind: the starting points of the mutations.
fn seeds() -> Vec<Value> {
    let page = Id("p".into());
    let put = |o: Object| WireOp::Put { page: page.clone(), object: Box::new(o) };
    let ops = vec![
        put(rich_object(0, "n0")),
        put(rich_object(1, "n1")),
        put(rich_object(2, "n2")),
        put(rich_object(3, "n3")),
        put(rich_object(4, "n4")),
        WireOp::Delete { page: page.clone(), ids: vec![Id("a".into())] },
        WireOp::Order { page: page.clone(), ids: vec![Id("b".into()), Id("a".into())] },
        WireOp::SetLayers { page: page.clone(), layers: vec![Layer::main()] },
        WireOp::AddPage { index: 1, page: Box::new(Page::new("two")) },
        WireOp::RenamePage { page: page.clone(), name: "x".into() },
        WireOp::RemovePage { page: Id("zz".into()) },
        WireOp::SetBackground { background: Default::default() },
        WireOp::SetName { name: "new".into() },
    ];
    let doc = base_doc();
    let mut big = base_doc();
    big.pages[0].objects.extend((0..40).map(|i| rich_object(i as u8, &format!("o{i}"))));
    let parts: Vec<Message> = into_messages(Snapshot::new(&big, 3, vec![]), 2_000);
    let mut v: Vec<Message> = vec![
        Message::Hello(Hello { proto: 2, name: "Anna".into(), join_proof: vec![1, 2, 3], role_requested: Role::Edit, resume: None }),
        Message::Snapshot(Snapshot::new(&doc, 5, vec![])),
        Message::Submit(Submit { client_seq: 1, base_seq: 0, ops: ops.clone() }),
        Message::Applied(Applied { seq: 1, author: ParticipantId(1), client_seq: Some(1), ops, superseded: vec![Id("a".into())] }),
        Message::AssetChunk(AssetChunk { hash: "0".repeat(64), offset: 0, data: vec![1, 2, 3], last: true }),
        Message::AssetRequest { hash: "0".repeat(64) },
        Message::Rejected { client_seq: 1, reason: RejectReason::Invalid { detail: "x".into() } },
        Message::Resync { last_seq: 0 },
        Message::Transient { object: Id("a".into()), state: Box::new(boxed("a", 5.0)), author: None },
        Message::StrokePoints { stroke: Id("s".into()), points: vec![Pt::new(1.0, 2.0)], author: None },
        Message::Presence(Presence {
            participant: None,
            page: page.clone(),
            cursor: Some(Pt::new(1.0, 1.0)),
            selection: vec![Id("a".into())],
            tool: Some("pen".into()),
            editing: Some(Id("a".into())),
            viewport: Some(inkboard_core::geom::Rect::new(0.0, 0.0, 100.0, 100.0)),
        }),
        Message::Participants { list: vec![] },
        Message::Undo,
        Message::Follow { participant: Some(ParticipantId(2)) },
        Message::Kick { participant: ParticipantId(1) },
        Message::Ping { nonce: 7 },
    ];
    v.extend(parts.into_iter().take(3));
    v.into_iter().map(|m| serde_json::from_slice(&JsonCodec.encode(&m)).expect("seed is JSON")).collect()
}

/// Checks everything the system does with bytes that claim to be a message. Panics (= a defect) on any broken promise.
fn exercise(bytes: &[u8]) {
    let Ok(msg) = JsonCodec.decode(bytes) else { return };
    let guest = Limits::default();
    let accepted = check_message(&guest, &msg).is_ok();
    let _ = check_message(&Limits::host_trust(), &msg);
    if accepted {
        // what the host lets through has to be relayable as is
        let back = JsonCodec.decode(&JsonCodec.encode(&msg)).expect("an accepted message re-encodes");
        assert_eq!(back, msg, "an accepted message does not survive encoding");
        if let Message::Submit(s) = &msg {
            let mut doc = base_doc();
            let before = doc_hash(&doc);
            let done = apply_wire_all(&mut doc, &s.ops);
            assert_eq!(doc_problem(&doc), None, "an accepted submit corrupts the document");
            rollback(&mut doc, &done);
            assert_eq!(doc_hash(&doc), before, "rollback does not restore the document");
        }
    }
    // the same bytes as if the host had sent them to a guest
    let mut c = joined_client();
    let before = doc_hash(c.doc());
    let r = c.handle(msg, 1);
    assert_eq!(doc_problem(c.doc()), None, "a host message corrupts the guest's document");
    if let Err(e) = r {
        if matches!(e, inkboard_session::Error::Invalid(_)) {
            assert_eq!(c.phase(), Phase::Closed, "an invalid host message must close the session");
        }
        assert_eq!(doc_hash(c.doc()), before, "a refused host message changed the document");
    }
    c.tick(10_000);
    c.take_outbox();
}

fn prop_mutated_messages(n: u32, sd: u64) {
    let seeds = seeds();
    let strat = (0..seeds.len(), prop::collection::vec(mutation(), 1..4));
    runner(n, sd)
        .run(&strat, |(i, muts)| {
            let mut v = seeds[i].clone();
            for m in &muts {
                mutate(&mut v, &m.path, m.kind, &m.with);
            }
            exercise(&serde_json::to_vec(&v).unwrap());
            Ok(())
        })
        .unwrap();
}

fn prop_raw_bytes(n: u32, sd: u64) {
    let seeds: Vec<Vec<u8>> = seeds().iter().map(|v| serde_json::to_vec(v).unwrap()).collect();
    let strat = (0..seeds.len(), prop::collection::vec((any::<usize>(), any::<u8>()), 0..12), any::<usize>(), prop::collection::vec(any::<u8>(), 0..64));
    runner(n, sd)
        .run(&strat, |(i, flips, cut, junk)| {
            let mut b = seeds[i].clone();
            for (at, byte) in flips {
                let len = b.len();
                b[at % len] = byte;
            }
            b.truncate(cut % (b.len() + 1));
            b.extend(junk);
            exercise(&b);
            Ok(())
        })
        .unwrap();
}

#[test]
fn mutated_messages_never_panic_corrupt_or_escape_validation() {
    prop_mutated_messages(cases(None, 1500), 11);
}

#[test]
fn damaged_bytes_never_panic_or_corrupt() {
    prop_raw_bytes(cases(None, 3000), 12);
}

#[test]
#[ignore = "long bench: see context/mcp-remote/test-robustesse.md"]
fn long_mutated_messages() {
    prop_mutated_messages(cases(Some(150_000), 0), seed(21, true));
    prop_raw_bytes(cases(Some(300_000), 0), seed(22, true));
}

/* ------------------------------------------------------------------------------------------- hostile operation sequences */

const POOL: [&str; 5] = ["a", "b", "c", "p", "q"];

fn pid(i: u8) -> Id {
    Id(POOL[i as usize % POOL.len()].into())
}

fn num(i: u8) -> f64 {
    [0.0, -0.0, 1.0, 40.0, 1e6, 1e7, -1e7, 1e300, f64::MAX, 5e-324, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 123.456, 1e7 + 1.0, -1.0][i as usize % 16]
}

/// An object built from small random choices: ids drawn from a tiny pool so that they collide (self-references, cycles, duplicates).
fn hostile_object(r: [u8; 8]) -> Object {
    let id = pid(r[1]);
    let mut o = match r[0] % 7 {
        0 => Object::new_box(BoxGeom::Rect, Frame::new(num(r[2]), num(r[3]), num(r[4]), num(r[5])), Style::default(), Label::default()),
        1 => Object::new_pen((0..(r[2] % 4)).map(|i| Pt::new(num(r[3].wrapping_add(i)), num(r[4])).clone()).collect(), false, Style::default()),
        2 => {
            let mut t = Table::new(Pt::new(num(r[2]), num(r[3])), 1 + usize::from(r[4] % 3), 1 + usize::from(r[5] % 3));
            if r[6].is_multiple_of(5) {
                t.cells.pop();
            }
            if r[6].is_multiple_of(7) {
                t.col_w[0] = num(r[7]);
            }
            Object::new(Kind::Table(t), Style::default())
        }
        3 => {
            let mut c = Connector::new(End::bound(pid(r[2])), End::bound(pid(r[3])));
            c.via = (0..(r[4] % 3)).map(|i| Pt::new(num(r[5].wrapping_add(i)), 1.0)).collect();
            Object::new(Kind::Connector(c), Style::default())
        }
        4 => Object::new(Kind::Image(ImageObj { frame: Frame::new(0.0, 0.0, num(r[2]), 10.0), asset: pid(r[3]), latex: None, mono: false }), Style::default()),
        5 => {
            let mut b = boxed(id.as_str(), 0.0);
            b.group = Some(pid(r[2]));
            b
        }
        _ => {
            let mut b = boxed(id.as_str(), 1.0);
            b.layer = Id(["main", "l2", "ghost"][usize::from(r[2] % 3)].into());
            b.locked = r[3].is_multiple_of(2);
            b
        }
    };
    o.id = id;
    o
}

fn hostile_op(r: [u8; 8]) -> WireOp {
    let page = pid(r[1]);
    let ids = |n: u8| (0..(n % 5)).map(|i| pid(r[2].wrapping_add(i))).collect::<Vec<_>>();
    match r[0] % 10 {
        0..=2 => WireOp::Put { page, object: Box::new(hostile_object([r[2], r[3], r[4], r[5], r[6], r[7], r[1], r[0]])) },
        3 => WireOp::Delete { page, ids: ids(r[3]) },
        4 => WireOp::Order { page, ids: ids(r[3]) },
        5 => WireOp::SetLayers {
            page,
            layers: (0..(r[3] % 4))
                .map(|i| Layer {
                    id: Id(["main", "l2", "ghost"][usize::from(r[4].wrapping_add(i) % 3)].into()),
                    name: "n".into(),
                    visible: true,
                    locked: r[5].is_multiple_of(2),
                })
                .collect(),
        },
        6 => {
            // a page that is not what it should be: id that exists already, objects that share an id, layers that do
            let mut p = Page::new("x");
            p.id = pid(r[2]);
            p.objects = (0..(r[3] % 4)).map(|i| hostile_object([r[4], r[5].wrapping_add(i % 2), r[6], r[7], 1, 1, 1, 1])).collect();
            if r[6].is_multiple_of(3) {
                p.layers.push(Layer::main());
            }
            WireOp::AddPage { index: usize::from(r[7]) * usize::from(r[4]), page: Box::new(p) }
        }
        7 => WireOp::RenamePage { page, name: "r".repeat(usize::from(r[3])) },
        8 => WireOp::RemovePage { page },
        _ => WireOp::SetName { name: "n".into() },
    }
}

fn ops_strategy() -> impl Strategy<Value = Vec<WireOp>> {
    prop::collection::vec(prop::array::uniform8(any::<u8>()).prop_map(hostile_op), 1..5)
}

/// Whatever a guest submits, the host either refuses it (validation) or applies it to a document that stays well formed; replaying
/// the `Applied` on a replica gives the same document; catching up from any `seq` never panics and is contiguous.
fn prop_sequencer(n: u32, sd: u64) {
    let strat = prop::collection::vec((0u8..3, ops_strategy(), any::<u64>(), any::<u64>()), 1..8);
    runner(n, sd)
        .run(&strat, |script| {
            let limits = Limits::default();
            let mut seqr = Sequencer::new(base_doc(), 4);
            let mut replica = base_doc();
            let mut accepted = 0u64;
            for (author, ops, base, cseq) in script {
                if check_ops(&limits, &ops).is_err() {
                    continue; // refused by validation, as the host does
                }
                let author = ParticipantId(u32::from(author));
                let s = Submit { client_seq: cseq % 4, base_seq: base, ops: ops.clone() };
                match seqr.submit(author, s.clone()) {
                    Submitted::Applied(a) => {
                        accepted += 1;
                        apply_wire_all(&mut replica, &a.ops);
                        prop_assert_eq!(a.seq, accepted);
                        // the same submit again is a duplicate, never applied twice
                        prop_assert!(matches!(seqr.submit(author, s), Submitted::Duplicate));
                    }
                    Submitted::Duplicate => {}
                }
                prop_assert_eq!(doc_problem(seqr.doc()), None);
                prop_assert_eq!(doc_hash(seqr.doc()), doc_hash(&replica));
                prop_assert_eq!(seqr.seq(), accepted);
            }
            for last in [0, 1, accepted.saturating_sub(1), accepted, accepted + 1, u64::MAX] {
                match seqr.catch_up(last, vec![]) {
                    CatchUp::Applied(v) => {
                        for (k, a) in v.iter().enumerate() {
                            prop_assert_eq!(a.seq, last + 1 + k as u64);
                        }
                        prop_assert_eq!(last + v.len() as u64, accepted);
                    }
                    CatchUp::Snapshot(s) => prop_assert_eq!(s.seq, accepted),
                }
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn hostile_operations_never_corrupt_the_sequencer() {
    prop_sequencer(cases(None, 1500), 31);
}

#[test]
#[ignore = "long bench: see context/mcp-remote/test-robustesse.md"]
fn long_hostile_operations() {
    prop_sequencer(cases(Some(200_000), 0), seed(41, true));
}

/* --------------------------------------------------------------------------------------- hostile host against the guest */

#[derive(Clone, Debug)]
enum HostMove {
    Welcome(u32),
    Snapshot { seq: u64, dup_page: bool, ops_dirty: [u8; 8] },
    Part { seq: u64, part: u32, last: bool, head: bool, objects: u8 },
    Applied { seq_delta: i8, extreme: bool, ops: Vec<WireOp> },
    Rejected(u64, bool),
    Misc(u8),
    Edit(Vec<WireOp>),
    Tick(u16),
    Lost,
    Reconnect,
}

fn host_move() -> impl Strategy<Value = HostMove> {
    prop_oneof![
        any::<u32>().prop_map(HostMove::Welcome),
        (prop_oneof![any::<u64>(), 0u64..8, Just(u64::MAX), Just(u64::MAX - 1)], any::<bool>(), prop::array::uniform8(any::<u8>()))
            .prop_map(|(seq, dup_page, ops_dirty)| HostMove::Snapshot { seq, dup_page, ops_dirty }),
        (prop_oneof![0u64..8, Just(u64::MAX)], prop_oneof![0u32..6, Just(u32::MAX), Just(4095), Just(4096)], any::<bool>(), any::<bool>(), 0u8..6)
            .prop_map(|(seq, part, last, head, objects)| HostMove::Part { seq, part, last, head, objects }),
        (-3i8..4, any::<bool>(), ops_strategy()).prop_map(|(seq_delta, extreme, ops)| HostMove::Applied { seq_delta, extreme, ops }),
        (0u64..6, any::<bool>()).prop_map(|(a, b)| HostMove::Rejected(a, b)),
        (0u8..6).prop_map(HostMove::Misc),
        ops_strategy().prop_map(HostMove::Edit),
        any::<u16>().prop_map(HostMove::Tick),
        Just(HostMove::Lost),
        Just(HostMove::Reconnect),
    ]
}

fn page_with(id: &str, n: u8) -> Page {
    let mut p = Page::new("p");
    p.id = Id(id.into());
    p.objects = (0..n).map(|i| boxed(&format!("{id}{i}"), f64::from(i))).collect();
    p
}

fn play(c: &mut Client, m: HostMove, now: &mut u64) {
    *now += 7;
    let before = (doc_hash(c.doc()), c.phase());
    let msg = match m {
        HostMove::Welcome(p) => Some(Message::Welcome(Welcome {
            participant: ParticipantId(p % 4),
            color: (1, 2, 3),
            role: Role::Edit,
            doc_meta: inkboard_session::proto::DocMeta { name: "x".into(), seq: 0, pages: 1 },
            host_proto: inkboard_session::PROTO_VERSION,
            host_name: String::new(),
            participants: vec![],
        })),
        HostMove::Snapshot { seq, dup_page, ops_dirty } => {
            let mut pages = vec![page_with("p", 2)];
            if dup_page {
                pages.push(page_with("p", 1));
            }
            if ops_dirty[0] % 3 == 0 {
                pages.push(page_with("p2", ops_dirty[1] % 3));
            }
            Some(Message::Snapshot(Snapshot { seq, name: "n".into(), background: Default::default(), pages, assets: vec![] }))
        }
        HostMove::Part { seq, part, last, head, objects } => Some(Message::SnapshotPart(SnapshotPart {
            seq,
            part,
            last,
            head: head.then(|| SnapshotHead { name: "n".into(), background: Default::default(), assets: vec![] }),
            pages: vec![page_with("p", objects)],
        })),
        HostMove::Applied { seq_delta, extreme, ops } => {
            let seq = if extreme { u64::MAX - u64::from(seq_delta.unsigned_abs()) } else { c.last_seq().saturating_add_signed(i64::from(seq_delta) + 1) };
            Some(Message::Applied(Applied { seq, author: ParticipantId(1), client_seq: Some(1), ops, superseded: vec![] }))
        }
        HostMove::Rejected(cs, dup) => {
            Some(Message::Rejected { client_seq: cs, reason: if dup { RejectReason::Duplicate { host_seq: u64::MAX } } else { RejectReason::Quota } })
        }
        HostMove::Misc(k) => Some(match k {
            0 => Message::Bye { reason: inkboard_session::proto::ByeReason::HostLeft },
            1 => Message::Kick { participant: ParticipantId(1) },
            2 => Message::RoleChange { participant: ParticipantId(1), role: Role::View },
            3 => Message::Ping { nonce: u64::MAX },
            4 => Message::Participants { list: vec![] },
            _ => Message::Undo,
        }),
        HostMove::Edit(ops) => {
            // the guest's own interface only makes modifications that pass the same checks as the host's
            if check_ops(&Limits::default(), &ops).is_ok() {
                let _ = c.edit(ops, *now);
            }
            None
        }
        HostMove::Tick(dt) => {
            *now += u64::from(dt) * 10;
            c.tick(*now);
            None
        }
        HostMove::Lost => {
            c.connection_lost();
            None
        }
        HostMove::Reconnect => {
            c.connect(*now);
            None
        }
    };
    if let Some(msg) = msg
        && let Err(e) = c.handle(msg, *now)
    {
        if matches!(e, inkboard_session::Error::Invalid(_)) {
            assert_eq!(c.phase(), Phase::Closed, "an invalid host message must close the session");
        }
        assert_eq!(doc_hash(c.doc()), before.0, "a refused host message changed the document");
    }
    c.take_outbox();
    c.take_events();
    assert_eq!(doc_problem(c.doc()), None, "the guest's document is corrupted");
    assert!(before.1 != Phase::Closed || c.phase() == Phase::Closed, "a closed session came back to life");
}

fn prop_hostile_host(n: u32, sd: u64) {
    runner(n, sd)
        .run(&prop::collection::vec(host_move(), 1..25), |script| {
            let mut c = joined_client();
            let mut now = 0u64;
            for m in script {
                play(&mut c, m, &mut now);
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_hostile_host_cannot_panic_or_corrupt_a_guest() {
    prop_hostile_host(cases(None, 2000), 51);
}

#[test]
#[ignore = "long bench: see context/mcp-remote/test-robustesse.md"]
fn long_hostile_host() {
    prop_hostile_host(cases(Some(300_000), 0), seed(61, true));
}

/* --------------------------------------------------------------------------------------------------- fixed attack cases */

#[test]
fn sequence_numbers_at_the_edge_of_u64_do_not_overflow() {
    let mut c = joined_client();
    // a host that claims the last possible sequence number, then goes on
    let snap = Snapshot { seq: u64::MAX, name: "n".into(), background: Default::default(), pages: vec![page_with("p", 1)], assets: vec![] };
    let _ = c.handle(Message::Snapshot(snap), 1);
    let applied = |seq| Message::Applied(Applied { seq, author: ParticipantId(2), client_seq: None, ops: vec![], superseded: vec![] });
    for seq in [u64::MAX, u64::MAX - 1, 0, 1] {
        let _ = c.handle(applied(seq), 2);
    }
    c.tick(100_000);
    c.take_outbox();
    assert_eq!(doc_problem(c.doc()), None);
}

#[test]
fn truncated_and_gigantic_declarations_are_plain_errors() {
    // a list of a million elements that is announced and never delivered, nested quantities, invalid UTF-8
    let open = br#"{"t":"submit","client_seq":1,"base_seq":0,"ops":[{"op":"delete","page":"p","ids":["#.to_vec();
    let mut cut = open.clone();
    cut.extend(std::iter::repeat_n(&br#""a","#[..], 400_000).flatten());
    assert!(JsonCodec.decode(&cut).is_err());
    let mut bad_utf8 = br#"{"t":"hello","proto":2,"name":""#.to_vec();
    bad_utf8.extend([0xff, 0xfe, 0xc0, 0x80, 0xed, 0xa0, 0x80]);
    bad_utf8.extend(br#"","join_proof":"","role_requested":"edit"}"#);
    assert!(JsonCodec.decode(&bad_utf8).is_err());
    for text in [
        r#"{"t":"submit","client_seq":1e999,"base_seq":0,"ops":[]}"#,
        r#"{"t":"submit","client_seq":-1,"base_seq":0,"ops":[]}"#,
        r#"{"t":"submit","client_seq":18446744073709551616,"base_seq":0,"ops":[]}"#,
        r#"{"t":"resync","last_seq":NaN}"#,
        r#"{"t":"resync","last_seq":Infinity}"#,
        r#"{"t":"asset_chunk","hash":"x","offset":0,"data":"not base64!!","last":true}"#,
        r#"{"t":"asset_chunk","hash":"x","offset":18446744073709551615,"data":"","last":false}"#,
    ] {
        let _ = JsonCodec.decode(text.as_bytes()); // an error or a message, never a panic
    }
    // a chunk at the last possible offset is refused by validation, not by an overflow
    let m = Message::AssetChunk(AssetChunk { hash: "0".repeat(64), offset: u64::MAX, data: vec![0; 10], last: true });
    assert!(check_message(&Limits::default(), &m).is_err());
}

#[test]
fn snapshot_parts_in_hostile_order_stay_bounded_and_typed() {
    let l = Limits::host_trust();
    // 5000 numbered parts that never end: refused at the cap, not stored without limit
    let mut asm = SnapshotAssembler::new(1);
    let mut refused = None;
    for part in 0..5000u32 {
        let p = SnapshotPart {
            seq: 1,
            part,
            last: false,
            head: (part == 0).then(|| SnapshotHead { name: "n".into(), background: Default::default(), assets: vec![] }),
            pages: vec![],
        };
        if let Err(e) = asm.add(p, &l, 200_000) {
            refused = Some((part, e));
            break;
        }
    }
    let (part, e) = refused.expect("a snapshot with no end must be refused");
    assert!(part <= 4096, "refused at part {part}");
    assert!(matches!(e, inkboard_session::Error::Invalid(_)), "{e:?}");
    // an end before parts that follow, two ends, a missing header
    let mut asm = SnapshotAssembler::new(1);
    let mk = |part, last, head: bool| SnapshotPart {
        seq: 1,
        part,
        last,
        head: head.then(|| SnapshotHead { name: "n".into(), background: Default::default(), assets: vec![] }),
        pages: vec![],
    };
    assert!(asm.add(mk(0, false, false), &l, 10).is_err(), "part 0 without its header");
    assert!(asm.add(mk(3, true, false), &l, 10).is_ok());
    assert!(asm.add(mk(5, true, false), &l, 10).is_err(), "two ends");
    assert!(asm.add(mk(4, false, false), &l, 10).is_err(), "a part after the end");
}

/* ------------------------------------------------------------------------------------------- regressions of defects found */

#[test]
fn a_page_cannot_arrive_with_an_identifier_used_twice() {
    let l = Limits::default();
    let dup_objects = {
        let mut p = Page::new("x");
        p.objects = vec![boxed("a", 0.0), boxed("a", 5.0)];
        WireOp::AddPage { index: 0, page: Box::new(p) }
    };
    assert!(check_ops(&l, &[dup_objects]).is_err(), "AddPage with two objects of the same id");
    let mut p = Page::new("x");
    p.layers.push(Layer::main());
    assert!(check_ops(&l, &[WireOp::AddPage { index: 0, page: Box::new(p) }]).is_err(), "AddPage with two layers of the same id");
    let layers = vec![Layer::main(), Layer::main()];
    assert!(check_ops(&l, &[WireOp::SetLayers { page: Id("p".into()), layers }]).is_err(), "SetLayers with two layers of the same id");
    // a hostile host: the same page twice in one snapshot is refused and the guest keeps its board
    let mut c = joined_client();
    let before = doc_hash(c.doc());
    let snap = Snapshot { seq: 9, name: "n".into(), background: Default::default(), pages: vec![page_with("p", 1), page_with("p", 2)], assets: vec![] };
    assert!(c.handle(Message::Snapshot(snap), 1).is_err());
    assert_eq!((c.phase(), doc_hash(c.doc())), (Phase::Closed, before));
}

#[test]
fn a_guest_does_not_keep_more_snapshot_bytes_than_its_budget() {
    // parts of about 100 KB each, none of them ending the snapshot: refused once the budget (1 MB here) is spent
    let l = Limits::host_trust();
    let mut asm = SnapshotAssembler::with_byte_budget(1, 1 << 20);
    let mut held = 0usize;
    let mut refused = false;
    for part in 0..200u32 {
        let mut o = Object::new_pen((0..4000).map(|i| Pt::new(f64::from(i), 1.5)).collect(), false, Style::default());
        o.id = Id(format!("big{part}"));
        let mut page = Page::new("p");
        page.id = Id("p".into());
        page.objects = vec![o];
        let head = (part == 0).then(|| SnapshotHead { name: "n".into(), background: Default::default(), assets: vec![] });
        match asm.add(SnapshotPart { seq: 1, part, last: false, head, pages: vec![page] }, &l, 100_000) {
            Ok(_) => held += 1,
            Err(e) => {
                assert!(matches!(e, inkboard_session::Error::Invalid(_)), "{e:?}");
                refused = true;
                break;
            }
        }
    }
    assert!(refused, "200 parts of 100 KB were all kept");
    assert!(held < 20, "kept {held} parts before refusing");
}
