//! Editing a document with history: every modification goes through here.

use crate::connector::{Connector, End};
use crate::geom::{Pt, Rect};
use crate::history::{History, Op};
use crate::model::{Background, Document, Id, Kind, Layer, Object, Page};
use crate::num;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Front,
    Forward,
    Backward,
    Back,
}

#[derive(Debug)]
pub struct Editor {
    pub doc: Document,
    pub current: Id,
    history: History,
    open: Option<Vec<Op>>,
    depth: usize,
    rev: u64,
    /// Shared-session guest hook (see the end of this impl): operations applied since the last `take_recorded_ops`.
    op_log: Option<Vec<Op>>,
    /// Shared-session guest hook: refuses every modification (read-only participant).
    read_only: bool,
}

impl Editor {
    #[must_use]
    pub fn new(doc: Document) -> Self {
        let current = doc.pages[0].id.clone();
        Self { doc, current, history: History::default(), open: None, depth: 0, rev: 0, op_log: None, read_only: false }
    }

    /// Counter incremented on every modification (to detect "needs saving").
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.rev
    }
    #[must_use]
    pub fn page(&self) -> &Page {
        self.doc.page(&self.current).unwrap_or(&self.doc.pages[0])
    }
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Groups several modifications into a single undo step.
    pub fn transact<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.depth += 1;
        if self.depth == 1 {
            self.open = Some(Vec::new());
        }
        let r = f(self);
        self.depth -= 1;
        if self.depth == 0 {
            let ops = self.open.take().unwrap_or_default();
            self.history.push(ops);
        }
        r
    }

    /// Like `transact`, but if `f` returns `Err`, the operations made by `f` are undone: the document, the current page and the
    /// revision return to their state at entry, and nothing is pushed to the undo or redo stacks. Inside an already open
    /// transaction, only the operations of this call are undone.
    ///
    /// # Errors
    ///
    /// Returns the error of `f`, after undoing its operations.
    pub fn try_transact<R, E>(&mut self, f: impl FnOnce(&mut Self) -> Result<R, E>) -> Result<R, E> {
        let (start, rev, current) = (self.open.as_ref().map_or(0, Vec::len), self.rev, self.current.clone());
        let logged = self.op_log.as_ref().map_or(0, Vec::len);
        self.transact(|e| {
            let r = f(e);
            if r.is_err() {
                if let Some(log) = e.op_log.as_mut() {
                    log.truncate(logged);
                }
                for op in e.open.as_mut().map(|v| v.split_off(start)).unwrap_or_default().iter().rev() {
                    e.doc.apply(&op.inverse());
                }
                e.rev = rev;
                e.current = current;
                e.fix_current();
            }
            r
        })
    }

    fn apply(&mut self, op: Op) {
        if self.read_only {
            return;
        }
        self.doc.apply(&op);
        self.rev += 1;
        if let Some(log) = self.op_log.as_mut() {
            log.push(op.clone());
        }
        match self.open.as_mut() {
            Some(v) => v.push(op),
            None => self.history.push(vec![op]),
        }
    }

    pub fn undo(&mut self) -> bool {
        if self.read_only {
            return false;
        }
        let ok = self.history.undo(&mut self.doc);
        if ok {
            self.rev += 1;
            self.fix_current();
        }
        ok
    }
    pub fn redo(&mut self) -> bool {
        if self.read_only {
            return false;
        }
        let ok = self.history.redo(&mut self.doc);
        if ok {
            self.rev += 1;
            self.fix_current();
        }
        ok
    }
    fn fix_current(&mut self) {
        if self.doc.page(&self.current).is_none() {
            self.current = self.doc.pages[0].id.clone();
        }
    }
    /// Starts again from an empty history (after opening a file).
    pub fn reset(&mut self, doc: Document) {
        self.current = doc.pages[0].id.clone();
        self.doc = doc;
        self.history.clear();
        if let Some(log) = self.op_log.as_mut() {
            log.clear();
        }
        self.rev += 1;
    }

    /* ------------------------------------------------------- Objets */

    pub fn insert(&mut self, obj: Object) -> Id {
        let id = obj.id.clone();
        let (page, index) = (self.current.clone(), self.page().objects.len());
        self.apply(Op::Insert { page, index, obj: Box::new(obj) });
        id
    }

    /// Replaces the object with the same id.
    pub fn replace(&mut self, new: Object) {
        let Some(before) = self.page().get(&new.id).cloned() else { return };
        if before == new {
            return;
        }
        let page = self.current.clone();
        self.apply(Op::Replace { page, before: Box::new(before), after: Box::new(new) });
    }

    pub fn update(&mut self, id: &Id, f: impl FnOnce(&mut Object)) {
        if let Some(mut o) = self.page().get(id).cloned() {
            f(&mut o);
            self.replace(o);
        }
    }

    pub fn update_many(&mut self, ids: &[Id], f: impl Fn(&mut Object)) {
        self.transact(|e| {
            for id in ids {
                e.update(id, &f);
            }
        });
    }

    /// Removes objects; bound connectors are detached (they keep their position).
    pub fn remove(&mut self, ids: &[Id]) {
        let ids: Vec<Id> = ids.to_vec();
        self.transact(|e| {
            let page = e.page().clone();
            for o in &page.objects {
                if ids.contains(&o.id) {
                    continue;
                }
                if let Kind::Connector(c) = &o.kind
                    && let Some(nc) = page.detach_ends_to(c, &ids)
                {
                    let mut n = o.clone();
                    n.kind = Kind::Connector(nc);
                    e.replace(n);
                }
            }
            let pid = e.current.clone();
            for (i, o) in page.objects.iter().enumerate().rev() {
                if ids.contains(&o.id) {
                    e.apply(Op::Remove { page: pid.clone(), index: i, obj: Box::new(o.clone()) });
                }
            }
        });
    }

    /// Copies of objects with new ids, offset by `d`. Links to objects that
    /// are not copied are broken (ends become free at their current position).
    #[must_use]
    pub fn clone_objects(page: &Page, ids: &[Id], d: Pt) -> Vec<Object> {
        let set: Vec<&Object> = page.objects.iter().filter(|o| ids.contains(&o.id)).collect();
        let idmap: HashMap<Id, Id> = set.iter().map(|o| (o.id.clone(), Id::new())).collect();
        let mut gmap: HashMap<Id, Id> = HashMap::new();
        let outside: Vec<Id> = page.objects.iter().filter(|o| !ids.contains(&o.id)).map(|o| o.id.clone()).collect();
        set.into_iter()
            .map(|o| {
                let mut c = o.clone();
                c.id = idmap[&o.id].clone();
                if let Some(g) = &o.group {
                    c.group = Some(gmap.entry(g.clone()).or_default().clone());
                }
                if let Kind::Connector(cn) = &o.kind {
                    let mut cn = page.detach_ends_to(cn, &outside).unwrap_or_else(|| cn.clone());
                    for e in [&mut cn.start, &mut cn.end] {
                        if let End::Bound { target, .. } = e
                            && let Some(n) = idmap.get(target)
                        {
                            *target = n.clone();
                        }
                    }
                    c.kind = Kind::Connector(cn);
                }
                c.translate(d);
                c
            })
            .collect()
    }

    /// Pastes (already cloned) objects on top.
    pub fn insert_all(&mut self, objs: Vec<Object>) -> Vec<Id> {
        self.transact(|e| objs.into_iter().map(|o| e.insert(o)).collect())
    }

    pub fn duplicate(&mut self, ids: &[Id], d: Pt) -> Vec<Id> {
        let copies = Self::clone_objects(self.page(), ids, d);
        self.insert_all(copies)
    }

    /// Links two objects with a new connector.
    pub fn connect(&mut self, from: &Id, to: &Id) -> Id {
        let o = Object::new(
            Kind::Connector(Connector::new(End::bound(from.clone()), End::bound(to.clone()))),
            crate::style::Style { width: 2.5, ..Default::default() },
        );
        self.insert(o)
    }

    /* ------------------------------------------------ Ordre, groupes */

    pub fn reorder(&mut self, ids: &[Id], mode: Order) {
        let before: Vec<Id> = self.page().objects.iter().map(|o| o.id.clone()).collect();
        let mut v = before.clone();
        let sel = |id: &Id| ids.contains(id);
        match mode {
            Order::Front => {
                let (s, r): (Vec<_>, Vec<_>) = v.iter().cloned().partition(&sel);
                v = r.into_iter().chain(s).collect();
            }
            Order::Back => {
                let (s, r): (Vec<_>, Vec<_>) = v.iter().cloned().partition(&sel);
                v = s.into_iter().chain(r).collect();
            }
            Order::Forward => {
                for i in (0..v.len().saturating_sub(1)).rev() {
                    if sel(&v[i]) && !sel(&v[i + 1]) {
                        v.swap(i, i + 1);
                    }
                }
            }
            Order::Backward => {
                for i in 1..v.len() {
                    if sel(&v[i]) && !sel(&v[i - 1]) {
                        v.swap(i, i - 1);
                    }
                }
            }
        }
        if v != before {
            let page = self.current.clone();
            self.apply(Op::Reorder { page, before, after: v });
        }
    }

    pub fn group(&mut self, ids: &[Id]) {
        if ids.len() < 2 {
            return;
        }
        let g = Id::new();
        self.update_many(ids, |o| o.group = Some(g.clone()));
    }
    pub fn ungroup(&mut self, ids: &[Id]) {
        self.update_many(ids, |o| o.group = None);
    }
    pub fn set_locked(&mut self, ids: &[Id], locked: bool) {
        self.update_many(ids, |o| o.locked = locked);
    }
    pub fn translate(&mut self, ids: &[Id], d: Pt) {
        self.update_many(ids, |o| o.translate(d));
    }

    /* ------------------------------------------- Alignement, calques */

    pub fn align(&mut self, ids: &[Id], mode: AlignMode) {
        let page = self.page().clone();
        let items: Vec<(Id, Rect)> =
            page.objects.iter().filter(|o| ids.contains(&o.id) && !page.is_locked(o)).map(|o| (o.id.clone(), page.bounds(o))).collect();
        let Some(u) = items.iter().map(|i| i.1).reduce(|a, b| a.union(&b)) else { return };
        if items.len() < 2 {
            return;
        }
        self.transact(|e| {
            for (id, b) in items {
                let d = match mode {
                    AlignMode::Left => Pt::new(u.x - b.x, 0.0),
                    AlignMode::CenterX => Pt::new(u.center().x - b.center().x, 0.0),
                    AlignMode::Right => Pt::new(u.right() - b.right(), 0.0),
                    AlignMode::Top => Pt::new(0.0, u.y - b.y),
                    AlignMode::CenterY => Pt::new(0.0, u.center().y - b.center().y),
                    AlignMode::Bottom => Pt::new(0.0, u.bottom() - b.bottom()),
                };
                e.update(&id, |o| o.translate(d));
            }
        });
    }

    /// Distributes objects at regular intervals (at least 3 objects).
    pub fn distribute(&mut self, ids: &[Id], axis: Axis) {
        let page = self.page().clone();
        let mut items: Vec<(Id, Rect)> =
            page.objects.iter().filter(|o| ids.contains(&o.id) && !page.is_locked(o)).map(|o| (o.id.clone(), page.bounds(o))).collect();
        if items.len() < 3 {
            return;
        }
        let hz = axis == Axis::Horizontal;
        items.sort_by(|a, b| if hz { a.1.x.total_cmp(&b.1.x) } else { a.1.y.total_cmp(&b.1.y) });
        let (first, last) = (items[0].1, items[items.len() - 1].1);
        let span = if hz { last.right() - first.x } else { last.bottom() - first.y };
        let sizes: f64 = items.iter().map(|i| if hz { i.1.w } else { i.1.h }).sum();
        let gap = (span - sizes) / num::f64_of(items.len() - 1);
        let mut pos = if hz { first.x } else { first.y };
        self.transact(|e| {
            for (id, b) in items {
                let d = if hz { Pt::new(pos - b.x, 0.0) } else { Pt::new(0.0, pos - b.y) };
                pos += (if hz { b.w } else { b.h }) + gap;
                e.update(&id, |o| o.translate(d));
            }
        });
    }

    pub fn set_layers(&mut self, layers: Vec<Layer>) {
        let before = self.page().layers.clone();
        if before != layers {
            let page = self.current.clone();
            self.apply(Op::SetLayers { page, before, after: layers });
        }
    }

    /* ------------------------------------------------- Pages, fond */

    pub fn add_page(&mut self, name: &str) -> Id {
        let p = Page::new(name);
        let id = p.id.clone();
        let index = self.doc.pages.len();
        self.apply(Op::AddPage { index, page: Box::new(p) });
        self.current = id.clone();
        id
    }
    pub fn remove_page(&mut self, id: &Id) {
        if self.doc.pages.len() <= 1 {
            return;
        }
        if let Some(index) = self.doc.pages.iter().position(|p| &p.id == id) {
            let page = Box::new(self.doc.pages[index].clone());
            self.apply(Op::RemovePage { index, page });
            self.fix_current();
        }
    }
    pub fn rename_page(&mut self, id: &Id, name: &str) {
        if let Some(p) = self.doc.page(id)
            && p.name != name
        {
            let before = p.name.clone();
            self.apply(Op::RenamePage { page: id.clone(), before, after: name.to_owned() });
        }
    }
    pub fn set_background(&mut self, bg: Background) {
        if self.doc.background != bg {
            let before = self.doc.background;
            self.apply(Op::SetBackground { before, after: bg });
        }
    }
    pub fn set_name(&mut self, name: &str) {
        if self.doc.name != name {
            let before = self.doc.name.clone();
            self.apply(Op::SetName { before, after: name.to_owned() });
        }
    }

    /* ------------------------------------- Hook for a shared-session guest */
    // Separate from the editing methods above on purpose. A guest edits a local mirror of the host's board: its modifications are
    // recorded here to be sent to the host, the mirror is refreshed from the host's state, and a read-only guest cannot modify it.

    /// Starts (`true`) or stops (`false`) recording the operations that modify the document. Stopping drops what was recorded.
    pub fn record_ops(&mut self, on: bool) {
        self.op_log = on.then(Vec::new);
    }

    /// Operations applied since the last call, in order (empty when not recording). Undo and redo are not recorded: a guest never
    /// undoes locally.
    pub fn take_recorded_ops(&mut self) -> Vec<Op> {
        self.op_log.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// A read-only editor ignores every modification (no change, no revision, no history, nothing recorded).
    pub fn set_read_only(&mut self, on: bool) {
        self.read_only = on;
    }
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// The document was changed without going through the editor (a transient state of somebody else's gesture): caches are rebuilt.
    pub fn bump_revision(&mut self) {
        self.rev += 1;
    }

    /// Replaces the document by a newer state of the same board (refresh of a mirror): the history is left alone, the current page
    /// is kept when it still exists, the revision moves so that caches are rebuilt.
    pub fn replace_document(&mut self, doc: Document) {
        self.doc = doc;
        self.rev += 1;
        self.fix_current();
    }
}

