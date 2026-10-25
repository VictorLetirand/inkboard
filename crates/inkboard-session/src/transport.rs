//! Abstract transport: connections that carry opaque frames. Polling, no async runtime, so that it works the same natively and in a
//! browser; a real transport (iroh, WebSocket...) wraps its own tasks behind these two traits.

use crate::error::Result;

/// One connection to one peer. Frames are opaque (already encrypted); delivery may be lossy, duplicated or reordered, and the
/// protocol above copes with it.
pub trait Connection {
    /// Identity of the other end, as authenticated by the transport (e.g. an iroh node id). Not a proof of the role: see `crypto`.
    fn peer(&self) -> &str;
    fn send(&mut self, frame: &[u8]) -> Result<()>;
    /// Next received frame, if any (never blocks).
    fn recv(&mut self) -> Option<Vec<u8>>;
    fn is_open(&self) -> bool;
    fn close(&mut self);
    /// Bytes handed to `send` that the network has not taken yet (0 when the transport cannot tell). The host uses it to pace what it
    /// sends to a guest that reads slowly.
    fn queued_bytes(&self) -> usize {
        0
    }
}

pub trait Transport {
    type Conn: Connection;
    /// Opens a connection to the host identified by `host` (from the ticket).
    fn connect(&mut self, host: &str) -> Result<Self::Conn>;
    /// Next incoming connection, if any (never blocks).
    fn accept(&mut self) -> Option<Self::Conn>;
}
