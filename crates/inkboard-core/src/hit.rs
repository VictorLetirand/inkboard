//! Bounding boxes, click and marquee selection.

use crate::geom::{Pt, Rect, in_poly, seg_dist};
use crate::model::{BoxGeom, Id, Kind, Object, Outline, Page};

impl Page {
    /// Bounding box (stroke width included) in world coordinates.
    #[must_use]
    pub fn bounds(&self, o: &Object) -> Rect {
        self.bounds_with(o, &|id| self.get(id))
    }

    /// Like [`Page::bounds`], with a supplied object lookup (see [`Page::index`]).
    pub fn bounds_with<'a>(&self, o: &Object, get: &dyn Fn(&Id) -> Option<&'a Object>) -> Rect {
        let m = o.style.width / 2.0;
        match &o.kind {
            Kind::Box(b) => b.frame.aabb().expand(if b.geom == BoxGeom::Text { 0.0 } else { m }),
            Kind::Image(i) => i.frame.aabb(),
            Kind::Table(t) => t.rect().expand(m),
            Kind::Pen(p) => Rect::from_points(p.pts.iter().copied()).unwrap_or_default().expand(m + p.head_at_end(o.style.width).map_or(0.0, |h| h.2)),
            Kind::Connector(c) => self
                .connector_path_with(c, get)
                .and_then(Rect::from_points)
                .unwrap_or_else(|| c.via.first().map(|p| Rect::new(p.x, p.y, 0.0, 0.0)).unwrap_or_default())
                .expand(m + 6.0),
        }
    }

    /// Bounding box of several objects; `None` when the list is empty.
    #[must_use]
    pub fn bounds_of(&self, ids: &[Id]) -> Option<Rect> {
        self.objects.iter().filter(|o| ids.contains(&o.id)).map(|o| self.bounds(o)).reduce(|a, b| a.union(&b))
    }

    /// Bounding box of all the visible content.
    #[must_use]
    pub fn content_bounds(&self) -> Option<Rect> {
        self.objects.iter().filter(|o| self.is_visible(o)).map(|o| self.bounds(o)).reduce(|a, b| a.union(&b))
    }

    /// Does point `p` touch the object (with a tolerance `tol` in world units)?
    #[must_use]
    pub fn hit(&self, o: &Object, p: Pt, tol: f64) -> bool {
        self.hit_with(o, p, tol, &|id| self.get(id))
    }

    /// Like [`Page::hit`], with a supplied object lookup (see [`crate::spatial::getter`]).
    pub fn hit_with<'a>(&self, o: &Object, p: Pt, tol: f64, get: &dyn Fn(&Id) -> Option<&'a Object>) -> bool {
        let reach = tol + o.style.width / 2.0;
        match &o.kind {
            Kind::Pen(pen) => {
                let pts = &pen.pts;
                match pts.len() {
                    0 => false,
                    1 => pts[0].dist(p) <= reach,
                    _ => pts.windows(2).any(|w| seg_dist(p, w[0], w[1]) <= reach),
                }
            }
            Kind::Connector(c) => self.connector_path_with(c, get).is_some_and(|path| path.windows(2).any(|w| seg_dist(p, w[0], w[1]) <= reach)),
            Kind::Table(tb) => tb.rect().expand(tol).contains(p),
            Kind::Image(i) => {
                let local = i.frame.to_local(p);
                local.x.abs() <= i.frame.w / 2.0 + tol && local.y.abs() <= i.frame.h / 2.0 + tol
            }
            Kind::Box(b) => {
                let (width, height) = (b.frame.w, b.frame.h);
                let l = b.frame.to_local(p);
                let (hw, hh) = (width / 2.0, height / 2.0);
                match b.geom {
                    BoxGeom::Text | BoxGeom::Sticky => l.x.abs() <= hw + tol && l.y.abs() <= hh + tol,
                    geom => {
                        let solid = o.style.fill.is_some() || !b.label.text.is_empty();
                        match geom.outline() {
                            Outline::Ellipse => {
                                let inside = |rx: f64, ry: f64| rx > 0.0 && ry > 0.0 && (l.x / rx).powi(2) + (l.y / ry).powi(2) <= 1.0;
                                inside(hw + reach, hh + reach) && (solid || !inside(hw - reach, hh - reach))
                            }
                            outline => {
                                let poly = outline.polygon(width, height);
                                (solid && in_poly(&poly, l)) || (0..poly.len()).any(|i| seg_dist(l, poly[i], poly[(i + 1) % poly.len()]) <= reach)
                            }
                        }
                    }
                }
            }
        }
    }

    /// Topmost object under the point, ignoring hidden or locked layers.
    #[must_use]
    pub fn pick(&self, p: Pt, tol: f64) -> Option<usize> {
        self.pick_with(p, tol, &|id| self.get(id), None)
    }

    /// Like [`Page::pick`]; `candidates` (increasing indices, e.g. from a [`crate::spatial::SpatialIndex`])
    /// limits the search, otherwise all objects are examined.
    pub fn pick_with<'a>(&self, p: Pt, tol: f64, get: &dyn Fn(&Id) -> Option<&'a Object>, candidates: Option<&[usize]>) -> Option<usize> {
        let ok = |i: usize| {
            let o = &self.objects[i];
            self.is_visible(o) && self.layer(&o.layer).is_none_or(|l| !l.locked) && self.hit_with(o, p, tol, get)
        };
        match candidates {
            Some(c) => c.iter().rev().copied().find(|&i| ok(i)),
            None => (0..self.objects.len()).rev().find(|&i| ok(i)),
        }
    }

    /// Object a connector can attach to under `p` (inside or edge, fill irrelevant).
    #[must_use]
    pub fn attach_target(&self, p: Pt, tol: f64, exclude: &[Id]) -> Option<Id> {
        self.objects.iter().rev().find_map(|o| {
            if exclude.contains(&o.id) || !self.is_visible(o) || self.layer(&o.layer).is_some_and(|l| l.locked) {
                return None;
            }
            let f = o.frame()?;
            let l = f.to_local(p);
            let (hw, hh) = (f.w / 2.0 + tol, f.h / 2.0 + tol);
            let inside = match &o.kind {
                Kind::Box(b) => match b.geom.outline() {
                    Outline::Ellipse => (l.x / hw).powi(2) + (l.y / hh).powi(2) <= 1.0,
                    outline => {
                        let poly = outline.polygon(f.w, f.h);
                        in_poly(&poly, l) || (0..poly.len()).any(|i| seg_dist(l, poly[i], poly[(i + 1) % poly.len()]) <= tol)
                    }
                },
                _ => l.x.abs() <= hw && l.y.abs() <= hh,
            };
            inside.then(|| o.id.clone())
        })
    }

    /// Like [`Page::objects_in_rect`], relying on an up-to-date spatial index.
    #[must_use]
    pub fn objects_in_rect_idx(&self, r: Rect, contain: bool, idx: &crate::spatial::SpatialIndex) -> Vec<Id> {
        idx.query(r)
            .into_iter()
            .filter(|&i| {
                let o = &self.objects[i];
                self.is_visible(o) && self.layer(&o.layer).is_none_or(|l| !l.locked) && (!contain || r.contains_rect(&idx.bounds(i)))
            })
            .map(|i| self.objects[i].id.clone())
            .collect()
    }

    /// Objects selectable by a marquee: entirely contained (`contain`) or merely touched.
    #[must_use]
    pub fn objects_in_rect(&self, r: Rect, contain: bool) -> Vec<Id> {
        self.objects
            .iter()
            .filter(|o| self.is_visible(o) && self.layer(&o.layer).is_none_or(|l| !l.locked))
            .filter(|o| {
                let b = self.bounds(o);
                if contain { r.contains_rect(&b) } else { r.intersects(&b) }
            })
            .map(|o| o.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::{Connector, End};
    use crate::geom::Frame;
    use crate::model::Label;
    use crate::style::{Color, Style};

    fn rect(x: f64, y: f64, w: f64, h: f64, filled: bool) -> Object {
        let style = Style { fill: filled.then_some(Color::rgb(200, 200, 255)), ..Style::default() };
        Object::new_box(BoxGeom::Rect, Frame::new(x, y, w, h), style, Label::default())
    }

    #[test]
    fn unfilled_shape_only_hits_on_border() {
        let o = rect(0.0, 0.0, 100.0, 100.0, false);
        let p = Page::new("t");
        assert!(!p.hit(&o, Pt::new(50.0, 50.0), 2.0));
        assert!(p.hit(&o, Pt::new(0.0, 50.0), 2.0));
    }

    #[test]
    fn filled_shape_hits_inside() {
        let o = rect(0.0, 0.0, 100.0, 100.0, true);
        assert!(Page::new("t").hit(&o, Pt::new(50.0, 50.0), 2.0));
    }

    #[test]
    fn pick_returns_topmost() {
        let mut p = Page::new("t");
        p.objects.push(rect(0.0, 0.0, 100.0, 100.0, true));
        p.objects.push(rect(50.0, 50.0, 100.0, 100.0, true));
        assert_eq!(p.pick(Pt::new(60.0, 60.0), 1.0), Some(1));
        assert_eq!(p.pick(Pt::new(10.0, 10.0), 1.0), Some(0));
        assert_eq!(p.pick(Pt::new(500.0, 500.0), 1.0), None);
    }

    #[test]
    fn hidden_and_locked_layers_are_not_pickable() {
        let mut p = Page::new("t");
        p.objects.push(rect(0.0, 0.0, 100.0, 100.0, true));
        p.layers[0].locked = true;
        assert_eq!(p.pick(Pt::new(10.0, 10.0), 1.0), None);
        p.layers[0].locked = false;
        p.layers[0].visible = false;
        assert_eq!(p.pick(Pt::new(10.0, 10.0), 1.0), None);
    }

    #[test]
    fn marquee_contain_vs_touch() {
        let mut p = Page::new("t");
        p.objects.push(rect(0.0, 0.0, 50.0, 50.0, true));
        p.objects.push(rect(40.0, 40.0, 100.0, 100.0, true));
        let r = Rect::new(-10.0, -10.0, 70.0, 70.0);
        assert_eq!(p.objects_in_rect(r, true).len(), 1);
        assert_eq!(p.objects_in_rect(r, false).len(), 2);
    }

    #[test]
    fn connector_hit_and_bounds_follow_targets() {
        let mut p = Page::new("t");
        let a = rect(0.0, 0.0, 100.0, 50.0, true);
        let b = rect(300.0, 0.0, 100.0, 50.0, true);
        let (ia, ib) = (a.id.clone(), b.id.clone());
        p.objects.extend([a, b]);
        let c = Object::new(Kind::Connector(Connector::new(End::bound(ia), End::bound(ib))), Style::default());
        assert!(p.hit(&c, Pt::new(200.0, 25.0), 3.0));
        assert!(!p.hit(&c, Pt::new(200.0, 80.0), 3.0));
        let bb = p.bounds(&c);
        assert!(bb.x <= 100.0 && bb.right() >= 300.0);
    }

    #[test]
    fn rotated_box_hit_uses_local_frame() {
        let mut o = rect(0.0, 0.0, 200.0, 20.0, true);
        if let Kind::Box(b) = &mut o.kind {
            b.frame.angle = std::f64::consts::FRAC_PI_2;
        }
        let p = Page::new("t");
        // once rotated by 90°, the bar becomes vertical around (100, 10)
        assert!(p.hit(&o, Pt::new(100.0, 90.0), 1.0));
        assert!(!p.hit(&o, Pt::new(190.0, 10.0), 1.0));
    }
}

#[cfg(test)]
mod attach_tests {
    use super::*;
    use crate::geom::Frame;
    use crate::model::Label;
    use crate::style::Style;

    #[test]
    fn attach_target_ignores_fill_and_respects_exclusion() {
        let mut p = Page::new("t");
        let o = Object::new_box(BoxGeom::Ellipse, Frame::new(0.0, 0.0, 100.0, 100.0), Style::default(), Label::default());
        let id = o.id.clone();
        p.objects.push(o);
        assert_eq!(p.attach_target(Pt::new(50.0, 50.0), 2.0, &[]), Some(id.clone()));
        assert_eq!(p.attach_target(Pt::new(2.0, 2.0), 0.0, &[]), None, "corner outside the ellipse");
        assert_eq!(p.attach_target(Pt::new(50.0, 50.0), 2.0, &[id]), None);
    }
}
