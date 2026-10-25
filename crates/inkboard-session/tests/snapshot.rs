//! Snapshots in parts (S-04) and what a guest accepts from the host (S-09): the host's data is checked like a guest's.

use inkboard_core::geom::Frame;
use inkboard_core::model::{Document, Id, Object, Page};
use inkboard_core::style::Style;
use inkboard_core::{BoxGeom, Label};
use inkboard_session::limits::{Limits, RateLimiter, Violation, check_message};
use inkboard_session::proto::{Applied, DocMeta, Message, ParticipantId, Presence, Role, Snapshot, SnapshotPart, Welcome};
use inkboard_session::snapshot::{PART_BYTES, SnapshotAssembler, into_messages};
use inkboard_session::{Client, ClientConfig, ClientEvent, Codec, Error, JsonCodec, Phase, WireOp};

fn boxed(id: &str, x: f64) -> Object {
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(x, 0.0, 80.0, 40.0), Style::default(), Label::default());
    o.id = Id(id.to_owned());
    o
}

fn big_doc(n: usize) -> Document {
    let mut doc = Document::new("big");
    doc.pages[0].objects = (0..n).map(|i| boxed(&format!("o{i}"), i as f64)).collect();
    let mut second = Page::new("second");
    second.objects = (0..50).map(|i| boxed(&format!("s{i}"), i as f64)).collect();
    doc.pages.push(second);
    doc
}

fn parts_of(doc: &Document) -> Vec<SnapshotPart> {
    into_messages(Snapshot::new(doc, 7, vec![]), PART_BYTES)
        .into_iter()
        .map(|m| match m {
            Message::SnapshotPart(p) => p,
            other => panic!("expected parts, got {other:?}"),
        })
        .collect()
}

fn same_pages(a: &[Page], b: &[Page]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.id == y.id && x.name == y.name && x.objects == y.objects)
}

fn joined_client() -> Client {
    let mut c = Client::new(ClientConfig::new("g", Role::Edit, vec![]));
    c.connect(0);
    c.take_outbox();
    let welcome = Welcome {
        participant: ParticipantId(1),
        color: (1, 2, 3),
        role: Role::Edit,
        doc_meta: DocMeta { name: "big".into(), seq: 0, pages: 1 },
        host_proto: inkboard_session::PROTO_VERSION,
        host_name: String::new(),
        participants: vec![],
    };
    c.handle(Message::Welcome(welcome), 0).unwrap();
    c
}

#[test]
fn a_board_of_10000_objects_is_cut_in_frames_that_fit_and_comes_back_identical() {
    let doc = big_doc(10_000);
    let msgs = into_messages(Snapshot::new(&doc, 7, vec![]), PART_BYTES);
    assert!(msgs.len() > 5, "cut in several parts ({})", msgs.len());
    for m in &msgs {
        // the real transport refuses 1 MiB + 1 KiB; a part stays far below
        assert!(JsonCodec.encode(m).len() < 400 << 10, "a part fits a frame");
    }
    let mut c = joined_client();
    for m in msgs {
        c.handle(m, 0).unwrap();
    }
    assert!(c.is_live() && c.last_seq() == 7);
    assert!(same_pages(&c.doc().pages, &doc.pages), "same pages and objects in the same order");
    assert_eq!(c.doc().pages[0].objects.len(), 10_000);
}

#[test]
fn a_small_board_still_travels_as_one_snapshot() {
    let msgs = into_messages(Snapshot::new(&big_doc(10), 1, vec![]), PART_BYTES);
    assert!(matches!(msgs.as_slice(), [Message::Snapshot(_)]));
}

#[test]
fn duplicated_and_reordered_parts_are_put_back_together() {
    let doc = big_doc(3_000);
    let mut parts = parts_of(&doc);
    assert!(parts.len() > 2);
    parts.reverse();
    let dup = parts[1].clone();
    parts.insert(0, dup);
    let l = Limits::host_trust();
    let mut asm = SnapshotAssembler::new(7);
    let mut done = None;
    for p in parts {
        if let Some(s) = asm.add(p, &l, 100_000).unwrap() {
            assert!(done.is_none(), "completes once");
            done = Some(s);
        }
    }
    assert!(same_pages(&done.expect("complete").pages, &doc.pages));
}

