//! Elementary reversible operations and the undo stack.

use crate::model::{Background, Document, Id, Layer, Object, Page};
use std::collections::HashMap;

/// Atomic modification of a document, always paired with its inverse.
#[derive(Clone, Debug)]
pub enum Op {
    Insert { page: Id, index: usize, obj: Box<Object> },
    Remove { page: Id, index: usize, obj: Box<Object> },
    Replace { page: Id, before: Box<Object>, after: Box<Object> },
    Reorder { page: Id, before: Vec<Id>, after: Vec<Id> },
    SetLayers { page: Id, before: Vec<Layer>, after: Vec<Layer> },
    AddPage { index: usize, page: Box<Page> },
    RemovePage { index: usize, page: Box<Page> },
    RenamePage { page: Id, before: String, after: String },
    SetBackground { before: Background, after: Background },
    SetName { before: String, after: String },
}

impl Op {
    pub fn inverse(&self) -> Op {
        match self.clone() {
            Op::Insert { page, index, obj } => Op::Remove { page, index, obj },
            Op::Remove { page, index, obj } => Op::Insert { page, index, obj },
            Op::Replace { page, before, after } => Op::Replace { page, before: after, after: before },
            Op::Reorder { page, before, after } => Op::Reorder { page, before: after, after: before },
            Op::SetLayers { page, before, after } => Op::SetLayers { page, before: after, after: before },
            Op::AddPage { index, page } => Op::RemovePage { index, page },
            Op::RemovePage { index, page } => Op::AddPage { index, page },
            Op::RenamePage { page, before, after } => Op::RenamePage { page, before: after, after: before },
            Op::SetBackground { before, after } => Op::SetBackground { before: after, after: before },
            Op::SetName { before, after } => Op::SetName { before: after, after: before },
        }
    }
}

impl Document {
    fn page_mut(&mut self, id: &Id) -> Option<&mut Page> {
        self.pages.iter_mut().find(|p| &p.id == id)
    }

    /// Applies an operation. Inconsistent operations (missing page or object) are ignored.
    pub fn apply(&mut self, op: &Op) {
        match op {
            Op::Insert { page, index, obj } => {
                if let Some(p) = self.page_mut(page) {
                    let i = (*index).min(p.objects.len());
                    p.objects.insert(i, (**obj).clone());
                }
            }
            Op::Remove { page, obj, .. } => {
                if let Some(p) = self.page_mut(page)
                    && let Some(i) = p.index_of(&obj.id)
                {
                    p.objects.remove(i);
                }
            }
            Op::Replace { page, after, .. } => {
                if let Some(p) = self.page_mut(page)
                    && let Some(i) = p.index_of(&after.id)
                {
                    p.objects[i] = (**after).clone();
                }
            }
            Op::Reorder { page, after, .. } => {
                if let Some(p) = self.page_mut(page) {
                    let mut rest: Vec<Object> = std::mem::take(&mut p.objects);
                    let mut out = Vec::with_capacity(rest.len());
                    for id in after {
                        if let Some(i) = rest.iter().position(|o| &o.id == id) {
                            out.push(rest.remove(i));
                        }
                    }
                    out.extend(rest);
                    p.objects = out;
                }
            }
            Op::SetLayers { page, after, .. } => {
                if let Some(p) = self.page_mut(page) {
                    p.layers = after.clone();
                }
            }
            Op::AddPage { index, page } => {
                let i = (*index).min(self.pages.len());
                self.pages.insert(i, (**page).clone());
            }
            Op::RemovePage { page, .. } => {
                if self.pages.len() > 1 {
                    self.pages.retain(|p| p.id != page.id);
                }
            }
            Op::RenamePage { page, after, .. } => {
                if let Some(p) = self.page_mut(page) {
                    p.name = after.clone();
                }
            }
            Op::SetBackground { after, .. } => self.background = *after,
            Op::SetName { after, .. } => self.name = after.clone(),
        }
    }
}

/// Undo stack: one entry = one transaction (a list of operations).
#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<Vec<Op>>,
    redo: Vec<Vec<Op>>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(500)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit }
    }
    pub fn push(&mut self, tx: Vec<Op>) {
        if tx.is_empty() {
            return;
        }
        self.redo.clear();
        self.undo.push(tx);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(tx) = self.undo.pop() else { return false };
        for op in tx.iter().rev() {
            doc.apply(&op.inverse());
        }
        self.redo.push(tx);
        true
    }
    pub fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(tx) = self.redo.pop() else { return false };
        for op in &tx {
            doc.apply(op);
        }
        self.undo.push(tx);
        true
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

