//! Relay diagnostics: what answers at the relay's address? Used to explain a failed connection on a network with a security gateway
//! (decision of 2026-10-21 in `context/PLAN_REMOTE.md` section 5): a gateway such as Zscaler answers the relay's HTTPS port with
//! a 403 warning page, which a browser can click through but an automatic connection cannot.
//!
//! Dependency choice: no HTTP client crate. The request is one `GET` over a blocking socket; TLS goes through `rustls` and
//! `webpki-roots`, which `iroh` already brings, so this adds no package to the build (an HTTP client such as `ureq` would add about
//! ten). The request carries nothing secret; when the certificate is not trusted (typical of TLS inspection) it is retried without
//! verification, only to read what the gateway answers, and the diagnosis says so (`tls_trusted == Some(false)`).
//! Limit: the system proxy settings and the operating system certificate store are not used (neither are they by iroh itself).

use iroh::RelayUrl;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// Kind of security gateway that answered instead of the relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayKind {
    Zscaler,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnosis {
    /// Host name of the diagnosed relay.
    pub host: String,
    /// Could a TCP connection be opened?
    pub reachable_tcp: bool,
    /// `Some(false)`: the certificate is not signed by a public root (TLS inspection, or a self-signed relay). `None`: plain
    /// HTTP, or TLS not reached.
    pub tls_trusted: Option<bool>,
    pub http_status: Option<u16>,
    /// Value of the `Server` response header.
    pub server_header: Option<String>,
    pub gateway: Option<GatewayKind>,
    /// One line for the user's "copy the diagnosis" button (English, technical).
    pub summary: String,
}

impl Diagnosis {
    fn new(host: &str) -> Self {
        Self { host: host.to_owned(), reachable_tcp: false, tls_trusted: None, http_status: None, server_header: None, gateway: None, summary: String::new() }
    }
}

const MAX_RESPONSE: usize = 32 * 1024;
const VENDORS: [&str; 12] =
    ["forcepoint", "fortiguard", "fortinet", "netskope", "blue coat", "bluecoat", "symantec", "palo alto", "umbrella", "mcafee", "websense", "sophos"];
const WARNING_WORDS: [&str; 7] = ["access denied", "blocked", "caution", "security policy", "firewall", "web filter", "not allowed"];

/// Does the answer come from a security gateway rather than from the relay? Pure, so that it is easy to test.
pub fn classify_gateway(status: u16, server: Option<&str>, body: &str, tls_trusted: Option<bool>) -> Option<GatewayKind> {
    let server = server.unwrap_or("").to_ascii_lowercase();
    let body = body.to_ascii_lowercase();
    let ok = (200..300).contains(&status);
    if status == 403 && server.starts_with("zscaler") {
        return Some(GatewayKind::Zscaler);
    }
    if !ok && (server.contains("zscaler") || body.contains("zscaler")) {
        return Some(GatewayKind::Zscaler);
    }
    let vendor = VENDORS.iter().any(|v| server.contains(v) || body.contains(v));
    let warning_page = WARNING_WORDS.iter().any(|w| body.contains(w));
    let untrusted = tls_trusted == Some(false);
    if !ok && (matches!(status, 403 | 407 | 451 | 511) || vendor || warning_page || untrusted) {
        return Some(GatewayKind::Other);
    }
    None
}

