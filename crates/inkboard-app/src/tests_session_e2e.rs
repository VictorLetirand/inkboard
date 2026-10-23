//! End-to-end tests of shared sessions (lot I): two or three applications (kittest, one egui frame each) in the same process, linked by
//! the REAL `IrohTransport` on the loopback interface (no relay, no Internet). The host is the real `SessionHost` and the guests are the
//! real `GuestSession`s built by `session_net`.
//!
//! No test asserts a duration. Waiting is polling of the state of the applications, one frame at a time, with a bound on the number of
//! turns that only prevents a hang (the real network answers in a few milliseconds on loopback).

use crate::app::App;
use crate::i18n::{Lang, set_for_thread};
use crate::session_guest::{EndReason, GuestConfig, GuestState, NetClass, join_url, parse_join_link};
use crate::session_net::{GuestNet, IrohHostNet, NetDiagnoser};
use crate::tests::T;
use egui_kittest::kittest::Queryable;
use inkboard_core::Id;
use inkboard_net::{IrohTransport, NetConfig};
use inkboard_session::JsonCodec;
use inkboard_session::crypto::{FrameCipher, HostKeys, Link, SecureChannel, SeededEntropy, Side, Ticket, join_proof};
use inkboard_session::error::{Error, NetCause};
use inkboard_session::proto::{Hello, Message, PROTO_VERSION, RefuseReason, Role};
use inkboard_session::transport::{Connection, Transport};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

/// Upper bound of the polling loops (turns of one frame per application and a 2 ms pause).
const TURNS: usize = 3000;

/// Test diagnoser: the servers of these tests are on the loopback interface over plain HTTP (a mode the application never uses).
fn diagnoser() -> NetDiagnoser {
    NetDiagnoser { timeout: Duration::from_secs(3), trusted: Vec::new(), allow_private: true, relays_dir: None }
}

fn loopback_guest() -> GuestNet {
    GuestNet {
        net: NetConfig { connect_timeout: Duration::from_secs(10), ..NetConfig::loopback() },
        session: GuestConfig::default(),
        diagnoser: Arc::new(diagnoser()),
        relays_dir: None,
        watch_network: false,
    }
}

/// A host application and its guests, all in this process.
struct Room {
    host: T,
    guests: Vec<T>,
    dirs: Vec<std::path::PathBuf>,
}

impl Room {
    fn new(host_net: NetConfig) -> Self {
        set_for_thread(Some(Lang::En));
        let mut host = T::new();
        host.app().share.net = Some(Box::new(IrohHostNet::new(host_net)));
        Room { host, guests: Vec::new(), dirs: Vec::new() }
    }

    fn loopback() -> Self {
        Self::new(NetConfig::loopback())
    }

