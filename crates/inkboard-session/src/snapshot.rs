//! Snapshots too large for one frame: the host cuts them into numbered parts, the guest puts them back together.
//!
//! A transport refuses frames above a limit (1 MiB for the real one), and a board of a few thousand objects is larger than that. A
//! snapshot that fits in `budget` bytes still travels as one `Message::Snapshot`; a larger one becomes `Message::SnapshotPart`s, cut
//! between objects (a page can span several parts). The assembler tolerates duplicated and reordered parts and starts over when a
//! part of a newer snapshot arrives.

use crate::error::{Error, Result};
use crate::limits::{Limits, Violation, check_manifest, check_snapshot_shape};
use crate::proto::{Message, Seq, Snapshot, SnapshotHead, SnapshotPart};
use inkboard_core::model::Page;
use std::collections::BTreeMap;

/// Size (in encoded bytes, estimated from the JSON form) a snapshot message aims at: well under the frame limit of the real transport.
pub const PART_BYTES: usize = 192 << 10;
/// Most parts a guest keeps for one snapshot.
pub const MAX_PARTS: usize = 4096;

fn json_len<T: serde::Serialize>(v: &T) -> usize {
    serde_json::to_vec(v).map_or(0, |b| b.len())
}

/// The messages that carry `s`: itself when it fits in `budget` bytes, otherwise numbered parts of about `budget` bytes each (a single
/// object larger than the budget travels alone in its part).
pub fn into_messages(s: Snapshot, budget: usize) -> Vec<Message> {
    let sizes: Vec<Vec<usize>> = s.pages.iter().map(|p| p.objects.iter().map(json_len).collect()).collect();
    let head_size = json_len(&s.assets) + 512;
    let total = head_size + s.pages.iter().zip(&sizes).map(|(p, sz)| json_len(&p.layers) + 512 + sz.iter().sum::<usize>()).sum::<usize>();
    if total <= budget {
        return vec![Message::Snapshot(s)];
    }
    let Snapshot { seq, name, background, pages, assets } = s;
    let mut parts: Vec<SnapshotPart> = Vec::new();
    let mut cur = SnapshotPart { seq, part: 0, last: false, head: Some(SnapshotHead { name, background, assets }), pages: Vec::new() };
    let (mut used, mut objects) = (head_size, 0usize);
    for (mut page, sz) in pages.into_iter().zip(sizes) {
        let all = std::mem::take(&mut page.objects);
        let shell = json_len(&page) + 256;
        cur.pages.push(page.clone());
        used += shell;
        for (o, n) in all.into_iter().zip(sz) {
            if used + n > budget && objects > 0 {
                cur.part = parts.len() as u32;
                let next = SnapshotPart { seq, part: 0, last: false, head: None, pages: vec![page.clone()] };
                parts.push(std::mem::replace(&mut cur, next));
                (used, objects) = (shell, 0);
            }
            if let Some(p) = cur.pages.last_mut() {
                p.objects.push(o);
            }
            used += n;
            objects += 1;
        }
    }
    cur.part = parts.len() as u32;
    cur.last = true;
    parts.push(cur);
    parts.into_iter().map(Message::SnapshotPart).collect()
}

/// The parts of one snapshot received so far.
#[derive(Debug)]
pub struct SnapshotAssembler {
    seq: Seq,
    parts: BTreeMap<u32, SnapshotPart>,
    last: Option<u32>,
    objects: usize,
}

fn bad(why: &'static str) -> Error {
    Error::Invalid(Violation::Malformed(why))
}

impl SnapshotAssembler {
    pub fn new(seq: Seq) -> Self {
        Self { seq, parts: BTreeMap::new(), last: None, objects: 0 }
    }

    pub fn seq(&self) -> Seq {
        self.seq
    }

    /// Adds a part (of this assembler's `seq`; a part already held is ignored). Returns the document once every part is here, checked
    /// as a whole against `limits` and `max_objects`. An `Err` means the sender is not following the protocol.
    pub fn add(&mut self, p: SnapshotPart, limits: &Limits, max_objects: usize) -> Result<Option<Snapshot>> {
        if self.parts.contains_key(&p.part) {
            return Ok(None);
        }
        if self.parts.len() >= MAX_PARTS || p.part as usize >= MAX_PARTS {
            return Err(Violation::TooMany { what: "snapshot parts", count: self.parts.len() + 1, max: MAX_PARTS }.into());
        }
        if p.part == 0 && p.head.is_none() {
            return Err(bad("first snapshot part without its header"));
        }
        if p.last {
            if self.last.is_some_and(|l| l != p.part) || self.parts.keys().any(|&k| k > p.part) {
                return Err(bad("snapshot with two ends"));
            }
            self.last = Some(p.part);
        } else if self.last.is_some_and(|l| p.part >= l) {
            return Err(bad("snapshot part after its end"));
        }
        self.objects += p.pages.iter().map(|q| q.objects.len()).sum::<usize>();
        if self.objects > max_objects {
            return Err(Violation::TooMany { what: "objects", count: self.objects, max: max_objects }.into());
        }
        self.parts.insert(p.part, p);
        match self.last {
            Some(l) if self.parts.len() == l as usize + 1 => self.finish(limits, max_objects).map(Some),
            _ => Ok(None),
        }
    }

    fn finish(&mut self, limits: &Limits, max_objects: usize) -> Result<Snapshot> {
        let mut parts = std::mem::take(&mut self.parts).into_values();
        let first = parts.next().ok_or_else(|| bad("empty snapshot"))?;
        let head = first.head.ok_or_else(|| bad("first snapshot part without its header"))?;
        let mut pages: Vec<Page> = Vec::new();
        for piece in first.pages.into_iter().chain(parts.flat_map(|p| p.pages)) {
            match pages.last_mut() {
                Some(last) if last.id == piece.id => last.objects.extend(piece.objects),
                _ => pages.push(piece),
            }
        }
        check_snapshot_shape(limits, &pages, max_objects)?;
        check_manifest(limits, &head.assets)?;
        Ok(Snapshot { seq: self.seq, name: head.name, background: head.background, pages, assets: head.assets })
    }
}
