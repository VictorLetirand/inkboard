//! `Ticket` (lot B) <-> iroh address.
//!
//! The ticket's `host` is the iroh endpoint id (text). Its `relays` field carries the *address hints*: a hint is either a relay
//! URL (`https://relay.example.org/`) or a direct socket address (`192.168.1.20:5000`). The ticket format is unchanged; the field
//! keeps its historical name. A ticket holds at most 8 hints in total.

use crate::config::parse_relay_url;
use inkboard_session::crypto::Ticket;
use inkboard_session::error::{Error, Result};
use iroh::{EndpointAddr, EndpointId, TransportAddr};
use std::net::SocketAddr;

/// Maximum number of hints a ticket can carry.
pub const MAX_HINTS: usize = 8;

/// iroh address of the host named by `ticket`. Unknown or malformed hints are ignored (forward compatibility); the caller decides
/// what an address without any hint means.
pub fn ticket_to_addr(ticket: &Ticket) -> Result<EndpointAddr> {
    let id: EndpointId = ticket.host.parse().map_err(|_| Error::BadTicket("the host identity in the link is not valid".to_owned()))?;
    Ok(EndpointAddr::from_parts(id, ticket.relays.iter().filter_map(|h| parse_hint(h))))
}

fn parse_hint(hint: &str) -> Option<TransportAddr> {
    if let Ok(sa) = hint.parse::<SocketAddr>() {
        return Some(TransportAddr::Ip(sa));
    }
    parse_relay_url(hint).ok().map(TransportAddr::Relay)
}

/// Hint strings for an address: relays first, then direct addresses (IPv4 before IPv6), at most [`MAX_HINTS`].
pub fn addr_to_hints(addr: &EndpointAddr) -> Vec<String> {
    let relays = addr.relay_urls().map(ToString::to_string);
    let mut ips: Vec<&SocketAddr> = addr.ip_addrs().collect();
    ips.sort_by_key(|sa| sa.is_ipv6());
    relays.chain(ips.into_iter().map(ToString::to_string)).take(MAX_HINTS).collect()
}
