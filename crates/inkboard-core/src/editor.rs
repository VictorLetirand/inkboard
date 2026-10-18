//! Édition d'un document avec historique : toutes les modifications passent ici.

use crate::connector::{Connector, End};
use crate::geom::{Pt, Rect};
use crate::history::{History, Op};
use crate::model::{Background, Document, Id, Kind, Layer, Object, Page};
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
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let current = doc.pages[0].id.clone();
        Self { doc, current, history: History::default(), open: None, depth: 0, rev: 0 }
    }

    /// Compteur incrémenté à chaque modification (pour détecter « à enregistrer »).
    pub fn revision(&self) -> u64 {
        self.rev
    }
    pub fn page(&self) -> &Page {
        self.doc.page(&self.current).unwrap_or(&self.doc.pages[0])
    }
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Regroupe plusieurs modifications en une seule étape d'annulation.
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

    fn apply(&mut self, op: Op) {
        self.doc.apply(&op);
        self.rev += 1;
        match self.open.as_mut() {
            Some(v) => v.push(op),
            None => self.history.push(vec![op]),
        }
    }

    pub fn undo(&mut self) -> bool {
        let ok = self.history.undo(&mut self.doc);
        if ok {
            self.rev += 1;
            self.fix_current();
        }
        ok
    }
    pub fn redo(&mut self) -> bool {
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
    /// Repart d'un historique vide (après ouverture d'un fichier).
    pub fn reset(&mut self, doc: Document) {
        self.current = doc.pages[0].id.clone();
        self.doc = doc;
        self.history.clear();
        self.rev += 1;
    }

    /* ------------------------------------------------------- Objets */

    pub fn insert(&mut self, obj: Object) -> Id {
        let id = obj.id.clone();
        let (page, index) = (self.current.clone(), self.page().objects.len());
        self.apply(Op::Insert { page, index, obj: Box::new(obj) });
        id
    }

    /// Remplace l'objet de même identifiant.
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

    /// Supprime des objets ; les connecteurs liés sont détachés (ils gardent leur position).
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

    /// Copies d'objets avec nouveaux identifiants, décalées de `d`. Les liens vers des objets
    /// non copiés sont rompus (extrémités libres à leur position actuelle).
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

    /// Colle des objets (déjà clonés) au premier plan.
    pub fn insert_all(&mut self, objs: Vec<Object>) -> Vec<Id> {
        self.transact(|e| objs.into_iter().map(|o| e.insert(o)).collect())
    }

    pub fn duplicate(&mut self, ids: &[Id], d: Pt) -> Vec<Id> {
        let copies = Self::clone_objects(self.page(), ids, d);
        self.insert_all(copies)
    }

    /// Relie deux objets par un nouveau connecteur.
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

    /// Répartit les objets à intervalles réguliers (3 objets au moins).
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
        let gap = (span - sizes) / (items.len() - 1) as f64;
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
        assert!(e.page().objects.is_empty());
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
        assert!(e.page().objects.is_empty());
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
        // premier connecteur : toujours lié aux copies
        assert!(conns[0].targets().all(|t| new.contains(t)));
        // second : extrémité vers l'extérieur devenue libre, à la position d'origine
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
}
