//! The simulated network: deterministic, virtual clock, injectable faults. No test waits for real time.

use inkboard_session::Error;
use inkboard_session::sim::{NetConfig, NetStats, SimConn, SimNet};
use inkboard_session::transport::{Connection, Transport};

/// A guest connection and the matching host connection.
fn pair(net: &SimNet, guest_name: &str) -> (SimConn, SimConn) {
    let mut host = net.transport("host");
    let guest = net.transport(guest_name).connect("host").unwrap();
    (guest, host.accept().unwrap())
}

fn frame(i: u32) -> [u8; 4] {
    i.to_be_bytes()
}

fn number(f: &[u8]) -> u32 {
    u32::from_be_bytes(f.try_into().unwrap())
}

/// Sends `n` numbered frames, one per virtual millisecond, from the guest to the host, then lets the network deliver everything.
/// Returns what the host received as `(virtual delivery time, number)` and the network counters.
fn run(seed: u64, cfg: NetConfig, n: u32) -> (Vec<(u64, u32)>, NetStats) {
    let net = SimNet::new(seed, cfg);
    let (mut guest, mut host) = pair(&net, "guest");
    let mut got = Vec::new();
    let drain = |net: &SimNet, host: &mut SimConn, got: &mut Vec<(u64, u32)>| {
        while let Some(f) = host.recv() {
            got.push((net.now(), number(&f)));
        }
    };
    for i in 0..n {
        guest.send(&frame(i)).unwrap();
        net.advance(1);
        drain(&net, &mut host, &mut got);
    }
    while net.advance_to_next() {
        drain(&net, &mut host, &mut got);
    }
    assert_eq!(net.pending_frames(), 0);
    (got, net.stats())
}

fn faulty() -> NetConfig {
    NetConfig { latency_ms: (5, 300), loss_ppm: 200_000, dup_ppm: 200_000, reorder_ppm: 300_000, reorder_extra_ms: 500 }
}

fn inversions(got: &[(u64, u32)]) -> usize {
    got.windows(2).filter(|w| w[0].1 > w[1].1).count()
}

#[test]
fn the_same_seed_gives_exactly_the_same_deliveries() {
    let (a, stats_a) = run(42, faulty(), 400);
    let (b, stats_b) = run(42, faulty(), 400);
    assert_eq!(a, b, "same order and same virtual instants");
    assert_eq!(stats_a, stats_b);
    assert!(!a.is_empty());
    let (c, _) = run(43, faulty(), 400);
    assert_ne!(a, c, "another seed gives another run");
}

#[test]
fn latency_is_exact_on_the_virtual_clock() {
    let net = SimNet::new(1, NetConfig { latency_ms: (100, 100), ..NetConfig::ideal() });
    let (mut guest, mut host) = pair(&net, "guest");
    net.advance(7);
    guest.send(b"x").unwrap(); // sent at t = 7
    assert_eq!(host.recv(), None);
    net.advance(99); // t = 106
    assert_eq!(host.recv(), None, "not before t + latency");
    net.advance(1); // t = 107
    assert_eq!(host.recv().as_deref(), Some(&b"x"[..]));
    assert_eq!(net.now(), 107);
    assert_eq!(host.recv(), None);

    // the other direction has the same latency, and the clock jumps straight to the delivery
    host.send(b"y").unwrap();
    assert!(net.advance_to_next());
    assert_eq!(net.now(), 207);
    assert_eq!(guest.recv().as_deref(), Some(&b"y"[..]));
    assert!(!net.advance_to_next(), "nothing left in flight");
}

#[test]
fn a_latency_range_is_respected_for_every_frame() {
    let (got, _) = run(9, NetConfig { latency_ms: (20, 80), ..NetConfig::ideal() }, 200);
    assert_eq!(got.len(), 200);
    for (at, i) in got {
        let sent = u64::from(i) + 1; // frame i is sent at t = i, then the clock moves by 1 ms
        assert!((sent - 1 + 20..=sent - 1 + 80).contains(&at), "frame {i} delivered at {at}");
    }
}

#[test]
fn an_ideal_network_delivers_everything_once_and_in_order() {
    let (got, stats) = run(1, NetConfig::ideal(), 100);
    assert_eq!(got.iter().map(|g| g.1).collect::<Vec<_>>(), (0..100).collect::<Vec<_>>());
    assert_eq!(stats, NetStats { sent: 100, lost: 0, duplicated: 0, delivered: 100 });
}

#[test]
fn loss_edge_cases_and_rate() {
    let (got, stats) = run(5, NetConfig { loss_ppm: 1_000_000, ..NetConfig::ideal() }, 100);
    assert!(got.is_empty(), "100 % loss: nothing arrives");
    assert_eq!(stats, NetStats { sent: 100, lost: 100, duplicated: 0, delivered: 0 });

    let (_, stats) = run(5, NetConfig { loss_ppm: 300_000, ..NetConfig::ideal() }, 2000);
    assert_eq!(stats.sent, 2000);
    assert_eq!(stats.lost + stats.delivered, 2000, "every frame is either lost or delivered");
    assert!((500..=700).contains(&stats.lost), "about 30 % of 2000 lost, got {}", stats.lost);
}

