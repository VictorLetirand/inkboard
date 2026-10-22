//! Configuration of the network transport: relays, timeouts, frame size.

use iroh::{RelayMode, RelayUrl};
use std::fmt;
use std::time::Duration;

/// Idle timeout of a QUIC connection. The iroh default (30 s) loses a session after a ~20 s network outage; 120 s absorbed a
/// 60 s outage in the real-network tests (see `context/mcp-remote/tests-reseau-reels-2026-10-21.md`).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// A relay URL that does not parse (or is not `http(s)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidRelayUrl(pub String);

impl fmt::Display for InvalidRelayUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid relay URL: {}", self.0)
    }
}

impl std::error::Error for InvalidRelayUrl {}

/// Which relays to use. A relay forwards (encrypted) traffic when no direct path exists; it never sees the content.
#[derive(Clone, Debug, Default)]
pub enum RelaySetting {
    /// The public relays of n0 (free, rate limited, for development and beta).
    #[default]
    Default,
    /// A custom list (self-hosted relay). A guest also uses the relays named in the ticket.
    Custom(Vec<RelayUrl>),
    /// No relay: direct connections only (local network, loopback, tests).
    Disabled,
}

impl RelaySetting {
    /// Parses a list of relay URLs (`https://relay.example.org`). An empty list means [`RelaySetting::Default`].
    pub fn custom<I, S>(urls: I) -> Result<Self, InvalidRelayUrl>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let urls = urls.into_iter().map(|u| parse_relay_url(u.as_ref())).collect::<Result<Vec<_>, _>>()?;
        Ok(if urls.is_empty() { Self::Default } else { Self::Custom(urls) })
    }

    pub(crate) fn mode(&self) -> RelayMode {
        match self {
            Self::Default => RelayMode::Default,
            Self::Custom(urls) => RelayMode::custom(urls.iter().cloned()),
            Self::Disabled => RelayMode::Disabled,
        }
    }

    pub(crate) fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

pub(crate) fn parse_relay_url(s: &str) -> Result<RelayUrl, InvalidRelayUrl> {
    let url: RelayUrl = s.trim().parse().map_err(|_| InvalidRelayUrl(s.to_owned()))?;
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() { Ok(url) } else { Err(InvalidRelayUrl(s.to_owned())) }
}

/// URLs of the default public relays (to diagnose them with [`crate::diagnose_relay`]).
pub fn default_relay_urls() -> Vec<String> {
    RelayMode::Default.relay_map().urls::<Vec<RelayUrl>>().iter().map(ToString::to_string).collect()
}

#[derive(Clone, Debug)]
pub struct NetConfig {
    pub relays: RelaySetting,
    /// Silence tolerated on an established connection, both ends (default [`IDLE_TIMEOUT`]).
    pub idle_timeout: Duration,
    /// Deadline of `connect` (QUIC handshake and stream opening).
    pub connect_timeout: Duration,
    /// How long `ticket`/`hints` wait for the relay connection and the local addresses before giving up on them.
    pub ready_timeout: Duration,
    /// Largest accepted/sent frame, in bytes (already encrypted: payload limit plus the AEAD overhead).
    pub max_frame: usize,
    /// Host only: bind `127.0.0.1` only and advertise it (tests, two processes on one machine).
    pub loopback_only: bool,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            relays: RelaySetting::Default,
            idle_timeout: IDLE_TIMEOUT,
            connect_timeout: Duration::from_secs(30),
            ready_timeout: Duration::from_secs(10),
            max_frame: (1 << 20) + 1024,
            loopback_only: false,
        }
    }
}

impl NetConfig {
    /// No relay, host bound to the loopback interface: works offline and deterministically.
    pub fn loopback() -> Self {
        Self { relays: RelaySetting::Disabled, loopback_only: true, ..Self::default() }
    }
}