/* ------------------------------------------------------------------------------------------------------------------------------
 * Per-author undo for shared sessions (host side). Independent of `History`, which stays the host's own stack.
 * ---------------------------------------------------------------------------------------------------------------------------- */

/// Identity of the author of a modification in a shared session (the session host maps its participants to it; 0 is the host).
pub type Author = u32;

/// An object, or a document-level setting (`object == None`), that an undo or redo left alone because somebody else changed it since.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub object: Option<Id>,
    /// Last author known to have modified it.
    pub by: Option<Author>,
}

/// What an author-based undo or redo does: the core operations to apply (in order) and what was left alone.
#[derive(Clone, Debug, Default)]
pub struct AuthorOutcome {
    pub ops: Vec<Op>,
    pub skipped: Vec<Skipped>,
}

impl AuthorOutcome {
    /// Number of skipped objects per last author ("2 objects modified by Anna, not undone").
    pub fn skipped_by(&self) -> Vec<(Option<Author>, usize)> {
        let mut out: Vec<(Option<Author>, usize)> = Vec::new();
        for s in &self.skipped {
            match out.iter_mut().find(|(b, _)| *b == s.by) {
                Some((_, n)) => *n += 1,
                None => out.push((s.by, 1)),
            }
        }
        out
    }
}

/// One undoable submission: its core operations (they carry the "before" state of every touched object) and an estimate of the
/// memory they hold.
#[derive(Clone, Debug)]
struct Step {
    ops: Vec<Op>,
    cost: usize,
}

impl Step {
    fn new(ops: Vec<Op>) -> Self {
        let cost = ops.iter().map(op_cost).sum();
        Self { ops, cost }
    }
}

fn json_len<T: serde::Serialize>(v: &T) -> usize {
    serde_json::to_vec(v).map_or(256, |b| b.len())
}

/// Estimated bytes held by an operation (the size of its JSON form plus a fixed overhead: deterministic and computed once).
fn op_cost(op: &Op) -> usize {
    64 + match op {
        Op::Insert { obj, .. } | Op::Remove { obj, .. } => json_len(obj),
        Op::Replace { before, after, .. } => json_len(before) + json_len(after),
        Op::Reorder { before, after, .. } => (before.len() + after.len()) * 40,
        Op::SetLayers { before, after, .. } => json_len(before) + json_len(after),
        Op::AddPage { page, .. } | Op::RemovePage { page, .. } => json_len(page),
        Op::RenamePage { before, after, .. } | Op::SetName { before, after } => before.len() + after.len(),
        Op::SetBackground { .. } => 16,
    }
}

#[derive(Clone, Debug, Default)]
struct AuthorStacks {
    /// One entry per submission.
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl AuthorStacks {
    fn bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(|s| s.cost).sum()
    }

    /// Drops the oldest step (undo side first). `false` when there is none.
    fn drop_oldest(&mut self) -> bool {
        if !self.undo.is_empty() {
            self.undo.remove(0);
        } else if !self.redo.is_empty() {
            self.redo.remove(0);
        } else {
            return false;
        }
        true
    }
}

/// Object touched by an operation (`None` for the operations that concern a page or the document).
fn op_object(op: &Op) -> Option<&Id> {
    match op {
        Op::Insert { obj, .. } | Op::Remove { obj, .. } => Some(&obj.id),
        Op::Replace { after, .. } => Some(&after.id),
        _ => None,
    }
}

