//! Canvas interactions: tools, selection, handles, text editing.

use crate::app::App;
use crate::i18n::t;
use crate::render::{Cam, Renderer};
use eframe::egui::{self, Color32, CursorIcon, PointerButton, Pos2, Sense, Shape, Stroke, vec2};
use inkboard_core::geom::{Frame, Pt, Rect};
use inkboard_core::transform::{Handle, constrain_angle, resize_frame, resize_rect, rotation_toward, scale_objects, snap_angle, snap_point};
use inkboard_core::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Hand,
    Pen,
    Highlighter,
    Eraser,
    Shape(BoxGeom),
    Line,
    Arrow,
    Text,
    Sticky,
    Table,
    /// Vote dots: one click drops a dot.
    Vote,
    /// Laser pointer: a fading red trail, to point without drawing.
    Laser,
}

pub use crate::rich_edit::Editing;

pub enum Drag {
    Pan {
        from: Pos2,
        view: View,
    },
    Move {
        start: Pt,
        orig: Vec<Object>,
        moved: bool,
        collapse_to: Option<Id>,
    },
    Resize {
        handle: Handle,
        orig: Vec<Object>,
        orig_frame: Frame,
        orig_rect: Rect,
        single_frame: bool,
    },
    Rotate {
        center: Pt,
        a0: f64,
        orig: Vec<Object>,
    },
    Endpoint {
        orig: Object,
        is_start: bool,
    },
    /// Waypoint of a connector; `inserted`: the point has just been created from a segment-midpoint handle.
    Via {
        orig: Object,
        idx: usize,
        inserted: bool,
    },
    Marquee {
        start: Pt,
        base: Vec<Id>,
    },
    Draw {
        pts: Vec<Pt>,
        pressures: Vec<f32>,
        has_force: bool,
    },
    Create {
        tool: Tool,
        start: Pt,
        start_target: Option<Id>,
    },
    Erase {
        hit: Vec<Id>,
        last: Pt,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HandleKind {
    Resize(Handle),
    Rotate,
    Start,
    End,
    Via(usize),
    Mid(usize),
}

struct SelBox {
    frame: Frame,
    edges: bool,
    rotate: bool,
    /// A single object with a frame (shape, image, table): exact resizing in its own frame of reference.
    single_frame: bool,
    ends: Option<(Pt, Pt)>,
    /// Waypoints (single connector only).
    via: Vec<Pt>,
}

const MIN_SIZE: f64 = 4.0;

impl App {
    /// Start of a "live" modification (panel slider): remembers the original state.
    pub fn live_apply(&mut self, f: &dyn Fn(&mut Object)) {
        if self.prop_edit.is_none() {
            self.prop_edit = Some(self.editable_selection());
        }
        let mut objs = self.prop_edit.clone().unwrap_or_default();
        objs.iter_mut().for_each(f);
        self.set_live(&objs);
    }

    /// End of the live modification: a single undo step.
    pub fn live_end(&mut self) {
        if let Some(orig) = self.prop_edit.take() {
            self.commit_live(&orig);
        }
    }

    fn page_mut(&mut self) -> &mut Page {
        let id = self.ed.current.clone();
        let idx = self.ed.doc.pages.iter().position(|p| p.id == id).unwrap_or(0);
        &mut self.ed.doc.pages[idx]
    }

    /// Applies objects to the page without touching the history (preview during a gesture).
    pub(crate) fn set_live(&mut self, objs: &[Object]) {
        let page = self.page_mut();
        for o in objs {
            if let Some(i) = page.index_of(&o.id) {
                page.objects[i] = o.clone();
            }
        }
    }

    /// Ends a gesture: the history receives a single "before / after".
    fn commit_live(&mut self, orig: &[Object]) {
        let finals: Vec<Object> = orig.iter().filter_map(|o| self.ed.page().get(&o.id).cloned()).collect();
        self.set_live(orig);
        self.ed.transact(|e| {
            for f in finals {
                e.replace(f);
            }
        });
    }

    fn selected_objects(&self) -> Vec<Object> {
        self.ed.page().objects.iter().filter(|o| self.sel.contains(&o.id)).cloned().collect()
    }

    pub fn editable_selection(&self) -> Vec<Object> {
        let p = self.ed.page();
        p.objects.iter().filter(|o| self.sel.contains(&o.id) && !p.is_locked(o)).cloned().collect()
    }

    fn select_with_groups(&mut self, ids: Vec<Id>) {
        self.sel = self.ed.page().expand_groups(&ids);
    }

    fn selection_box(&self) -> Option<SelBox> {
        let page = self.ed.page();
        let objs = self.selected_objects();
        if objs.is_empty() || self.editing.is_some() {
            return None;
        }
        if objs.iter().all(|o| page.is_locked(o)) {
            return None;
        }
        if let [o] = objs.as_slice() {
            return Some(match &o.kind {
                Kind::Box(b) => SelBox { frame: b.frame, edges: b.geom != BoxGeom::Text, rotate: true, single_frame: true, ends: None, via: Vec::new() },
                Kind::Image(i) => SelBox { frame: i.frame, edges: true, rotate: true, single_frame: true, ends: None, via: Vec::new() },
                Kind::Table(t) => SelBox { frame: Frame::from_rect(t.rect()), edges: true, rotate: false, single_frame: true, ends: None, via: Vec::new() },
                Kind::Pen(p) => SelBox {
                    frame: Frame::from_rect(Rect::from_points(p.pts.iter().copied()).unwrap_or_default()),
                    edges: true,
                    rotate: true,
                    single_frame: false,
                    ends: None,
                    via: Vec::new(),
                },
                Kind::Connector(c) => {
                    let path = page.connector_path(c)?;
                    SelBox {
                        frame: Frame::from_rect(page.bounds(o)),
                        edges: false,
                        rotate: false,
                        single_frame: false,
                        ends: Some((path[0], path[path.len() - 1])),
                        via: c.via.clone(),
                    }
                }
            });
        }
        let r = page.bounds_of(&self.sel)?;
        Some(SelBox { frame: Frame::from_rect(r), edges: true, rotate: true, single_frame: false, ends: None, via: Vec::new() })
    }

    fn handles(&self, cam: &Cam) -> Vec<(HandleKind, Pos2)> {
        let Some(sb) = self.selection_box() else { return Vec::new() };
        if let Some((a, b)) = sb.ends {
            let mut v = vec![(HandleKind::Start, cam.s(a)), (HandleKind::End, cam.s(b))];
            // waypoint handles, then the midpoint of each segment (to create a new one)
            let controls: Vec<Pt> = std::iter::once(a).chain(sb.via.iter().copied()).chain(std::iter::once(b)).collect();
            for (i, p) in sb.via.iter().enumerate() {
                v.push((HandleKind::Via(i), cam.s(*p)));
            }
            for (k, w) in controls.windows(2).enumerate() {
                if w[0].dist(w[1]) * f64::from(cam.zoom) > 30.0 {
                    v.push((HandleKind::Mid(k), cam.s(w[0].lerp(w[1], 0.5))));
                }
            }
            return v;
        }
        let f = sb.frame;
        let mut v: Vec<(HandleKind, Pos2)> =
            Handle::ALL.iter().filter(|h| sb.edges || h.is_corner()).map(|h| (HandleKind::Resize(*h), cam.s(f.to_world(h.local(f.w, f.h))))).collect();
        if sb.rotate {
            v.push((HandleKind::Rotate, cam.s(f.to_world(Pt::new(0.0, -f.h / 2.0 - 28.0 / f64::from(cam.zoom))))));
        }
        v
    }

    fn hit_handle(&self, cam: &Cam, p: Pos2) -> Option<HandleKind> {
        self.handles(cam).into_iter().filter(|(_, hp)| hp.distance(p) <= 10.0).min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p))).map(|h| h.0)
    }

    /* ----------------------------------------------- Gesture start */

    fn press_select(&mut self, cam: &Cam, screen: Pos2, w: Pt, shift: bool, alt: bool) {
        if let Some(h) = self.hit_handle(cam, screen) {
            let orig = self.editable_selection();
            let Some(sb) = self.selection_box() else { return };
            match h {
                HandleKind::Rotate => {
                    let center = if sb.single_frame { sb.frame.center() } else { sb.frame.rect().center() };
                    self.drag = Some(Drag::Rotate { center, a0: rotation_toward(center, w), orig });
                }
                HandleKind::Resize(handle) => {
                    let orig_rect = self.ed.page().bounds_of(&self.sel).unwrap_or(sb.frame.rect());
                    self.drag = Some(Drag::Resize { handle, orig, orig_frame: sb.frame, orig_rect, single_frame: sb.single_frame });
                }
                HandleKind::Start | HandleKind::End => {
                    if let Some(o) = orig.into_iter().next() {
                        self.drag = Some(Drag::Endpoint { orig: o, is_start: h == HandleKind::Start });
                    }
                }
                HandleKind::Via(i) => {
                    if let Some(o) = orig.into_iter().next() {
                        if alt {
                            // Alt + click: removes this waypoint
                            self.ed.update(&o.id, |x| {
                                if let Kind::Connector(c) = &mut x.kind
                                    && i < c.via.len()
                                {
                                    c.via.remove(i);
                                }
                            });
                        } else {
                            self.drag = Some(Drag::Via { orig: o, idx: i, inserted: false });
                        }
                    }
                }
                HandleKind::Mid(k) => {
                    if let Some(o) = orig.into_iter().next() {
                        self.drag = Some(Drag::Via { orig: o, idx: k, inserted: true });
                    }
                }
            }
            return;
        }
        let tol = 6.0 / f64::from(cam.zoom);
        match self.pick_at(w, tol) {
            Some(i) => {
                let o = self.ed.page().objects[i].clone();
                if shift {
                    let ids = self.ed.page().expand_groups(std::slice::from_ref(&o.id));
                    if self.sel.contains(&o.id) {
                        self.sel.retain(|s| !ids.contains(s));
                    } else {
                        self.sel.extend(ids);
                    }
                    return;
                }
                let was = self.sel.contains(&o.id);
                if !was {
                    self.select_with_groups(vec![o.id.clone()]);
                }
                let mut orig = self.editable_selection();
                if alt && !orig.is_empty() {
                    let ids: Vec<Id> = orig.iter().map(|o| o.id.clone()).collect();
                    self.sel = self.ed.duplicate(&ids, Pt::ZERO);
                    orig = self.editable_selection();
                }
                self.drag = Some(Drag::Move { start: w, orig, moved: false, collapse_to: was.then_some(o.id) });
            }
            None => {
                let base = if shift { self.sel.clone() } else { Vec::new() };
                self.sel = base.clone();
                self.drag = Some(Drag::Marquee { start: w, base });
            }
        }
    }

    fn press_tool(&mut self, cam: &Cam, screen: Pos2, w: Pt, shift: bool, alt: bool) {
        // the pen's eraser end erases, whatever the active tool (except the hand)
        let tool = if self.tool != Tool::Hand && (self.eraser_probe)() { Tool::Eraser } else { self.tool };
        match tool {
            Tool::Select => self.press_select(cam, screen, w, shift, alt),
            Tool::Hand => self.drag = Some(Drag::Pan { from: screen, view: self.ed.page().view }),
            Tool::Pen | Tool::Highlighter => {
                self.drag = Some(Drag::Draw { pts: vec![w], pressures: vec![self.pen_force.unwrap_or(0.5)], has_force: self.pen_force.is_some() })
            }
            Tool::Vote => self.insert_vote_dot(w),
            Tool::Laser => {}
            Tool::Eraser => {
                self.drag = Some(Drag::Erase { hit: Vec::new(), last: w });
                self.erase_at(w, w, f64::from(cam.zoom));
            }
            Tool::Text | Tool::Sticky | Tool::Shape(_) | Tool::Line | Tool::Arrow | Tool::Table => {
                let start = if self.snap { snap_point(w, f64::from(crate::render::GRID)) } else { w };
                let target = matches!(tool, Tool::Line | Tool::Arrow).then(|| self.ed.page().attach_target(w, 4.0 / f64::from(cam.zoom), &[])).flatten();
                self.drag = Some(Drag::Create { tool, start, start_target: target });
            }
        }
    }

    fn erase_at(&mut self, a: Pt, b: Pt, zoom: f64) {
        let tol = 8.0 / zoom;
        let n = ((a.dist(b) / (tol * 0.8)).ceil() as usize).max(1);
        let mut found: Vec<Id> = Vec::new();
        for i in 0..=n {
            let p = a.lerp(b, i as f64 / n as f64);
            if let Some(idx) = self.pick_at(p, tol) {
                let o = &self.ed.page().objects[idx];
                if !self.ed.page().is_locked(o) {
                    found.push(o.id.clone());
                }
            }
        }
        if let Some(Drag::Erase { hit, .. }) = &mut self.drag {
            for id in found {
                if !hit.contains(&id) {
                    hit.push(id);
                }
            }
        }
    }

    /* ----------------------------------------------- Pendant le geste */

    fn move_drag(&mut self, cam: &Cam, w: Pt, shift: bool, alt: bool) {
        let Some(drag) = self.drag.take() else { return };
        let zoom = f64::from(cam.zoom);
        let drag = match drag {
            Drag::Move { start, orig, mut moved, collapse_to } => {
                if !moved && start.dist(w) * zoom < 3.0 {
                    self.drag = Some(Drag::Move { start, orig, moved, collapse_to });
                    return;
                }
                moved = true;
                let _ = alt;
                let mut d = w - start;
                if shift {
                    if d.x.abs() > d.y.abs() { d.y = 0.0 } else { d.x = 0.0 }
                }
                if self.snap
                    && let Some(r) = orig.iter().map(|o| self.ed.page().bounds(o)).reduce(|a, b| a.union(&b))
                {
                    let g = f64::from(crate::render::GRID);
                    let s = snap_point(Pt::new(r.x + d.x, r.y + d.y), g);
                    d = Pt::new(s.x - r.x, s.y - r.y);
                }
                let moved_objs: Vec<Object> = orig
                    .iter()
                    .map(|o| {
                        let mut c = o.clone();
                        c.translate(d);
                        c
                    })
                    .collect();
                self.set_live(&moved_objs);
                Drag::Move { start, orig, moved, collapse_to }
            }
            Drag::Resize { handle, orig, orig_frame, orig_rect, single_frame } => {
                if single_frame {
                    if let Some(first) = orig.first() {
                        let ratio = shift || matches!(&first.kind, Kind::Image(_)) || matches!(&first.kind, Kind::Box(b) if b.geom == BoxGeom::Text);
                        let nf = resize_frame(&orig_frame, handle, w, ratio, MIN_SIZE);
                        let mut o = first.clone();
                        match &mut o.kind {
                            Kind::Box(b) => {
                                b.frame = nf;
                                if b.geom == BoxGeom::Text {
                                    b.label.size = (b.label.size * nf.h / orig_frame.h.max(1.0)).max(6.0);
                                    b.fit(&self.measurer);
                                    b.frame.x = nf.x;
                                    b.frame.y = nf.y;
                                }
                            }
                            Kind::Image(i) => i.frame = nf,
                            Kind::Table(t) => {
                                let (sx, sy) = (nf.w / orig_frame.w.max(1.0), nf.h / orig_frame.h.max(1.0));
                                if let Kind::Table(ot) = &first.kind {
                                    t.col_w = ot.col_w.iter().map(|c| (c * sx).max(20.0)).collect();
                                    t.row_h = ot.row_h.iter().map(|r| (r * sy).max(16.0)).collect();
                                }
                                t.origin = Pt::new(nf.x, nf.y);
                            }
                            _ => {}
                        }
                        self.set_live(&[o]);
                    }
                } else {
                    let nr = resize_rect(orig_rect, handle, w, shift, MIN_SIZE);
                    let mut objs = orig.clone();
                    scale_objects(&mut objs, orig_rect, nr);
                    self.set_live(&objs);
                }
                Drag::Resize { handle, orig, orig_frame, orig_rect, single_frame }
            }
            Drag::Rotate { center, a0, orig } => {
                let a = rotation_toward(center, w);
                let mut da = a - a0;
                if shift {
                    let base = match orig.as_slice() {
                        [o] => o.frame().map_or(0.0, |f| f.angle),
                        _ => 0.0,
                    };
                    da = snap_angle(base + da, std::f64::consts::PI / 12.0) - base;
                }
                let objs: Vec<Object> = orig
                    .iter()
                    .map(|o| {
                        let mut c = o.clone();
                        c.rotate_about(center, da);
                        c
                    })
                    .collect();
                self.set_live(&objs);
                Drag::Rotate { center, a0, orig }
            }
            Drag::Endpoint { orig, is_start } => {
                let target = self.ed.page().attach_target(w, 4.0 / zoom, std::slice::from_ref(&orig.id));
                let mut o = orig.clone();
                if let Kind::Connector(c) = &mut o.kind {
                    let other_free = if is_start { &c.end } else { &c.start };
                    let mut p = w;
                    if let (true, End::Free { at }) = (shift && target.is_none(), other_free) {
                        p = constrain_angle(*at, w, std::f64::consts::PI / 12.0);
                    } else if self.snap && target.is_none() {
                        p = snap_point(w, f64::from(crate::render::GRID));
                    }
                    let end = match target {
                        Some(t) => End::bound(t),
                        None => End::free(p),
                    };
                    if is_start { c.start = end } else { c.end = end }
                }
                self.set_live(&[o]);
                Drag::Endpoint { orig, is_start }
            }
            Drag::Via { orig, idx, inserted } => {
                let mut o = orig.clone();
                let p = if self.snap { snap_point(w, f64::from(crate::render::GRID)) } else { w };
                if let Kind::Connector(c) = &mut o.kind {
                    if inserted {
                        c.via.insert(idx.min(c.via.len()), p);
                    } else if let Some(v) = c.via.get_mut(idx) {
                        *v = p;
                    }
                }
                self.set_live(&[o]);
                Drag::Via { orig, idx, inserted }
            }
            Drag::Marquee { start, base } => {
                let r = Rect::from_corners(start, w);
                let ids = self.rect_select(r, w.x >= start.x);
                let mut sel = base.clone();
                sel.extend(ids);
                self.sel = self.ed.page().expand_groups(&sel);
                Drag::Marquee { start, base }
            }
            Drag::Draw { mut pts, mut pressures, mut has_force } => {
                if pts.last().is_none_or(|l| l.dist(w) * zoom >= 1.2) {
                    pts.push(w);
                    pressures.push(self.pen_force.unwrap_or(0.5));
                    has_force |= self.pen_force.is_some();
                }
                Drag::Draw { pts, pressures, has_force }
            }
            Drag::Create { tool, start, start_target } => {
                self.draft = self.make_draft(tool, start, start_target.clone(), w, shift, alt);
                Drag::Create { tool, start, start_target }
            }
            Drag::Erase { hit, last } => {
                self.drag = Some(Drag::Erase { hit, last: w });
                self.erase_at(last, w, zoom);
                return;
            }
            Drag::Pan { from, view } => {
                let _ = (from, view);
                Drag::Pan { from, view }
            }
        };
        self.drag = Some(drag);
    }

    fn make_draft(&self, tool: Tool, start: Pt, start_target: Option<Id>, w: Pt, shift: bool, alt: bool) -> Option<Object> {
        let cur = if self.snap { snap_point(w, f64::from(crate::render::GRID)) } else { w };
        match tool {
            Tool::Line | Tool::Arrow => {
                let target = self.ed.page().attach_target(w, 4.0, start_target.as_slice());
                let s = start_target.clone().map_or(End::free(start), End::bound);
                let end = match target {
                    Some(t) => End::bound(t),
                    None => End::free(if shift { constrain_angle(start, cur, std::f64::consts::PI / 12.0) } else { cur }),
                };
                Some(self.connector_object(tool, s, end))
            }
            Tool::Shape(_) | Tool::Sticky | Tool::Table | Tool::Text => {
                let mut d = cur - start;
                if shift {
                    let m = d.x.abs().max(d.y.abs());
                    d = Pt::new(m.copysign(d.x), m.copysign(d.y));
                }
                let r = if alt {
                    Rect::new(start.x - d.x.abs(), start.y - d.y.abs(), d.x.abs() * 2.0, d.y.abs() * 2.0)
                } else {
                    Rect::from_corners(start, start + d)
                };
                (matches!(tool, Tool::Shape(_) | Tool::Sticky)).then(|| self.box_object(tool, r))
            }
            _ => None,
        }
    }

    fn connector_object(&self, tool: Tool, start: End, end: End) -> Object {
        let mut c = Connector::new(start, end);
        c.route = self.route;
        c.head_end = if tool == Tool::Arrow { Arrowhead::Triangle } else { Arrowhead::None };
        Object::new(Kind::Connector(c), Style { fill: None, width: self.style.width.clamp(2.0, 8.0), ..self.style.clone() })
    }

    fn box_object(&self, tool: Tool, r: Rect) -> Object {
        let (geom, style, align) = match tool {
            Tool::Sticky => {
                (BoxGeom::Sticky, Style { fill: Some(self.style.fill.unwrap_or(style::NOTE_YELLOW)), width: 0.0, ..self.style.clone() }, Align::Center)
            }
            Tool::Text => (BoxGeom::Text, Style { width: 0.0, fill: None, ..self.style.clone() }, Align::Left),
            Tool::Shape(g) => (g, self.style.clone(), Align::Center),
            _ => (BoxGeom::Rect, self.style.clone(), Align::Center),
        };
        Object::new_box(geom, Frame::from_rect(r), style, Label { align, ..self.label.clone() })
    }

    /* ----------------------------------------------- Fin de geste */

    fn release(&mut self, cam: &Cam, w: Pt, shift: bool) {
        let Some(drag) = self.drag.take() else { return };
        let zoom = f64::from(cam.zoom);
        match drag {
            Drag::Move { orig, moved, collapse_to, start } => {
                let _ = start;
                if moved {
                    self.commit_live(&orig);
                } else if let (Some(id), false) = (collapse_to, shift) {
                    self.select_with_groups(vec![id]);
                }
            }
            Drag::Resize { orig, .. } | Drag::Rotate { orig, .. } => self.commit_live(&orig),
            Drag::Endpoint { orig, .. } | Drag::Via { orig, .. } => self.commit_live(std::slice::from_ref(&orig)),
            Drag::Marquee { .. } | Drag::Pan { .. } => {}
            Drag::Draw { pts, pressures, has_force } => {
                let hl = self.tool == Tool::Highlighter;
                let eps = 0.45 / zoom;
                // simplification: keep the indices so that each pressure follows its point
                let keep = if pts.len() > 2 { inkboard_core::geom::rdp_indices(&pts, eps) } else { (0..pts.len()).collect() };
                let mut p: Vec<Pt> = keep.iter().map(|i| pts[*i]).collect();
                let mut pr: Vec<f32> = keep.iter().map(|i| pressures[*i]).collect();
                if p.len() == 1 {
                    p.push(p[0]);
                    pr.push(pr[0]);
                }
                let style = Style { fill: None, dash: Dash::Solid, ..self.style.clone() };
                self.ed.insert(if has_force && !hl { Object::new_pen_with_pressure(p, pr, false, style) } else { Object::new_pen(p, hl, style) });
            }
            Drag::Erase { hit, .. } => {
                if !hit.is_empty() {
                    self.ed.remove(&hit);
                    self.sel.retain(|s| !hit.contains(s));
                }
            }
            Drag::Create { tool, start, start_target } => {
                self.draft = None;
                self.finish_create(tool, start, start_target, w, shift);
            }
        }
    }

    fn finish_create(&mut self, tool: Tool, start: Pt, start_target: Option<Id>, w: Pt, shift: bool) {
        let end_pt = if self.snap { snap_point(w, f64::from(crate::render::GRID)) } else { w };
        let dragged = start.dist(end_pt) > 6.0;
        let new_id: Option<Id> = match tool {
            Tool::Line | Tool::Arrow => {
                let target = self.ed.page().attach_target(w, 4.0, start_target.as_slice());
                let s = start_target.clone().map_or(End::free(start), End::bound);
                let e = match target.clone() {
                    Some(t) => End::bound(t),
                    None => End::free(if shift { constrain_angle(start, end_pt, std::f64::consts::PI / 12.0) } else { end_pt }),
                };
                (dragged || (start_target.is_some() && target.is_some())).then(|| self.ed.insert(self.connector_object(tool, s, e)))
            }
            Tool::Table => {
                let t = Table::new(Pt::new(start.x - 210.0, start.y - 66.0), 3, 3);
                Some(self.ed.insert(Object::new(Kind::Table(t), Style { width: 2.0, fill: None, ..self.style.clone() })))
            }
            Tool::Text => {
                let o = self.box_object(Tool::Text, Rect::new(start.x, start.y - self.label.size * 0.65, 12.0, self.label.size * 1.3));
                Some(self.ed.insert(o))
            }
            Tool::Shape(_) | Tool::Sticky => {
                let (dw, dh) = match tool {
                    Tool::Sticky => (200.0, 200.0),
                    Tool::Shape(BoxGeom::Triangle) => (140.0, 120.0),
                    Tool::Shape(BoxGeom::Diamond) => (150.0, 120.0),
                    _ => (150.0, 100.0),
                };
                let r = match &self.draft_rect(tool, start, end_pt, shift) {
                    Some(r) if r.w >= 6.0 || r.h >= 6.0 => *r,
                    _ => Rect::new(start.x - dw / 2.0, start.y - dh / 2.0, dw, dh),
                };
                let r = if tool == Tool::Sticky { Rect::new(r.x, r.y, r.w.max(80.0), r.h.max(80.0)) } else { Rect::new(r.x, r.y, r.w.max(6.0), r.h.max(6.0)) };
                Some(self.ed.insert(self.box_object(tool, r)))
            }
            _ => None,
        };
        if let Some(id) = new_id {
            self.sel = vec![id.clone()];
            if matches!(tool, Tool::Text | Tool::Sticky) {
                self.start_editing(&id, None);
                if let Some(e) = &mut self.editing {
                    e.fresh = tool == Tool::Text;
                }
            }
            if !self.tool_lock {
                self.tool = Tool::Select;
            } else if !matches!(tool, Tool::Text | Tool::Sticky) {
                self.sel.clear();
            }
        }
    }

    fn draft_rect(&self, tool: Tool, start: Pt, end: Pt, shift: bool) -> Option<Rect> {
        self.make_draft(tool, start, None, end, shift, false).and_then(|o| o.frame().map(|f| f.rect()))
    }

    /* ----------------------------------------------- Texte */

    /// Inserts images chosen in a dialog, at the center of the view.
    pub fn insert_images_dialog(&mut self) {
        let files = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"]).pick_files().unwrap_or_default();
        let c = self.view_center();
        self.insert_image_files(&files, c);
    }

    pub fn insert_image_files(&mut self, files: &[std::path::PathBuf], at: Pt) {
        let mut ids = Vec::new();
        for (i, f) in files.iter().enumerate() {
            let Ok(bytes) = std::fs::read(f) else { continue };
            let Ok(img) = image::load_from_memory(&bytes) else {
                self.toast(&t("Unreadable image"));
                continue;
            };
            let (w, h) = (f64::from(img.width()), f64::from(img.height()));
            let k = (480.0 / w).min(360.0 / h).min(1.0);
            let mime = match f.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("jpg" | "jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                _ => "image/png",
            };
            let aid = self.ed.doc.add_asset(mime, bytes);
            let off = 24.0 * i as f64;
            let frame = Frame::new(at.x - w * k / 2.0 + off, at.y - h * k / 2.0 + off, w * k, h * k);
            ids.push(self.ed.insert(Object::new(
                Kind::Image(ImageObj { frame, asset: aid, latex: None, mono: false }),
                Style { width: 0.0, fill: None, ..Style::default() },
            )));
        }
        if !ids.is_empty() {
            self.sel = ids;
            self.tool = Tool::Select;
        }
    }
}

