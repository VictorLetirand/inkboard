//! Index-free operations (`WireOp`) and their conversion from/to the core's index-based `Op`.
//!
//! Stacking order is expressed by lists of ids instead of positions, so that an operation does not depend on a fragile index.
//! Applying a `WireOp` goes through [`apply_wire`], which returns the core `Op`s it produced: they carry the "before" state, so
//! `Op::inverse` gives an exact rollback (used by the optimistic client and by an author-based undo on the host).

use inkboard_core::history::Op;
use inkboard_core::model::{Background, Document, Id, Layer, Object, Page};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Modification of a document that does not depend on any index.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WireOp {
    /// Inserts the whole object (on top of the stack) or replaces the existing object with the same id.
    Put {
        page: Id,
        object: Box<Object>,
    },
    Delete {
        page: Id,
        ids: Vec<Id>,
    },
    /// Complete list of ids in stacking order (the last one is on top).
    Order {
        page: Id,
        ids: Vec<Id>,
    },
    SetLayers {
        page: Id,
        layers: Vec<Layer>,
    },
    /// Pages are few and ordered by the host, so the position is kept (clamped on application).
    AddPage {
        index: usize,
        page: Box<Page>,
    },
    RenamePage {
        page: Id,
        name: String,
    },
    RemovePage {
        page: Id,
    },
    SetBackground {
        background: Background,
    },
    SetName {
        name: String,
    },
}

impl WireOp {
    /// Ids of the objects this operation replaces or deletes (used to detect concurrent changes).
    #[must_use]
    pub fn object_ids(&self) -> Vec<&Id> {
        match self {
            WireOp::Put { object, .. } => vec![&object.id],
            WireOp::Delete { ids, .. } => ids.iter().collect(),
            _ => Vec::new(),
        }
    }
}

/// Order obtained by `Op::Reorder`: the known ids of `wanted` first (first occurrence), then the remaining objects in their order.
#[must_use]
pub fn normalize_order(before: &[Id], wanted: &[Id]) -> Vec<Id> {
    let exists: HashSet<&Id> = before.iter().collect();
    let mut seen: HashSet<&Id> = HashSet::new();
    let mut out: Vec<Id> = Vec::with_capacity(before.len());
    for id in wanted {
        if exists.contains(id) && seen.insert(id) {
            out.push(id.clone());
        }
    }
    out.extend(before.iter().filter(|id| !seen.contains(id)).cloned());
    out
}

/// Applies `op` to `doc` and returns the core operations actually performed (empty when nothing changed: unknown page, same value...).
/// Applying the inverses of the result in reverse order restores `doc` exactly.
pub fn apply_wire(doc: &mut Document, op: &WireOp) -> Vec<Op> {
    let mut out: Vec<Op> = Vec::new();
    match op {
        WireOp::Put { page, object } => {
            let Some(p) = doc.page(page) else { return out };
            match p.index_of(&object.id) {
                Some(i) if p.objects[i] == **object => {}
                Some(i) => out.push(Op::Replace { page: page.clone(), before: Box::new(p.objects[i].clone()), after: object.clone() }),
                None => out.push(Op::Insert { page: page.clone(), index: p.objects.len(), obj: object.clone() }),
            }
        }
        WireOp::Delete { page, ids } => {
            // sequential: the index recorded by each removal is the one at the time of that removal
            let Some(p) = doc.page(page) else { return out };
            let mut objs: Vec<&Object> = p.objects.iter().collect();
            for id in ids {
                if let Some(i) = objs.iter().position(|o| &o.id == id) {
                    out.push(Op::Remove { page: page.clone(), index: i, obj: Box::new(objs.remove(i).clone()) });
                }
            }
        }
        WireOp::Order { page, ids } => {
            let Some(p) = doc.page(page) else { return out };
            let before: Vec<Id> = p.objects.iter().map(|o| o.id.clone()).collect();
            let after = normalize_order(&before, ids);
            if after != before {
                out.push(Op::Reorder { page: page.clone(), before, after });
            }
        }
        WireOp::SetLayers { page, layers } => {
            if let Some(p) = doc.page(page)
                && &p.layers != layers
            {
                out.push(Op::SetLayers { page: page.clone(), before: p.layers.clone(), after: layers.clone() });
            }
        }
        WireOp::AddPage { index, page } => {
            if doc.page(&page.id).is_none() {
                out.push(Op::AddPage { index: (*index).min(doc.pages.len()), page: page.clone() });
            }
        }
        WireOp::RenamePage { page, name } => {
            if let Some(p) = doc.page(page)
                && &p.name != name
            {
                out.push(Op::RenamePage { page: page.clone(), before: p.name.clone(), after: name.clone() });
            }
        }
        WireOp::RemovePage { page } => {
            if doc.pages.len() > 1
                && let Some(index) = doc.pages.iter().position(|p| &p.id == page)
            {
                out.push(Op::RemovePage { index, page: Box::new(doc.pages[index].clone()) });
            }
        }
        WireOp::SetBackground { background } => {
            if doc.background != *background {
                out.push(Op::SetBackground { before: doc.background, after: *background });
            }
        }
        WireOp::SetName { name } => {
            if &doc.name != name {
                out.push(Op::SetName { before: doc.name.clone(), after: name.clone() });
            }
        }
    }
    for o in &out {
        doc.apply(o);
    }
    out
}

