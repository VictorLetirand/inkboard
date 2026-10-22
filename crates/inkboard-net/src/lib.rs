//! `inkboard-net`: the native network of shared sessions.
//!
//! - [`IrohTransport`] / [`IrohConn`]: `inkboard_session::transport::{Transport, Connection}` over iroh 1.4 (QUIC, direct path or
//!   relay, 120 s idle timeout), see [`transport`] for the wire format.
//! - [`NetConfig`], [`RelaySetting`]: relays (n0 public relays by default, custom list, or none) and timeouts.
//! - [`ticket_to_addr`], [`addr_to_hints`]: conversion between the session `Ticket` and the iroh address.
//! - [`diagnose_relay`]: what answers at a relay's address (security gateway detection).
//!
//! Native only: the browser transport comes with the web client.
#![cfg(not(target_arch = "wasm32"))]

pub mod config;
pub mod diagnose;
pub mod ticket;
pub mod transport;

pub use config::{InvalidRelayUrl, NetConfig, RelaySetting, default_relay_urls};
pub use diagnose::{Diagnosis, GatewayKind, classify_gateway, diagnose_relay};
pub use ticket::{addr_to_hints, ticket_to_addr};
pub use transport::{CancelToken, IrohConn, IrohTransport};