#[test]
fn a_newer_snapshot_starts_over_and_an_older_straggler_is_ignored() {
    let doc = big_doc(3_000);
    let old = parts_of(&doc);
    let mut newer_doc = doc.clone();
    newer_doc.pages[0].objects.truncate(2_500);
    let newer: Vec<SnapshotPart> = into_messages(Snapshot::new(&newer_doc, 9, vec![]), PART_BYTES)
        .into_iter()
        .filter_map(|m| if let Message::SnapshotPart(p) = m { Some(p) } else { None })
        .collect();
    let mut c = joined_client();
    // the first half of the old snapshot, then the whole newer one, then the old tail: the newer wins
    for p in &old[..old.len() / 2] {
        c.handle(Message::SnapshotPart(p.clone()), 0).unwrap();
    }
    assert_eq!(c.last_seq(), 0, "not complete yet");
    for p in newer {
        c.handle(Message::SnapshotPart(p), 0).unwrap();
    }
    assert_eq!(c.last_seq(), 9);
    for p in &old[old.len() / 2..] {
        c.handle(Message::SnapshotPart(p.clone()), 0).unwrap();
    }
    assert_eq!(c.last_seq(), 9, "the old tail changes nothing");
    assert_eq!(c.doc().pages[0].objects.len(), 2_500);
}

#[test]
fn a_dropped_connection_forgets_the_parts_received_so_far() {
    let doc = big_doc(3_000);
    let parts = parts_of(&doc);
    let mut c = joined_client();
    c.handle(Message::SnapshotPart(parts[0].clone()), 0).unwrap();
    c.connection_lost();
    c.connect(10);
    c.take_outbox();
    // a series that restarts after the reconnection is not mixed with the one that was cut
    for p in &parts[1..] {
        c.handle(Message::SnapshotPart(p.clone()), 11).unwrap();
    }
    assert_eq!(c.last_seq(), 0, "the missing first part is never guessed");
}

#[test]
fn a_hostile_snapshot_is_refused_and_leaves_the_document_alone() {
    let mut c = joined_client();
    c.handle(Message::Snapshot(Snapshot::new(&big_doc(5), 1, vec![])), 0).unwrap();
    let before = c.doc().clone();
    let mut evil = big_doc(5);
    if let inkboard_core::model::Kind::Box(b) = &mut evil.pages[0].objects[2].kind {
        b.frame.x = 1e300;
    }
    let err = c.handle(Message::Snapshot(Snapshot::new(&evil, 2, vec![])), 0).unwrap_err();
    assert!(matches!(err, Error::Invalid(Violation::OutOfBounds { .. })), "{err:?}");
    assert_eq!(c.phase(), Phase::Closed);
    assert!(same_pages(&c.doc().pages, &before.pages), "the board is untouched");
    assert!(c.take_events().iter().any(|e| matches!(e, ClientEvent::Closed(_))));
}

#[test]
fn a_hostile_part_or_a_part_series_without_header_is_refused() {
    let mut evil = big_doc(3_000);
    if let inkboard_core::model::Kind::Box(b) = &mut evil.pages[0].objects[2_999].kind {
        b.frame.w = f64::INFINITY;
    }
    // the infinity does not even survive the encoding: a part carrying a huge number does
    let mut parts = parts_of(&big_doc(3_000));
    let last = parts.len() - 1;
    if let inkboard_core::model::Kind::Box(b) = &mut parts[last].pages[0].objects[0].kind {
        b.frame.y = 1e300;
    }
    let mut c = joined_client();
    let mut refused = false;
    for p in parts {
        refused |= c.handle(Message::SnapshotPart(p), 0).is_err();
    }
    assert!(refused && c.phase() == Phase::Closed && c.last_seq() == 0);
    let mut headless = joined_client();
    let p = SnapshotPart { seq: 1, part: 0, last: true, head: None, pages: vec![] };
    assert!(headless.handle(Message::SnapshotPart(p), 0).is_err());
}

#[test]
fn a_modification_from_the_host_over_the_bounds_is_refused() {
    let mut c = joined_client();
    c.handle(Message::Snapshot(Snapshot::new(&big_doc(3), 1, vec![])), 0).unwrap();
    let page = c.doc().pages[0].id.clone();
    let mut o = boxed("far", 0.0);
    if let inkboard_core::model::Kind::Box(b) = &mut o.kind {
        b.frame.x = 1e300;
    }
    let applied = Applied { seq: 2, author: ParticipantId::HOST, client_seq: None, ops: vec![WireOp::Put { page, object: Box::new(o) }], superseded: vec![] };
    assert!(c.handle(Message::Applied(applied), 0).is_err());
    assert_eq!(c.phase(), Phase::Closed);
    assert_eq!(c.last_seq(), 1);
    assert!(c.doc().pages[0].get(&Id("far".into())).is_none());
}

#[test]
fn the_queue_of_modifications_that_wait_for_an_earlier_one_is_bounded() {
    let mut c = joined_client();
    c.handle(Message::Snapshot(Snapshot::new(&big_doc(3), 1, vec![])), 0).unwrap();
    for seq in 3..=2_000u64 {
        let a = Applied { seq, author: ParticipantId::HOST, client_seq: None, ops: vec![], superseded: vec![] };
        c.handle(Message::Applied(a), 0).unwrap();
    }
    // the missing one arrives: only what was still held follows it. Unbounded, everything up to 2 000 would be applied
    let a = Applied { seq: 2, author: ParticipantId::HOST, client_seq: None, ops: vec![], superseded: vec![] };
    c.handle(Message::Applied(a), 0).unwrap();
    assert_eq!(c.last_seq(), 2, "the held ones were dropped: a resync fetches what is missing");
}