/// Requests `url` (the relay's address, `https://` or `http://`) and reports what answers. Never panics, never blocks longer
/// than about `timeout` (plus a small margin).
pub fn diagnose_relay(url: &str, timeout: Duration) -> Diagnosis {
    let Ok(parsed) = url.trim().parse::<RelayUrl>() else {
        return Diagnosis { summary: format!("invalid relay URL: {url}"), ..Diagnosis::new(url) };
    };
    let Some(host) = parsed.host_str().map(str::to_owned) else {
        return Diagnosis { summary: format!("invalid relay URL (no host): {url}"), ..Diagnosis::new(url) };
    };
    let tls = parsed.scheme() == "https";
    let port = parsed.port_or_known_default().unwrap_or(if tls { 443 } else { 80 });
    let path = match parsed.query() {
        Some(q) => format!("{}?{q}", parsed.path()),
        None => parsed.path().to_owned(),
    };
    let (tx, rx) = mpsc::channel();
    let worker_host = host.clone();
    let deadline = Instant::now() + timeout;
    let spawned = std::thread::Builder::new().name("relay-diagnosis".to_owned()).spawn(move || {
        tx.send(run(&worker_host, port, tls, &path, deadline)).ok();
    });
    let mut d = Diagnosis::new(&host);
    if spawned.is_err() {
        d.summary = "cannot start the diagnosis thread".to_owned();
        return d;
    }
    // The worker may be stuck in a name resolution (no timeout in the standard library): give up on it, it ends by itself.
    match rx.recv_timeout(timeout + Duration::from_secs(1)) {
        Ok(done) => done,
        Err(_) => {
            d.summary = format!("{host}:{port}: no answer within {} s", timeout.as_secs_f32());
            d
        }
    }
}

fn run(host: &str, port: u16, tls: bool, path: &str, deadline: Instant) -> Diagnosis {
    let mut d = Diagnosis::new(host);
    let sock = match connect_tcp(host, port, deadline) {
        Ok(s) => s,
        Err(e) => {
            d.summary = format!("cannot open a TCP connection to {host}:{port}: {e}");
            return d;
        }
    };
    d.reachable_tcp = true;
    let answer = if tls {
        match https(sock, host, path, deadline, true) {
            Err(Failure::Certificate(_)) => {
                d.tls_trusted = Some(false);
                connect_tcp(host, port, deadline).map_err(|e| e.to_string()).and_then(|s| https(s, host, path, deadline, false).map_err(Failure::into_text))
            }
            Err(f) => Err(f.into_text()),
            Ok(raw) => {
                d.tls_trusted = Some(true);
                Ok(raw)
            }
        }
    } else {
        http(sock, host, path, deadline)
    };
    let raw = match answer {
        Ok(raw) => raw,
        Err(e) => {
            d.summary = format!("{host}:{port} reachable, but the HTTP request failed: {e}");
            return d;
        }
    };
    let Some(resp) = parse_response(&raw) else {
        d.summary = format!("{host}:{port} reachable, but the answer is not HTTP");
        return d;
    };
    d.http_status = Some(resp.status);
    d.gateway = classify_gateway(resp.status, resp.server.as_deref(), &resp.body, d.tls_trusted);
    d.server_header = resp.server;
    d.summary = summarize(&d, port);
    d
}

fn summarize(d: &Diagnosis, port: u16) -> String {
    let status = d.http_status.map_or_else(|| "no HTTP status".to_owned(), |s| format!("HTTP {s}"));
    let server = d.server_header.as_deref().map(|s| format!(" (Server: {s})")).unwrap_or_default();
    let tls = match d.tls_trusted {
        Some(true) => ", certificate trusted",
        Some(false) => ", certificate NOT trusted by public roots",
        None => "",
    };
    let verdict = match d.gateway {
        Some(GatewayKind::Zscaler) => "a Zscaler security gateway answers instead of the relay",
        Some(GatewayKind::Other) => "a security gateway or firewall seems to answer instead of the relay",
        None if d.http_status.is_some_and(|s| (200..300).contains(&s)) => "the relay answers normally",
        None => "unexpected answer, no known gateway recognized",
    };
    format!("{}:{port}: {status}{server}{tls}; {verdict}", d.host)
}

/* ------------------------------------------------------------------- I/O */

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1))
}