    fn turn(&mut self) {
        self.host.h.step();
        for g in &mut self.guests {
            g.h.step();
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    fn until(&mut self, what: &str, mut cond: impl FnMut(&mut Room) -> bool) {
        for _ in 0..TURNS {
            if cond(self) {
                return;
            }
            self.turn();
        }
        panic!("gave up waiting for: {what}");
    }

    fn turns(&mut self, n: usize) {
        for _ in 0..n {
            self.turn();
        }
    }

    /// "Start sharing" (what the button does): the start runs on a thread, the frames go on until it is done.
    fn start_sharing(&mut self) {
        crate::share_ui::start(self.host.app());
        self.until("sharing to start", |r| {
            let a = r.host.app();
            a.share.starting.is_none() && (a.share.is_running() || a.share.error.is_some())
        });
        let a = self.host.app();
        assert!(a.share.is_running(), "sharing did not start: {:?}", a.share.error);
    }

    fn link(&mut self, role: Role) -> String {
        let a = self.host.app();
        let (host, addr) = (a.share.host.as_ref().expect("a session"), a.share.addr.as_ref().expect("an address"));
        host.link(role, addr).expect("a link")
    }

    fn add_guest_with(&mut self, name: &str, link: &str, net: GuestNet) -> usize {
        let dir = std::env::temp_dir().join(format!("inkboard-e2e-{}", Id::new()));
        let mut t = T::new();
        t.app().join.factory = Some(net.into_factory());
        t.app().join.name_dir = Some(dir.clone());
        t.app().open_join_window(Some(link.to_owned()));
        if let Some(w) = t.app().join.window.as_mut() {
            w.name = name.to_owned();
        }
        t.app().submit_join();
        self.dirs.push(dir);
        self.guests.push(t);
        self.guests.len() - 1
    }

    /// The guest pastes the link and clicks "Join" (the host has not answered yet).
    fn add_guest(&mut self, name: &str, role: Role) -> usize {
        let link = self.link(role);
        self.add_guest_with(name, &link, loopback_guest())
    }

    /// The host accepts the request of `name` with `role`; waits until that guest has the board.
    fn admit(&mut self, guest: usize, name: &str, role: Role) {
        self.until(&format!("the request of {name}"), |r| {
            let host = r.host.app().share.host.as_ref().expect("a session");
            host.pending().iter().any(|p| p.name == name)
        });
        let host = self.host.app().share.host.as_mut().expect("a session");
        let request = host.pending().iter().find(|p| p.name == name).map(|p| p.request).expect("the request");
        host.accept(request, role);
        self.until(&format!("{name} to have the board"), |r| {
            r.guests[guest].app().guest.as_ref().is_some_and(|g| g.state() == GuestState::Live && g.mirror_ready())
        });
    }

    fn join(&mut self, name: &str, role: Role) -> usize {
        let i = self.add_guest(name, role);
        self.admit(i, name, role);
        i
    }

    fn guest_state(&mut self, guest: usize) -> GuestState {
        self.guests[guest].app().guest.as_ref().map(|g| g.state()).expect("a guest session")
    }

    fn host_has(&mut self, id: &Id) -> bool {
        self.host.app().ed.doc.pages[0].get(id).is_some()
    }

    fn guest_has(&mut self, guest: usize, id: &Id) -> bool {
        self.guests[guest].app().ed.doc.pages[0].get(id).is_some()
    }

    fn members(&mut self) -> Vec<String> {
        self.host.app().share.host.as_ref().expect("a session").members().into_iter().map(|m| m.name).collect()
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        for d in &self.dirs {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

fn has_cursor_of(app: &mut App, name: &str) -> bool {
    app.agent.remote.people.values().any(|p| p.state.participant.name == name)
}

/// A one-answer HTTP server standing for the connection service (a gateway or a relay); returns its base URL.
fn serve(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}/")
}

fn closed_port_url() -> String {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    format!("http://127.0.0.1:{port}/")
}

/// A valid ticket of a host that does not exist any more (its endpoint is closed).
fn dead_host_ticket() -> Ticket {
    let keys = HostKeys::generate(&mut SeededEntropy::new(7)).unwrap();
    let host = IrohTransport::host(NetConfig::loopback()).unwrap();
    host.ticket(&keys, Role::Edit).unwrap()
}

/* ------------------------------------------------------------------------------------------------ sharing and joining */

#[test]
fn the_link_of_the_host_is_read_by_the_guests_parser_and_nothing_starts_before_the_click() {
    let mut room = Room::loopback();
    // before "Start sharing": a network object is installed but nothing runs, nothing is offered
    assert!(!room.host.app().share.is_running() && room.host.app().share.addr.is_none());
    assert_eq!(room.host.app().share.net.as_mut().expect("net").refresh(), None);
    room.start_sharing();
    let (edit, view) = (room.link(Role::Edit), room.link(Role::View));
    assert!(edit.starts_with("inkboard://join#") && view.starts_with("inkboard://join#"), "{edit}");
    let (te, tv) = (parse_join_link(&edit).expect("edit link"), parse_join_link(&view).expect("view link"));
    assert_eq!((te.role, tv.role), (Role::Edit, Role::View));
    assert_ne!(te.key, tv.key);
    let id = room.host.app().share.addr.as_ref().expect("addr").host.clone();
    assert_eq!(te.host, id);
    assert!(te.relays.iter().all(|h| h.parse::<std::net::SocketAddr>().is_ok_and(|a| a.ip().is_loopback())), "loopback host: {:?}", te.relays);
    // the same ticket through the shapes a person can paste
    assert_eq!(parse_join_link(&edit.replace("join#", "join/")).expect("slash form").host, id);
    assert_eq!(Link::parse(&edit).expect("link").ticket, te);
    assert_eq!(join_url(&te).trim_start_matches("inkboard://join/"), te.encode());
}

#[test]
fn a_guest_joins_with_a_name_and_both_boards_stay_the_same_in_both_directions() {
    let mut room = Room::loopback();
    room.host.add_rect(0.0, 0.0, 120.0, 80.0);
    room.start_sharing();
    let g = room.add_guest("Marc", Role::Edit);
    // the host is asked, by name, and nothing is shared before it answers
    room.until("the request", |r| r.host.app().share.host.as_ref().is_some_and(|h| !h.pending().is_empty()));
    assert_eq!(room.host.app().share.host.as_ref().unwrap().pending()[0].name, "Marc");
    assert!(room.host.app().share.open, "the Share window opens on a join request");
    assert!(!room.guests[g].app().guest_active(), "no board before the agreement");
    room.admit(g, "Marc", Role::Edit);
    assert_eq!(room.members(), ["Marc"]);
    let seed = room.host.app().ed.doc.pages[0].objects[0].id.clone();
    assert!(room.guest_has(g, &seed), "the guest has the host's board");
    assert_eq!(room.guests[g].app().guest.as_ref().unwrap().host_name(), "Host");

    // guest -> host
    let from_guest = room.guests[g].add_rect(300.0, 0.0, 100.0, 60.0);
    room.until("the host to get the guest's shape", |r| r.host_has(&from_guest));
    // host -> guest
    let from_host = room.host.add_rect(0.0, 200.0, 100.0, 60.0);
    room.until("the guest to get the host's shape", |r| r.guest_has(g, &from_host));
    room.turns(5);
    let (h, m) = (room.host.app().ed.doc.pages[0].objects.clone(), room.guests[g].app().ed.doc.pages[0].objects.clone());
    assert_eq!(h, m, "same objects in the same order");
}

#[test]
fn a_read_only_guest_cannot_write() {
    let mut room = Room::loopback();
    room.start_sharing();
    let g = room.join("Viewer", Role::View);
    assert_eq!(room.guests[g].app().guest.as_ref().unwrap().role(), Role::View);
    assert!(room.guests[g].app().view_only());
    let refused = room.guests[g].add_rect(10.0, 10.0, 50.0, 50.0);
    room.turns(40);
    assert!(!room.guest_has(g, &refused), "the guest's own editor refuses");
    assert!(!room.host_has(&refused) && room.host.app().ed.doc.pages[0].objects.is_empty(), "nothing reached the host");
    // the host still reaches the viewer
    let shown = room.host.add_rect(0.0, 0.0, 50.0, 50.0);
    room.until("the viewer to see the host's shape", |r| r.guest_has(g, &shown));
    // an edit link accepted as read-only by the host stays read-only
    let h = room.add_guest("Demoted", Role::Edit);
    room.admit(h, "Demoted", Role::View);
    assert!(room.guests[h].app().view_only());
}

#[test]
fn two_guests_see_each_other_with_names_and_cursors() {
    let mut room = Room::loopback();
    room.start_sharing();
    let anna = room.join("Anna", Role::Edit);
    let marc = room.join("Marc", Role::Edit);
    assert_eq!(room.members(), ["Anna", "Marc"]);
    room.until("the rosters", |r| {
        let names = |t: &mut T| t.app().guest.as_ref().unwrap().roster().iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        names(&mut r.guests[anna]).len() == 3 && names(&mut r.guests[marc]).len() == 3
    });
    let roster: Vec<String> = room.guests[anna].app().guest.as_ref().unwrap().roster().iter().map(|p| p.name.clone()).collect();
    assert_eq!(roster, ["Host", "Anna", "Marc"]);

    // cursors: each pointer is over its own canvas
    let p = room.host.at(150.0, 120.0);
    room.host.mv(p);
    for g in [anna, marc] {
        let p = room.guests[g].at(150.0, 120.0);
        room.guests[g].mv(p);
    }
    room.until("the cursors", |r| {
        has_cursor_of(r.host.app(), "Anna")
            && has_cursor_of(r.host.app(), "Marc")
            && has_cursor_of(r.guests[anna].app(), "Marc")
            && has_cursor_of(r.guests[anna].app(), "Host")
            && has_cursor_of(r.guests[marc].app(), "Anna")
    });

    // a shape of one guest reaches the other guest through the host, and the reverse
    let a = room.guests[anna].add_rect(0.0, 0.0, 60.0, 60.0);
    room.until("Marc to get Anna's shape", |r| r.guest_has(marc, &a));
    let m = room.guests[marc].add_rect(100.0, 0.0, 60.0, 60.0);
    room.until("Anna and the host to get Marc's shape", |r| r.guest_has(anna, &m) && r.host_has(&m));
    assert!(room.host_has(&a));
}

/* -------------------------------------------------------------------------------------- outage, departure, versions */

#[test]
fn a_cut_connection_comes_back_with_the_same_participant_and_keeps_the_pending_changes() {
    let mut room = Room::loopback();
    room.start_sharing();
    let g = room.join("Marc", Role::Edit);
    let before = room.guests[g].app().guest.as_ref().unwrap().participant();
    assert!(before.is_some());
    let first = room.guests[g].add_rect(0.0, 0.0, 50.0, 50.0);
    room.until("the first shape", |r| r.host_has(&first));

    // the network drops under the guest, who edits meanwhile
    room.guests[g].app().guest.as_mut().unwrap().cut_connection();
    let offline = room.guests[g].add_rect(200.0, 0.0, 50.0, 50.0);
    let mut reconnecting = false;
    room.until("the guest to be back", |r| {
        let state = r.guest_state(g);
        reconnecting |= state == GuestState::Reconnecting;
        reconnecting && state == GuestState::Live && r.host_has(&offline)
    });
    assert_eq!(room.guests[g].app().guest.as_ref().unwrap().participant(), before, "the same place");
    assert_eq!(room.members(), ["Marc"], "no second participant");
    assert!(room.host.app().share.host.as_ref().unwrap().pending().is_empty(), "no new request to answer");
    assert_eq!(room.guests[g].app().guest.as_ref().unwrap().pending_len(), 0);

    // both directions work again
    let after_host = room.host.add_rect(0.0, 300.0, 50.0, 50.0);
    room.until("the guest to get the host's shape", |r| r.guest_has(g, &after_host));
    let after_guest = room.guests[g].add_rect(300.0, 300.0, 50.0, 50.0);
    room.until("the host to get the guest's shape", |r| r.host_has(&after_guest));
}

#[test]
fn the_host_leaving_ends_the_session_and_offers_a_copy_and_the_endpoint_is_released() {
    let mut room = Room::loopback();
    room.start_sharing();
    let g = room.join("Marc", Role::Edit);
    let ticket = parse_join_link(&room.link(Role::Edit)).unwrap();
    let kept = room.host.add_rect(0.0, 0.0, 80.0, 80.0);
    room.until("the shape", |r| r.guest_has(g, &kept));

    crate::share_ui::stop(room.host.app());
    assert!(!room.host.app().share.is_running());
    room.until("the end of the session", |r| r.guest_state(g) == GuestState::Ended(EndReason::HostLeft));
    assert!(room.guests[g].app().guest_active(), "the mirror stays on screen");
    assert!(room.guest_has(g, &kept), "and holds the whole board");
    room.turns(3);
    assert!(room.guests[g].h.query_by_label("Save a copy of the board").is_some(), "a copy is offered");
    assert!(room.guests[g].h.query_by_label_contains("The host has ended the session.").is_some());

    // the host's endpoint and threads are gone: nobody answers at the old address
    let mut late = IrohTransport::guest(NetConfig { connect_timeout: Duration::from_secs(1), ..NetConfig::loopback() }).unwrap();
    let err = late.connect_ticket(&ticket).expect_err("the endpoint is closed");
    assert!(matches!(err, Error::Network { cause: NetCause::HostUnreachable, .. }), "{err:?}");
}

#[test]
fn a_guest_leaving_frees_its_place_and_the_host_can_share_again() {
    let mut room = Room::loopback();
    room.start_sharing();
    let g = room.join("Marc", Role::Edit);
    assert_eq!(room.members(), ["Marc"]);
    room.guests[g].app().leave_guest();
    room.until("the host to see the departure", |r| r.members().is_empty());
    // stop and start again: new endpoint, new links, a new guest gets in
    crate::share_ui::stop(room.host.app());
    room.start_sharing();
    let h = room.join("Anna", Role::Edit);
    assert_eq!(room.members(), ["Anna"]);
    assert_eq!(room.guest_state(h), GuestState::Live);
}

#[test]
fn an_incompatible_protocol_version_is_refused_by_the_host() {
    let mut room = Room::loopback();
    room.start_sharing();
    let ticket = parse_join_link(&room.link(Role::Edit)).unwrap();
    let mut transport = IrohTransport::guest(NetConfig { connect_timeout: Duration::from_secs(10), ..NetConfig::loopback() }).unwrap();
    let mut conn = transport.connect_ticket(&ticket).unwrap();
    let mut chan = SecureChannel::new(FrameCipher::new(&ticket.key, Side::Guest), Box::new(JsonCodec), Box::new(SeededEntropy::new(5)), 1 << 20);
    let hello = Hello {
        proto: PROTO_VERSION + 1,
        name: "From the future".to_owned(),
        join_proof: join_proof(&ticket.key, "From the future", Role::Edit),
        role_requested: Role::Edit,
        resume: None,
    };
    conn.send(&chan.seal(&Message::Hello(hello)).unwrap()).unwrap();
    let mut answer = None;
    room.until("the host's answer", |_| {
        if let Some(frame) = conn.recv() {
            answer = chan.open(&frame).ok();
        }
        answer.is_some()
    });
    assert!(matches!(answer, Some(Message::Refused { reason: RefuseReason::Version { host_proto } }) if host_proto == PROTO_VERSION), "{answer:?}");
    assert!(room.host.app().share.host.as_ref().unwrap().pending().is_empty(), "an incompatible peer never reaches the consent window");
}

#[test]
fn a_guest_facing_a_host_of_another_version_says_to_update() {
    // a host that answers like the real one does to a version it cannot serve, over the real transport
    let keys = HostKeys::generate(&mut SeededEntropy::new(8)).unwrap();
    let mut old_host = IrohTransport::host(NetConfig::loopback()).unwrap();
    let ticket = old_host.ticket(&keys, Role::Edit).unwrap();
    let mut room = Room::loopback();
    let g = room.add_guest_with("Marc", &join_url(&ticket), loopback_guest());
    let (mut conn, mut answered) = (None, false);
    room.until("the guest to be refused", |r| {
        if conn.is_none() {
            conn = old_host.accept();
        }
        if let Some(c) = conn.as_mut()
            && !answered
            && let Some(frame) = c.recv()
        {
            let (_, cipher, _) = keys.open_first(&frame).expect("the guest's first frame");
            let mut chan = SecureChannel::new(cipher, Box::new(JsonCodec), Box::new(SeededEntropy::new(9)), 1 << 20);
            c.send(&chan.seal(&Message::Refused { reason: RefuseReason::Version { host_proto: PROTO_VERSION + 1 } }).unwrap()).unwrap();
            answered = true;
        }
        matches!(r.guest_state(g), GuestState::Ended(EndReason::Refused(RefuseReason::Version { .. })))
    });
    room.turns(3);
    assert!(room.guests[g].h.query_by_label_contains("another version of Inkboard").is_some());
}

/* ----------------------------------------------------------------------------------------------- real network failures */

/// Joins a host that does not exist, with a short handshake deadline, and waits for the diagnosis.
fn join_a_dead_host(relay_url: String) -> (Room, usize, crate::session_guest::NetIssue) {
    let mut room = Room::loopback();
    let net = GuestNet {
        net: NetConfig { connect_timeout: Duration::from_secs(1), ..NetConfig::loopback() },
        session: GuestConfig { relay_url, ..GuestConfig::default() },
        diagnoser: Arc::new(diagnoser()),
        relays_dir: None,
        watch_network: false,
    };
    let g = room.add_guest_with("Marc", &join_url(&dead_host_ticket()), net);
    room.until("the failure and its diagnosis", |r| {
        let guest = r.guests[g].app().guest.as_ref().expect("a guest session");
        matches!(guest.state(), GuestState::Failed(_)) && !guest.is_diagnosing()
    });
    let GuestState::Failed(issue) = room.guest_state(g) else { unreachable!("waited for it") };
    room.turns(3);
    (room, g, issue)
}

#[test]
fn a_dead_host_behind_a_security_gateway_gets_the_enterprise_message() {
    let url = serve("HTTP/1.1 403 Forbidden\r\nServer: Zscaler/6.2\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<html>Access blocked</html>");
    let (room, g, issue) = join_a_dead_host(url);
    assert_eq!(issue.class, NetClass::Blocked { gateway: Some("Zscaler".to_owned()), status: Some(403) });
    assert!(issue.headline().contains("Zscaler"), "{}", issue.headline());
    assert!(issue.diagnostic_text().contains("Zscaler"));
    assert_eq!(issue.tips().len(), 3, "the three suggestions of the plan");
    assert!(room.guests[g].h.query_by_label_contains("Zscaler").is_some(), "shown in the window");
    assert!(room.guests[g].h.query_by_label("Copy the diagnostic").is_some());
}

#[test]
fn a_dead_host_with_a_healthy_service_is_not_blamed_on_the_network() {
    let (room, g, issue) = join_a_dead_host(serve("HTTP/1.1 200 OK\r\nServer: iroh-relay\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"));
    assert_eq!(issue.class, NetClass::HostUnreachable);
    assert!(issue.tips().is_empty());
    assert!(room.guests[g].h.query_by_label_contains("host cannot be reached").is_some());
}

#[test]
fn a_dead_host_and_a_silent_service_gets_the_generic_firewall_message() {
    let (room, g, issue) = join_a_dead_host(closed_port_url());
    assert_eq!(issue.class, NetClass::Unreachable);
    assert!(!issue.detail.is_empty(), "the transport's words are kept for the diagnostic");
    assert_eq!(issue.tips().len(), 3);
    assert!(room.guests[g].h.query_by_label_contains("firewall or a security gateway").is_some());
}

#[test]
fn a_failed_join_does_not_freeze_the_interface() {
    // the handshake of a dead host lasts a full second: the application keeps running frames meanwhile
    let mut room = Room::loopback();
    let net = GuestNet {
        net: NetConfig { connect_timeout: Duration::from_secs(1), ..NetConfig::loopback() },
        session: GuestConfig { relay_url: closed_port_url(), ..GuestConfig::default() },
        diagnoser: Arc::new(diagnoser()),
        relays_dir: None,
        watch_network: false,
    };
    let g = room.add_guest_with("Marc", &join_url(&dead_host_ticket()), net);
    let mut frames = 0;
    room.until("the failure", |r| {
        frames += 1;
        matches!(r.guest_state(g), GuestState::Failed(_))
    });
    assert!(frames > 3, "frames kept running while the network was busy ({frames})");
}

#[test]
fn network_errors_are_classified_for_the_guest() {
    use crate::session_guest::RelayDiagnoser;
    let d = diagnoser().diagnose(&serve("HTTP/1.1 403 Forbidden\r\nServer: Zscaler/6.2\r\nConnection: close\r\n\r\nblocked"));
    assert!(d.reachable && d.http_status == Some(403) && d.gateway.as_deref() == Some("Zscaler"), "{d:?}");
    let d = diagnoser().diagnose(&closed_port_url());
    assert!(!d.reachable && d.gateway.is_none() && d.http_status.is_none(), "{d:?}");
}

/* ------------------------------------------------------------------------------------------------------ public relay */

/// By hand, with Internet access: `cargo +1.99.1 test -p inkboard-app public_relay -- --ignored`. Both sides use the public relays
/// of n0 (no loopback); the guest reaches the host through a relay or a direct path found with its help.
#[test]
#[ignore = "needs Internet: uses the public relays of n0"]
fn public_relay_two_applications_share_a_board() {
    let mut room = Room::new(NetConfig::default());
    room.host.add_rect(0.0, 0.0, 120.0, 80.0);
    room.start_sharing();
    let link = room.link(Role::Edit);
    let ticket = parse_join_link(&link).unwrap();
    assert!(ticket.relays.iter().any(|h| h.starts_with("https://")), "a relay hint is in the link: {:?}", ticket.relays);
    let g = room.add_guest_with("Marc", &link, GuestNet::public());
    room.admit(g, "Marc", Role::Edit);
    let from_guest = room.guests[g].add_rect(300.0, 0.0, 100.0, 60.0);
    room.until("the host to get the guest's shape", |r| r.host_has(&from_guest));
    let from_host = room.host.add_rect(0.0, 200.0, 100.0, 60.0);
    room.until("the guest to get the host's shape", |r| r.guest_has(g, &from_host));
}

/* ------------------------------------------------------------------------------------------------------------ lot R */

mod lot_r {
    use super::*;
    use crate::session_host::{HostAddress, HostTransport, SessionNet, StartJob};
    use crate::session_net::{LocalAddrWatch, RelayListError, load_relays_from, parse_relay_list, save_relays_to};
    use inkboard_net::RelaySetting;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::mpsc::{Receiver, Sender, channel};

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("inkboard-lot-r-{}", Id::new()))
    }

    /* ---- relay list */

    #[test]
    fn a_relay_list_accepts_https_only_ignores_empty_lines_and_repeats() {
        assert_eq!(parse_relay_list(""), Ok(vec![]));
        assert_eq!(parse_relay_list("\n  \n\t\n"), Ok(vec![]));
        let text = "\u{feff} https://a.example.org \n\nHTTPS://b.example.org/path\r\nhttps://a.example.org\n";
        assert_eq!(parse_relay_list(text), Ok(vec!["https://a.example.org".to_owned(), "HTTPS://b.example.org/path".to_owned()]));
        assert_eq!(parse_relay_list("https://a.example.org\nhttp://b.example.org"), Err(RelayListError::NotHttps("http://b.example.org".into())));
        assert_eq!(parse_relay_list("relay.example.org"), Err(RelayListError::NotHttps("relay.example.org".into())));
        assert_eq!(parse_relay_list("ws://relay.example.org"), Err(RelayListError::NotHttps("ws://relay.example.org".into())));
        assert!(matches!(parse_relay_list("https://"), Err(RelayListError::Invalid(_))));
        assert!(matches!(parse_relay_list("https://not a host"), Err(RelayListError::Invalid(_))));
        let many: String = (0..5).map(|i| format!("https://r{i}.example.org\n")).collect();
        assert_eq!(parse_relay_list(&many), Err(RelayListError::TooMany));
    }

    #[test]
    fn the_relays_file_is_read_by_the_host_and_the_guest_and_an_invalid_one_is_an_error() {
        let dir = temp_dir();
        let host = IrohHostNet::new(NetConfig::default()).with_relays_dir(dir.clone());
        let guest = GuestNet { net: NetConfig::default(), relays_dir: Some(dir.clone()), ..loopback_guest() };
        let is_default = |c: NetConfig| matches!(c.relays, RelaySetting::Default);
        assert!(is_default(host.config().unwrap()) && is_default(guest.config()), "no file: the public relays");

        save_relays_to(&dir, &["https://relay.example.org".to_owned(), "https://other.example.org".to_owned()]);
        assert_eq!(std::fs::read_to_string(dir.join("relays")).unwrap(), "https://relay.example.org\nhttps://other.example.org\n");
        assert_eq!(load_relays_from(&dir).unwrap().len(), 2);
        for cfg in [host.config().unwrap(), guest.config()] {
            let RelaySetting::Custom(urls) = cfg.relays else { panic!("the custom relays were not used") };
            let urls: Vec<String> = urls.iter().map(ToString::to_string).collect();
            assert!(urls.iter().any(|u| u.contains("relay.example.org")) && urls.iter().any(|u| u.contains("other.example.org")), "{urls:?}");
        }
        // an explicit choice (no relay, loopback tests) is not overridden by the file
        let off = IrohHostNet::new(NetConfig::loopback()).with_relays_dir(dir.clone());
        assert!(matches!(off.config().unwrap().relays, RelaySetting::Disabled));

        // a hand-edited file that is not valid: an error for the host, never a silent return to the public relays
        std::fs::write(dir.join("relays"), "http://insecure.example.org\n").unwrap();
        assert!(host.config().unwrap_err().contains("https://"));
        // an empty list removes the file: back to the public relays
        save_relays_to(&dir, &[]);
        assert!(!dir.join("relays").exists() && is_default(host.config().unwrap()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn starting_to_share_reads_the_relays_file_and_reports_an_invalid_one() {
        set_for_thread(Some(Lang::En));
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("relays"), "http://insecure.example.org\n").unwrap();
        let mut host = T::new();
        host.app().share.net = Some(Box::new(IrohHostNet::new(NetConfig::default()).with_relays_dir(dir.clone())));
        crate::share_ui::start(host.app());
        for _ in 0..TURNS {
            host.h.step();
            if host.app().share.starting.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let a = host.app();
        assert!(!a.share.is_running(), "nothing starts with an invalid list");
        assert!(a.share.error.as_deref().is_some_and(|e| e.contains("https://")), "{:?}", a.share.error);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_share_window_checks_the_relays_field_before_saving() {
        set_for_thread(Some(Lang::En));
        let dir = temp_dir();
        let mut t = T::new();
        t.app().share.net = Some(Box::new(IrohHostNet::new(NetConfig::loopback())));
        t.app().share.data_dir = Some(dir.clone());
        t.app().share.open = true;
        t.h.step();
        t.h.step();
        assert!(t.h.query_by_label("Custom relays").is_some());
        crate::share_ui::set_relays(t.app(), "http://insecure.example.org");
        t.h.step();
        t.h.step();
        assert!(t.h.query_by_label_contains("Not an https:// address").is_some());
        assert!(!dir.join("relays").exists());
        crate::share_ui::set_relays(t.app(), "\nhttps://relay.example.org\n");
        t.h.step();
        t.h.step();
        assert!(t.h.query_by_label_contains("Not an https:// address").is_none());
        assert_eq!(load_relays_from(&dir).unwrap(), ["https://relay.example.org"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /* ---- network detection */

    #[test]
    fn the_network_detection_fires_when_it_comes_back_or_changes_and_not_at_the_first_look() {
        let a = Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)));
        let b = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)));
        let mut w = LocalAddrWatch::default();
        assert!(!w.observe(a), "the first look is not a change");
        assert!(!w.observe(a), "nothing changed");
        assert!(!w.observe(None), "the network went away: nothing to retry yet");
        assert!(!w.observe(None));
        assert!(w.observe(a), "back, with the same address");
        assert!(w.observe(b), "another interface took over");
        assert!(!w.observe(b));
    }

    /* ---- a start that does not freeze the interface */

    struct NullTransport;

    impl HostTransport for NullTransport {
        fn accept(&mut self) -> Option<Box<dyn Connection>> {
            None
        }
    }

    /// A network whose start waits for the test to release it (no timer: the "simulated wait" is a channel).
    struct GatedNet {
        gate: Option<Receiver<()>>,
        saw_cancel: Sender<bool>,
    }

    impl SessionNet for GatedNet {
        fn start(&mut self) -> Result<(Box<dyn HostTransport>, HostAddress), String> {
            Err("the blocking start must not be used".into())
        }

        fn start_job(&mut self) -> Option<StartJob> {
            let (gate, saw) = (self.gate.take()?, self.saw_cancel.clone());
            Some(Box::new(move |cancel| {
                gate.recv().ok();
                saw.send(cancel.load(std::sync::atomic::Ordering::SeqCst)).ok();
                Ok((Box::new(NullTransport), HostAddress { host: "gated-host".into(), relays: Vec::new() }))
            }))
        }
    }

    fn gated() -> (T, Sender<()>, Receiver<bool>) {
        set_for_thread(Some(Lang::En));
        let (open, gate) = channel();
        let (saw_cancel, saw) = channel();
        let mut t = T::new();
        t.app().share.net = Some(Box::new(GatedNet { gate: Some(gate), saw_cancel }));
        t.app().share.open = true;
        (t, open, saw)
    }

    #[test]
    fn starting_to_share_does_not_block_the_interface_and_the_link_appears_when_ready() {
        let (mut t, release, _saw) = gated();
        crate::share_ui::start(t.app());
        // the network is still "busy": the call returned, the interface answers, the state says so
        for _ in 0..5 {
            t.h.step();
        }
        assert!(t.app().share.starting.is_some() && !t.app().share.is_running());
        assert!(t.h.query_by_label("Starting…").is_some(), "the window shows the state");
        assert!(t.h.query_by_label("Start sharing").is_none(), "a second start is not offered");
        release.send(()).unwrap();
        for _ in 0..TURNS {
            t.h.step();
            if t.app().share.is_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let a = t.app();
        assert!(a.share.is_running() && a.share.starting.is_none() && a.share.error.is_none());
        assert_eq!(a.share.addr.as_ref().map(|x| x.host.as_str()), Some("gated-host"));
    }

    #[test]
    fn cancel_while_starting_drops_the_start_and_tells_the_thread() {
        let (mut t, release, saw) = gated();
        crate::share_ui::start(t.app());
        t.h.step();
        t.h.step();
        t.h.get_by_label("Cancel").click();
        t.h.step();
        t.h.step();
        assert!(t.app().share.starting.is_none() && !t.app().share.is_running());
        release.send(()).unwrap();
        assert!(saw.recv().unwrap(), "the thread saw the cancellation");
        for _ in 0..5 {
            t.h.step();
        }
        let a = t.app();
        assert!(!a.share.is_running() && a.share.error.is_none(), "the late result was dropped");
        assert!(t.h.query_by_label("Start sharing").is_some(), "the user can start again");
    }

    #[test]
    fn the_real_network_starts_on_a_thread_and_a_cancelled_start_can_be_redone() {
        let mut room = Room::loopback();
        crate::share_ui::start(room.host.app());
        assert!(room.host.app().share.starting.is_some() && !room.host.app().share.is_running(), "the call did not wait for the network");
        crate::share_ui::cancel_start(room.host.app());
        assert!(room.host.app().share.starting.is_none());
        room.turns(3);
        assert!(!room.host.app().share.is_running());
        room.start_sharing();
        assert_eq!(room.host.app().share.error, None);
        let link = room.link(Role::Edit);
        assert!(link.starts_with("inkboard://join#"), "{link}");
    }

    /* ---- reconnection with the real network */

    #[test]
    fn retry_now_after_a_cut_resumes_the_same_participant_without_a_new_request() {
        let mut room = Room::loopback();
        room.start_sharing();
        let g = room.join("Marc", Role::Edit);
        let before = room.guests[g].app().guest.as_ref().unwrap().participant();
        room.guests[g].app().guest.as_mut().unwrap().cut_connection();
        room.guests[g].app().guest.as_mut().unwrap().retry_now();
        let offline = room.guests[g].add_rect(200.0, 0.0, 50.0, 50.0);
        room.until("the guest to be back", |r| r.guest_state(g) == GuestState::Live && r.host_has(&offline));
        assert_eq!(room.guests[g].app().guest.as_ref().unwrap().participant(), before, "the same place");
        assert_eq!(room.members(), ["Marc"]);
        assert!(room.host.app().share.host.as_ref().unwrap().pending().is_empty(), "no new request to answer");
    }

    #[test]
    fn cancelling_a_real_connection_attempt_ends_it_instead_of_waiting_for_the_timeout() {
        // the handshake would last ten minutes: only the cancellation can end the dial thread, whose end lets the endpoint close
        let mut room = Room::loopback();
        let net = GuestNet {
            net: NetConfig { connect_timeout: Duration::from_secs(600), ..NetConfig::loopback() },
            session: GuestConfig { relay_url: closed_port_url(), ..GuestConfig::default() },
            diagnoser: Arc::new(diagnoser()),
            relays_dir: None,
            watch_network: false,
        };
        let g = room.add_guest_with("Marc", &join_url(&dead_host_ticket()), net);
        room.turns(20);
        assert_eq!(room.guest_state(g), GuestState::Joining);
        room.guests[g].app().leave_guest();
        assert!(room.guests[g].app().guest.is_none());
        let mut ended = false;
        for _ in 0..1500 {
            if crate::session_net::background_closes_pending() == 0 {
                ended = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ended, "the attempt and the closing of its endpoint ended");
    }
}