/* ------------------------------------------------------------------------------------------------------------------------------
 * Shared sessions: modifications made by another participant (see `history::AuthorHistories` for the per-author undo).
 * ---------------------------------------------------------------------------------------------------------------------------- */
impl Editor {
    /// Applies modifications made by a remote participant. They do **not** go into this editor's undo stack (the local user's Ctrl+Z
    /// never undoes somebody else's work) but they do increment the revision (unsaved changes, caches).
    pub fn apply_external(&mut self, ops: &[Op]) {
        if ops.is_empty() {
            return;
        }
        for op in ops {
            self.doc.apply(op);
        }
        self.rev += 1;
        self.fix_current();
    }

    /// Like [`Editor::apply_external`] for a change that is computed on the document itself (e.g. applying index-free operations):
    /// `f` modifies the document and returns the core operations it performed (nothing changed: the revision stays the same).
    pub fn apply_external_with(&mut self, f: impl FnOnce(&mut Document) -> Vec<Op>) -> Vec<Op> {
        let ops = f(&mut self.doc);
        if !ops.is_empty() {
            self.rev += 1;
            self.fix_current();
        }
        ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Frame;
    use crate::model::{BoxGeom, Label};
    use crate::style::Style;

    fn shape(x: f64, y: f64) -> Object {
        Object::new_box(BoxGeom::Rect, Frame::new(x, y, 100.0, 50.0), Style::default(), Label::default())
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut e = Editor::new(Document::default());
        let a = e.insert(shape(0.0, 0.0));
        e.update(&a, |o| o.translate(Pt::new(10.0, 0.0)));
        assert_eq!(e.page().objects.len(), 1);
        assert!(e.undo());
        assert_eq!(e.page().get(&a).unwrap().frame().unwrap().x, 0.0);
        assert!(e.undo());
        assert_eq!(e.page().objects, [] as [crate::model::Object; 0]);
        assert!(e.redo() && e.redo());
        assert_eq!(e.page().get(&a).unwrap().frame().unwrap().x, 10.0);
        assert!(!e.redo());
    }

    #[test]
    fn transaction_is_one_undo_step() {
        let mut e = Editor::new(Document::default());
        e.transact(|e| {
            e.insert(shape(0.0, 0.0));
            e.insert(shape(200.0, 0.0));
        });
        assert_eq!(e.page().objects.len(), 2);
        e.undo();
        assert_eq!(e.page().objects, [] as [crate::model::Object; 0]);
    }

    #[test]
    fn try_transact_success_is_one_undo_step() {
        let mut e = Editor::new(Document::default());
        let r: Result<usize, String> = e.try_transact(|e| {
            e.insert(shape(0.0, 0.0));
            e.insert(shape(200.0, 0.0));
            Ok(e.page().objects.len())
        });
        assert_eq!(r, Ok(2));
        assert!(e.revision() > 0);
        e.undo();
        assert_eq!(e.page().objects, [] as [crate::model::Object; 0]);
        assert!(!e.can_undo());
    }

    #[test]
    fn try_transact_failure_restores_everything_and_keeps_redo() {
        let mut e = Editor::new(Document::default());
        e.insert(shape(0.0, 0.0));
        let kept = e.insert(shape(10.0, 0.0));
        e.undo();
        assert!(e.can_redo());
        let (doc, rev) = (e.doc.clone(), e.revision());
        let r: Result<(), &str> = e.try_transact(|e| {
            let a = e.insert(shape(5.0, 5.0));
            e.update(&a, |o| o.translate(Pt::new(1.0, 1.0)));
            e.remove(std::slice::from_ref(&kept));
            Err("boom")
        });
        assert_eq!(r, Err("boom"));
        assert_eq!(e.doc, doc);
        assert_eq!(e.revision(), rev, "an undone transaction must not look like a modification");
        assert!(e.can_redo(), "the redo stack must survive a failed transaction");
        assert!(e.redo());
        assert!(e.page().get(&kept).is_some());
        // the history holds exactly the two original steps
        assert!(e.undo() && e.undo() && !e.undo());
    }

    #[test]
    fn try_transact_inside_a_transaction_undoes_only_its_own_operations() {
        let mut e = Editor::new(Document::default());
        e.transact(|e| {
            let before = e.insert(shape(0.0, 0.0));
            let r: Result<(), ()> = e.try_transact(|e| {
                e.insert(shape(50.0, 0.0));
                e.update(&before, |o| o.translate(Pt::new(7.0, 0.0)));
                Err(())
            });
            assert!(r.is_err());
            assert_eq!(e.page().objects.len(), 1);
            assert_eq!(e.page().get(&before).unwrap().frame().unwrap().x, 0.0);
            let ok: Result<Id, ()> = e.try_transact(|e| Ok(e.insert(shape(90.0, 0.0))));
            assert!(ok.is_ok());
            e.insert(shape(120.0, 0.0));
        });
        assert_eq!(e.page().objects.len(), 3);
        // the whole outer transaction is still a single undo step
        e.undo();
        assert_eq!(e.page().objects, [] as [crate::model::Object; 0]);
        assert!(!e.can_undo());
    }

    #[test]
    fn try_transact_failure_after_page_operations_undoes_them() {
        let mut e = Editor::new(Document::default());
        let first = e.current.clone();
        let second = e.add_page("Two");
        e.current = first.clone();
        e.insert(shape(0.0, 0.0));
        let (doc, rev) = (e.doc.clone(), e.revision());
        let r: Result<(), ()> = e.try_transact(|e| {
            let p3 = e.add_page("Three");
            e.insert(shape(1.0, 1.0));
            e.rename_page(&second, "Renamed");
            e.remove_page(&second);
            e.rename_page(&p3, "Last");
            Err(())
        });
        assert!(r.is_err());
        assert_eq!(e.doc, doc);
        assert_eq!(e.doc.pages.len(), 2);
        assert_eq!(e.current, first, "the current page comes back too");
        assert_eq!(e.revision(), rev);
        // removing a page that holds objects, then failing, brings the page and its objects back
        e.current = second.clone();
        e.insert(shape(3.0, 3.0));
        let doc = e.doc.clone();
        let r: Result<(), ()> = e.try_transact(|e| {
            e.remove_page(&second);
            Err(())
        });
        assert!(r.is_err());
        assert_eq!(e.doc, doc);
        assert_eq!(e.current, second);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut e = Editor::new(Document::default());
        e.insert(shape(0.0, 0.0));
        e.undo();
        assert!(e.can_redo());
        e.insert(shape(5.0, 5.0));
        assert!(!e.can_redo());
    }

    #[test]
    fn removing_a_shape_detaches_its_connectors_and_undo_restores() {
        let mut e = Editor::new(Document::default());
        let a = e.insert(shape(0.0, 0.0));
        let b = e.insert(shape(300.0, 0.0));
        let c = e.connect(&a, &b);
        e.remove(std::slice::from_ref(&a));
        let Kind::Connector(cn) = &e.page().get(&c).unwrap().kind else { panic!() };
        assert!(matches!(cn.start, End::Free { .. }));
        assert!(matches!(cn.end, End::Bound { .. }));
        assert!(e.page().connector_path(cn).is_some());
        e.undo();
        let Kind::Connector(cn) = &e.page().get(&c).unwrap().kind else { panic!() };
        assert!(matches!(cn.start, End::Bound { .. }));
        assert!(e.page().get(&a).is_some());
    }

    #[test]
    fn duplicate_keeps_internal_links_and_breaks_external_ones() {
        let mut e = Editor::new(Document::default());
        let a = e.insert(shape(0.0, 0.0));
        let b = e.insert(shape(300.0, 0.0));
        let outside = e.insert(shape(600.0, 0.0));
        let c1 = e.connect(&a, &b);
        let c2 = e.connect(&b, &outside);
        let new = e.duplicate(&[a, b, c1, c2], Pt::new(0.0, 200.0));
        assert_eq!(new.len(), 4);
        let page = e.page();
        let copies: Vec<&Object> = new.iter().map(|i| page.get(i).unwrap()).collect();
        let conns: Vec<&Connector> = copies.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!(conns.len(), 2);
        // first connector: always bound to the copies
        assert!(conns[0].targets().all(|t| new.contains(t)));
        // second: the end pointing outside has become free, at the original position
        assert!(matches!(conns[1].end, End::Free { .. }));
        assert!(page.connector_path(conns[1]).is_some());
    }

    #[test]
    fn reorder_front_and_back() {
        let mut e = Editor::new(Document::default());
        let a = e.insert(shape(0.0, 0.0));
        let b = e.insert(shape(1.0, 0.0));
        let c = e.insert(shape(2.0, 0.0));
        e.reorder(std::slice::from_ref(&a), Order::Front);
        let ids: Vec<&Id> = e.page().objects.iter().map(|o| &o.id).collect();
        assert_eq!(ids, vec![&b, &c, &a]);
        e.reorder(std::slice::from_ref(&a), Order::Backward);
        assert_eq!(e.page().objects[1].id, a);
        e.undo();
        e.undo();
        assert_eq!(e.page().objects[0].id, a);
    }

    #[test]
    fn align_and_distribute() {
        let mut e = Editor::new(Document::default());
        let ids: Vec<Id> = [0.0, 30.0, 400.0].iter().map(|x| e.insert(shape(*x, *x))).collect();
        e.align(&ids, AlignMode::Left);
        assert!(ids.iter().all(|i| e.page().get(i).unwrap().frame().unwrap().x.abs() < 1e-9));
        e.distribute(&ids, Axis::Vertical);
        let ys: Vec<f64> = ids.iter().map(|i| e.page().get(i).unwrap().frame().unwrap().y).collect();
        assert!((ys[1] - ys[0] - (ys[2] - ys[1])).abs() < 1e-9);
    }

    #[test]
    fn groups_expand_selection() {
        let mut e = Editor::new(Document::default());
        let a = e.insert(shape(0.0, 0.0));
        let b = e.insert(shape(1.0, 0.0));
        let _c = e.insert(shape(2.0, 0.0));
        e.group(&[a.clone(), b.clone()]);
        let ex = e.page().expand_groups(std::slice::from_ref(&a));
        assert_eq!(ex.len(), 2);
        assert!(ex.contains(&b));
        e.ungroup(&[a.clone(), b]);
        assert_eq!(e.page().expand_groups(&[a]).len(), 1);
    }

    #[test]
    fn pages_add_rename_remove_undo() {
        let mut e = Editor::new(Document::default());
        let first = e.current.clone();
        let p2 = e.add_page("Deux");
        e.insert(shape(0.0, 0.0));
        assert_eq!(e.doc.pages.len(), 2);
        e.rename_page(&p2, "Autre");
        assert_eq!(e.doc.page(&p2).unwrap().name, "Autre");
        e.remove_page(&p2);
        assert_eq!(e.current, first);
        e.undo();
        assert_eq!(e.doc.pages.len(), 2);
        assert_eq!(e.doc.page(&p2).unwrap().objects.len(), 1);
    }

    #[test]
    fn last_page_cannot_be_removed() {
        let mut e = Editor::new(Document::default());
        let id = e.current.clone();
        e.remove_page(&id);
        assert_eq!(e.doc.pages.len(), 1);
    }

    #[test]
    fn guest_hook_records_operations_and_read_only_ignores_everything() {
        let mut e = Editor::new(Document::default());
        e.record_ops(true);
        let a = e.insert(shape(0.0, 0.0));
        let r: Result<(), ()> = e.try_transact(|e| {
            e.insert(shape(5.0, 5.0));
            Err(())
        });
        assert!(r.is_err());
        e.update(&a, |o| o.translate(Pt::new(1.0, 0.0)));
        assert_eq!(e.take_recorded_ops().len(), 2, "the undone transaction is not recorded");
        assert!(e.take_recorded_ops().is_empty());
        e.set_read_only(true);
        let rev = e.revision();
        e.insert(shape(9.0, 9.0));
        e.remove(std::slice::from_ref(&a));
        assert!(!e.undo() && !e.redo());
        assert_eq!((e.revision(), e.page().objects.len()), (rev, 1));
        assert!(e.take_recorded_ops().is_empty());
        e.replace_document(Document::default());
        assert!(e.revision() > rev && e.page().objects.is_empty());
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        /// Applies one pseudo-random editor operation chosen by `code`, `pick` selecting its target.
        fn step(e: &mut Editor, (code, pick): (u8, u8)) {
            let ids: Vec<Id> = e.page().objects.iter().map(|o| o.id.clone()).collect();
            let target = (!ids.is_empty()).then(|| ids[pick as usize % ids.len()].clone());
            let other = (!ids.is_empty()).then(|| ids[(pick as usize / 3 + 1) % ids.len()].clone());
            match (code % 11, target, other) {
                (0, ..) | (_, None, _) => {
                    e.insert(shape(f64::from(pick), f64::from(code)));
                }
                (1, Some(t), _) => e.update(&t, |o| o.translate(Pt::new(3.0, -2.0))),
                (2, Some(t), _) => e.remove(&[t]),
                (3, Some(t), _) => e.reorder(&[t], Order::Front),
                (4, Some(t), Some(o)) => {
                    e.connect(&t, &o);
                }
                (5, Some(t), Some(o)) => e.group(&[t, o]),
                (6, ..) => {
                    e.add_page("Extra");
                }
                (7, ..) => {
                    let page = e.doc.pages[pick as usize % e.doc.pages.len()].id.clone();
                    e.remove_page(&page);
                    e.current = e.doc.pages[0].id.clone();
                }
                (8, ..) => {
                    let page = e.doc.pages[pick as usize % e.doc.pages.len()].id.clone();
                    e.rename_page(&page, &format!("P{pick}"));
                }
                (9, ..) => e.current = e.doc.pages[pick as usize % e.doc.pages.len()].id.clone(),
                (_, Some(t), _) => {
                    e.duplicate(&[t], Pt::new(20.0, 20.0));
                }
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

            #[test]
            fn failed_try_transact_leaves_the_editor_as_it_was(
                before in proptest::collection::vec(any::<(u8, u8)>(), 0..12),
                inside in proptest::collection::vec(any::<(u8, u8)>(), 0..25),
                nested in any::<bool>(),
            ) {
                let mut e = Editor::new(Document::default());
                for s in before {
                    step(&mut e, s);
                }
                let (doc, current, rev) = (e.doc.clone(), e.current.clone(), e.revision());
                let (can_undo, can_redo) = (e.can_undo(), e.can_redo());
                let run = |e: &mut Editor| {
                    e.try_transact(|e| {
                        for s in inside {
                            step(e, s);
                        }
                        Err::<(), ()>(())
                    })
                };
                if nested {
                    // an open transaction holding one operation of its own: that operation stays
                    e.transact(|e| -> Result<(), TestCaseError> {
                        e.insert(shape(1.0, 1.0));
                        let (doc_in, rev_in) = (e.doc.clone(), e.revision());
                        prop_assert!(run(e).is_err());
                        prop_assert_eq!(&e.doc, &doc_in);
                        prop_assert_eq!(e.revision(), rev_in);
                        Ok(())
                    })?;
                    e.undo();
                    prop_assert_eq!(e.can_undo(), can_undo);
                } else {
                    prop_assert!(run(&mut e).is_err());
                    prop_assert_eq!((e.can_undo(), e.can_redo()), (can_undo, can_redo));
                    prop_assert_eq!(e.revision(), rev);
                }
                prop_assert_eq!(&e.doc, &doc);
                prop_assert_eq!(&e.current, &current);
            }
        }
    }

    /* ----------------------------------------------- external modifications */

    #[test]
    fn external_changes_bump_the_revision_but_never_enter_the_undo_stack() {
        let mut e = Editor::new(Document::default());
        let mine = e.insert(shape(0.0, 0.0));
        let page = e.current.clone();
        let theirs = shape(200.0, 0.0);
        let their_id = theirs.id.clone();
        let rev = e.revision();
        e.apply_external(&[Op::Insert { page, index: 1, obj: Box::new(theirs) }]);
        assert_eq!(e.revision(), rev + 1);
        assert_eq!(e.page().objects.len(), 2);
        e.apply_external(&[]);
        assert_eq!(e.revision(), rev + 1, "nothing applied: same revision");
        assert!(e.undo(), "the local user can undo their own insertion");
        assert!(e.page().get(&mine).is_none() && e.page().get(&their_id).is_some(), "the remote object is untouched");
        assert!(!e.can_undo(), "the remote modification was never on the stack");
    }

    #[test]
    fn external_removal_of_the_current_page_moves_to_another_page() {
        let mut e = Editor::new(Document::default());
        let first = e.current.clone();
        let second = e.add_page("two");
        let index = e.doc.pages.iter().position(|p| p.id == second).unwrap();
        let page = Box::new(e.doc.pages[index].clone());
        e.apply_external(&[Op::RemovePage { index, page }]);
        assert_eq!(e.current, first);
    }

    #[test]
    fn apply_external_with_reports_what_was_performed() {
        let mut e = Editor::new(Document::default());
        let page = e.current.clone();
        let rev = e.revision();
        let ops = e.apply_external_with(|d| {
            let op = Op::Insert { page, index: 0, obj: Box::new(shape(1.0, 1.0)) };
            d.apply(&op);
            vec![op]
        });
        assert_eq!((ops.len(), e.revision()), (1, rev + 1));
        assert!(e.apply_external_with(|_| Vec::new()).is_empty());
        assert_eq!(e.revision(), rev + 1);
        assert!(!e.can_undo());
    }
}
