//! Relay diagnostics against a tiny local HTTP server (no Internet).
#![cfg(not(target_arch = "wasm32"))]

use inkboard_net::diagnose::{DiagnoseOptions, diagnose_relay_with};
use inkboard_net::{GatewayKind, classify_gateway, diagnose_relay};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Test mode: the local servers of these tests are on the loopback interface over plain HTTP.
fn diagnose_local(url: &str) -> inkboard_net::Diagnosis {
    diagnose_relay_with(url, TIMEOUT, &DiagnoseOptions { allow_private: true, ..DiagnoseOptions::default() })
}

/// Serves `response` to every connection until the test ends; returns the base URL.
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

#[test]
fn zscaler_block_page_is_recognized() {
    let url = serve("HTTP/1.1 403 Forbidden\r\nServer: Zscaler/6.2\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<html>Access blocked</html>");
    let d = diagnose_local(&url);
    assert!(d.reachable_tcp);
    assert_eq!(d.http_status, Some(403));
    assert_eq!(d.server_header.as_deref(), Some("Zscaler/6.2"));
    assert_eq!(d.gateway, Some(GatewayKind::Zscaler));
    assert_eq!(d.tls_trusted, None);
    assert!(d.summary.contains("Zscaler") && d.summary.contains("403"), "{}", d.summary);
}

#[test]
fn a_healthy_relay_is_reachable_and_not_a_gateway() {
    let url = serve("HTTP/1.1 200 OK\r\nServer: iroh-relay\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
    let d = diagnose_local(&url);
    assert!(d.reachable_tcp);
    assert_eq!(d.http_status, Some(200));
    assert_eq!(d.gateway, None);
    assert!(d.summary.contains("normally"), "{}", d.summary);
}

#[test]
fn another_gateway_warning_page_is_reported_as_other() {
    let url = serve("HTTP/1.1 403 Forbidden\r\nServer: nginx\r\nConnection: close\r\n\r\n<html>Access denied by security policy</html>");
    assert_eq!(diagnose_local(&url).gateway, Some(GatewayKind::Other));
}

#[test]
fn a_closed_port_is_unreachable() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let d = diagnose_local(&format!("http://127.0.0.1:{port}/"));
    assert!(!d.reachable_tcp);
    assert_eq!(d.http_status, None);
    assert_eq!(d.gateway, None);
    assert!(d.summary.contains("TCP"), "{}", d.summary);
}

#[test]
fn a_bad_url_gives_a_diagnosis_not_a_panic() {
    let d = diagnose_relay("not a url", TIMEOUT);
    assert!(!d.reachable_tcp);
    assert!(d.summary.contains("invalid"));
}

/// A local server that counts the connections it receives, answering `response` to each.
fn serve_counting(response: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            counter.fetch_add(1, Ordering::SeqCst);
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}/"), hits)
}

#[test]
fn the_default_policy_never_contacts_a_private_or_plain_address() {
    // a real server is listening on the loopback interface: it must not see a single connection
    let (url, hits) = serve_counting("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhi");
    let port = url.trim_start_matches("http://127.0.0.1:").trim_end_matches('/').to_owned();
    let refused = [
        url.clone(),
        format!("https://127.0.0.1:{port}/"),
        format!("https://localhost:{port}/"),
        "https://10.0.0.1/".to_owned(),
        "https://192.168.1.1/".to_owned(),
        "https://172.16.0.9/".to_owned(),
        "https://169.254.169.254/latest/meta-data/".to_owned(),
        "https://100.64.0.1/".to_owned(),
        "https://[::1]/".to_owned(),
        "https://[fe80::1]/".to_owned(),
        "https://[fd00::1]/".to_owned(),
        "https://router/".to_owned(),
        "https://printer.local/".to_owned(),
        "http://relay.example.org/".to_owned(),
        "https://relay.example.org:22/".to_owned(),
        "https://user:secret@relay.example.org/".to_owned(),
    ];
    for u in &refused {
        let d = diagnose_relay(u, TIMEOUT);
        assert!(!d.reachable_tcp && d.http_status.is_none(), "{u}: {d:?}");
        assert!(d.summary.contains("not contacted"), "{u}: {}", d.summary);
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0, "a private address was contacted");
}

#[test]
fn a_redirect_is_reported_and_never_followed() {
    let (target, target_hits) = serve_counting("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nsecret");
    let response: &'static str = Box::leak(format!("HTTP/1.1 302 Found\r\nLocation: {target}admin\r\nConnection: close\r\n\r\n").into_boxed_str());
    let (url, hits) = serve_counting(response);
    let d = diagnose_local(&url);
    assert_eq!(d.http_status, Some(302));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(target_hits.load(Ordering::SeqCst), 0, "the Location was followed");
}

#[test]
fn the_server_header_cannot_smuggle_control_characters() {
    let url = serve("HTTP/1.1 200 OK\r\nServer: iroh\x07\x1b[31m-relay\x08\r\nConnection: close\r\n\r\nok");
    let d = diagnose_local(&url);
    let server = d.server_header.expect("a server header");
    assert!(server.chars().all(|c| !c.is_control()), "{server:?}");
    assert!(server.starts_with("iroh"));
}

#[test]
fn classification_rules() {
    // Statement of the plan: 403 and a Server header starting with Zscaler.
    assert_eq!(classify_gateway(403, Some("Zscaler/6.2"), "", Some(true)), Some(GatewayKind::Zscaler));
    assert_eq!(classify_gateway(403, Some("zscaler"), "", None), Some(GatewayKind::Zscaler));
    // The block page names Zscaler although the header does not.
    assert_eq!(classify_gateway(403, None, "<title>Zscaler</title>", None), Some(GatewayKind::Zscaler));
    // Other gateways and warning pages.
    assert_eq!(classify_gateway(407, None, "", None), Some(GatewayKind::Other));
    assert_eq!(classify_gateway(302, Some("FortiGate"), "FortiGuard web filter", None), Some(GatewayKind::Other));
    assert_eq!(classify_gateway(404, None, "", Some(false)), Some(GatewayKind::Other));
    // A relay that works is never a gateway, even behind an untrusted (self-signed) certificate.
    assert_eq!(classify_gateway(200, Some("iroh"), "ok", Some(false)), None);
    assert_eq!(classify_gateway(200, None, "this page talks about a firewall", None), None);
    assert_eq!(classify_gateway(404, Some("iroh"), "not found", Some(true)), None);
}
