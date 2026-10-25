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
    /// The last `Applied`, each with an estimate of the bytes it holds.
    log: VecDeque<(usize, Applied)>,
    log_cap: usize,
    log_bytes: usize,
    log_budget: usize,
    /// Last change of each object: when and by whom.
    changed: HashMap<Id, (Seq, ParticipantId)>,
    changed_cap: usize,
    seen: HashMap<ParticipantId, SeqWindow>,
}

impl Sequencer {
    /// Bytes (estimated: JSON size of the operations) the log of `Applied` may hold; the oldest entries go first, and a guest that
    /// needs one of them is given a snapshot instead.
    pub const DEFAULT_LOG_BYTES: usize = 64 << 20;
    /// Objects whose last change is remembered (it only tells a guest that somebody changed the same object meanwhile).
    pub const DEFAULT_CHANGED_CAP: usize = 200_000;

    /// `log_cap`: number of `Applied` kept to catch guests up (the plan suggests 1 000).
    #[must_use]
    pub fn new(doc: Document, log_cap: usize) -> Self {
        Self {
            doc,
            seq: 0,
            log: VecDeque::new(),
            log_cap,
            log_bytes: 0,
            log_budget: Self::DEFAULT_LOG_BYTES,
            changed: HashMap::new(),
            changed_cap: Self::DEFAULT_CHANGED_CAP,
            seen: HashMap::new(),
        }
    }

    /// Changes the budgets: bytes of the log and objects whose last change is remembered.
    pub fn set_budgets(&mut self, log_bytes: usize, changed_cap: usize) {
        self.log_budget = log_bytes;
        self.changed_cap = changed_cap;
        self.trim_log();
    }

    /// Entries of the log and their estimated bytes.
    #[must_use]
    pub fn log_stats(&self) -> (usize, usize) {
        (self.log.len(), self.log_bytes)
    }

    /// Objects whose last change is remembered.
    #[must_use]
    pub fn tracked_objects(&self) -> usize {
        self.changed.len()
    }

    fn trim_log(&mut self) {
        while self.log.len() > self.log_cap || self.log_bytes > self.log_budget {
            let Some((cost, _)) = self.log.pop_front() else { break };
            self.log_bytes -= cost;
        }
    }

    #[must_use]
    pub fn doc(&self) -> &Document {
        &self.doc
    }
    #[must_use]
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
        if self.changed.len() > self.changed_cap {
            // eviction: the older half goes (by sequence number); a very large tie at the cut empties the table, which is also bounded
            let keep = self.changed_cap / 2;
            let mut seqs: Vec<Seq> = self.changed.values().map(|(s, _)| *s).collect();
            seqs.sort_unstable();
            let threshold = seqs.get(seqs.len().saturating_sub(keep)).copied().unwrap_or(Seq::MAX);
            self.changed.retain(|_, (s, _)| *s >= threshold);
            if self.changed.len() > self.changed_cap {
                self.changed.clear();
            }
        }
        let applied = Applied { seq: self.seq, author, client_seq, ops, superseded };
        let cost = serde_json::to_vec(&applied.ops).map_or(0, |b| b.len()) + 64;
        self.log.push_back((cost, applied.clone()));
        self.log_bytes += cost;
        self.trim_log();
        applied
    }

    /// Adds an image to the authoritative document (images do not go through `Applied`: guests fetch them by hash).
    pub fn insert_asset(&mut self, id: inkboard_core::model::AssetId, asset: inkboard_core::model::Asset) {
        self.doc.assets.insert(id, asset);
    }

    #[must_use]
    pub fn snapshot(&self, assets: Vec<crate::proto::AssetInfo>) -> Snapshot {
        Snapshot::new(&self.doc, self.seq, assets)
    }

    /// Forgets the duplicate window of a participant (when it is removed or its place expires).
    pub fn forget(&mut self, p: ParticipantId) {
        self.seen.remove(&p);
    }

    /// The `Applied`s after `last_seq` (possibly none), or `None` when they are not all retained or `last_seq` is in the future: the
    /// guest needs the whole document. Unlike [`Sequencer::catch_up`] it builds no snapshot.
    #[must_use]
    pub fn applied_since(&self, last_seq: Seq) -> Option<Vec<Applied>> {
        if last_seq > self.seq {
            return None;
        }
        if last_seq == self.seq {
            return Some(Vec::new());
        }
        match self.log.front() {
            Some((_, first)) if first.seq <= last_seq + 1 => Some(self.log.iter().map(|(_, a)| a).filter(|a| a.seq > last_seq).cloned().collect()),
            _ => None,
        }
    }

    /// The `Applied`s after `last_seq`, or a `Snapshot` when they are not all retained (or `last_seq` is in the future).
    #[must_use]
    pub fn catch_up(&self, last_seq: Seq, assets: Vec<crate::proto::AssetInfo>) -> CatchUp {
        if last_seq > self.seq {
            return CatchUp::Snapshot(self.snapshot(assets));
        }
        if last_seq == self.seq {
            return CatchUp::Applied(Vec::new());
        }
        match self.log.front() {
            Some((_, first)) if first.seq <= last_seq + 1 => CatchUp::Applied(self.log.iter().map(|(_, a)| a).filter(|a| a.seq > last_seq).cloned().collect()),
            _ => CatchUp::Snapshot(self.snapshot(assets)),
        }
    }
}
