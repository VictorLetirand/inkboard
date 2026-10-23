//! Compressed messages (protocol v3). The board is sent as JSON, which deflate shrinks about eightfold: a 5 000-object board goes from
//! 8 MiB to 1 MiB, which is what a guest on a relay or a slow link waits for, and what the host seals for each arrival.
//!
//! [`pack`] wraps a message in `Message::Packed` (deflate, then base64 like every binary field); [`unpack`] undoes it with a hard
//! ceiling on the size it will produce, so that a hostile peer cannot make a small frame expand into gigabytes. Only the host packs
//! (snapshot parts), and only guests unpack: the host never accepts a packed message.

use crate::error::{Error, Result};
use crate::limits::Violation;
use crate::proto::Message;
use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec_with_limit;

/// Compression level (1 = fastest): the encoding runs once per board, on a thread, and the text compresses well even at this level.
const LEVEL: u8 = 3;
/// Below this size packing is not worth its cost.
const MIN_BYTES: usize = 2 * 1024;

/// The message `m` in its compressed form, when it is big enough and gets smaller; `None` otherwise (send `m` as it is).
pub fn pack(m: &Message) -> Option<Message> {
    let json = serde_json::to_vec(m).ok()?;
    if json.len() < MIN_BYTES {
        return None;
    }
    let data = compress_to_vec(&json, LEVEL);
    (data.len() < json.len() / 2).then_some(Message::Packed { data })
}

/// The message inside a packed one. `max` bounds the size of the text it expands to. Only what the host packs may be inside: a snapshot
/// or a part of one (never another `Packed`).
pub fn unpack(data: &[u8], max: usize) -> Result<Message> {
    let json = decompress_to_vec_with_limit(data, max).map_err(|_| Error::Invalid(Violation::MessageTooLarge { size: max, max }))?;
    match serde_json::from_slice::<Message>(&json) {
        Ok(m @ (Message::Snapshot(_) | Message::SnapshotPart(_))) => Ok(m),
        Ok(_) => Err(Error::Unexpected("a packed message that is not a snapshot")),
        Err(e) => Err(Error::Codec(e.to_string())),
    }
}
