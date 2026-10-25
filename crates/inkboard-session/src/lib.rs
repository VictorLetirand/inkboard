//! `inkboard-session`: the shared-session protocol, independent of the real network and of the interface.
//!
//! Pure Rust that builds natively and for `wasm32-unknown-unknown`.
//!
//! - [`proto`]: versioned messages. [`wire`]: index-free operations and their conversion from/to the core's `Op`.
//! - [`codec`]: message encoding (JSON for now). [`crypto`]: invitation link, key derivation, encrypted frames.
//! - [`transport`]: abstract connections; [`sim`]: in-memory network with injected latency, loss, duplication and reordering.
//! - [`sequencer`]: ordering, duplicate detection and catch-up on the host side; [`client`]: optimistic guest.
//! - [`limits`]: validation of everything a guest sends. [`assets`]: images by hash.
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

pub mod assets;
pub mod client;
pub mod codec;
pub mod crypto;
pub mod error;
pub mod limits;
pub mod pack;
pub mod proto;
pub mod sequencer;
pub mod sim;
pub mod snapshot;
pub mod transport;
pub mod wire;

pub use client::{Client, ClientConfig, ClientEvent, Phase};
pub use codec::{Codec, JsonCodec};
pub use error::{Error, NetCause, Result};
pub use limits::{Limits, Violation};
pub use proto::{Message, PROTO_VERSION, ParticipantId, Role};
pub use sequencer::Sequencer;
pub use wire::WireOp;
