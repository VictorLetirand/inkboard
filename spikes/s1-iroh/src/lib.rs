//! Spike S1: shared protocol, ticket and benchmark logic, compiled for native and wasm32.
//!
//! Wire format: one bi-directional QUIC stream per guest, made of frames
//! `[u32 LE length (tag + payload)][u8 tag][payload]`. Bulk transfers send raw bytes
//! right after a request frame.

use anyhow::{Context, Result, bail};
use iroh::{
    Endpoint, EndpointAddr, RelayMode, RelayUrl,
    endpoint::{Builder, Connection, RecvStream, SendStream, presets},
};
use n0_future::time::Instant;
use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
pub mod host;
#[cfg(target_arch = "wasm32")]
mod wasm;

pub const ALPN: &[u8] = b"inkboard/s1/0";
pub const TAG_HELLO: u8 = 1;
pub const TAG_WELCOME: u8 = 2;
pub const TAG_PING: u8 = 3;
pub const TAG_PONG: u8 = 4;
pub const TAG_BULK_DOWN: u8 = 5;
pub const TAG_BULK_UP: u8 = 6;
pub const TAG_ACK: u8 = 7;
pub const TAG_BYE: u8 = 8;
pub const TAG_REFUSED: u8 = 9;
const MAX_FRAME: usize = 64 * 1024;
const CHUNK: usize = 64 * 1024;

/// Join ticket: host identity + addressing hints (relay URLs, direct addresses) + random secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    pub version: u8,
    pub addr: EndpointAddr,
    pub secret: [u8; 32],
}

impl Ticket {
    const PREFIX: &'static str = "inkb";

    /// Text form suitable for a URL fragment (lowercase base32, no padding).
    pub fn encode(&self) -> Result<String> {
        let bytes = postcard::to_stdvec(self)?;
        let b32 = data_encoding::BASE32_NOPAD.encode(&bytes).to_ascii_lowercase();
        Ok(format!("{}{b32}", Self::PREFIX))
    }

    pub fn decode(text: &str) -> Result<Self> {
        let body = text.trim().strip_prefix(Self::PREFIX).context("missing ticket prefix")?;
        let bytes = data_encoding::BASE32_NOPAD.decode(body.to_ascii_uppercase().as_bytes()).context("invalid base32")?;
        Ok(postcard::from_bytes(&bytes)?)
    }
}

pub async fn write_frame(send: &mut SendStream, tag: u8, payload: &[u8]) -> Result<()> {
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32 + 1).to_le_bytes());
    buf.push(tag);
    buf.extend_from_slice(payload);
    send.write_all(&buf).await?;
    Ok(())
}

pub async fn read_frame(recv: &mut RecvStream) -> Result<(u8, Vec<u8>)> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await?;
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > MAX_FRAME {
        bail!("invalid frame length {len}");
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    let payload = buf.split_off(1);
    Ok((buf[0], payload))
}

pub async fn send_raw(send: &mut SendStream, mut remaining: usize) -> Result<()> {
    let chunk = vec![0xA5u8; CHUNK];
    while remaining > 0 {
        let n = remaining.min(CHUNK);
        send.write_all(&chunk[..n]).await?;
        remaining -= n;
    }
    Ok(())
}

pub async fn drain_raw(recv: &mut RecvStream, mut remaining: usize) -> Result<()> {
    let mut chunk = vec![0u8; CHUNK];
    while remaining > 0 {
        let n = remaining.min(CHUNK);
        recv.read_exact(&mut chunk[..n]).await?;
        remaining -= n;
    }
    Ok(())
}

/// Idle timeout of the QUIC connection (iroh default: 30 s). A longer one lets a session survive a longer network outage
/// (tested on a phone: a 60 s outage survived with 120 s; lost with the default 30 s from 20 s).
pub const IDLE_SECS: u64 = 120;

pub fn quic_config() -> iroh::endpoint::QuicTransportConfig {
    iroh::endpoint::QuicTransportConfig::builder()
        .max_idle_timeout(Some(std::time::Duration::from_secs(IDLE_SECS).try_into().expect("idle timeout fits")))
        .build()
}

/// How a guest reaches the host, derived only from the ticket contents.
pub fn guest_builder(ticket: &Ticket) -> Builder {
    let relays: Vec<RelayUrl> = ticket
        .addr
        .addrs
        .iter()
        .filter_map(|a| match a {
            iroh::TransportAddr::Relay(url) => Some(url.clone()),
            _ => None,
        })
        .collect();
    let mode = if relays.is_empty() { RelayMode::Disabled } else { RelayMode::custom(relays) };
    let builder = Endpoint::builder(presets::Minimal).relay_mode(mode).transport_config(quic_config());
    // No direct address in the ticket: also remove IP sockets so only the relay is possible.
    #[cfg(not(target_arch = "wasm32"))]
    let builder = if ticket.addr.addrs.iter().any(|a| a.is_ip()) { builder } else { builder.clear_ip_transports() };
    builder
}

/// An opened and authenticated session on the guest side.
pub struct Session {
    pub conn: Connection,
    pub send: SendStream,
    pub recv: RecvStream,
    pub connect_ms: f64,
    pub handshake_ms: f64,
    pub resumed: bool,
}

