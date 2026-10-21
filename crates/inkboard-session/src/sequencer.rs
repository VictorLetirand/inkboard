//! Host side of the consistency model: orders the accepted modifications, applies them to the authoritative document, ignores
//! duplicates, flags concurrent changes and answers "what did I miss". The host manager of the application (permissions, validation,
//! quotas, relaying) is built around it; this type only needs the document.

use crate::proto::{Applied, ParticipantId, Seq, Snapshot, Submit};
use crate::wire::{WireOp, apply_wire_all};
use inkboard_core::model::{Document, Id};
use std::collections::{BTreeSet, HashMap, VecDeque};

/// The `client_seq`s already applied for one participant (a floor and the ones above it, as they can arrive out of order).
#[derive(Clone, Debug, Default)]
struct SeqWindow {
    floor: u64,
    above: BTreeSet<u64>,
}

impl SeqWindow {
    const MAX_GAPS: usize = 4096;

    /// `false` when `n` was already seen.
    fn insert(&mut self, n: u64) -> bool {
        if n <= self.floor || !self.above.insert(n) {
            return false;
        }
        while self.above.remove(&(self.floor + 1)) {
            self.floor += 1;
        }
        if self.above.len() > Self::MAX_GAPS
            && let Some(m) = self.above.pop_first()
        {
            self.floor = m; // a guest that leaves so many holes forfeits the oldest ones
        }
        true
    }
}

/// Result of [`Sequencer::submit`].
#[derive(Clone, Debug, PartialEq)]
pub enum Submitted {
    Applied(Applied),
    /// This `client_seq` was already applied: nothing was done again.
    Duplicate,
}

/// What a guest that reconnects or detects a gap needs.
#[derive(Clone, Debug, PartialEq)]
pub enum CatchUp {
    /// The missing `Applied`s, in order (possibly none).
    Applied(Vec<Applied>),
    /// Too far behind (beyond the retained window): start again from the document.
    Snapshot(Snapshot),
}

#[derive(Clone, Debug)]
pub struct Sequencer {
    doc: Document,
    seq: Seq,
    log: VecDeque<Applied>,
    log_cap: usize,
    /// Last change of each object: when and by whom.
    changed: HashMap<Id, (Seq, ParticipantId)>,
    seen: HashMap<ParticipantId, SeqWindow>,
}

impl Sequencer {
    /// `log_cap`: number of `Applied` kept to catch guests up (the plan suggests 1 000).
    pub fn new(doc: Document, log_cap: usize) -> Self {
        Self { doc, seq: 0, log: VecDeque::new(), log_cap, changed: HashMap::new(), seen: HashMap::new() }
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }
    pub fn seq(&self) -> Seq {
        self.seq
    }

    /// Accepts a (validated) `Submit` from `author`, in arrival order.
    pub fn submit(&mut self, author: ParticipantId, s: Submit) -> Submitted {
        if !self.seen.entry(author).or_default().insert(s.client_seq) {
            return Submitted::Duplicate;
        }
        let mut superseded: Vec<Id> = Vec::new();
        for id in s.ops.iter().flat_map(WireOp::object_ids) {
            if self.changed.get(id).is_some_and(|&(at, by)| at > s.base_seq && by != author) && !superseded.contains(id) {
                superseded.push(id.clone());
            }
        }
        Submitted::Applied(self.commit(author, Some(s.client_seq), s.ops, superseded))
    }

    /// A modification made by the host itself (or by the agent) rather than submitted by a guest.
    pub fn apply_local(&mut self, author: ParticipantId, ops: Vec<WireOp>) -> Applied {
        self.commit(author, None, ops, Vec::new())
    }

    fn commit(&mut self, author: ParticipantId, client_seq: Option<u64>, ops: Vec<WireOp>, superseded: Vec<Id>) -> Applied {
        apply_wire_all(&mut self.doc, &ops);
        self.seq += 1;
        for id in ops.iter().flat_map(WireOp::object_ids) {
            self.changed.insert(id.clone(), (self.seq, author));
        }
        let applied = Applied { seq: self.seq, author, client_seq, ops, superseded };
        self.log.push_back(applied.clone());
        while self.log.len() > self.log_cap {
            self.log.pop_front();
        }
        applied
    }

    pub fn snapshot(&self, assets: Vec<crate::proto::AssetInfo>) -> Snapshot {
        Snapshot::new(&self.doc, self.seq, assets)
    }

    /// Forgets the duplicate window of a participant (when it is removed or its place expires).
    pub fn forget(&mut self, p: ParticipantId) {
        self.seen.remove(&p);
    }

    /// The `Applied`s after `last_seq`, or a `Snapshot` when they are not all retained (or `last_seq` is in the future).
    pub fn catch_up(&self, last_seq: Seq, assets: Vec<crate::proto::AssetInfo>) -> CatchUp {
        if last_seq > self.seq {
            return CatchUp::Snapshot(self.snapshot(assets));
        }
        if last_seq == self.seq {
            return CatchUp::Applied(Vec::new());
        }
        match self.log.front() {
            Some(first) if first.seq <= last_seq + 1 => CatchUp::Applied(self.log.iter().filter(|a| a.seq > last_seq).cloned().collect()),
            _ => CatchUp::Snapshot(self.snapshot(assets)),
        }
    }
}
