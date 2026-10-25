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

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn mode(&self) -> RelayMode {
        match self {
            Self::Default => RelayMode::Default,
            Self::Custom(urls) => RelayMode::custom(urls.iter().cloned()),
            Self::Disabled => RelayMode::Disabled,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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

/// Is `ip` an address of the public Internet? Loopback, private, link-local, CGNAT, documentation, multicast, reserved and
/// unspecified addresses are not (IPv4-mapped and NAT64 IPv6 addresses are judged by the IPv4 address they carry).
pub(crate) fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            !(a.is_loopback()
                || a.is_private()
                || a.is_link_local()
                || a.is_unspecified()
                || a.is_broadcast()
                || a.is_multicast()
                || a.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 shared address space
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
                || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15 benchmarking
                || o[0] >= 240)
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            if let Some(v4) = a.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                let ([b0, b1], [b2, b3]) = (s[6].to_be_bytes(), s[7].to_be_bytes());
                let v4 = std::net::Ipv4Addr::new(b0, b1, b2, b3);
                return is_public_ip(IpAddr::V4(v4));
            }
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (s[0] == 0x2001 && s[1] == 0x0db8)) // documentation
        }
    }
}

/// Host name of a relay URL, lower case, without the trailing dot of a fully qualified name or the brackets of an IPv6 literal.
pub(crate) fn relay_host(url: &RelayUrl) -> Option<String> {
    let h = url.host_str()?.trim_end_matches('.').trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    (!h.is_empty()).then_some(h)
}

/// Checks a relay URL that comes from somebody else (the ticket of a link) before anything connects to it: `https` only, no
/// credentials, an ordinary port (443 or 1024 and above), a public DNS name (not `localhost`, not a single label, not a `.local`/
/// `.internal`/`.lan` name) or a public IP literal. `allow_private`: test mode, accepts any `http(s)` address (loopback, LAN).
/// A name that resolves to a private address is not caught here (see `diagnose_relay`, which resolves and checks).
pub fn check_relay_url(s: &str, allow_private: bool) -> Result<RelayUrl, InvalidRelayUrl> {
    let bad = || InvalidRelayUrl(s.to_owned());
    let url = parse_relay_url(s)?;
    if allow_private {
        return Ok(url);
    }
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(bad());
    }
    if !url.port_or_known_default().is_some_and(|p| p == 443 || p >= 1024) {
        return Err(bad());
    }
    let host = relay_host(&url).ok_or_else(bad)?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return if is_public_ip(ip) { Ok(url) } else { Err(bad()) };
    }
    let local_suffix = [".localhost", ".local", ".internal", ".lan", ".home.arpa", ".intranet", ".corp", ".private"];
    if host == "localhost" || !host.contains('.') || local_suffix.iter().any(|x| host.ends_with(x)) {
        return Err(bad());
    }
    Ok(url)
}

/// Host names of the relays that need no confirmation: the public relays of n0 and the configured list.
pub fn known_relay_hosts(setting: &RelaySetting) -> Vec<String> {
    let default = RelayMode::Default.relay_map().urls::<Vec<RelayUrl>>();
    let configured: &[RelayUrl] = if let RelaySetting::Custom(urls) = setting { urls } else { &[] };
    default.iter().chain(configured).filter_map(relay_host).collect()
}

/// Is the relay of this URL one of the public relays of n0 or of `configured` (host names or URLs)? A relay that is neither is
/// *unknown*: a link that uses it must be confirmed by the user, and no advice about it is ever given.
#[must_use]
pub fn is_known_relay(url: &str, configured: &[String]) -> bool {
    let Ok(url) = parse_relay_url(url) else { return false };
    let Some(host) = relay_host(&url) else { return false };
    let named = |c: &String| relay_host_of_text(c).is_some_and(|h| h == host);
    known_relay_hosts(&RelaySetting::Default).contains(&host) || configured.iter().any(named)
}

fn relay_host_of_text(s: &str) -> Option<String> {
    parse_relay_url(s).ok().and_then(|u| relay_host(&u)).or_else(|| Some(s.trim().trim_end_matches('.').to_ascii_lowercase()).filter(|h| !h.is_empty()))
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
    /// Browser guest only: time-out of the HTTPS probe of the relay that explains a failed connection (see [`crate::relay_signals`]).
    pub probe_timeout: Duration,
    /// Browser guest only: how long a connection attempt runs before the probe starts (an attempt that succeeds first costs no probe).
    pub probe_delay: Duration,
    /// Largest accepted/sent frame, in bytes (already encrypted: payload limit plus the AEAD overhead).
    pub max_frame: usize,
    /// Host only: bind `127.0.0.1` only and advertise it (tests, two processes on one machine).
    pub loopback_only: bool,
    /// Native only: no direct path at all (the endpoint opens no UDP socket of its own and talks through a relay). For the tests of
    /// the relay path and the load tests; a real session lets iroh upgrade to a direct path when it can.
    pub relay_only: bool,
    /// Test mode: a guest also uses relays from a ticket that [`check_relay_url`] would refuse (`http`, loopback, LAN). Off by default.
    pub allow_private_relays: bool,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            relays: RelaySetting::Default,
            idle_timeout: IDLE_TIMEOUT,
            connect_timeout: Duration::from_secs(30),
            ready_timeout: Duration::from_secs(10),
            probe_timeout: Duration::from_secs(5),
            probe_delay: Duration::from_secs(2),
            max_frame: (1 << 20) + 1024,
            loopback_only: false,
            relay_only: false,
            allow_private_relays: false,
        }
    }
}

impl NetConfig {
    /// No relay, host bound to the loopback interface: works offline and deterministically.
    #[must_use]
    pub fn loopback() -> Self {
        Self { relays: RelaySetting::Disabled, loopback_only: true, allow_private_relays: true, ..Self::default() }
    }

    /// May a guest use this relay, named by a ticket? The relays of the configured list always; any other only if it passes
    /// [`check_relay_url`] (https, public address, ordinary port).
    pub(crate) fn allows_ticket_relay(&self, url: &RelayUrl) -> bool {
        if let RelaySetting::Custom(urls) = &self.relays
            && urls.contains(url)
        {
            return true;
        }
        check_relay_url(url.as_str(), self.allow_private_relays).is_ok()
    }
}