/// Is the state produced by `op` still the current one (nobody changed that part of the document since)?
fn holds(doc: &Document, op: &Op) -> bool {
    match op {
        Op::Insert { page, obj, .. } => doc.page(page).and_then(|p| p.get(&obj.id)).is_some_and(|o| o == &**obj),
        Op::Remove { page, obj, .. } => doc.page(page).is_some_and(|p| p.get(&obj.id).is_none()),
        Op::Replace { page, after, .. } => doc.page(page).and_then(|p| p.get(&after.id)).is_some_and(|o| o == &**after),
        Op::Reorder { page, after, .. } => {
            doc.page(page).is_some_and(|p| p.objects.len() == after.len() && p.objects.iter().zip(after).all(|(o, id)| &o.id == id))
        }
        Op::SetLayers { page, after, .. } => doc.page(page).is_some_and(|p| &p.layers == after),
        Op::AddPage { page, .. } => doc.page(&page.id).is_some_and(|p| p.name == page.name && p.layers == page.layers && p.objects == page.objects),
        Op::RemovePage { page, .. } => doc.page(&page.id).is_none(),
        Op::RenamePage { page, after, .. } => doc.page(page).is_some_and(|p| &p.name == after),
        Op::SetBackground { after, .. } => doc.background == *after,
        Op::SetName { after, .. } => &doc.name == after,
    }
}

/// One undo/redo stack per author of a shared session, plus who modified each object last. Undoing restores what the author changed
/// **except what somebody else modified since**: those objects are skipped and reported. Pure: it computes the operations, the caller
/// applies them (see `Editor::apply_external`).
#[derive(Clone, Debug)]
pub struct AuthorHistories {
    stacks: HashMap<Author, AuthorStacks>,
    /// Who modified each object last, and when (a counter): only used to name who changed an object that an undo had to skip.
    last: HashMap<Id, (Author, u64)>,
    tick: u64,
    limit: usize,
    max_author_bytes: usize,
    max_total_bytes: usize,
    max_tracked: usize,
}

impl Default for AuthorHistories {
    fn default() -> Self {
        Self::new(200)
    }
}

impl AuthorHistories {
    /// Estimated bytes of undo steps one author may hold (the oldest steps go first).
    pub const DEFAULT_AUTHOR_BYTES: usize = 32 << 20;
    /// Same, all authors together (the author holding the most loses its oldest step first).
    pub const DEFAULT_TOTAL_BYTES: usize = 128 << 20;
    /// Objects whose last author is remembered (the least recently modified are forgotten: their author is then just "someone").
    pub const DEFAULT_TRACKED: usize = 100_000;

    /// `limit`: undo steps kept per author.
    pub fn new(limit: usize) -> Self {
        Self::with_budgets(limit, Self::DEFAULT_AUTHOR_BYTES, Self::DEFAULT_TOTAL_BYTES, Self::DEFAULT_TRACKED)
    }

    /// `limit` steps per author, at most `author_bytes` per author and `total_bytes` overall (estimates, see [`Self::bytes`]),
    /// and the last author of at most `tracked` objects.
    pub fn with_budgets(limit: usize, author_bytes: usize, total_bytes: usize, tracked: usize) -> Self {
        Self {
            stacks: HashMap::new(),
            last: HashMap::new(),
            tick: 0,
            limit,
            max_author_bytes: author_bytes,
            max_total_bytes: total_bytes,
            max_tracked: tracked,
        }
    }

    /// Estimated bytes held by all the undo and redo steps.
    pub fn bytes(&self) -> usize {
        self.stacks.values().map(AuthorStacks::bytes).sum()
    }

    /// Objects whose last author is remembered.
    pub fn tracked(&self) -> usize {
        self.last.len()
    }

    /// Who modified `id` last, if still remembered.
    pub fn last_author(&self, id: &Id) -> Option<Author> {
        self.last.get(id).map(|(a, _)| *a)
    }