/// Applies several operations in order and concatenates the performed core operations.
pub fn apply_wire_all(doc: &mut Document, ops: &[WireOp]) -> Vec<Op> {
    ops.iter().flat_map(|o| apply_wire(doc, o)).collect()
}

/// Undoes core operations (in reverse order).
pub fn rollback(doc: &mut Document, ops: &[Op]) {
    for op in ops.iter().rev() {
        doc.apply(&op.inverse());
    }
}

/// Converts core operations into `WireOp`s. It follows the stacking order of every touched page, starting from the document as it was
/// before the operations, to express an insertion in the middle of the stack as `Put` + `Order`.
#[derive(Debug)]
pub struct OpEncoder<'a> {
    doc: &'a Document,
    orders: HashMap<Id, Vec<Id>>,
}

impl<'a> OpEncoder<'a> {
    /// `doc_before`: the document before the first operation that will be encoded.
    #[must_use]
    pub fn new(doc_before: &'a Document) -> Self {
        Self { doc: doc_before, orders: HashMap::new() }
    }

    fn order(&mut self, page: &Id) -> &mut Vec<Id> {
        let doc = self.doc;
        self.orders.entry(page.clone()).or_insert_with(|| doc.page(page).map(|p| p.objects.iter().map(|o| o.id.clone()).collect()).unwrap_or_default())
    }

    pub fn encode(&mut self, op: &Op) -> Vec<WireOp> {
        match op {
            Op::Insert { page, index, obj } => {
                let o = self.order(page);
                let put = WireOp::Put { page: page.clone(), object: obj.clone() };
                if *index >= o.len() {
                    o.push(obj.id.clone());
                    vec![put]
                } else {
                    o.insert(*index, obj.id.clone());
                    vec![put, WireOp::Order { page: page.clone(), ids: o.clone() }]
                }
            }
            Op::Remove { page, obj, .. } => {
                self.order(page).retain(|id| id != &obj.id);
                vec![WireOp::Delete { page: page.clone(), ids: vec![obj.id.clone()] }]
            }
            Op::Replace { page, after, .. } => vec![WireOp::Put { page: page.clone(), object: after.clone() }],
            Op::Reorder { page, after, .. } => {
                let o = self.order(page);
                *o = normalize_order(o, after);
                vec![WireOp::Order { page: page.clone(), ids: after.clone() }]
            }
            Op::SetLayers { page, after, .. } => vec![WireOp::SetLayers { page: page.clone(), layers: after.clone() }],
            Op::AddPage { index, page } => {
                self.orders.insert(page.id.clone(), page.objects.iter().map(|o| o.id.clone()).collect());
                vec![WireOp::AddPage { index: *index, page: page.clone() }]
            }
            Op::RemovePage { page, .. } => {
                self.orders.remove(&page.id);
                vec![WireOp::RemovePage { page: page.id.clone() }]
            }
            Op::RenamePage { page, after, .. } => vec![WireOp::RenamePage { page: page.clone(), name: after.clone() }],
            Op::SetBackground { after, .. } => vec![WireOp::SetBackground { background: *after }],
            Op::SetName { after, .. } => vec![WireOp::SetName { name: after.clone() }],
        }
    }
}

/// Converts a list of core operations (applied in order starting from `doc_before`) into `WireOp`s.
#[must_use]
pub fn ops_to_wire(ops: &[Op], doc_before: &Document) -> Vec<WireOp> {
    let mut enc = OpEncoder::new(doc_before);
    ops.iter().flat_map(|o| enc.encode(o)).collect()
}
