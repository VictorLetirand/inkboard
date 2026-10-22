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

#[derive(Clone, Debug, Default)]
struct AuthorStacks {
    /// One entry per submission: the core operations it performed (they carry the "before" state of every touched object).
    undo: Vec<Vec<Op>>,
    redo: Vec<Vec<Op>>,
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
    last: HashMap<Id, Author>,
    limit: usize,
}

impl Default for AuthorHistories {
    fn default() -> Self {
        Self::new(200)
    }
}

impl AuthorHistories {
    /// `limit`: undo steps kept per author.
    pub fn new(limit: usize) -> Self {
        Self { stacks: HashMap::new(), last: HashMap::new(), limit }
    }

    /// Notes that `author` modified these objects (also for modifications that are not undoable per author, e.g. the host's own).
    pub fn note<'a>(&mut self, author: Author, ids: impl IntoIterator<Item = &'a Id>) {
        for id in ids {
            self.last.insert(id.clone(), author);
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
        s.undo.push(ops);
        if s.undo.len() > self.limit {
            s.undo.remove(0);
        }
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
        let by = object.as_ref().and_then(|id| self.last.get(id)).copied();
        Skipped { object, by }
    }

    /// Undoes the author's last submission against `doc` (the current document). `None` when there is nothing to undo.
    pub fn undo(&mut self, author: Author, doc: &Document) -> Option<AuthorOutcome> {
        let step = self.stacks.get_mut(&author)?.undo.pop()?;
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
            self.stacks.entry(author).or_default().redo.push(done);
        }
        Some(out)
    }

    /// Redoes the author's last undone submission (same rule: what somebody else changed since is skipped).
    pub fn redo(&mut self, author: Author, doc: &Document) -> Option<AuthorOutcome> {
        let step = self.stacks.get_mut(&author)?.redo.pop()?;
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
            s.undo.push(done);
            if s.undo.len() > self.limit {
                s.undo.remove(0);
            }
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
