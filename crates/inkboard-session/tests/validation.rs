//! Input validation: every limit, once exceeded, is refused (and the value just inside it is accepted).
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

use inkboard_core::geom::{Frame, Pt, Rect};
use inkboard_core::model::{Id, Kind, Object, Page, PenObj, Table};
use inkboard_core::style::Style;
use inkboard_core::{Align, BoxGeom, Color, Label};
use inkboard_session::assets::{AssetAssembler, chunks, hash_hex};
use inkboard_session::limits::*;
use inkboard_session::proto::{AssetChunk, Hello, Message, Presence, Role, Submit};
use inkboard_session::{Error, WireOp};
use std::sync::Arc;

fn l() -> Limits {
    Limits::default()
}

fn boxed(id: &str) -> Object {
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 80.0, 40.0), Style::default(), Label::default());
    o.id = Id(id.into());
    o
}

fn put(o: Object) -> WireOp {
    WireOp::Put { page: Id("p".into()), object: Box::new(o) }
}

fn with_frame(f: impl FnOnce(&mut Frame)) -> Object {
    let mut o = boxed("a");
    let Kind::Box(b) = &mut o.kind else { unreachable!() };
    f(&mut b.frame);
    o
}

fn refused(o: Object) -> bool {
    check_object(&l(), &o).is_err()
}

#[test]
fn ordinary_input_is_accepted() {
    assert_eq!(check_object(&l(), &boxed("a")), Ok(()));
    assert_eq!(check_ops(&l(), &[put(boxed("a")), WireOp::Delete { page: Id("p".into()), ids: vec!["a".into()] }]), Ok(()));
    let m = Message::Submit(Submit { client_seq: 1, base_seq: 0, ops: vec![put(boxed("a"))] });
    assert_eq!(check_message(&l(), &m), Ok(()));
}

#[test]
fn message_size() {
    let max = l().max_message_bytes;
    assert_eq!(check_message_size(&l(), max), Ok(()));
    assert_eq!(check_message_size(&l(), max + 1), Err(Violation::MessageTooLarge { size: max + 1, max }));
    let small = Limits { max_message_bytes: 100, ..l() };
    assert!(check_message_size(&small, 101).is_err(), "the limit is configurable");
}

#[test]
fn counts_per_message() {
    let name = |i: usize| WireOp::SetName { name: format!("n{i}") };
    assert!(check_ops(&l(), &(0..200).map(name).collect::<Vec<_>>()).is_ok());
    assert!(matches!(check_ops(&l(), &(0..201).map(name).collect::<Vec<_>>()), Err(Violation::TooMany { what: "operations", .. })));

    let puts = |n: usize| (0..n).map(|i| put(boxed(&format!("o{i}")))).collect::<Vec<_>>();
    let few = Limits { max_ops_per_message: 10_000, ..l() };
    assert!(check_ops(&few, &puts(500)).is_ok());
    assert!(matches!(check_ops(&few, &puts(501)), Err(Violation::TooMany { what: "objects", .. })));

    let del = |n: usize| vec![WireOp::Delete { page: Id("p".into()), ids: (0..n).map(|i| Id(format!("o{i}"))).collect() }];
    assert!(check_ops(&l(), &del(5000)).is_ok());
    assert!(matches!(check_ops(&l(), &del(5001)), Err(Violation::TooMany { what: "identifiers", .. })));

    // "bring to front" lists the whole page: a board of 20 000 objects on one page must be reorderable (found by the load test)
    let order = |n: usize| vec![WireOp::Order { page: Id("p".into()), ids: (0..n).map(|i| Id(format!("o{i}"))).collect() }];
    assert!(check_ops(&l(), &order(20_000)).is_ok());
    assert!(check_ops(&l(), &order(60_000)).is_ok());
    assert!(matches!(check_ops(&l(), &order(60_001)), Err(Violation::TooMany { what: "identifiers in the stacking order", .. })));

    let mut page = Page::new("big");
    page.objects = (0..501).map(|i| boxed(&format!("o{i}"))).collect();
    let add = WireOp::AddPage { index: 0, page: Box::new(page) };
    assert!(matches!(check_ops(&l(), &[add]), Err(Violation::TooMany { what: "objects", .. })), "objects inside a new page count too");
    assert!(matches!(check_page_count(&l(), 101), Err(Violation::TooMany { what: "pages", .. })));
    assert!(check_page_count(&l(), 100).is_ok());
}

#[test]
fn positions_sizes_and_angles() {
    assert!(!refused(with_frame(|f| f.x = 1e7)));
    assert!(refused(with_frame(|f| f.x = 1e7 + 1.0)));
    assert!(refused(with_frame(|f| f.y = -1e9)));
    assert!(refused(with_frame(|f| f.x = f64::NAN)));
    assert!(refused(with_frame(|f| f.y = f64::INFINITY)));
    assert!(refused(with_frame(|f| f.w = -1.0)));
    assert!(refused(with_frame(|f| f.h = 1e6 + 1.0)));
    assert!(refused(with_frame(|f| f.w = f64::NAN)));
    assert!(refused(with_frame(|f| f.angle = f64::NEG_INFINITY)));
}

