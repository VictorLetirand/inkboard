//! Host and guests over the real iroh transport, on the loopback interface with relays disabled: offline and deterministic.
//! Waits are polling loops with a generous bound that only prevents a hang; no test asserts a duration.
#![cfg(not(target_arch = "wasm32"))]

use inkboard_net::{IrohConn, IrohTransport, NetConfig, RelaySetting, addr_to_hints, ticket_to_addr};
use inkboard_session::crypto::{FrameCipher, HostKeys, SecureChannel, SeededEntropy, SessionKey, Side, Ticket};
use inkboard_session::error::{Error, NetCause};
use inkboard_session::limits::Violation;
use inkboard_session::proto::{Message, Role};
use inkboard_session::transport::{Connection, Transport};
use inkboard_session::{JsonCodec, Limits};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(30);

fn wait_for<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = probe() {
            return v;
        }
        assert!(start.elapsed() < WAIT, "gave up waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn cfg() -> NetConfig {
    NetConfig { connect_timeout: Duration::from_secs(10), ..NetConfig::loopback() }
}

fn host() -> (IrohTransport, HostKeys) {
    let keys = HostKeys::generate(&mut SeededEntropy::new(1)).unwrap();
    (IrohTransport::host(cfg()).unwrap(), keys)
}

fn accept(host: &mut IrohTransport) -> IrohConn {
    wait_for("an incoming connection", || host.accept())
}

fn recv(conn: &mut IrohConn) -> Vec<u8> {
    wait_for("a frame", || conn.recv())
}

fn secure_channel(cipher: FrameCipher, seed: u64) -> SecureChannel {
    SecureChannel::new(cipher, Box::new(JsonCodec), Box::new(SeededEntropy::new(seed)), Limits::default().max_message_bytes + 1024)
}

#[test]
fn frames_flow_both_ways_and_peers_are_identified() {
    let (mut host, keys) = host();
    let ticket = host.ticket(&keys, Role::Edit).unwrap();
    let mut guest = IrohTransport::guest(cfg()).unwrap();
    let mut g = guest.connect_ticket(&ticket).unwrap();
    assert_eq!(g.peer(), host.local_id().unwrap());

    g.send(b"hello").unwrap();
    let mut h = accept(&mut host);
    assert_eq!(h.peer(), guest.local_id().unwrap());
    assert_eq!(recv(&mut h), b"hello");

    // Order is kept, a large frame and an empty frame go through.
    let big = vec![0xAB; 500_000];
    h.send(b"one").unwrap();
    h.send(&big).unwrap();
    h.send(b"").unwrap();
    assert_eq!(recv(&mut g), b"one");
    assert_eq!(recv(&mut g), big);
    assert_eq!(recv(&mut g), b"");
    assert!(g.is_open() && h.is_open());
}

#[test]
fn two_guests_get_their_own_connection() {
    let (mut host, keys) = host();
    let ticket = host.ticket(&keys, Role::Edit).unwrap();
    let mut t1 = IrohTransport::guest(cfg()).unwrap();
    let mut t2 = IrohTransport::guest(cfg()).unwrap();
    let mut g1 = t1.connect_ticket(&ticket).unwrap();
    let mut g2 = t2.connect_ticket(&ticket).unwrap();
    g1.send(b"from-1").unwrap();
    g2.send(b"from-2").unwrap();
    let (id1, id2) = (t1.local_id().unwrap(), t2.local_id().unwrap());
    assert_ne!(id1, id2);

    let mut a = accept(&mut host);
    let mut b = accept(&mut host);
    for conn in [&mut a, &mut b] {
        let frame = recv(conn);
        let expected = if conn.peer() == id1 { b"from-1".to_vec() } else { b"from-2".to_vec() };
        assert!(conn.peer() == id1 || conn.peer() == id2);
        assert_eq!(frame, expected);
    }
    assert_ne!(a.peer(), b.peer());

    // Each one is answered on its own connection.
    a.send(b"to-a").unwrap();
    b.send(b"to-b").unwrap();
    let (ga, gb) = if a.peer() == id1 { (&mut g1, &mut g2) } else { (&mut g2, &mut g1) };
    assert_eq!(recv(ga), b"to-a");
    assert_eq!(recv(gb), b"to-b");
}

#[test]
fn a_clean_close_delivers_everything_then_closes_both_ends() {
    let (mut host, keys) = host();
    let ticket = host.ticket(&keys, Role::Edit).unwrap();
    let mut guest = IrohTransport::guest(cfg()).unwrap();
    let mut g = guest.connect_ticket(&ticket).unwrap();
    for i in 0..3u8 {
        g.send(&[i; 100]).unwrap();
    }
    g.close();
    assert!(!g.is_open());
    assert_eq!(g.send(b"late"), Err(Error::Closed));

    let mut h = accept(&mut host);
    for i in 0..3u8 {
        assert_eq!(recv(&mut h), vec![i; 100]);
    }
    wait_for("the host to see the close", || (!h.is_open()).then_some(()));
    assert_eq!(h.close_cause(), Some(Error::Closed));
    assert_eq!(h.send(b"x"), Err(Error::Closed));
}