/// Draws the canvas and handles the user's input.
pub fn canvas(app: &mut App, ui: &mut egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    let (resp, painter) = ui.allocate_painter(rect.size(), Sense::click_and_drag());
    app.canvas_size = (rect.width(), rect.height());
    app.canvas_rect = rect;
    if app.fit_pending {
        app.fit_view(false);
        app.fit_pending = false;
    }
    if app.fit_selection_next {
        app.fit_view(true);
        app.fit_selection_next = false;
    }
    let ctx = ui.ctx().clone();
    app.refresh_caches();
    let view = app.ed.page().view;
    let mut cam = Cam { origin: rect.min + vec2(view.x as f32, view.y as f32), zoom: view.zoom as f32 };

    // ---- zoom / scrolling
    let (scroll, zoom_d, mods, ptr) = ctx.input(|i| (i.raw_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.hover_pos()));
    if let (true, Some(p)) = (resp.hovered(), ptr) {
        let factor = if (zoom_d - 1.0).abs() > 1e-4 {
            zoom_d
        } else if scroll.y != 0.0 && !mods.shift && !mods.alt {
            (scroll.y * 0.0015).exp()
        } else {
            1.0
        };
        if (factor - 1.0).abs() > 1e-4 {
            let nz = (cam.zoom * factor).clamp(0.05, 8.0);
            let k = nz / cam.zoom;
            let o = p - (p - cam.origin) * k;
            app.set_view(View { x: f64::from(o.x - rect.min.x), y: f64::from(o.y - rect.min.y), zoom: f64::from(nz) });
        } else if mods.shift || mods.alt {
            let d = if mods.shift { vec2(scroll.y, 0.0) } else { vec2(0.0, scroll.y) } + vec2(scroll.x, 0.0);
            let v = app.ed.page().view;
            app.set_view(View { x: v.x + f64::from(d.x), y: v.y + f64::from(d.y), zoom: v.zoom });
        }
        let v = app.ed.page().view;
        cam = Cam { origin: rect.min + vec2(v.x as f32, v.y as f32), zoom: v.zoom as f32 };
    }

    // ---- pen pressure: touch events carry the force (0..1) when the hardware provides it
    ctx.input(|i| {
        for e in &i.events {
            if let egui::Event::Touch { phase, force, .. } = e {
                match phase {
                    egui::TouchPhase::Start | egui::TouchPhase::Move => app.pen_force = force.or(app.pen_force),
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => app.pen_force = None,
                }
            }
        }
    });

    // ---- pointer
    let (pressed, down, released, middle, secondary, dbl, pos, alt, shift) = ctx.input(|i| {
        (
            i.pointer.button_pressed(PointerButton::Primary),
            i.pointer.button_down(PointerButton::Primary),
            i.pointer.button_released(PointerButton::Primary),
            i.pointer.button_pressed(PointerButton::Middle),
            i.pointer.button_pressed(PointerButton::Secondary),
            i.pointer.button_double_clicked(PointerButton::Primary),
            i.pointer.interact_pos(),
            i.modifiers.alt,
            i.modifiers.shift,
        )
    });
    let inside = resp.contains_pointer();
    let editing_now = app.editing.is_some();
    if let Some(sp) = pos {
        let w = cam.w(sp);
        if (pressed || middle) && inside && !editing_now && app.drag.is_none() {
            if middle || app.space_down || app.tool == Tool::Hand {
                app.drag = Some(Drag::Pan { from: sp, view: app.ed.page().view });
            } else {
                app.press_tool(&cam, sp, w, shift, alt);
            }
        } else if pressed && editing_now && inside {
            app.commit_editing();
        }
        if secondary && inside {
            app.ctx_pos = w;
        }
        if secondary
            && inside
            && let Some(i) = app.pick_at(w, 6.0 / f64::from(cam.zoom))
        {
            let id = app.ed.page().objects[i].id.clone();
            if !app.sel.contains(&id) {
                app.select_with_groups(vec![id]);
            }
        }
        if dbl && inside && (app.drag.is_none() || matches!(app.drag, Some(Drag::Move { moved: false, .. }) | Some(Drag::Marquee { .. }))) {
            app.drag = None;
            handle_double_click(app, &cam, w);
        }
        if let Some(Drag::Pan { from, view }) = &app.drag {
            let d = sp - *from;
            let v = *view;
            if down || ctx.input(|i| i.pointer.button_down(PointerButton::Middle)) {
                app.set_view(View { x: v.x + f64::from(d.x), y: v.y + f64::from(d.y), zoom: v.zoom });
                cam = Cam { origin: rect.min + vec2((v.x + f64::from(d.x)) as f32, (v.y + f64::from(d.y)) as f32), zoom: cam.zoom };
            }
        }
        if app.drag.is_some() && !matches!(app.drag, Some(Drag::Pan { .. })) && down {
            app.move_drag(&cam, w, shift, alt);
        }
        if released || (!down && !ctx.input(|i| i.pointer.button_down(PointerButton::Middle)) && app.drag.is_some()) {
            if matches!(app.drag, Some(Drag::Pan { .. })) {
                app.drag = None;
            } else {
                app.release(&cam, w, shift);
            }
        }
        // cursor
        if inside && app.drag.is_none() && app.editing.is_none() {
            let icon = match app.tool {
                Tool::Select => match app.hit_handle(&cam, sp) {
                    Some(HandleKind::Resize(h)) => match h {
                        Handle::N | Handle::S => CursorIcon::ResizeVertical,
                        Handle::E | Handle::W => CursorIcon::ResizeHorizontal,
                        Handle::NE | Handle::SW => CursorIcon::ResizeNeSw,
                        _ => CursorIcon::ResizeNwSe,
                    },
                    Some(HandleKind::Rotate) => CursorIcon::Grab,
                    Some(_) => CursorIcon::Crosshair,
                    None => match app.pick_at(w, 6.0 / f64::from(cam.zoom)) {
                        Some(i) if app.sel.contains(&app.ed.page().objects[i].id) => CursorIcon::Move,
                        _ => CursorIcon::Default,
                    },
                },
                Tool::Hand => CursorIcon::Grab,
                Tool::Text => CursorIcon::Text,
                _ => CursorIcon::Crosshair,
            };
            ctx.set_cursor_icon(if app.space_down { CursorIcon::Grab } else { icon });
        } else if matches!(app.drag, Some(Drag::Pan { .. })) {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        }
    }

    // ---- drawing
    let dark = app.ed.doc.background.preset.is_dark();
    let (r, g, b) = app.ed.doc.background.preset.rgb();
    let ghost: Vec<Id> = match &app.drag {
        Some(Drag::Erase { hit, .. }) => hit.clone(),
        _ => Vec::new(),
    };
    let editing_id = app.editing.as_ref().map(|e| e.id.clone());
    {
        let page = app.ed.page();
        let fresh = !app.objects_moving();
        let mut rd = Renderer {
            painter: &painter,
            cam,
            dark,
            doc: &app.ed.doc,
            page,
            measurer: &app.measurer,
            textures: &mut app.textures,
            bg: Color32::from_rgb(r, g, b),
            ids: Some(&app.caches.ids),
            index: if fresh { app.caches.spatial.as_ref() } else { None },
        };
        rd.background(rect);
        rd.page(rect, editing_id.as_ref(), &ghost);
    }
    draw_overlay(app, &painter, &cam, dark);
    laser_trail(app, &painter, &ctx, if resp.contains_pointer() { ptr } else { None });
    crate::rich_edit::draw_text_editor(app, ui, &cam);
    let _ = rect;

    // ---- context menu
    resp.context_menu(|ui| {
        if app.sel.is_empty() {
            crate::chrome::canvas_menu(app, ui);
        } else {
            crate::chrome::selection_menu(app, ui);
        }
    });
}