    /// Notes that `author` modified these objects (also for modifications that are not undoable per author, e.g. the host's own).
    pub fn note<'a>(&mut self, author: Author, ids: impl IntoIterator<Item = &'a Id>) {
        for id in ids {
            self.tick += 1;
            self.last.insert(id.clone(), (author, self.tick));
        }
        if self.last.len() > self.max_tracked {
            // eviction: the older half of the notes goes (by time of the last modification), so the work is amortized
            let keep = self.max_tracked / 2;
            let mut ticks: Vec<u64> = self.last.values().map(|(_, t)| *t).collect();
            ticks.sort_unstable();
            let threshold = ticks.get(ticks.len().saturating_sub(keep)).copied().unwrap_or(u64::MAX);
            self.last.retain(|_, (_, t)| *t >= threshold);
        }
    }

    /// Applies the byte budgets after `author`'s stacks grew: the oldest steps are dropped (they can no longer be undone).
    fn enforce_budgets(&mut self, author: Author) {
        if let Some(s) = self.stacks.get_mut(&author) {
            while s.bytes() > self.max_author_bytes && s.drop_oldest() {}
        }
        while self.bytes() > self.max_total_bytes {
            let Some(biggest) = self.stacks.iter().max_by_key(|(a, s)| (s.bytes(), **a)).map(|(a, _)| *a) else { break };
            if !self.stacks.get_mut(&biggest).is_some_and(AuthorStacks::drop_oldest) {
                break;
            }
        }
    }

    /// Records one submission of `author` (the core operations it performed). A new submission clears the author's redo stack.
    pub fn record(&mut self, author: Author, ops: Vec<Op>) {
        if ops.is_empty() {
            return;
        }
        self.note(author, ops.iter().filter_map(op_object));
        let s = self.stacks.entry(author).or_default();
        s.redo.clear();
        s.undo.push(Step::new(ops));
        if s.undo.len() > self.limit {
            s.undo.remove(0);
        }
        self.enforce_budgets(author);
    }

    pub fn can_undo(&self, author: Author) -> bool {
        self.stacks.get(&author).is_some_and(|s| !s.undo.is_empty())
    }
    pub fn can_redo(&self, author: Author) -> bool {
        self.stacks.get(&author).is_some_and(|s| !s.redo.is_empty())
    }

    /// Forgets an author's stacks (it left for good).
    pub fn forget(&mut self, author: Author) {
        self.stacks.remove(&author);
    }

    fn skip(&self, op: &Op) -> Skipped {
        let object = op_object(op).cloned();
        let by = object.as_ref().and_then(|id| self.last_author(id));
        Skipped { object, by }
    }

    /// Undoes the author's last submission against `doc` (the current document). `None` when there is nothing to undo.
    pub fn undo(&mut self, author: Author, doc: &Document) -> Option<AuthorOutcome> {
        let step = self.stacks.get_mut(&author)?.undo.pop()?.ops;
        let mut scratch = doc.clone();
        let (mut out, mut done) = (AuthorOutcome::default(), Vec::new());
        for op in step.iter().rev() {
            if holds(&scratch, op) {
                let inv = op.inverse();
                scratch.apply(&inv);
                out.ops.push(inv);
                done.push(op.clone());
            } else {
                out.skipped.push(self.skip(op));
            }
        }
        self.note(author, out.ops.iter().filter_map(op_object));
        if !done.is_empty() {
            done.reverse();
            self.stacks.entry(author).or_default().redo.push(Step::new(done));
            self.enforce_budgets(author);
        }
        Some(out)
    }

    /// Redoes the author's last undone submission (same rule: what somebody else changed since is skipped).
    pub fn redo(&mut self, author: Author, doc: &Document) -> Option<AuthorOutcome> {
        let step = self.stacks.get_mut(&author)?.redo.pop()?.ops;
        let mut scratch = doc.clone();
        let (mut out, mut done) = (AuthorOutcome::default(), Vec::new());
        for op in &step {
            if holds(&scratch, &op.inverse()) {
                scratch.apply(op);
                out.ops.push(op.clone());
                done.push(op.clone());
            } else {
                out.skipped.push(self.skip(op));
            }
        }
        self.note(author, out.ops.iter().filter_map(op_object));
        if !done.is_empty() {
            let s = self.stacks.entry(author).or_default();
            s.undo.push(Step::new(done));
            if s.undo.len() > self.limit {
                s.undo.remove(0);
            }
            self.enforce_budgets(author);
        }
        Some(out)
    }
}

#[cfg(test)]
mod author_tests {
    use super::*;
    use crate::geom::Frame;
    use crate::model::{BoxGeom, Label};
    use crate::style::Style;
    use proptest::prelude::*;

    fn shape(id: &str, x: f64) -> Object {
        let mut o = Object::new_box(BoxGeom::Rect, Frame::new(x, 0.0, 10.0, 10.0), Style::default(), Label::default());
        o.id = id.into();
        o
    }

    fn doc_with(n: usize) -> Document {
        let mut d = Document::default();
        d.pages[0].objects = (0..n).map(|i| shape(&format!("o{i}"), i as f64)).collect();
        d
    }