#[test]
fn an_oversized_frame_is_refused_before_sending() {
    let (host, keys) = host();
    let ticket = host.ticket(&keys, Role::Edit).unwrap();
    let mut guest = IrohTransport::guest(NetConfig { max_frame: 1000, ..cfg() }).unwrap();
    let mut g = guest.connect_ticket(&ticket).unwrap();
    assert_eq!(g.send(&[0; 1001]), Err(Error::Invalid(Violation::MessageTooLarge { size: 1001, max: 1000 })));
    assert!(g.is_open());
    g.send(&[0; 1000]).unwrap();
}

#[test]
fn a_secure_channel_runs_over_the_transport() {
    let (mut host, keys) = host();
    for role in [Role::Edit, Role::View] {
        let ticket = host.ticket(&keys, role).unwrap();
        let mut guest = IrohTransport::guest(cfg()).unwrap();
        let mut g = guest.connect_ticket(&ticket).unwrap();
        let mut guest_channel = secure_channel(FrameCipher::new(&ticket.key, Side::Guest), 10);
        g.send(&guest_channel.seal(&Message::Ping { nonce: 7 }).unwrap()).unwrap();

        let mut h = accept(&mut host);
        let first = recv(&mut h);
        // The key that opens the first frame gives the role.
        let (granted, cipher, plain) = keys.open_first(&first).unwrap();
        assert_eq!(granted, role);
        let mut host_channel = secure_channel(cipher, 20);
        assert_eq!(host_channel.open(&first).unwrap(), Message::Ping { nonce: 7 });
        assert!(!plain.is_empty());
        h.send(&host_channel.seal(&Message::Pong { nonce: 7 }).unwrap()).unwrap();

        assert_eq!(guest_channel.open(&recv(&mut g)).unwrap(), Message::Pong { nonce: 7 });
    }
}

#[test]
fn a_wrong_key_is_refused_by_the_host() {
    let (mut host, keys) = host();
    let ticket = host.ticket(&keys, Role::Edit).unwrap();
    let mut guest = IrohTransport::guest(cfg()).unwrap();
    let mut g = guest.connect_ticket(&ticket).unwrap();
    let wrong = SessionKey::generate(&mut SeededEntropy::new(99)).unwrap();
    let mut channel = secure_channel(FrameCipher::new(&wrong, Side::Guest), 3);
    g.send(&channel.seal(&Message::Ping { nonce: 1 }).unwrap()).unwrap();

    let mut h = accept(&mut host);
    let frame = recv(&mut h);
    assert_eq!(keys.open_first(&frame).unwrap_err(), Error::Decrypt);
    // The host drops the connection; the guest sees it end.
    h.refuse();
    wait_for("the guest to see the refusal", || (!g.is_open()).then_some(()));
    assert_eq!(g.close_cause(), Some(Error::Network { cause: NetCause::Refused, detail: "the host refused the connection".to_owned() }));
}

#[test]
fn a_ticket_to_a_vanished_host_fails_as_unreachable() {
    let (host, keys) = host();
    let ticket = host.ticket(&keys, Role::Edit).unwrap();
    drop(host);
    let mut guest = IrohTransport::guest(NetConfig { connect_timeout: Duration::from_secs(2), ..cfg() }).unwrap();
    match guest.connect_ticket(&ticket) {
        Err(Error::Network { cause: NetCause::HostUnreachable, .. }) => {}
        other => panic!("expected HostUnreachable, got {other:?}"),
    }
}

#[test]
fn an_unreachable_relay_is_told_apart_from_an_unreachable_host() {
    let (host, keys) = host();
    // A relay on a port where nothing listens, and no direct address.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let relay = format!("http://127.0.0.1:{port}/");
    let ticket = Ticket::new(&host.local_id().unwrap(), std::slice::from_ref(&relay), keys.edit.clone(), Role::Edit).unwrap();
    let setting = RelaySetting::custom([relay]).unwrap();
    let mut guest = IrohTransport::guest(NetConfig { relays: setting, connect_timeout: Duration::from_secs(2), ..cfg() }).unwrap();
    match guest.connect_ticket(&ticket) {
        Err(Error::Network { cause: NetCause::RelayUnreachable, .. }) => {}
        other => panic!("expected RelayUnreachable, got {other:?}"),
    }
}