fn handle_double_click(app: &mut App, cam: &Cam, w: Pt) {
    let tol = 6.0 / f64::from(cam.zoom);
    match app.pick_at(w, tol) {
        Some(i) => {
            let o = app.ed.page().objects[i].clone();
            match &o.kind {
                Kind::Box(_) => {
                    app.sel = vec![o.id.clone()];
                    app.start_editing(&o.id, None);
                }
                Kind::Table(t) => {
                    let cell = t.cell_at(w);
                    app.sel = vec![o.id.clone()];
                    app.start_editing(&o.id, cell);
                }
                Kind::Connector(_) => {
                    app.sel = vec![o.id.clone()];
                    app.start_editing(&o.id, None);
                }
                _ => {}
            }
        }
        None => {
            let t = app.box_object(Tool::Text, Rect::new(w.x, w.y - app.label.size * 0.65, 12.0, app.label.size * 1.3));
            let id = app.ed.insert(t);
            app.sel = vec![id.clone()];
            app.start_editing(&id, None);
            if let Some(e) = &mut app.editing {
                e.fresh = true;
            }
        }
    }
}

fn draw_overlay(app: &mut App, painter: &egui::Painter, cam: &Cam, dark: bool) {
    let accent = Color32::from_rgb(0x5b, 0x7c, 0xfa);
    let page = app.ed.page();
    // draft being created
    if let Some(d) = &app.draft {
        let (r, g, b) = app.ed.doc.background.preset.rgb();
        let mut rd = Renderer {
            painter,
            cam: *cam,
            dark,
            doc: &app.ed.doc,
            page,
            measurer: &app.measurer,
            textures: &mut app.textures,
            bg: Color32::from_rgb(r, g, b),
            ids: None,
            index: None,
        };
        rd.draw_one(d);
    }
    // preview of a connector being created / edited: target aimed at
    let hover_target = match (&app.drag, app.ed.page()) {
        (Some(Drag::Create { tool: Tool::Line | Tool::Arrow, .. }), _) | (Some(Drag::Endpoint { .. }), _) => {
            let p = painter.ctx().input(|i| i.pointer.hover_pos());
            p.and_then(|p| app.ed.page().attach_target(cam.w(p), 4.0 / f64::from(cam.zoom), &[]))
        }
        _ => None,
    };
    if let Some(id) = hover_target
        && let Some(f) = app.ed.page().get(&id).and_then(Object::frame)
    {
        let pts: Vec<Pos2> = f.corners().iter().map(|p| cam.s(*p)).collect();
        painter.add(Shape::closed_line(pts, Stroke::new(2.0_f32, accent)));
    }
    // selection
    let page = app.ed.page();
    for o in page.objects.iter().filter(|o| app.sel.contains(&o.id)) {
        let corners: Vec<Pos2> = match o.frame() {
            Some(f) => f.corners().iter().map(|p| cam.s(*p)).collect(),
            None => {
                let r = cam.rect_s(page.bounds(o));
                vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
            }
        };
        let locked = page.is_locked(o);
        painter.add(Shape::closed_line(corners, Stroke::new(1.5_f32, if locked { Color32::GRAY } else { accent })));
    }
    if app.sel.len() > 1
        && let Some(r) = page.bounds_of(&app.sel)
    {
        let sr = cam.rect_s(r);
        painter.add(Shape::dashed_line(
            &[sr.left_top(), sr.right_top(), sr.right_bottom(), sr.left_bottom(), sr.left_top()],
            Stroke::new(1.0_f32, accent),
            5.0,
            4.0,
        ));
    }
    for (kind, p) in app.handles(cam) {
        if matches!(kind, HandleKind::Via(_)) {
            painter.circle(p, 5.0, accent, Stroke::new(1.5_f32, Color32::WHITE));
        } else if matches!(kind, HandleKind::Mid(_)) {
            painter.circle(p, 4.0, accent.gamma_multiply(0.25), Stroke::new(1.0_f32, accent));
        } else if matches!(kind, HandleKind::Rotate | HandleKind::Start | HandleKind::End) {
            painter.circle(p, 5.5, Color32::WHITE, Stroke::new(1.5_f32, accent));
        } else {
            painter.rect(egui::Rect::from_center_size(p, vec2(9.0, 9.0)), 1.0, Color32::WHITE, Stroke::new(1.5_f32, accent), egui::StrokeKind::Middle);
        }
    }
    if let Some(Drag::Marquee { start, .. }) = &app.drag
        && let Some(p) = painter.ctx().input(|i| i.pointer.hover_pos())
    {
        let r = egui::Rect::from_two_pos(cam.s(*start), p);
        painter.rect(r, 0.0, accent.gamma_multiply(0.12), Stroke::new(1.0_f32, accent), egui::StrokeKind::Middle);
    }
    // stroke being drawn
    if let Some(Drag::Draw { pts, pressures, has_force }) = &app.drag {
        let hl = app.tool == Tool::Highlighter;
        let style = Style { fill: None, dash: Dash::Solid, ..app.style.clone() };
        let o = if *has_force && !hl && pts.len() >= 2 {
            Object::new_pen_with_pressure(pts.clone(), pressures.clone(), false, style)
        } else {
            Object::new_pen(pts.clone(), hl, style)
        };
        let (r, g, b) = app.ed.doc.background.preset.rgb();
        let page = app.ed.page();
        let mut rd = Renderer {
            painter,
            cam: *cam,
            dark,
            doc: &app.ed.doc,
            page,
            measurer: &app.measurer,
            textures: &mut app.textures,
            bg: Color32::from_rgb(r, g, b),
            ids: None,
            index: None,
        };
        rd.draw_one(&Object {
            kind: if pts.len() < 2 {
                Kind::Pen(PenObj { pts: std::sync::Arc::new(vec![pts[0], pts[0]]), highlighter: hl, pressures: None })
            } else {
                o.kind.clone()
            },
            ..o
        });
    }
}