pub fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// Dials the host, performs the Hello/Welcome exchange.
pub async fn open_session(endpoint: &Endpoint, ticket: &Ticket, participant: [u8; 16], name: &str) -> Result<Session> {
    let t0 = Instant::now();
    let conn = endpoint.connect(ticket.addr.clone(), ALPN).await.context("connect to host")?;
    let connect_ms = ms(t0);
    let t1 = Instant::now();
    let (mut send, mut recv) = conn.open_bi().await?;
    let mut hello = Vec::new();
    hello.extend_from_slice(&ticket.secret);
    hello.extend_from_slice(&participant);
    hello.extend_from_slice(name.as_bytes());
    write_frame(&mut send, TAG_HELLO, &hello).await?;
    let (tag, payload) = read_frame(&mut recv).await?;
    let handshake_ms = ms(t1);
    match tag {
        TAG_WELCOME => Ok(Session { conn, send, recv, connect_ms, handshake_ms, resumed: payload.first() == Some(&1) }),
        TAG_REFUSED => bail!("host refused the join"),
        other => bail!("unexpected tag {other} instead of Welcome"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LatencyStats {
    pub count: usize,
    pub min_ms: f64,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
}

fn stats(mut v: Vec<f64>) -> LatencyStats {
    if v.is_empty() {
        return LatencyStats::default();
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let at = |q: f64| v[(((n - 1) as f64) * q).round() as usize];
    LatencyStats { count: n, min_ms: v[0], median_ms: at(0.5), p95_ms: at(0.95), max_ms: v[n - 1], mean_ms: v.iter().sum::<f64>() / n as f64 }
}

/// Sequential ping/pong round trips with a 32-byte payload.
pub async fn ping_bench(s: &mut Session, count: u32) -> Result<LatencyStats> {
    let mut samples = Vec::with_capacity(count as usize);
    for i in 0..count {
        let mut payload = [0u8; 32];
        payload[..4].copy_from_slice(&i.to_le_bytes());
        let t = Instant::now();
        write_frame(&mut s.send, TAG_PING, &payload).await?;
        let (tag, echo) = read_frame(&mut s.recv).await?;
        samples.push(ms(t));
        if tag != TAG_PONG || echo != payload {
            bail!("bad pong at {i}");
        }
    }
    Ok(stats(samples))
}

/// Host to guest raw transfer; returns elapsed seconds.
pub async fn bulk_down(s: &mut Session, bytes: u32) -> Result<f64> {
    let t = Instant::now();
    write_frame(&mut s.send, TAG_BULK_DOWN, &bytes.to_le_bytes()).await?;
    drain_raw(&mut s.recv, bytes as usize).await?;
    Ok(t.elapsed().as_secs_f64())
}

/// Guest to host raw transfer, until the host acknowledges; returns elapsed seconds.
pub async fn bulk_up(s: &mut Session, bytes: u32) -> Result<f64> {
    let t = Instant::now();
    write_frame(&mut s.send, TAG_BULK_UP, &bytes.to_le_bytes()).await?;
    send_raw(&mut s.send, bytes as usize).await?;
    let (tag, _) = read_frame(&mut s.recv).await?;
    if tag != TAG_ACK {
        bail!("expected Ack, got {tag}");
    }
    Ok(t.elapsed().as_secs_f64())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathDesc {
    pub selected: bool,
    pub kind: String,
    pub remote: String,
    pub rtt_ms: f64,
}

/// Snapshot of the QUIC paths currently open on a connection.
pub fn describe_paths(conn: &Connection) -> Vec<PathDesc> {
    conn.paths()
        .iter()
        .map(|p| PathDesc {
            selected: p.is_selected(),
            kind: if p.is_relay() { "relay" } else { "ip" }.to_string(),
            remote: format!("{:?}", p.remote_addr()),
            rtt_ms: p.rtt().as_secs_f64() * 1000.0,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub bind_ms: f64,
    pub connect_ms: f64,
    pub handshake_ms: f64,
    pub resumed: bool,
    pub rtt: LatencyStats,
    pub bytes: u32,
    pub down_mbit_s: f64,
    pub up_mbit_s: f64,
    pub paths_after_handshake: Vec<PathDesc>,
    pub paths_after_bulk: Vec<PathDesc>,
}

pub fn mbit_s(bytes: u32, secs: f64) -> f64 {
    bytes as f64 * 8.0 / secs / 1e6
}

/// Full guest scenario: handshake, ping bench, download and upload of `bytes`.
pub async fn run_guest(endpoint: &Endpoint, bind_ms: f64, ticket: &Ticket, participant: [u8; 16], pings: u32, bytes: u32) -> Result<Report> {
    let mut s = open_session(endpoint, ticket, participant, "guest").await?;
    let paths_after_handshake = describe_paths(&s.conn);
    let rtt = ping_bench(&mut s, pings).await?;
    let down = bulk_down(&mut s, bytes).await?;
    let up = bulk_up(&mut s, bytes).await?;
    let paths_after_bulk = describe_paths(&s.conn);
    write_frame(&mut s.send, TAG_BYE, &[]).await?;
    s.send.finish().ok();
    s.conn.close(0u8.into(), b"bye");
    Ok(Report {
        bind_ms,
        connect_ms: s.connect_ms,
        handshake_ms: s.handshake_ms,
        resumed: s.resumed,
        rtt,
        bytes,
        down_mbit_s: mbit_s(bytes, down),
        up_mbit_s: mbit_s(bytes, up),
        paths_after_handshake,
        paths_after_bulk,
    })
}

pub fn random_id() -> [u8; 16] {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).expect("system randomness");
    id
}