#[test]
fn stroke_width_font_size_and_any_other_number() {
    let styled = |w: f64| {
        let mut o = boxed("a");
        o.style = Style { width: w, ..Style::default() };
        o
    };
    assert!(!refused(styled(1000.0)));
    assert!(refused(styled(1000.5)));
    assert!(refused(styled(-1.0)));
    assert!(refused(styled(f64::NAN)), "a non-finite number cannot hide as null in the JSON form");
    let sized = |s: f64| {
        let mut o = boxed("a");
        let Kind::Box(b) = &mut o.kind else { unreachable!() };
        b.label.size = s;
        o
    };
    assert!(!refused(sized(2000.0)));
    assert!(refused(sized(2001.0)));
    assert!(refused(sized(0.0)));
    assert!(refused(sized(f64::NAN)));
    let mut o = boxed("a");
    o.style.opacity = 1e12;
    assert!(refused(o), "any number is bounded, typed or not");
}

#[test]
fn text_and_names() {
    let texted = |n: usize| {
        let mut o = boxed("a");
        let Kind::Box(b) = &mut o.kind else { unreachable!() };
        b.label = Label::new(&"é".repeat(n), 20.0, Align::Left);
        o
    };
    assert!(!refused(texted(99_900)), "the limit covers all the strings of the object, ids included");
    assert!(matches!(check_object(&l(), &texted(100_001)), Err(Violation::TextTooLong { .. })), "counted in characters, not bytes");
    assert!(check_ops(&l(), &[WireOp::SetName { name: "x".repeat(200) }]).is_ok());
    assert!(matches!(check_ops(&l(), &[WireOp::SetName { name: "x".repeat(201) }]), Err(Violation::TextTooLong { .. })));
    assert!(check_ops(&l(), &[WireOp::RenamePage { page: Id("p".into()), name: "x".repeat(201) }]).is_err());
    let hello = |n: usize| Message::Hello(Hello { proto: 1, name: "x".repeat(n), join_proof: vec![], role_requested: Role::Edit, resume: None });
    assert!(check_message(&l(), &hello(200)).is_ok());
    assert!(check_message(&l(), &hello(201)).is_err());
}

#[test]
fn identifiers() {
    assert!(refused(boxed("")));
    assert!(!refused(boxed(&"i".repeat(64))));
    assert!(refused(boxed(&"i".repeat(65))));
    let mut o = boxed("a");
    o.layer = Id("l".repeat(65));
    assert!(refused(o));
    assert!(check_ops(&l(), &[WireOp::Delete { page: Id("p".into()), ids: vec![Id(String::new())] }]).is_err());
    assert!(check_ops(&l(), &[WireOp::Order { page: Id("x".repeat(65)), ids: vec![] }]).is_err());
}

#[test]
fn image_color_replacements_are_bounded() {
    let img = |n: usize| {
        let recolor = (0..n).map(|i| (Color::rgb(i as u8, 0, 0), Color::Ink)).collect();
        Object::new(
            Kind::Image(inkboard_core::ImageObj { frame: Frame::new(0.0, 0.0, 10.0, 10.0), asset: Id("a".into()), latex: None, mono: false, recolor }),
            Style::default(),
        )
    };
    assert!(!refused(img(256)));
    assert!(matches!(check_object(&l(), &img(257)), Err(Violation::TooMany { what: "color replacements", .. })));
}

#[test]
fn strokes_and_tables_must_be_consistent() {
    let pen = |n: usize, pressures: Option<usize>| {
        let pts: Vec<Pt> = (0..n).map(|i| Pt::new(i as f64, 0.0)).collect();
        Object::new(Kind::Pen(PenObj { pts: Arc::new(pts), highlighter: false, pressures: pressures.map(|k| Arc::new(vec![0.5f32; k])) }), Style::default())
    };
    assert!(!refused(pen(50_000, None)));
    assert!(matches!(check_object(&l(), &pen(50_001, None)), Err(Violation::TooMany { what: "points", .. })));
    assert!(!refused(pen(3, Some(3))));
    assert!(refused(pen(3, Some(2))), "the renderer indexes pressures by point");
    let mut far = pen(2, None);
    let Kind::Pen(p) = &mut far.kind else { unreachable!() };
    Arc::make_mut(&mut p.pts)[1] = Pt::new(f64::NAN, 0.0);
    assert!(refused(far));

    let table = |f: &dyn Fn(&mut Table)| {
        let mut t = Table::new(Pt::ZERO, 2, 2);
        f(&mut t);
        Object::new(Kind::Table(t), Style::default())
    };
    assert!(!refused(table(&|_| {})));
    assert!(refused(table(&|t| {
        t.cells.pop();
    })));
    assert!(refused(table(&|t| t.row_h.clear())));
    assert!(refused(table(&|t| t.col_w[0] = -5.0)));
    assert!(refused(table(&|t| t.origin = Pt::new(1e9, 0.0))));
    assert!(refused(Object::new(Kind::Table(Table::new(Pt::ZERO, 201, 1)), Style::default())));
    assert!(!refused(Object::new(Kind::Table(Table::new(Pt::ZERO, 200, 1)), Style::default())));
}