/// Lifetime of a point of the laser pointer trail.
const LASER_LIFE: std::time::Duration = std::time::Duration::from_millis(900);

/// Records the pointer position (laser tool) and draws the fading trail.
fn laser_trail(app: &mut App, painter: &egui::Painter, ctx: &egui::Context, pointer: Option<Pos2>) {
    let now = std::time::Instant::now();
    app.laser.retain(|(_, t)| now.duration_since(*t) < LASER_LIFE);
    if app.tool == Tool::Laser
        && let Some(p) = pointer
        && app.laser.last().is_none_or(|(l, _)| l.distance(p) > 1.0)
    {
        app.laser.push((p, now));
    }
    if app.laser.is_empty() {
        return;
    }
    let red = Color32::from_rgb(0xff, 0x2d, 0x2d);
    let alpha = |t: std::time::Instant| 1.0 - now.duration_since(t).as_secs_f32() / LASER_LIFE.as_secs_f32();
    for w in app.laser.windows(2) {
        let a = alpha(w[1].1).clamp(0.0, 1.0);
        painter.line_segment([w[0].0, w[1].0], Stroke::new(3.0 + 5.0 * a, red.gamma_multiply(a * 0.45)));
        painter.line_segment([w[0].0, w[1].0], Stroke::new(2.5_f32, red.gamma_multiply(a)));
    }
    if let Some((p, t)) = app.laser.last() {
        let a = alpha(*t).clamp(0.0, 1.0);
        painter.circle_filled(*p, 9.0, red.gamma_multiply(a * 0.3));
        painter.circle_filled(*p, 4.5, red.gamma_multiply(a));
    }
    ctx.request_repaint();
}
