//! Encoding of messages as bytes. JSON for the trial version; replaceable through the [`Codec`] trait (a binary format later).

use crate::error::{Error, Result};
use crate::proto::{Hello, Message, check_proto};
use serde::Deserialize;

pub trait Codec: Send {
    fn encode(&self, msg: &Message) -> Vec<u8>;
    /// Never panics, whatever the bytes.
    fn decode(&self, bytes: &[u8]) -> Result<Message>;
    /// Reads only the protocol version of a `Hello`, so that an incompatible peer gets a clear error even when the rest of its
    /// message cannot be understood.
    fn peek_hello_proto(&self, bytes: &[u8]) -> Result<u32>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct JsonCodec;

#[derive(Deserialize)]
struct Peek {
    t: String,
    proto: u32,
}

impl Codec for JsonCodec {
    fn encode(&self, msg: &Message) -> Vec<u8> {
        serde_json::to_vec(msg).expect("serializing a message cannot fail")
    }
    fn decode(&self, bytes: &[u8]) -> Result<Message> {
        serde_json::from_slice(bytes).map_err(|e| Error::Codec(e.to_string()))
    }
    fn peek_hello_proto(&self, bytes: &[u8]) -> Result<u32> {
        let p: Peek = serde_json::from_slice(bytes).map_err(|e| Error::Codec(e.to_string()))?;
        if p.t == "hello" { Ok(p.proto) } else { Err(Error::Unexpected("the first message must be a hello")) }
    }
}

/// Host side of the handshake: checks the protocol version first, then decodes the `Hello`.
pub fn read_hello(codec: &dyn Codec, bytes: &[u8]) -> Result<Hello> {
    check_proto(codec.peek_hello_proto(bytes)?)?;
    match codec.decode(bytes)? {
        Message::Hello(h) => Ok(h),
        _ => Err(Error::Unexpected("the first message must be a hello")),
    }
}
