//! Link and frame encryption: ticket format, altered tickets, wrong keys, per-role keys.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use inkboard_session::crypto::{FrameCipher, HostKeys, Link, SeededEntropy, SessionKey, Side, Ticket, join_proof, verify_join_proof};
use inkboard_session::proto::Role;
use inkboard_session::{Error, Message};

fn entropy(seed: u64) -> SeededEntropy {
    SeededEntropy::new(seed)
}

fn ticket() -> Ticket {
    let key = SessionKey::generate(&mut entropy(1)).unwrap();
    Ticket::new("node-1234", &["https://relay-a.example".to_owned(), "https://relay-b.example".to_owned()], key, Role::Edit).unwrap()
}

#[test]
fn a_link_round_trips_and_keeps_the_secret_in_the_fragment() {
    let link = Link { base: "https://inkboard.example/join".into(), ticket: ticket() };
    let url = link.to_url();
    let (before, after) = url.split_once('#').unwrap();
    assert_eq!(before, "https://inkboard.example/join");
    assert!(!before.contains(&URL_SAFE_NO_PAD.encode(link.ticket.key.as_bytes())));
    assert!(after.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "URL-safe: {after}");
    assert_eq!(Link::parse(&url).unwrap(), link);
    assert_eq!(Link::parse(&format!("  {url}\n")).unwrap(), link, "surrounding whitespace from copy/paste is ignored");
    assert_eq!(format!("{:?}", link.ticket.key), "SessionKey(..)", "the key never shows up in logs");
}

#[test]
fn every_altered_character_of_a_ticket_is_refused() {
    let text = ticket().encode();
    for i in 0..text.len() {
        let mut chars: Vec<char> = text.chars().collect();
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        let altered: String = chars.into_iter().collect();
        let err = Ticket::decode(&altered).expect_err(&format!("alteration at {i} accepted"));
        assert!(matches!(err, Error::BadTicket(_)));
        assert_ne!(err.to_string(), "");
    }
}

#[test]
fn truncated_empty_foreign_and_future_tickets_are_refused_with_a_reason() {
    let text = ticket().encode();
    for cut in [0, 1, 10, text.len() / 2, text.len() - 1] {
        assert!(matches!(Ticket::decode(&text[..cut]), Err(Error::BadTicket(_))), "cut at {cut}");
    }
    assert!(Ticket::decode("not a ticket at all !!!").unwrap_err().to_string().contains("bad encoding"));
    let foreign = URL_SAFE_NO_PAD.encode([0u8; 60]);
    assert!(Ticket::decode(&foreign).unwrap_err().to_string().contains("not an Inkboard link"));
    // a link made by a future format: clear message, whatever follows
    let mut bytes = URL_SAFE_NO_PAD.decode(&text).unwrap();
    bytes[3] = 2;
    let err = Ticket::decode(&URL_SAFE_NO_PAD.encode(&bytes)).unwrap_err().to_string();
    assert!(err.contains("v2") && err.contains("update Inkboard"), "{err}");
    assert!(Link::parse("https://inkboard.example/join").unwrap_err().to_string().contains("no ticket"));
}

#[test]
fn oversized_hints_are_refused_when_building_a_ticket() {
    let key = SessionKey::generate(&mut entropy(2)).unwrap();
    assert!(Ticket::new("", &[], key.clone(), Role::View).is_err());
    assert!(Ticket::new(&"h".repeat(300), &[], key.clone(), Role::View).is_err());
    assert!(Ticket::new("h", &vec!["r".to_owned(); 9], key, Role::View).is_err());
}

#[test]
fn frames_round_trip_between_guest_and_host_only() {
    let key = SessionKey::generate(&mut entropy(3)).unwrap();
    let (guest, host) = (FrameCipher::new(&key, Side::Guest), FrameCipher::new(&key, Side::Host));
    let mut e = entropy(4);
    let frame = guest.seal(b"hello host", &mut e).unwrap();
    assert_eq!(host.open(&frame).unwrap(), b"hello host");
    assert_eq!(guest.open(&frame), Err(Error::Decrypt), "a frame cannot be reflected back to its sender");
    let f2 = guest.seal(b"hello host", &mut e).unwrap();
    assert_ne!(frame, f2, "a new nonce for every frame");
}

