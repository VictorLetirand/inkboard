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
// Internal protocol crate: what can fail is told by the error types (`Error`, `Violation`), not repeated in a `# Errors` section on each function.
#![allow(clippy::missing_errors_doc)]
// Test code counts, casts and compares freely: these `clippy::pedantic` lints are for the code that ships.
#![cfg_attr(
    test,
    allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_lossless,
        clippy::float_cmp,
        clippy::many_single_char_names,
        clippy::similar_names,
        clippy::assert_is_empty,
        clippy::default_trait_access,
        clippy::format_collect,
        clippy::format_push_string,
        clippy::too_many_lines,
        clippy::items_after_statements,
        clippy::needless_raw_string_hashes,
        clippy::needless_pass_by_value,
        clippy::explicit_iter_loop,
        clippy::needless_for_each,
        clippy::collapsible_else_if,
        clippy::single_match_else,
        clippy::manual_assert,
        clippy::redundant_closure_for_method_calls,
        clippy::unnested_or_patterns,
        clippy::range_plus_one,
        clippy::clone_on_copy,
        clippy::match_same_arms,
        clippy::struct_excessive_bools
    )
)]

mod cancel;
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod diagnose;
pub mod relay_signals;
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