    /// Core operation that moves object `i` to `x` (the object must exist), applied to `doc`.
    fn mv(doc: &mut Document, i: usize, x: f64) -> Vec<Op> {
        let page = doc.pages[0].id.clone();
        let id: Id = format!("o{i}").as_str().into();
        let before = doc.pages[0].get(&id).unwrap().clone();
        let after = shape(&format!("o{i}"), x);
        let op = Op::Replace { page, before: Box::new(before), after: Box::new(after) };
        doc.apply(&op);
        vec![op]
    }

    fn x_of(doc: &Document, i: usize) -> f64 {
        doc.pages[0].get(&format!("o{i}").as_str().into()).unwrap().frame().unwrap().x
    }

    fn apply_all(doc: &mut Document, ops: &[Op]) {
        ops.iter().for_each(|o| doc.apply(o));
    }

    #[test]
    fn undo_restores_the_authors_change_and_redo_brings_it_back() {
        let mut doc = doc_with(2);
        let mut h = AuthorHistories::default();
        let ops = mv(&mut doc, 0, 50.0);
        h.record(7, ops);
        assert!(h.can_undo(7) && !h.can_undo(8));
        let out = h.undo(7, &doc).unwrap();
        apply_all(&mut doc, &out.ops);
        assert!(out.skipped.is_empty());
        assert_eq!(x_of(&doc, 0), 0.0);
        assert!(h.can_redo(7));
        let out = h.redo(7, &doc).unwrap();
        apply_all(&mut doc, &out.ops);
        assert_eq!(x_of(&doc, 0), 50.0);
        assert!(h.undo(8, &doc).is_none(), "another author has nothing to undo");
    }

    #[test]
    fn an_object_modified_since_by_somebody_else_is_skipped_and_attributed() {
        let mut doc = doc_with(3);
        let mut h = AuthorHistories::default();
        let mut ops = mv(&mut doc, 0, 50.0);
        ops.extend(mv(&mut doc, 1, 60.0));
        h.record(1, ops);
        let theirs = mv(&mut doc, 1, 99.0);
        h.record(2, theirs);
        let out = h.undo(1, &doc).unwrap();
        apply_all(&mut doc, &out.ops);
        assert_eq!(x_of(&doc, 0), 0.0, "untouched by the other author: restored");
        assert_eq!(x_of(&doc, 1), 99.0, "the other author's change is kept");
        assert_eq!(out.skipped, vec![Skipped { object: Some("o1".into()), by: Some(2) }]);
        assert_eq!(out.skipped_by(), vec![(Some(2), 1)]);
    }

    #[test]
    fn an_insertion_and_a_deletion_are_undone_and_redone() {
        let mut doc = doc_with(1);
        let page = doc.pages[0].id.clone();
        let mut h = AuthorHistories::default();
        let ins = Op::Insert { page: page.clone(), index: 1, obj: Box::new(shape("new", 5.0)) };
        let del = Op::Remove { page, index: 0, obj: Box::new(doc.pages[0].objects[0].clone()) };
        for op in [&ins, &del] {
            doc.apply(op);
        }
        let start = doc_with(1);
        h.record(3, vec![ins, del]);
        assert_eq!(doc.pages[0].objects.len(), 1);
        let out = h.undo(3, &doc).unwrap();
        apply_all(&mut doc, &out.ops);
        assert_eq!(doc.pages[0].objects, start.pages[0].objects, "back to the initial objects");
        let out = h.redo(3, &doc).unwrap();
        apply_all(&mut doc, &out.ops);
        assert!(doc.pages[0].get(&"new".into()).is_some() && doc.pages[0].get(&"o0".into()).is_none());
    }

    #[test]
    fn a_new_submission_clears_the_redo_stack_and_the_stack_is_bounded() {
        let mut doc = doc_with(1);
        let mut h = AuthorHistories::new(2);
        for x in [10.0, 20.0, 30.0] {
            let ops = mv(&mut doc, 0, x);
            h.record(1, ops);
        }
        let out = h.undo(1, &doc).unwrap();
        apply_all(&mut doc, &out.ops);
        assert!(h.can_redo(1));
        let ops = mv(&mut doc, 0, 40.0);
        h.record(1, ops);
        assert!(!h.can_redo(1));
        let mut n = 0;
        while let Some(out) = h.undo(1, &doc) {
            apply_all(&mut doc, &out.ops);
            n += 1;
        }
        assert_eq!(n, 2, "only the last two steps are kept");
    }