fn connect_tcp(host: &str, port: u16, deadline: Instant) -> std::io::Result<TcpStream> {
    let mut last = std::io::Error::new(ErrorKind::NotFound, "the name does not resolve to any address");
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, remaining(deadline).min(Duration::from_secs(5))) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn request(host: &str, path: &str) -> String {
    let path = if path.is_empty() { "/" } else { path };
    format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: inkboard-relay-diagnostic\r\nAccept: */*\r\nConnection: close\r\n\r\n")
}

fn http(mut sock: TcpStream, host: &str, path: &str, deadline: Instant) -> Result<Vec<u8>, String> {
    sock.set_write_timeout(Some(remaining(deadline))).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(remaining(deadline))).map_err(|e| e.to_string())?;
    sock.write_all(request(host, path).as_bytes()).map_err(|e| e.to_string())?;
    read_response(&mut sock, deadline)
}

enum Failure {
    /// The certificate was refused (the retry without verification reads the gateway's answer anyway).
    Certificate(String),
    Other(String),
}

impl Failure {
    fn into_text(self) -> String {
        match self {
            Self::Certificate(s) | Self::Other(s) => s,
        }
    }
}

fn https(mut sock: TcpStream, host: &str, path: &str, deadline: Instant, verify: bool) -> Result<Vec<u8>, Failure> {
    let other = |e: &dyn std::fmt::Display| Failure::Other(e.to_string());
    let name = ServerName::try_from(host.to_owned()).map_err(|e| other(&e))?;
    let mut conn = ClientConnection::new(tls_config(verify), name).map_err(|e| other(&e))?;
    sock.set_write_timeout(Some(remaining(deadline))).map_err(|e| other(&e))?;
    sock.set_read_timeout(Some(remaining(deadline))).map_err(|e| other(&e))?;
    while conn.is_handshaking() {
        if let Err(e) = conn.complete_io(&mut sock) {
            let rustls_error = e.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>());
            return Err(match rustls_error {
                Some(rustls::Error::InvalidCertificate(_)) => Failure::Certificate(e.to_string()),
                _ => Failure::Other(format!("TLS handshake failed: {e}")),
            });
        }
    }
    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
    tls.write_all(request(host, path).as_bytes()).map_err(|e| other(&e))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            // A server that closes without `close_notify` or goes quiet after its answer: keep what was received.
            Err(e) if !buf.is_empty() && matches!(e.kind(), ErrorKind::UnexpectedEof | ErrorKind::WouldBlock | ErrorKind::TimedOut) => break,
            Err(e) => return Err(other(&e)),
        }
        if buf.len() >= MAX_RESPONSE || Instant::now() >= deadline {
            break;
        }
    }
    Ok(buf)
}

/// Reads until the end of the stream, the size limit or the deadline; whatever was received is kept.
fn read_response(sock: &mut TcpStream, deadline: Instant) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match sock.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if !buf.is_empty() && matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => break,
            Err(e) => return Err(e.to_string()),
        }
        if buf.len() >= MAX_RESPONSE || Instant::now() >= deadline {
            break;
        }
    }
    Ok(buf)
}

struct Response {
    status: u16,
    server: Option<String>,
    body: String,
}

fn parse_response(raw: &[u8]) -> Option<Response> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    if !first.next()?.starts_with("HTTP/") {
        return None;
    }
    let status = first.next()?.parse().ok()?;
    let server = lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case("server")).map(|(_, v)| v.trim().to_owned());
    let body = String::from_utf8_lossy(raw.get(end + 4..).unwrap_or(&[])).into_owned();
    Some(Response { status, server, body })
}

/* ------------------------------------------------------------------- TLS */

fn tls_config(verify: bool) -> Arc<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().expect("ring supports the default TLS versions");
    Arc::new(if verify {
        builder.with_root_certificates(RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() }).with_no_client_auth()
    } else {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate(provider.signature_verification_algorithms))).with_no_client_auth()
    })
}

/// Accepts any certificate: ONLY for the diagnosis request, which sends no secret and whose answer is never trusted as data.
#[derive(Debug)]
struct AcceptAnyCertificate(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}
