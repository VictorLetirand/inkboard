//! Messages: every variant survives the codec; protocol version negotiation.

use inkboard_core::geom::{Frame, Pt, Rect};
use inkboard_core::model::{Document, Id, Layer, Object, Page};
use inkboard_core::style::Style;
use inkboard_core::{Background, BoxGeom, Label};
use inkboard_session::codec::read_hello;
use inkboard_session::proto::*;
use inkboard_session::{Codec, Error, JsonCodec, WireOp};

fn obj(id: &str) -> Object {
    let mut o = Object::new_box(BoxGeom::Ellipse, Frame::new(1.5, -2.25, 30.0, 20.0), Style::default(), Label::default());
    o.id = Id(id.into());
    o
}

fn all_wire_ops() -> Vec<WireOp> {
    let page = Id("p".into());
    vec![
        WireOp::Put { page: page.clone(), object: Box::new(obj("a")) },
        WireOp::Delete { page: page.clone(), ids: vec!["a".into()] },
        WireOp::Order { page: page.clone(), ids: vec!["b".into(), "a".into()] },
        WireOp::SetLayers { page: page.clone(), layers: vec![Layer::main()] },
        WireOp::AddPage { index: 2, page: Box::new(Page::new("new")) },
        WireOp::RenamePage { page: page.clone(), name: "renamed".into() },
        WireOp::RemovePage { page },
        WireOp::SetBackground { background: Background::default() },
        WireOp::SetName { name: "board".into() },
    ]
}

fn all_messages() -> Vec<Message> {
    let doc = Document::new("d");
    let id = |s: &str| Id(s.into());
    vec![
        Message::Hello(Hello { proto: PROTO_VERSION, name: "Anna".into(), join_proof: vec![1, 2, 3, 255], role_requested: Role::Edit, resume: None }),
        Message::Hello(Hello {
            proto: PROTO_VERSION,
            name: "Bob".into(),
            join_proof: vec![],
            role_requested: Role::View,
            resume: Some(Resume { participant: ParticipantId(4), last_seq: 17 }),
        }),
        Message::Welcome(Welcome {
            participant: ParticipantId(3),
            color: (1, 2, 3),
            role: Role::View,
            doc_meta: DocMeta { name: "d".into(), seq: 9, pages: 2 },
            host_proto: PROTO_VERSION,
            host_name: "Anna".into(),
            participants: vec![ParticipantInfo { id: ParticipantId(0), name: "Anna".into(), color: (9, 8, 7), role: Role::Edit }],
        }),
        Message::Participants { list: vec![ParticipantInfo { id: ParticipantId(3), name: "Bob (2)".into(), color: (1, 2, 3), role: Role::View }] },
        Message::Undo,
        Message::Redo,
        Message::Refused { reason: RefuseReason::Version { host_proto: 7 } },
        Message::Refused { reason: RefuseReason::Full },
        Message::Snapshot(Snapshot::new(&doc, 5, vec![AssetInfo { id: id("img"), hash: "ab".repeat(32), size: 1234, mime: "image/png".into() }])),
        Message::SnapshotPart(SnapshotPart {
            seq: 5,
            part: 0,
            last: false,
            head: Some(SnapshotHead { name: "d".into(), background: Background::default(), assets: vec![] }),
            pages: vec![Page::new("p")],
        }),
        Message::SnapshotPart(SnapshotPart { seq: 5, part: 1, last: true, head: None, pages: vec![] }),
        Message::AssetRequest { hash: "ab".repeat(32) },
        Message::AssetChunk(AssetChunk { hash: "cd".repeat(32), offset: 65_536, data: (0..=255).collect(), last: true }),
        Message::AssetAck { hash: "cd".repeat(32), have: 262_144, status: UploadStatus::Receiving },
        Message::AssetAck { hash: "cd".repeat(32), have: 0, status: UploadStatus::Stored },
        Message::AssetAck { hash: "cd".repeat(32), have: 0, status: UploadStatus::Refused { reason: RejectReason::Quota } },
        Message::Submit(Submit { client_seq: 3, base_seq: 2, ops: all_wire_ops() }),
        Message::Applied(Applied { seq: 4, author: ParticipantId::HOST, client_seq: None, ops: all_wire_ops(), superseded: vec![] }),
        Message::Applied(Applied { seq: 5, author: ParticipantId(2), client_seq: Some(8), ops: vec![], superseded: vec![id("a")] }),
        Message::Rejected { client_seq: 1, reason: RejectReason::Invalid { detail: "too big".into() } },
        Message::Rejected { client_seq: 2, reason: RejectReason::Duplicate { host_seq: 40 } },
        Message::Resync { last_seq: 12 },
        Message::Transient { object: id("a"), state: Box::new(obj("a")), author: Some(ParticipantId(1)) },
        Message::StrokePoints { stroke: id("s"), points: vec![Pt::new(0.1, 0.2), Pt::new(-3.0, 4e5)], author: None },
        Message::Presence(Presence {
            participant: Some(ParticipantId(2)),
            page: id("p"),
            cursor: Some(Pt::new(1.0, 2.0)),
            selection: vec![id("a"), id("b")],
            tool: Some("pen".into()),
            editing: Some(id("a")),
            viewport: Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
        }),
        Message::Presence(Presence { participant: None, page: id("p"), cursor: None, selection: vec![], tool: None, editing: None, viewport: None }),
        Message::Agent(AgentEvent { name: "Claude".into(), action: "add_shape".into(), targets: vec![id("a")], at: Some(Pt::new(5.0, 6.0)) }),
        Message::Follow { participant: Some(ParticipantId(2)) },
        Message::Follow { participant: None },
        Message::Kick { participant: ParticipantId(2) },
        Message::RoleChange { participant: ParticipantId(2), role: Role::Edit },
        Message::Bye { reason: ByeReason::HostLeft },
        Message::Bye { reason: ByeReason::ProtocolError { detail: "x".into() } },
        Message::Ping { nonce: u64::MAX },
        Message::Pong { nonce: 0 },
    ]
}