    /// An operation that weighs about `n * 80` estimated bytes (a reordering of `n` objects), for the memory-bound tests.
    fn heavy(doc: &Document, n: usize) -> Vec<Op> {
        let ids: Vec<Id> = (0..n).map(|i| Id(format!("h{i}"))).collect();
        vec![Op::Reorder { page: doc.pages[0].id.clone(), before: ids.clone(), after: ids }]
    }

    fn steps_left(h: &mut AuthorHistories, author: Author, doc: &Document) -> usize {
        std::iter::from_fn(|| h.undo(author, doc)).count()
    }

    #[test]
    fn undo_memory_is_bounded_per_author_and_overall_and_the_oldest_steps_go_first() {
        let doc = doc_with(1);
        // one step weighs about 80 kB: 2 fit in 200 kB, and 5 authors together keep at most 500 kB
        let mut h = AuthorHistories::with_budgets(200, 200_000, 500_000, 1_000);
        for _ in 0..50 {
            h.record(1, heavy(&doc, 1_000));
            assert!(h.bytes() <= 500_000, "{}", h.bytes());
        }
        assert_eq!(steps_left(&mut h, 1, &doc), 2, "only the newest steps of one author are kept");
        let mut h = AuthorHistories::with_budgets(200, 200_000, 500_000, 1_000);
        for round in 0..20 {
            for author in 1..=5 {
                h.record(author, heavy(&doc, 1_000 + round));
                assert!(h.bytes() <= 500_000, "{}", h.bytes());
            }
        }
        assert!((1..=5).all(|a| h.can_undo(a)), "every author keeps its latest step: the biggest holder pays first");
        let kept: usize = (1..=5).map(|a| steps_left(&mut h, a, &doc)).sum();
        assert!((5..=7).contains(&kept), "about 500 kB of 80 kB steps, not 100 of them: {kept}");
    }

    #[test]
    fn the_memory_of_who_modified_what_is_bounded_and_forgets_the_oldest_first() {
        let mut h = AuthorHistories::with_budgets(200, usize::MAX, usize::MAX, 100);
        let ids: Vec<Id> = (0..10_000).map(|i| Id(format!("o{i}"))).collect();
        for (i, id) in ids.iter().enumerate() {
            h.note(1 + (i % 3) as u32, std::iter::once(id));
            assert!(h.tracked() <= 100);
        }
        assert_eq!(h.last_author(&ids[9_999]), Some(1 + (9_999 % 3) as u32), "the latest is kept");
        assert_eq!(h.last_author(&ids[0]), None, "the oldest was forgotten");
        // an object modified again is a recent one again
        h.note(9, std::iter::once(&ids[9_999]));
        assert_eq!(h.last_author(&ids[9_999]), Some(9));
    }

    proptest! {
        /// Whatever the two authors touch, undoing the first one's step changes only the objects that nobody else touched since,
        /// and leaves every other object exactly as it is; the redo then restores exactly what the undo changed.
        #[test]
        fn undo_never_touches_what_another_author_changed(
            a_set in proptest::collection::btree_set(0usize..8, 1..5),
            b_set in proptest::collection::btree_set(0usize..8, 0..5),
        ) {
            let mut doc = doc_with(8);
            let original = doc.clone();
            let mut h = AuthorHistories::default();
            let mut ops = Vec::new();
            for &i in &a_set { ops.extend(mv(&mut doc, i, 100.0 + i as f64)); }
            h.record(1, ops);
            let mut ops = Vec::new();
            for &i in &b_set { ops.extend(mv(&mut doc, i, 200.0 + i as f64)); }
            h.record(2, ops);
            let before_undo = doc.clone();
            let out = h.undo(1, &doc).unwrap();
            apply_all(&mut doc, &out.ops);
            for i in 0..8 {
                let want = if a_set.contains(&i) && !b_set.contains(&i) { x_of(&original, i) } else { x_of(&before_undo, i) };
                prop_assert_eq!(x_of(&doc, i), want, "object {}", i);
            }
            prop_assert_eq!(out.skipped.len(), a_set.intersection(&b_set).count());
            if let Some(r) = h.redo(1, &doc) {
                apply_all(&mut doc, &r.ops);
                for i in 0..8 {
                    prop_assert_eq!(x_of(&doc, i), x_of(&before_undo, i), "object {} after redo", i);
                }
            }
        }
    }
}