#[test]
fn duplication_edge_cases_and_rate() {
    let (got, stats) = run(5, NetConfig { dup_ppm: 1_000_000, ..NetConfig::ideal() }, 100);
    assert_eq!(got.len(), 200, "100 % duplication: every frame twice");
    assert_eq!(stats, NetStats { sent: 100, lost: 0, duplicated: 100, delivered: 200 });

    let (got, stats) = run(5, NetConfig { dup_ppm: 250_000, ..NetConfig::ideal() }, 2000);
    assert_eq!(got.len() as u64, 2000 + stats.duplicated);
    assert!((400..=600).contains(&stats.duplicated), "about 25 % of 2000 duplicated, got {}", stats.duplicated);
    for i in 0..2000 {
        assert!(got.iter().any(|g| g.1 == i), "a duplicated network still delivers frame {i}");
    }
}

#[test]
fn reordering_edge_cases() {
    // fixed latency and no reordering: the order is the order of sending
    let (got, _) = run(3, NetConfig { latency_ms: (50, 50), ..NetConfig::ideal() }, 300);
    assert_eq!(inversions(&got), 0);
    // wide jitter reorders
    let (got, _) = run(3, NetConfig { latency_ms: (0, 400), ..NetConfig::ideal() }, 300);
    assert!(inversions(&got) > 50, "jitter reorders frames, got {} inversions", inversions(&got));
    // reorder_ppm alone, without jitter, also reorders; at 0 % it does not
    let extra = |ppm| NetConfig { latency_ms: (10, 10), reorder_ppm: ppm, reorder_extra_ms: 500, ..NetConfig::ideal() };
    let (got, stats) = run(3, extra(500_000), 300);
    assert!(inversions(&got) > 20, "got {} inversions", inversions(&got));
    assert_eq!((stats.lost, stats.duplicated, got.len()), (0, 0, 300), "reordering alone loses and duplicates nothing");
    let (got, _) = run(3, extra(0), 300);
    assert_eq!(inversions(&got), 0);
}

#[test]
fn a_closed_connection_delivers_nothing_more_and_refuses_sends() {
    let net = SimNet::new(1, NetConfig { latency_ms: (10, 10), ..NetConfig::ideal() });
    let (mut guest, mut host) = pair(&net, "guest");
    guest.send(b"in flight").unwrap();
    assert!(guest.is_open() && host.is_open());
    guest.close();
    assert!(!guest.is_open() && !host.is_open(), "closing is seen from both ends");
    net.advance(1000);
    assert_eq!(host.recv(), None, "what was in flight when the connection closed never arrives");
    assert_eq!(net.stats().delivered, 0);

    for conn in [&mut guest, &mut host] {
        let err = conn.send(b"late").unwrap_err();
        assert_eq!(err, Error::Closed);
        assert_eq!(err.to_string(), "connection closed");
    }
    assert_eq!(net.stats().sent, 1, "refused sends are not counted");
    assert_eq!(net.pending_frames(), 0);

    // closing from the host side is the same
    let (mut guest, mut host) = pair(&net, "guest2");
    host.close();
    assert_eq!(guest.send(b"x"), Err(Error::Closed));
}

#[test]
fn connecting_to_an_unknown_host_is_an_error() {
    let net = SimNet::new(1, NetConfig::ideal());
    assert!(net.transport("someone").connect("nobody").is_err());
}

#[test]
fn two_connections_are_independent() {
    let net = SimNet::new(1, NetConfig { latency_ms: (5, 5), ..NetConfig::ideal() });
    let mut listener = net.transport("host");
    let mut a = net.transport("anna").connect("host").unwrap();
    let mut b = net.transport("bob").connect("host").unwrap();
    let mut host_a = listener.accept().unwrap();
    let mut host_b = listener.accept().unwrap();
    assert!(listener.accept().is_none());
    assert_eq!((host_a.peer(), host_b.peer()), ("anna", "bob"));
    assert_ne!(a.id(), b.id());
    assert_eq!(a.id(), host_a.id());

    a.send(b"from anna").unwrap();
    b.send(b"from bob").unwrap();
    host_a.send(b"to anna").unwrap();
    host_b.send(b"to bob").unwrap();
    net.advance(5);
    assert_eq!(host_a.recv().as_deref(), Some(&b"from anna"[..]));
    assert_eq!(host_a.recv(), None);
    assert_eq!(host_b.recv().as_deref(), Some(&b"from bob"[..]));
    assert_eq!(host_b.recv(), None);
    assert_eq!(a.recv().as_deref(), Some(&b"to anna"[..]));
    assert_eq!(a.recv(), None);
    assert_eq!(b.recv().as_deref(), Some(&b"to bob"[..]));
    assert_eq!(b.recv(), None);

    // closing one leaves the other working
    a.send(b"lost with the connection").unwrap();
    a.close();
    b.send(b"still here").unwrap();
    net.advance(5);
    assert_eq!(host_a.recv(), None);
    assert!(host_b.is_open());
    assert_eq!(host_b.recv().as_deref(), Some(&b"still here"[..]));
}
