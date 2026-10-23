//! Errors of the session crate. Messages are meant to be shown to the user as is.

use crate::limits::Violation;
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("malformed message: {0}")]
    Codec(String),
    #[error("incompatible protocol version: the peer speaks v{theirs}, this build speaks v{ours}; update Inkboard (or reload the page)")]
    IncompatibleProtocol { ours: u32, theirs: u32 },
    #[error("invalid invitation link: {0}")]
    BadTicket(String),
    #[error("cannot decrypt the frame: wrong session key or altered data")]
    Decrypt,
    #[error("malformed frame: {0}")]
    BadFrame(&'static str),
    #[error("refused input: {0}")]
    Invalid(#[from] Violation),
    #[error("unexpected message: {0}")]
    Unexpected(&'static str),
    #[error("no source of randomness available")]
    Entropy,
    #[error("connection closed")]
    Closed,
    /// A real transport failed; `cause` lets the interface choose its message, `detail` is for the diagnostic.
    #[error("{cause}: {detail}")]
    Network { cause: NetCause, detail: String },
}

/// Why a real network operation failed (see [`Error::Network`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetCause {
    /// The relay service cannot be reached (firewall, security gateway, no Internet).
    RelayUnreachable,
    /// The host cannot be reached (offline, wrong address, blocked).
    HostUnreachable,
    /// An established connection stayed silent for longer than the idle timeout.
    TimedOut,
    /// The host actively refused the connection.
    Refused,
    /// The outgoing queue is full: the peer or the network is too slow.
    Congested,
    /// This machine could not open its network endpoint (no socket, no runtime).
    Local,
}

impl std::fmt::Display for NetCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RelayUnreachable => "cannot reach the connection relay",
            Self::HostUnreachable => "cannot reach the host",
            Self::TimedOut => "the connection timed out",
            Self::Refused => "the host refused the connection",
            Self::Congested => "the connection is too slow",
            Self::Local => "the network endpoint of this computer could not start",
        })
    }
}

pub type Result<T> = std::result::Result<T, Error>;