#[test]
fn every_message_survives_the_codec() {
    for m in all_messages() {
        let bytes = JsonCodec.encode(&m);
        assert_eq!(JsonCodec.decode(&bytes).unwrap(), m, "{}", String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn a_snapshot_carries_no_image_bytes() {
    let mut doc = Document::new("d");
    doc.add_asset("image/png", vec![7u8; 5000]);
    let manifest = inkboard_session::assets::manifest(&doc);
    let bytes = JsonCodec.encode(&Message::Snapshot(Snapshot::new(&doc, 1, manifest.clone())));
    assert!(bytes.len() < 2000, "the 5000 image bytes are not in the snapshot ({} bytes)", bytes.len());
    assert_eq!(manifest[0].size, 5000);
    assert_eq!(manifest[0].hash, inkboard_session::assets::hash_hex(&[7u8; 5000]));
}

fn hello_bytes(proto: u32) -> Vec<u8> {
    JsonCodec.encode(&Message::Hello(Hello { proto, name: "x".into(), join_proof: vec![], role_requested: Role::Edit, resume: None }))
}

#[test]
fn the_first_message_carries_the_protocol_version_and_a_mismatch_is_explicit() {
    assert!(read_hello(&JsonCodec, &hello_bytes(PROTO_VERSION)).is_ok());
    let err = read_hello(&JsonCodec, &hello_bytes(PROTO_VERSION + 1)).unwrap_err();
    assert_eq!(err, Error::IncompatibleProtocol { ours: PROTO_VERSION, theirs: PROTO_VERSION + 1 });
    let text = err.to_string();
    assert!(text.contains("incompatible protocol") && text.contains("update Inkboard"), "{text}");
}

#[test]
fn a_future_hello_with_other_fields_still_gets_the_version_error() {
    let future = br#"{"t":"hello","proto":99,"something_new":{"nested":[1,2,3]}}"#;
    assert_eq!(read_hello(&JsonCodec, future).unwrap_err(), Error::IncompatibleProtocol { ours: PROTO_VERSION, theirs: 99 });
}

#[test]
fn the_first_message_must_be_a_hello() {
    let bytes = JsonCodec.encode(&Message::Ping { nonce: 1 });
    assert!(matches!(read_hello(&JsonCodec, &bytes), Err(Error::Codec(_) | Error::Unexpected(_))));
    assert!(matches!(read_hello(&JsonCodec, b"not json"), Err(Error::Codec(_))));
}
