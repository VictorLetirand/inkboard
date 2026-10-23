//! What the native transport (`transport`) and the browser one (`web_transport`) have in common: the wire format (ALPN, preamble,
//! length-prefixed frames), the mapping of QUIC errors onto session errors, the relays a guest uses, the opening of the stream.
//! Nothing here needs a thread or an async runtime: only iroh's own types, which are the same in a browser.

use crate::config::{NetConfig, RelaySetting};
use crate::ticket::ticket_to_addr;
use inkboard_session::crypto::Ticket;
use inkboard_session::error::{Error, NetCause, Result};
use iroh::endpoint::{Connection as QuicConn, ConnectionError, IdleTimeout, QuicTransportConfig, ReadError, ReadExactError, RecvStream, SendStream};
use iroh::endpoint::{VarInt, WriteError, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, TransportAddr};
use std::fmt::Display;
use std::time::Duration;

/// ALPN of the session protocol (the trailing number is the transport framing version).
pub const ALPN: &[u8] = b"inkboard/session/1";
pub(crate) const PREAMBLE: &[u8; 4] = b"IBN1";
/// Application close codes.
pub(crate) const CLOSE_NORMAL: u32 = 0;
pub(crate) const CLOSE_REFUSED: u32 = 1;
/// Frames buffered per direction before back-pressure (reader) or `Congested` (sender).
pub(crate) const QUEUE: usize = 1024;
/// Bytes buffered per direction, whatever the number of frames: a peer that does not read (sender side: `Congested`) or a flood
/// that the application does not drain (reader side: back-pressure) cannot make this side hold more.
pub(crate) const MAX_QUEUED_BYTES: usize = 32 << 20;
/// On a local `close`, how long to wait for the peer to confirm it has read everything.
pub(crate) const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn net(cause: NetCause, detail: impl Into<String>) -> Error {
    Error::Network { cause, detail: detail.into() }
}

pub(crate) fn local(e: impl Display) -> Error {
    net(NetCause::Local, e.to_string())
}

/* ------------------------------------------------------------------------------------------------------------------ errors */

pub(crate) fn map_conn_error(e: &ConnectionError) -> Error {
    match e {
        ConnectionError::TimedOut => net(NetCause::TimedOut, "nothing received from the peer within the idle timeout"),
        ConnectionError::ApplicationClosed(c) if c.error_code == VarInt::from_u32(CLOSE_REFUSED) => net(NetCause::Refused, "the host refused the connection"),
        _ => Error::Closed,
    }
}

pub(crate) fn read_fault(e: &ReadError, conn: &QuicConn) -> Error {
    match e {
        ReadError::ConnectionLost(ce) => map_conn_error(ce),
        _ => conn.close_reason().map_or(Error::Closed, |ce| map_conn_error(&ce)),
    }
}

pub(crate) fn write_fault(e: &WriteError, conn: &QuicConn) -> Error {
    match e {
        WriteError::ConnectionLost(ce) => map_conn_error(ce),
        _ => conn.close_reason().map_or(Error::Closed, |ce| map_conn_error(&ce)),
    }
}

/* ------------------------------------------------------------------------------------------------------------------ frames */

pub(crate) enum FrameError {
    Read(ReadError),
    Truncated,
    TooLarge(usize),
}

