//! Relay diagnostics against a tiny local HTTP server (no Internet).
#![cfg(not(target_arch = "wasm32"))]

use inkboard_net::{GatewayKind, classify_gateway, diagnose_relay};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

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
    let d = diagnose_relay(&url, TIMEOUT);
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
    let d = diagnose_relay(&url, TIMEOUT);
    assert!(d.reachable_tcp);
    assert_eq!(d.http_status, Some(200));
    assert_eq!(d.gateway, None);
    assert!(d.summary.contains("normally"), "{}", d.summary);
}

#[test]
fn another_gateway_warning_page_is_reported_as_other() {
    let url = serve("HTTP/1.1 403 Forbidden\r\nServer: nginx\r\nConnection: close\r\n\r\n<html>Access denied by security policy</html>");
    assert_eq!(diagnose_relay(&url, TIMEOUT).gateway, Some(GatewayKind::Other));
}

#[test]
fn a_closed_port_is_unreachable() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let d = diagnose_relay(&format!("http://127.0.0.1:{port}/"), TIMEOUT);
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
