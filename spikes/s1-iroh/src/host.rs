//! Native host: accepts guests, checks the ticket secret, answers pings and bulk requests.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{Result, bail};
use iroh::{
    Endpoint, RelayMode, RelayUrl, TransportAddr,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use tracing::{info, warn};

use crate::*;

/// How the host advertises itself in the ticket.
#[derive(Debug, Clone)]
pub enum HostNet {
    /// Loopback direct addresses only, no relay.
    Direct,
    /// Non-loopback direct addresses only, no relay.
    Lan,
    /// Public n0 relays only (IP sockets removed on the host).
    RelayN0,
    /// A custom relay only (IP sockets removed on the host).
    RelayLocal(RelayUrl),
    /// Public n0 relay plus direct addresses (normal product behaviour).
    HybridN0,
}

pub struct HostHandle {
    pub router: Router,
    pub ticket: Ticket,
    pub bind_ms: f64,
}

#[derive(Debug, Clone)]
struct Host {
    secret: [u8; 32],
    participants: Arc<Mutex<HashMap<[u8; 16], u32>>>,
}

pub async fn spawn_host(net: HostNet) -> Result<HostHandle> {
    let t0 = n0_future::time::Instant::now();
    let builder = Endpoint::builder(presets::Minimal).alpns(vec![ALPN.to_vec()]).transport_config(crate::quic_config());
    let builder = match &net {
        HostNet::Direct | HostNet::Lan => builder.relay_mode(RelayMode::Disabled),
        HostNet::RelayN0 => builder.relay_mode(RelayMode::Default).clear_ip_transports(),
        HostNet::RelayLocal(url) => builder.relay_mode(RelayMode::custom([url.clone()])).clear_ip_transports(),
        HostNet::HybridN0 => builder.relay_mode(RelayMode::Default),
    };
    let endpoint = builder.bind().await?;
    let uses_relay = !matches!(net, HostNet::Direct | HostNet::Lan);
    if uses_relay {
        n0_future::time::timeout(std::time::Duration::from_secs(30), endpoint.online())
            .await
            .map_err(|_| anyhow::anyhow!("endpoint did not reach a relay within 30 s"))?;
    }
    let bind_ms = ms(t0);

    // Direct addresses are discovered asynchronously (and exclude loopback): poll briefly
    // for LAN-style addresses, and for `Direct` derive loopback addresses from the bound sockets.
    let mut addr = endpoint.addr();
    for _ in 0..50 {
        if addr.addrs.iter().any(|a| a.is_ip()) || !matches!(net, HostNet::Lan | HostNet::HybridN0) {
            break;
        }
        n0_future::time::sleep(std::time::Duration::from_millis(100)).await;
        addr = endpoint.addr();
    }
    if matches!(net, HostNet::Direct) {
        for sa in endpoint.bound_sockets() {
            let ip: std::net::IpAddr = if sa.is_ipv4() { std::net::Ipv4Addr::LOCALHOST.into() } else { std::net::Ipv6Addr::LOCALHOST.into() };
            addr.addrs.insert(TransportAddr::Ip((ip, sa.port()).into()));
        }
    }
    addr.addrs.retain(|a| match (&net, a) {
        (HostNet::Direct, TransportAddr::Ip(sa)) => sa.ip().is_loopback(),
        (HostNet::Lan, TransportAddr::Ip(sa)) => !sa.ip().is_loopback(),
        (HostNet::RelayN0 | HostNet::RelayLocal(_), a) => a.is_relay(),
        (HostNet::HybridN0, a) => a.is_relay() || a.is_ip(),
        _ => false,
    });
    tracing::debug!(addrs = ?addr.addrs, bound = ?endpoint.bound_sockets(), "ticket candidates");
    if addr.addrs.is_empty() {
        bail!("ticket would have no address for {net:?}");
    }

    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).expect("system randomness");
    let ticket = Ticket { version: 1, addr, secret };
    let host = Host { secret, participants: Arc::default() };
    let router = Router::builder(endpoint).accept(ALPN, host).spawn();
    Ok(HostHandle { router, ticket, bind_ms })
}

fn paths_line(conn: &Connection) -> String {
    describe_paths(conn)
        .iter()
        .map(|p| format!("[{}{} {} rtt={:.1}ms]", if p.selected { "*" } else { "" }, p.kind, p.remote, p.rtt_ms))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Host {
    async fn serve(&self, conn: Connection) -> Result<()> {
        let remote = conn.remote_id();
        let (mut send, mut recv) = conn.accept_bi().await?;
        let (tag, hello) = read_frame(&mut recv).await?;
        if tag != TAG_HELLO || hello.len() < 48 {
            bail!("malformed Hello");
        }
        if hello[..32] != self.secret {
            write_frame(&mut send, TAG_REFUSED, b"bad secret").await?;
            send.finish().ok();
            warn!(%remote, "Hello refused: wrong secret");
            conn.closed().await;
            return Ok(());
        }
        let mut id = [0u8; 16];
        id.copy_from_slice(&hello[32..48]);
        let name = String::from_utf8_lossy(&hello[48..]).to_string();
        let resumed = {
            let mut map = self.participants.lock().expect("participants lock");
            let known = map.contains_key(&id);
            *map.entry(id).or_insert(0) += 1;
            known
        };
        write_frame(&mut send, TAG_WELCOME, &[u8::from(resumed)]).await?;
        info!(%remote, %name, resumed, paths = %paths_line(&conn), "guest joined");

        let mut pings = 0u32;
        loop {
            let (tag, payload) = match read_frame(&mut recv).await {
                Ok(f) => f,
                Err(e) => {
                    info!(%remote, pings, "stream ended: {e}");
                    break;
                }
            };
            match tag {
                TAG_PING => {
                    pings += 1;
                    write_frame(&mut send, TAG_PONG, &payload).await?;
                }
                TAG_BULK_DOWN => {
                    let n = u32::from_le_bytes(payload[..4].try_into()?) as usize;
                    send_raw(&mut send, n).await?;
                }
                TAG_BULK_UP => {
                    let n = u32::from_le_bytes(payload[..4].try_into()?) as usize;
                    drain_raw(&mut recv, n).await?;
                    write_frame(&mut send, TAG_ACK, &[]).await?;
                }
                TAG_BYE => {
                    info!(%remote, pings, paths = %paths_line(&conn), "guest said bye");
                    break;
                }
                other => bail!("unknown tag {other}"),
            }
        }
        let t = n0_future::time::Instant::now();
        let reason = conn.closed().await;
        info!(%remote, waited_ms = ms(t), "connection closed: {reason:?}");
        Ok(())
    }
}

impl ProtocolHandler for Host {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        self.serve(connection).await.map_err(|e| AcceptError::from_boxed(e.into()))
    }
}