/// `Ok(None)` at a clean end of stream (between two frames).
pub(crate) async fn read_frame(recv: &mut RecvStream, max_frame: usize) -> std::result::Result<Option<Vec<u8>>, FrameError> {
    let mut len = [0u8; 4];
    match recv.read_exact(&mut len).await {
        Ok(()) => {}
        Err(ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(ReadExactError::FinishedEarly(_)) => return Err(FrameError::Truncated),
        Err(ReadExactError::ReadError(e)) => return Err(FrameError::Read(e)),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > max_frame {
        return Err(FrameError::TooLarge(n));
    }
    let mut frame = vec![0u8; n];
    match recv.read_exact(&mut frame).await {
        Ok(()) => Ok(Some(frame)),
        Err(ReadExactError::FinishedEarly(_)) => Err(FrameError::Truncated),
        Err(ReadExactError::ReadError(e)) => Err(FrameError::Read(e)),
    }
}

pub(crate) async fn write_frame(send: &mut SendStream, frame: &[u8]) -> std::result::Result<(), WriteError> {
    send.write_all(&(frame.len() as u32).to_le_bytes()).await?;
    send.write_all(frame).await
}

/* ------------------------------------------------------------------------------------------------------------------ dialing */

pub(crate) fn quic_config(idle: Duration) -> QuicTransportConfig {
    let idle: IdleTimeout = idle.clamp(Duration::from_secs(1), Duration::from_secs(3600)).try_into().expect("clamped idle timeout fits");
    QuicTransportConfig::builder().max_idle_timeout(Some(idle)).build()
}

/// Relays a guest uses to reach the host of a given ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GuestRelays {
    Off,
    Default,
    List(Vec<RelayUrl>),
}

impl GuestRelays {
    fn for_ticket(setting: &RelaySetting, ticket_relays: &[RelayUrl]) -> Self {
        let mut list: Vec<RelayUrl> = match setting {
            RelaySetting::Disabled => return Self::Off,
            RelaySetting::Default => Vec::new(),
            RelaySetting::Custom(urls) => urls.clone(),
        };
        list.extend(ticket_relays.iter().cloned());
        list.sort();
        list.dedup();
        if list.is_empty() { Self::Default } else { Self::List(list) }
    }

    fn mode(&self) -> RelayMode {
        match self {
            Self::Off => RelayMode::Disabled,
            Self::Default => RelayMode::Default,
            Self::List(urls) => RelayMode::custom(urls.iter().cloned()),
        }
    }
}

pub(crate) async fn bind_guest(cfg: &NetConfig, relays: &GuestRelays) -> Result<Endpoint> {
    let builder = Endpoint::builder(presets::Minimal).relay_mode(relays.mode()).transport_config(quic_config(cfg.idle_timeout));
    #[cfg(not(target_arch = "wasm32"))]
    let builder = if cfg.relay_only { builder.clear_ip_transports() } else { builder };
    builder.bind().await.map_err(local)
}

/// The address of a ticket, without the relays this guest may not use (see `IrohTransport::connect_ticket`).
pub(crate) fn ticket_addr(cfg: &NetConfig, ticket: &Ticket) -> Result<EndpointAddr> {
    let mut addr = ticket_to_addr(ticket)?;
    let before = addr.addrs.len();
    addr.addrs.retain(|a| match a {
        TransportAddr::Relay(url) => cfg.allows_ticket_relay(url),
        _ => true,
    });
    if addr.addrs.is_empty() && before > 0 {
        return Err(net(NetCause::HostUnreachable, "the relay address in the link is not allowed (https on a public address is required)"));
    }
    Ok(addr)
}

/// What a guest dials: the address (without relays when they are disabled) and the relays its endpoint must use.
pub(crate) fn dial_plan(cfg: &NetConfig, mut addr: EndpointAddr) -> Result<(EndpointAddr, GuestRelays)> {
    let ticket_relays: Vec<RelayUrl> = addr.relay_urls().cloned().collect();
    let relays = GuestRelays::for_ticket(&cfg.relays, &ticket_relays);
    if relays == GuestRelays::Off {
        addr.addrs.retain(|a| !a.is_relay());
    }
    if addr.is_empty() {
        return Err(net(NetCause::HostUnreachable, "the link has no usable address (relays are disabled and no direct address is known)"));
    }
    Ok((addr, relays))
}

/// A failed dial: when relays were expected but the endpoint never reached one, the relay is the culprit.
pub(crate) fn dial_failure(endpoint: &Endpoint, relays_on: bool, detail: String) -> Error {
    if relays_on && endpoint.addr().relay_urls().next().is_none() {
        net(NetCause::RelayUnreachable, format!("no connection to a relay ({detail})"))
    } else {
        net(NetCause::HostUnreachable, detail)
    }
}

/// Connects to `addr`, opens the stream and writes the preamble (so that the host sees the stream at once).
pub(crate) async fn open_stream(endpoint: &Endpoint, addr: EndpointAddr, relays_on: bool) -> Result<(QuicConn, SendStream, RecvStream)> {
    let conn = endpoint.connect(addr, ALPN).await.map_err(|e| dial_failure(endpoint, relays_on, e.to_string()))?;
    let (mut send, recv) = conn.open_bi().await.map_err(|e| map_conn_error(&e))?;
    send.write_all(PREAMBLE).await.map_err(|e| net(NetCause::HostUnreachable, e.to_string()))?;
    Ok((conn, send, recv))
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkboard_session::crypto::{HostKeys, SeededEntropy};
    use inkboard_session::proto::Role;

    fn id() -> iroh::EndpointId {
        iroh::SecretKey::from_bytes(&[7; 32]).public()
    }

    fn relay(url: &str) -> RelayUrl {
        url.parse().expect("a relay url")
    }

    #[test]
    fn with_relays_disabled_only_a_direct_address_can_be_dialed() {
        let cfg = NetConfig { relays: RelaySetting::Disabled, ..NetConfig::default() };
        let only_relay = EndpointAddr::from_parts(id(), [TransportAddr::Relay(relay("https://relay.example.org/"))]);
        assert!(matches!(dial_plan(&cfg, only_relay), Err(Error::Network { cause: NetCause::HostUnreachable, .. })));
        let both =
            EndpointAddr::from_parts(id(), [TransportAddr::Relay(relay("https://relay.example.org/")), TransportAddr::Ip("127.0.0.1:9".parse().unwrap())]);
        let (addr, relays) = dial_plan(&cfg, both).expect("the direct address is enough");
        assert_eq!(relays, GuestRelays::Off);
        assert!(addr.relay_urls().next().is_none() && addr.ip_addrs().count() == 1);
    }

    #[test]
    fn the_relays_of_the_ticket_join_the_configured_ones_without_repeats() {
        let (a, b) = (relay("https://a.example.org/"), relay("https://b.example.org/"));
        assert_eq!(GuestRelays::for_ticket(&RelaySetting::Default, &[]), GuestRelays::Default);
        assert_eq!(GuestRelays::for_ticket(&RelaySetting::Custom(vec![b.clone()]), &[a.clone(), b.clone()]), GuestRelays::List(vec![a.clone(), b]));
        assert_eq!(GuestRelays::for_ticket(&RelaySetting::Disabled, &[a]), GuestRelays::Off);
    }

    #[test]
    fn a_ticket_whose_only_relay_is_not_allowed_is_refused() {
        let keys = HostKeys::generate(&mut SeededEntropy::new(7)).unwrap();
        let ticket = keys.ticket(Role::Edit, &id().to_string(), &["https://192.168.1.5/".to_owned()]).unwrap();
        assert!(matches!(ticket_addr(&NetConfig::default(), &ticket), Err(Error::Network { cause: NetCause::HostUnreachable, .. })));
        assert!(ticket_addr(&NetConfig { allow_private_relays: true, ..NetConfig::default() }, &ticket).is_ok());
    }
}
