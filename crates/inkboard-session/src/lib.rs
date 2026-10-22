//! `inkboard-session`: the shared-session protocol, independent of the real network and of the interface.
//!
//! Pure Rust that builds natively and for `wasm32-unknown-unknown`.
//!
//! - [`proto`]: versioned messages. [`wire`]: index-free operations and their conversion from/to the core's `Op`.
//! - [`codec`]: message encoding (JSON for now). [`crypto`]: invitation link, key derivation, encrypted frames.
//! - [`transport`]: abstract connections; [`sim`]: in-memory network with injected latency, loss, duplication and reordering.
//! - [`sequencer`]: ordering, duplicate detection and catch-up on the host side; [`client`]: optimistic guest.
//! - [`limits`]: validation of everything a guest sends. [`assets`]: images by hash.

pub mod assets;
pub mod client;
pub mod codec;
pub mod crypto;
pub mod error;
pub mod limits;
pub mod proto;
pub mod sequencer;
pub mod sim;
pub mod transport;
pub mod wire;

pub use client::{Client, ClientConfig, ClientEvent, Phase};
pub use codec::{Codec, JsonCodec};
pub use error::{Error, NetCause, Result};
pub use limits::{Limits, Violation};
pub use proto::{Message, PROTO_VERSION, ParticipantId, Role};
pub use sequencer::Sequencer;
pub use wire::WireOp;
