//! Elementary reversible operations and the undo stack.

use crate::model::{Background, Document, Id, Layer, Object, Page};

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