#[test]
fn bad_tickets_and_missing_addresses_are_rejected_at_once() {
    let (host, keys) = host();
    let mut guest = IrohTransport::guest(cfg()).unwrap();
    let bad_id = Ticket::new("not-an-endpoint-id", &[], keys.edit.clone(), Role::Edit).unwrap();
    assert!(matches!(guest.connect_ticket(&bad_id), Err(Error::BadTicket(_))));
    // Relays are disabled in this guest and the ticket only names a relay.
    let relay_only = Ticket::new(&host.local_id().unwrap(), &["https://relay.example.org/".to_owned()], keys.edit.clone(), Role::Edit).unwrap();
    assert!(matches!(guest.connect_ticket(&relay_only), Err(Error::Network { cause: NetCause::HostUnreachable, .. })));
    // `Transport::connect` takes an identity only.
    assert!(matches!(guest.connect("nope"), Err(Error::BadTicket(_))));
}

#[test]
fn ticket_hints_round_trip_through_the_iroh_address() {
    let (host, keys) = host();
    let ticket = host.ticket(&keys, Role::View).unwrap();
    assert_eq!(ticket.role, Role::View);
    assert_eq!(ticket.host, host.local_id().unwrap());
    assert!(!ticket.relays.is_empty() && ticket.relays.iter().all(|h| h.starts_with("127.0.0.1:")));
    let addr = ticket_to_addr(&ticket).unwrap();
    assert_eq!(addr.id.to_string(), ticket.host);
    assert_eq!(addr_to_hints(&addr), ticket.relays);
    // The textual link survives too.
    let again = Ticket::decode(&ticket.encode()).unwrap();
    assert_eq!(addr_to_hints(&ticket_to_addr(&again).unwrap()), ticket.relays);
}

#[test]
fn dropping_a_transport_right_after_closing_its_connections_delivers_the_last_frames() {
    let (mut host, keys) = host();
    let ticket = host.ticket(&keys, Role::Edit).unwrap();
    let mut guest = IrohTransport::guest(cfg()).unwrap();
    let mut g = guest.connect_ticket(&ticket).unwrap();
    g.send(b"hello").unwrap();
    let mut h = accept(&mut host);
    assert_eq!(recv(&mut h), b"hello");
    // what the application does when sharing stops: the last message, close, release the transport at once
    for i in 0..20u8 {
        h.send(&[i; 2000]).unwrap();
    }
    h.close();
    drop(host);
    for i in 0..20u8 {
        assert_eq!(recv(&mut g), vec![i; 2000]);
    }
    wait_for("the guest to see the end", || (!g.is_open()).then_some(()));
}

#[test]
fn current_hints_are_available_without_waiting_and_match_the_ticket() {
    let (host, keys) = host();
    let ticket = host.ticket(&keys, Role::Edit).unwrap();
    assert_eq!(host.current_hints(), ticket.relays);
    assert!(!ticket.relays.is_empty());
    let guest = IrohTransport::guest(cfg()).unwrap();
    assert!(guest.current_hints().is_empty());
}

#[test]
fn defaults_follow_the_decisions_of_the_plan() {
    let cfg = NetConfig::default();
    assert_eq!(cfg.idle_timeout, Duration::from_secs(120));
    assert!(matches!(cfg.relays, RelaySetting::Default));
    assert!(RelaySetting::custom(["not a url"]).is_err());
    assert!(RelaySetting::custom(["ftp://relay.example.org"]).is_err());
    assert!(matches!(RelaySetting::custom(Vec::<String>::new()), Ok(RelaySetting::Default)));
}

/// Real public relay of n0: needs Internet. Run by hand: `cargo +1.99.1 test -p inkboard-net -- --ignored public_relay`.
#[test]
#[ignore = "needs Internet access (public n0 relays)"]
fn public_relay_joins_through_the_relay_only() {
    let keys = HostKeys::generate(&mut SeededEntropy::new(5)).unwrap();
    let mut host = IrohTransport::host(NetConfig::default()).unwrap();
    let mut ticket = host.ticket(&keys, Role::Edit).unwrap();
    assert!(host.relay_online(), "the host did not reach a public relay");
    // Keep only the relay hints so that the path cannot be direct.
    ticket.relays.retain(|h| h.starts_with("http"));
    assert!(!ticket.relays.is_empty());
    let mut guest = IrohTransport::guest(NetConfig::default()).unwrap();
    let mut g = guest.connect_ticket(&ticket).unwrap();
    g.send(b"through the relay").unwrap();
    let mut h = accept(&mut host);
    assert_eq!(recv(&mut h), b"through the relay");
    h.send(b"ack").unwrap();
    assert_eq!(recv(&mut g), b"ack");
}
