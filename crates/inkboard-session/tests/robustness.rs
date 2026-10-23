//! Hostile input: nothing that arrives from the network may panic the decoder, the cipher or the link parser.

use inkboard_core::geom::Frame;
use inkboard_core::model::{Id, Object};
use inkboard_core::style::Style;
use inkboard_core::{BoxGeom, Label};
use inkboard_session::codec::read_hello;
use inkboard_session::crypto::{FrameCipher, HostKeys, Link, SecureChannel, SeededEntropy, SessionKey, Side, Ticket};
use inkboard_session::proto::{Message, Submit};
use inkboard_session::{Codec, JsonCodec, WireOp};
use proptest::prelude::*;

fn cfg() -> ProptestConfig {
    ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() }
}

fn valid_message() -> Message {
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 10.0, 10.0), Style::default(), Label::default());
    o.id = Id("a".into());
    Message::Submit(Submit { client_seq: 1, base_seq: 0, ops: vec![WireOp::Put { page: Id("p".into()), object: Box::new(o) }] })
}

fn key() -> SessionKey {
    SessionKey::generate(&mut SeededEntropy::new(1)).unwrap()
}

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn random_bytes_never_panic_the_codec(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = JsonCodec.decode(&bytes);
        let _ = JsonCodec.peek_hello_proto(&bytes);
        let _ = read_hello(&JsonCodec, &bytes);
    }

    #[test]
    fn random_bytes_never_panic_the_cipher_or_the_channel(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let k = key();
        prop_assert!(FrameCipher::new(&k, Side::Host).open(&bytes).is_err());
        let ch = SecureChannel::new(FrameCipher::new(&k, Side::Host), Box::new(JsonCodec), Box::new(SeededEntropy::new(2)), 1 << 20);
        prop_assert!(ch.open(&bytes).is_err());
        let keys = HostKeys::generate(&mut SeededEntropy::new(3)).unwrap();
        prop_assert!(keys.open_first(&bytes).is_err());
    }

    #[test]
    fn random_text_never_panics_the_link_parser(text in ".*", raw in prop::collection::vec(any::<u8>(), 0..200)) {
        let _ = Ticket::decode(&text);
        let _ = Link::parse(&text);
        let _ = Link::parse(&format!("https://x.example/#{text}"));
        let _ = Ticket::decode(&String::from_utf8_lossy(&raw));
    }

    /// Valid JSON of the right shape with a few damaged bytes: the closest to a real attack on the decoder.
    #[test]
    fn damaged_valid_messages_never_panic(flips in prop::collection::vec((any::<usize>(), any::<u8>()), 1..8), cut in any::<usize>()) {
        let mut bytes = JsonCodec.encode(&valid_message());
        for (i, b) in flips {
            let n = bytes.len();
            bytes[i % n] = b;
        }
        let n = bytes.len();
        let _ = JsonCodec.decode(&bytes);
        let _ = JsonCodec.decode(&bytes[..cut % (n + 1)]);
    }

    /// A damaged valid frame is always refused (authenticated encryption), never panics.
    #[test]
    fn damaged_valid_frames_are_refused(i in any::<usize>(), b in 1u8..=255, cut in any::<usize>()) {
        let k = key();
        let frame = FrameCipher::new(&k, Side::Guest).seal(&JsonCodec.encode(&valid_message()), &mut SeededEntropy::new(4)).unwrap();
        let host = FrameCipher::new(&k, Side::Host);
        let mut bad = frame.clone();
        let n = bad.len();
        bad[i % n] ^= b;
        prop_assert!(host.open(&bad).is_err());
        prop_assert!(host.open(&frame[..cut % n]).is_err());
        prop_assert!(host.open(&frame).is_ok());
    }
}

#[test]
fn deeply_nested_json_is_an_error_not_a_stack_overflow() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    assert!(JsonCodec.decode(deep.as_bytes()).is_err());
    let nested = format!(r#"{{"t":"submit","client_seq":1,"base_seq":0,"ops":{deep}}}"#);
    assert!(JsonCodec.decode(nested.as_bytes()).is_err());
}
