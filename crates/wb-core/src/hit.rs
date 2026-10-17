//! Boîtes englobantes, sélection au clic et au lasso.

use crate::geom::{Pt, Rect, in_poly, seg_dist};
use crate::model::{BoxGeom, Id, Kind, Object, Outline, Page};

impl Page {
    /// Boîte englobante (épaisseur du trait comprise) dans le repère monde.
    pub fn bounds(&self, o: &Object) -> Rect {
        self.bounds_with(o, &|id| self.get(id))
    }

    /// Comme [`Page::bounds`], avec une recherche d'objet fournie (voir [`Page::index`]).
    pub fn bounds_with<'a>(&self, o: &Object, get: &dyn Fn(&Id) -> Option<&'a Object>) -> Rect {
        let m = o.style.width / 2.0;
        match &o.kind {
            Kind::Box(b) => b.frame.aabb().expand(if b.geom == BoxGeom::Text { 0.0 } else { m }),
            Kind::Image(i) => i.frame.aabb(),
            Kind::Table(t) => t.rect().expand(m),
            Kind::Pen(p) => Rect::from_points(p.pts.iter().copied()).unwrap_or_default().expand(m),
            Kind::Connector(c) => self
                .connector_path_with(c, get)
                .and_then(Rect::from_points)
                .unwrap_or_else(|| c.via.first().map(|p| Rect::new(p.x, p.y, 0.0, 0.0)).unwrap_or_default())
                .expand(m + 6.0),
        }
    }

    /// Boîte englobante de plusieurs objets ; `None` si la liste est vide.
    pub fn bounds_of(&self, ids: &[Id]) -> Option<Rect> {
        self.objects.iter().filter(|o| ids.contains(&o.id)).map(|o| self.bounds(o)).reduce(|a, b| a.union(&b))
    }

    /// Boîte englobante de tout le contenu visible.
    pub fn content_bounds(&self) -> Option<Rect> {
        self.objects.iter().filter(|o| self.is_visible(o)).map(|o| self.bounds(o)).reduce(|a, b| a.union(&b))
    }

    /// Le point `p` touche-t-il l'objet (avec une tolérance `tol` en unités monde) ?
    pub fn hit(&self, o: &Object, p: Pt, tol: f64) -> bool {
        let t = tol + o.style.width / 2.0;
        match &o.kind {
            Kind::Pen(pen) => {
                let pts = &pen.pts;
                match pts.len() {
                    0 => false,
                    1 => pts[0].dist(p) <= t,
                    _ => pts.windows(2).any(|w| seg_dist(p, w[0], w[1]) <= t),
                }
            }
            Kind::Connector(c) => self.connector_path(c).is_some_and(|path| path.windows(2).any(|w| seg_dist(p, w[0], w[1]) <= t)),
            Kind::Table(tb) => tb.rect().expand(tol).contains(p),
            Kind::Image(i) => {
                let l = i.frame.to_local(p);
                l.x.abs() <= i.frame.w / 2.0 + tol && l.y.abs() <= i.frame.h / 2.0 + tol
            }
            Kind::Box(b) => {
                let (w, h) = (b.frame.w, b.frame.h);
                let l = b.frame.to_local(p);
                let (hw, hh) = (w / 2.0, h / 2.0);
                match b.geom {
                    BoxGeom::Text | BoxGeom::Sticky => l.x.abs() <= hw + tol && l.y.abs() <= hh + tol,
                    geom => {
                        let solid = o.style.fill.is_some() || !b.label.text.is_empty();
                        match geom.outline() {
                            Outline::Ellipse => {
                                let inside = |rx: f64, ry: f64| rx > 0.0 && ry > 0.0 && (l.x / rx).powi(2) + (l.y / ry).powi(2) <= 1.0;
                                inside(hw + t, hh + t) && (solid || !inside(hw - t, hh - t))
                            }
                            outline => {
                                let poly = outline.polygon(w, h);
                                (solid && in_poly(&poly, l)) || (0..poly.len()).any(|i| seg_dist(l, poly[i], poly[(i + 1) % poly.len()]) <= t)
                            }
                        }
                    }
                }
            }
        }
    }

    /// Objet le plus au premier plan sous le point, en ignorant calques masqués ou verrouillés.
    pub fn pick(&self, p: Pt, tol: f64) -> Option<usize> {
        (0..self.objects.len()).rev().find(|&i| {
            let o = &self.objects[i];
            self.is_visible(o) && self.layer(&o.layer).is_none_or(|l| !l.locked) && self.hit(o, p, tol)
        })
    }

    /// Objet auquel un connecteur peut s'accrocher sous `p` (intérieur ou bord, remplissage indifférent).
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

    /// Objets sélectionnables par un lasso : entièrement contenus (`contain`) ou simplement touchés.
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
        // une fois tournée de 90°, la barre devient verticale autour de (100, 10)
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
        assert_eq!(p.attach_target(Pt::new(2.0, 2.0), 0.0, &[]), None, "coin hors de l'ellipse");
        assert_eq!(p.attach_target(Pt::new(50.0, 50.0), 2.0, &[id]), None);
    }
}