#[test]
fn a_wrong_key_or_an_altered_frame_is_refused() {
    let key = SessionKey::generate(&mut entropy(5)).unwrap();
    let other = SessionKey::generate(&mut entropy(6)).unwrap();
    let guest = FrameCipher::new(&key, Side::Guest);
    let frame = guest.seal(b"secret board data", &mut entropy(7)).unwrap();
    let err = FrameCipher::new(&other, Side::Host).open(&frame).unwrap_err();
    assert_eq!(err, Error::Decrypt);
    assert!(err.to_string().contains("wrong session key or altered data"));
    let host = FrameCipher::new(&key, Side::Host);
    for i in 0..frame.len() {
        let mut bad = frame.clone();
        bad[i] ^= 0x01;
        assert!(host.open(&bad).is_err(), "alteration of byte {i} accepted");
    }
    assert!(host.open(&frame[..frame.len() - 1]).is_err());
    assert!(host.open(&[]).is_err());
    assert!(!String::from_utf8_lossy(&frame).contains("secret"), "the plaintext is not visible");
}

#[test]
fn the_key_that_opens_the_first_frame_tells_the_role() {
    let keys = HostKeys::generate(&mut entropy(8)).unwrap();
    assert_ne!(keys.edit, keys.view);
    let first = |role: Role| FrameCipher::new(keys.key(role), Side::Guest).seal(b"hello", &mut entropy(9)).unwrap();
    assert_eq!(keys.open_first(&first(Role::Edit)).unwrap().0, Role::Edit);
    assert_eq!(keys.open_first(&first(Role::View)).unwrap().0, Role::View);
    let stranger = SessionKey::generate(&mut entropy(10)).unwrap();
    let frame = FrameCipher::new(&stranger, Side::Guest).seal(b"hello", &mut entropy(9)).unwrap();
    assert_eq!(keys.open_first(&frame).unwrap_err(), Error::Decrypt);
    // regenerating the keys revokes the old links
    let regenerated = HostKeys::generate(&mut entropy(11)).unwrap();
    assert_eq!(regenerated.open_first(&first(Role::Edit)).unwrap_err(), Error::Decrypt);
    // the tickets carry the matching key and role
    let t = keys.ticket(Role::View, "host", &[]).unwrap();
    assert_eq!((t.role, &t.key), (Role::View, &keys.view));
}

#[test]
fn the_join_proof_is_bound_to_key_name_and_role() {
    let key = SessionKey::generate(&mut entropy(12)).unwrap();
    let other = SessionKey::generate(&mut entropy(13)).unwrap();
    let proof = join_proof(&key, "Anna", Role::Edit);
    assert!(verify_join_proof(&key, "Anna", Role::Edit, &proof));
    assert!(!verify_join_proof(&other, "Anna", Role::Edit, &proof));
    assert!(!verify_join_proof(&key, "Anne", Role::Edit, &proof));
    assert!(!verify_join_proof(&key, "Anna", Role::View, &proof));
    assert!(!verify_join_proof(&key, "Anna", Role::Edit, &proof[..31]));
}

#[test]
fn a_secure_channel_carries_messages_and_checks_the_size() {
    use inkboard_session::JsonCodec;
    use inkboard_session::crypto::SecureChannel;
    let key = SessionKey::generate(&mut entropy(14)).unwrap();
    let mk = |side, max| SecureChannel::new(FrameCipher::new(&key, side), Box::new(JsonCodec), Box::new(entropy(15)), max);
    let (mut guest, host) = (mk(Side::Guest, 1 << 20), mk(Side::Host, 1 << 20));
    let frame = guest.seal(&Message::Ping { nonce: 5 }).unwrap();
    assert_eq!(host.open(&frame).unwrap(), Message::Ping { nonce: 5 });
    let small = mk(Side::Host, 10);
    assert!(matches!(small.open(&frame), Err(Error::Invalid(_))), "oversized frames are refused before any decryption");
}
