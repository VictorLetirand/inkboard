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
}

pub type Result<T> = std::result::Result<T, Error>;