#[test]
fn images_and_quota() {
    let max = l().max_image_bytes;
    assert!(check_image_size(&l(), max).is_ok());
    assert_eq!(check_image_size(&l(), max + 1), Err(Violation::ImageTooLarge { size: max + 1, max }));

    let mut q = Quota::default();
    let mib = 1u64 << 20;
    for _ in 0..6 {
        q.charge(&l(), 15 * mib).unwrap();
    }
    assert_eq!(q.used(), 90 * mib);
    assert!(matches!(q.charge(&l(), 15 * mib), Err(Violation::QuotaExceeded { .. })));
    assert_eq!(q.used(), 90 * mib, "a refused image is not charged");
    q.charge(&l(), 10 * mib).unwrap();
    assert!(q.charge(&l(), 1).is_err());
    assert!(Quota::default().charge(&l(), 16 * mib).is_err(), "each image is also bounded alone");

    let chunk = |n: usize, offset: u64| AssetChunk { hash: "h".into(), offset, data: vec![0; n], last: false };
    assert!(check_asset_chunk(&l(), &chunk(256 << 10, 0)).is_ok());
    assert!(check_asset_chunk(&l(), &chunk((256 << 10) + 1, 0)).is_err());
    assert!(check_asset_chunk(&l(), &chunk(10, max)).is_err(), "a chunk cannot extend the image past the maximum");
    assert!(check_message(&l(), &Message::AssetChunk(chunk((256 << 10) + 1, 0))).is_err());
}

#[test]
fn ephemeral_messages() {
    let presence = |n: usize, cursor: Pt| {
        Message::Presence(Presence {
            participant: None,
            page: Id("p".into()),
            cursor: Some(cursor),
            selection: (0..n).map(|i| Id(format!("o{i}"))).collect(),
            tool: None,
            editing: None,
            viewport: Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
        })
    };
    assert!(check_message(&l(), &presence(5000, Pt::ZERO)).is_ok());
    assert!(check_message(&l(), &presence(5001, Pt::ZERO)).is_err());
    assert!(check_message(&l(), &presence(1, Pt::new(f64::NAN, 0.0))).is_err());
    assert!(check_message(&l(), &presence(1, Pt::new(1e8, 0.0))).is_err());

    let stroke = |n: usize| Message::StrokePoints { stroke: Id("s".into()), points: vec![Pt::ZERO; n], author: None };
    assert!(check_message(&l(), &stroke(50_000)).is_ok());
    assert!(check_message(&l(), &stroke(50_001)).is_err());
    let transient = Message::Transient { object: Id("a".into()), state: Box::new(with_frame(|f| f.w = -1.0)), author: None };
    assert!(check_message(&l(), &transient).is_err());
}

#[test]
fn rate_limit_with_a_virtual_clock() {
    let mut r = RateLimiter::new(50, 5);
    assert!((0..5).all(|_| r.allow(0)), "the burst");
    assert!(!r.allow(0));
    assert!(!r.allow(19), "50 per second: one token every 20 ms");
    assert!(r.allow(20));
    assert!(!r.allow(20));
    assert!((0..5).all(|_| r.allow(10_000)), "refilled, but never beyond the burst");
    assert_eq!(r.check(10_000, "operations"), Err(Violation::RateExceeded { what: "operations" }));
    assert!(!r.allow(5), "a clock going backwards neither refills nor panics");
}

#[test]
fn images_by_hash_are_chunked_and_verified() {
    let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    let hash = hash_hex(&data);
    assert_eq!(hash.len(), 64);
    let parts = chunks(&data, &hash, 300);
    assert_eq!(parts.len(), 4);
    assert!(parts[3].last && !parts[0].last);

    let mut asm = AssetAssembler::new(&hash, 1000, 15 << 20).unwrap();
    assert!(asm.push(&parts[0]).unwrap().is_none());
    assert!(asm.push(&parts[0]).unwrap().is_none(), "a repeated chunk is ignored");
    assert!(asm.push(&parts[2]).is_err(), "a gap is reported");
    asm.push(&parts[1]).unwrap();
    asm.push(&parts[2]).unwrap();
    assert_eq!(asm.push(&parts[3]).unwrap().unwrap(), data);

    let mut forged = chunks(&data, &hash, 300);
    forged[1].data[0] ^= 1;
    let mut asm = AssetAssembler::new(&hash, 1000, 15 << 20).unwrap();
    forged.iter().take(3).for_each(|c| {
        asm.push(c).unwrap();
    });
    assert_eq!(asm.push(&forged[3]).unwrap_err(), Error::BadFrame("image does not match its hash"));
    assert!(AssetAssembler::new(&hash, (15 << 20) + 1, 15 << 20).is_err(), "announced size is bounded");
    let mut small = AssetAssembler::new(&hash, 10, 15 << 20).unwrap();
    assert!(small.push(&parts[0]).is_err(), "more bytes than announced");
    assert_eq!(chunks(&[], "h", 10).len(), 1, "an empty image is one empty last chunk");
}
