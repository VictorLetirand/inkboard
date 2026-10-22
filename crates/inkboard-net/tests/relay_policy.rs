//! Which relay URLs of a ticket are trusted (S-06): `https` only, public address, ordinary port; the relays of n0 and the configured
//! list are known; anything else is dropped before the transport dials. Offline: nothing leaves the machine.
#![cfg(not(target_arch = "wasm32"))]

use inkboard_net::config::{check_relay_url, is_known_relay};
use inkboard_net::{IrohTransport, NetConfig, RelaySetting, default_relay_urls};
use inkboard_session::crypto::{HostKeys, SeededEntropy, Ticket};
use inkboard_session::error::{Error, NetCause};
use inkboard_session::proto::Role;
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn urls_that_could_reach_the_internal_network_are_refused() {
    for url in [
        "http://127.0.0.1/",
        "http://relay.example.org/",
        "https://127.0.0.1/",
        "https://10.0.0.1/",
        "https://192.168.0.10:8443/",
        "https://172.20.1.1/",
        "https://169.254.169.254/",
        "https://0.0.0.0/",
        "https://[::1]/",
        "https://[fd12:3456::1]/",
        "https://[::ffff:10.0.0.1]/",
        "https://localhost/",
        "https://app.localhost/",
        "https://intranet/",
        "https://nas.local/",
        "https://relay.example.org:22/",
        "https://relay.example.org:0/",
        "https://user:pw@relay.example.org/",
        "file:///etc/passwd",
        "file:///C:/Windows/win.ini",
        "ftp://relay.example.org/",
        "not a url",
        "",
    ] {
        assert!(check_relay_url(url, false).is_err(), "accepted: {url}");
    }
}

#[test]
fn ordinary_public_https_relays_are_accepted() {
    for url in [
        "https://euc1-1.relay.n0.iroh.link./",
        "https://relay.example.org/",
        "https://relay.example.org:8443/",
        "https://relay.example.org:443/",
        "https://1.1.1.1/",
    ] {
        assert!(check_relay_url(url, false).is_ok(), "refused: {url}");
    }
    for url in default_relay_urls() {
        assert!(check_relay_url(&url, false).is_ok(), "the public relay of n0 is refused: {url}");
    }
    // test mode only
    assert!(check_relay_url("http://127.0.0.1:3340/", true).is_ok());
    assert!(check_relay_url("file:///etc/passwd", true).is_err(), "even in test mode it must be http(s)");
}

#[test]
fn n0_relays_and_the_configured_list_are_known_everything_else_is_not() {
    for url in default_relay_urls() {
        assert!(is_known_relay(&url, &[]), "{url}");
    }
    assert!(!is_known_relay("https://evil.example/", &[]));
    assert!(is_known_relay("https://relay.mycorp.example/", &["https://relay.mycorp.example/".to_owned()]));
    assert!(is_known_relay("https://relay.mycorp.example:8443/path", &["relay.mycorp.example".to_owned()]));
    assert!(!is_known_relay("https://evil.example/", &["https://relay.mycorp.example/".to_owned()]));
}

fn ticket(relays: &[String]) -> Ticket {
    let keys = HostKeys::generate(&mut SeededEntropy::new(5)).unwrap();
    // a valid endpoint identity: the one of a throw-away loopback host
    let host = IrohTransport::host(NetConfig::loopback()).unwrap();
    Ticket::new(&host.local_id().unwrap(), relays, keys.edit.clone(), Role::Edit).unwrap()
}

#[test]
fn a_ticket_with_only_a_private_relay_is_refused_without_any_connection() {
    // something listens on the loopback interface: the guest must not even knock
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut guest =
        IrohTransport::guest(NetConfig { connect_timeout: Duration::from_secs(2), relays: RelaySetting::Disabled, ..NetConfig::default() }).unwrap();
    for relay in [format!("http://127.0.0.1:{port}/"), format!("https://127.0.0.1:{port}/"), "https://10.0.0.1/".to_owned()] {
        match guest.connect_ticket(&ticket(&[relay.clone()])) {
            Err(Error::Network { cause: NetCause::HostUnreachable, detail }) => assert!(detail.contains("not allowed"), "{relay}: {detail}"),
            other => panic!("{relay}: expected a refusal, got {other:?}"),
        }
    }
    assert!(listener.accept().is_err(), "somebody connected to the loopback listener");
}

#[test]
fn a_configured_relay_is_used_even_on_a_private_address() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let relay = format!("http://127.0.0.1:{port}/");
    let cfg = NetConfig { relays: RelaySetting::custom([relay.clone()]).unwrap(), connect_timeout: Duration::from_secs(2), ..NetConfig::default() };
    assert!(!cfg.allow_private_relays);
    let mut guest = IrohTransport::guest(cfg).unwrap();
    // the relay is accepted (it is configured): the attempt really dials it, and fails because nothing listens there
    match guest.connect_ticket(&ticket(&[relay])) {
        Err(Error::Network { cause: NetCause::RelayUnreachable, .. }) => {}
        other => panic!("expected RelayUnreachable, got {other:?}"),
    }
}