#[test]
fn what_the_host_relays_for_display_is_checked_too() {
    let mut c = joined_client();
    c.handle(Message::Snapshot(Snapshot::new(&big_doc(3), 1, vec![])), 0).unwrap();
    c.take_events();
    // a transient state of another object than the one it names
    let t = Message::Transient { object: Id("o1".into()), state: Box::new(boxed("o2", 5.0)), author: Some(ParticipantId(2)) };
    c.handle(t, 0).unwrap();
    // an `editing` identifier of 900 KB
    let p = Presence {
        participant: Some(ParticipantId(2)),
        page: Id("p".into()),
        cursor: None,
        selection: vec![],
        tool: None,
        editing: Some(Id("x".repeat(900_000))),
        viewport: None,
    };
    c.handle(Message::Presence(p), 0).unwrap();
    assert!(c.take_events().iter().all(|e| !matches!(e, ClientEvent::Relay(_))), "dropped, never shown");
    // a consistent one passes
    let ok = Message::Transient { object: Id("o1".into()), state: Box::new(boxed("o1", 5.0)), author: Some(ParticipantId(2)) };
    c.handle(ok, 0).unwrap();
    assert!(c.take_events().iter().any(|e| matches!(e, ClientEvent::Relay(_))));
}

#[test]
fn the_guest_limits_on_what_the_host_sends_are_checked_like_the_hosts_on_guests() {
    let l = Limits::default();
    let wrong = Message::Transient { object: Id("a".into()), state: Box::new(boxed("b", 0.0)), author: None };
    assert!(matches!(check_message(&l, &wrong), Err(Violation::Malformed(_))));
    let editing = Message::Presence(Presence {
        participant: None,
        page: Id("p".into()),
        cursor: None,
        selection: vec![],
        tool: None,
        editing: Some(Id("x".repeat(900_000))),
        viewport: None,
    });
    assert_eq!(check_message(&l, &editing), Err(Violation::BadId));
}

#[test]
fn a_byte_limiter_takes_the_size_of_what_it_counts() {
    let mut rl = RateLimiter::new(1_000, 4_000);
    assert!(rl.allow_n(0, 3_000));
    assert!(!rl.allow_n(0, 2_000), "not enough left, and nothing is taken by a refusal");
    assert!(rl.allow_n(0, 1_000));
    assert!(!rl.allow_n(0, 1));
    assert!(rl.allow_n(1_000, 1_000), "refilled with time");
}

#[test]
fn a_packed_board_is_much_smaller_and_comes_back_identical() {
    use inkboard_session::pack::pack;
    let doc = big_doc(10_000);
    let msgs = into_messages(Snapshot::new(&doc, 7, vec![]), PART_BYTES);
    let plain: usize = msgs.iter().map(|m| JsonCodec.encode(m).len()).sum();
    let packed: Vec<Message> = msgs.into_iter().map(|m| pack(&m).unwrap_or(m)).collect();
    let small: usize = packed.iter().map(|m| JsonCodec.encode(m).len()).sum();
    assert!(packed.iter().all(|m| matches!(m, Message::Packed { .. })), "every part is big enough to be packed");
    assert!(small * 3 < plain, "{small} bytes packed for {plain}: at least three times smaller");
    let mut c = joined_client();
    for m in packed {
        c.handle(m, 0).unwrap();
    }
    assert!(c.is_live() && c.last_seq() == 7);
    assert!(same_pages(&c.doc().pages, &doc.pages));
}

#[test]
fn a_packed_message_cannot_expand_without_limit_nor_hide_anything_but_a_snapshot() {
    use inkboard_session::pack::{pack, unpack};
    // a small frame that expands to 40 MB of zeros: refused at the ceiling, the guest closes
    let bomb = miniz_oxide::deflate::compress_to_vec(&vec![b' '; 40 << 20], 9);
    assert!(bomb.len() < 100 << 10);
    let mut c = joined_client();
    let r = c.handle(Message::Packed { data: bomb }, 0);
    assert!(r.is_err() && c.take_events().iter().any(|e| matches!(e, ClientEvent::Closed(_))));
    // a packed message that is not a board (here a Pong, and a Packed inside a Packed)
    let inner = serde_json::to_vec(&Message::Pong { nonce: 1 }).unwrap();
    assert!(unpack(&miniz_oxide::deflate::compress_to_vec(&inner, 1), 1 << 20).is_err());
    let part = &into_messages(Snapshot::new(&big_doc(500), 1, vec![]), PART_BYTES)[0];
    let nested = serde_json::to_vec(&pack(part).expect("big enough")).unwrap();
    assert!(unpack(&miniz_oxide::deflate::compress_to_vec(&nested, 1), 1 << 20).is_err());
}
