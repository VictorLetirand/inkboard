//! Images by hash: manifest, chunking and reassembly. An image already known (same hash) is never sent again.

use crate::error::{Error, Result};
use crate::limits::Violation;
use crate::proto::{AssetChunk, AssetInfo};
use inkboard_core::model::Document;
use sha2::{Digest, Sha256};

/// Lowercase hexadecimal SHA-256 of `data`.
pub fn hash_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Manifest of the images of a document (what a `Snapshot` lists instead of their bytes).
pub fn manifest(doc: &Document) -> Vec<AssetInfo> {
    doc.assets.iter().map(|(id, a)| AssetInfo { id: id.clone(), hash: hash_hex(&a.data), size: a.data.len() as u64, mime: a.mime.clone() }).collect()
}

/// Splits an image into chunks of at most `chunk_size` bytes (at least one chunk; the last one is flagged).
pub fn chunks(data: &[u8], hash: &str, chunk_size: usize) -> Vec<AssetChunk> {
    let size = chunk_size.max(1);
    let n = data.len().div_ceil(size).max(1);
    (0..n)
        .map(|i| {
            let from = (i * size).min(data.len());
            let to = (from + size).min(data.len());
            AssetChunk { hash: hash.to_owned(), offset: from as u64, data: data[from..to].to_vec(), last: i + 1 == n }
        })
        .collect()
}

/// Reassembles an image from its chunks and checks it against the announced hash and size.
#[derive(Clone, Debug)]
pub struct AssetAssembler {
    hash: String,
    size: u64,
    /// `false` when the size is unknown (an image learnt from a modification, not from a manifest): `size` is then only a ceiling.
    exact: bool,
    buf: Vec<u8>,
}

impl AssetAssembler {
    /// `size` and `hash` come from the manifest; `max_size` is the largest image accepted.
    pub fn new(hash: &str, size: u64, max_size: u64) -> Result<Self> {
        if size > max_size {
            return Err(Violation::ImageTooLarge { size, max: max_size }.into());
        }
        Ok(Self { hash: hash.to_owned(), size, exact: true, buf: Vec::new() })
    }

    /// For an image whose size is not known in advance: at most `max_size` bytes are taken, the hash alone vouches for the content.
    pub fn with_unknown_size(hash: &str, max_size: u64) -> Self {
        Self { hash: hash.to_owned(), size: max_size, exact: false, buf: Vec::new() }
    }

    /// Adds a chunk. Returns the complete image after the last chunk. A repeated chunk is ignored; a gap is an error (ask again).
    pub fn push(&mut self, c: &AssetChunk) -> Result<Option<Vec<u8>>> {
        if c.hash != self.hash {
            return Err(Error::Unexpected("chunk of another image"));
        }
        let have = self.buf.len() as u64;
        if c.offset < have {
            return Ok(None);
        }
        if c.offset > have {
            return Err(Error::Unexpected("chunk out of order"));
        }
        if have + c.data.len() as u64 > self.size {
            return Err(Violation::ImageTooLarge { size: have + c.data.len() as u64, max: self.size }.into());
        }
        self.buf.extend_from_slice(&c.data);
        if !c.last {
            return Ok(None);
        }
        if (self.exact && self.buf.len() as u64 != self.size) || hash_hex(&self.buf) != self.hash {
            return Err(Error::BadFrame("image does not match its hash"));
        }
        Ok(Some(std::mem::take(&mut self.buf)))
    }
}
