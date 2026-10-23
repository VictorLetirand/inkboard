//! `inkboard-net`: the network of shared sessions (native and browser).
//!
//! - [`IrohTransport`] / [`IrohConn`]: `inkboard_session::transport::{Transport, Connection}` over iroh 1.4 (QUIC, direct path or
//!   relay, 120 s idle timeout), see [`transport`] for the wire format.
//! - [`NetConfig`], [`RelaySetting`]: relays (n0 public relays by default, custom list, or none) and timeouts.
//! - [`ticket_to_addr`], [`addr_to_hints`]: conversion between the session `Ticket` and the iroh address.
//! - [`diagnose_relay`]: what answers at a relay's address (security gateway detection).
//! - [`WebTransport`] / [`WebConn`] (browser, `wasm32` only): the guest side over iroh in a page, through a relay (no direct path in a
//!   browser). Same wire format and error causes as the native transport (see `wire`); asynchronous instead of blocking.
//!
//! Everything that needs sockets, threads or tokio's runtime (`transport`, `diagnose`) is native only.

mod cancel;
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod diagnose;
pub mod ticket;
#[cfg(not(target_arch = "wasm32"))]
pub mod transport;
#[cfg(target_arch = "wasm32")]
pub mod web_transport;
mod wire;

pub use cancel::CancelToken;
pub use config::{InvalidRelayUrl, NetConfig, RelaySetting, default_relay_urls};
#[cfg(not(target_arch = "wasm32"))]
pub use diagnose::{Diagnosis, GatewayKind, classify_gateway, diagnose_relay};
pub use ticket::{addr_to_hints, ticket_to_addr};
#[cfg(not(target_arch = "wasm32"))]
pub use transport::{IrohConn, IrohTransport, QueueProbe};
#[cfg(target_arch = "wasm32")]
pub use web_transport::{WebConn, WebTransport};
