//! Index spatial par grille : retrouver rapidement les objets d'une zone (affichage, clic, lasso).
//!
//! L'index est une photo de la page à un instant donné : à reconstruire quand les objets changent
//! (l'application le fait à chaque nouvelle révision du document).

use crate::geom::{Pt, Rect};
use crate::model::{Id, Object, Page};
use std::collections::HashMap;

/// Au-delà de ce nombre de cellules, un objet est rangé dans la liste des « grands » objets, toujours testés.
const MAX_CELLS_PER_OBJECT: i64 = 64;

#[derive(Debug, Default)]
pub struct SpatialIndex {
    cell: f64,
    cells: HashMap<(i32, i32), Vec<u32>>,
    large: Vec<u32>,
    bounds: Vec<Rect>,
}

/// Table identifiant → position dans `page.objects` (la recherche directe est linéaire).
pub fn id_index(page: &Page) -> HashMap<Id, usize> {
    page.objects.iter().enumerate().map(|(i, o)| (o.id.clone(), i)).collect()
}

impl SpatialIndex {
    /// Construit l'index d'une page. `ids` doit correspondre à `page` (voir [`id_index`]).
    pub fn build(page: &Page, ids: &HashMap<Id, usize>) -> Self {
        let get = |id: &Id| ids.get(id).map(|&i| &page.objects[i]);
        let bounds: Vec<Rect> = page.objects.iter().map(|o| page.bounds_with(o, &get)).collect();
        let mut sizes: Vec<f64> = bounds.iter().map(|b| b.w.max(b.h)).filter(|s| s.is_finite()).collect();
        let cell = if sizes.is_empty() {
            256.0
        } else {
            let mid = sizes.len() / 2;
            let (_, m, _) = sizes.select_nth_unstable_by(mid, f64::total_cmp);
            (*m * 2.0).clamp(64.0, 2048.0)
        };
        let mut idx = SpatialIndex { cell, cells: HashMap::new(), large: Vec::new(), bounds };
        for i in 0..idx.bounds.len() {
            let ((x0, y0), (x1, y1)) = (idx.cell_of(idx.bounds[i].x, idx.bounds[i].y), idx.cell_of(idx.bounds[i].right(), idx.bounds[i].bottom()));
            if (i64::from(x1) - i64::from(x0) + 1) * (i64::from(y1) - i64::from(y0) + 1) > MAX_CELLS_PER_OBJECT {
                idx.large.push(i as u32);
                continue;
            }
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    idx.cells.entry((cx, cy)).or_default().push(i as u32);
                }
            }
        }
        idx
    }

    fn cell_of(&self, x: f64, y: f64) -> (i32, i32) {
        ((x / self.cell).floor().clamp(-1.0e9, 1.0e9) as i32, (y / self.cell).floor().clamp(-1.0e9, 1.0e9) as i32)
    }

    /// Boîte englobante en cache de l'objet `i`.
    pub fn bounds(&self, i: usize) -> Rect {
        self.bounds[i]
    }

    pub fn len(&self) -> usize {
        self.bounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bounds.is_empty()
    }

    /// Indices (croissants, donc dans l'ordre de dessin) des objets dont la boîte touche `r`.
    pub fn query(&self, r: Rect) -> Vec<usize> {
        let ((x0, y0), (x1, y1)) = (self.cell_of(r.x, r.y), self.cell_of(r.right(), r.bottom()));
        let mut out: Vec<u32> = self.large.clone();
        let span = (i64::from(x1) - i64::from(x0) + 1) * (i64::from(y1) - i64::from(y0) + 1);
        if span as usize > self.cells.len() {
            // zone plus vaste que le nombre de cellules occupées : on parcourt les cellules existantes
            for (&(cx, cy), v) in &self.cells {
                if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
                    out.extend_from_slice(v);
                }
            }
        } else {
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    if let Some(v) = self.cells.get(&(cx, cy)) {
                        out.extend_from_slice(v);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out.into_iter().map(|i| i as usize).filter(|&i| self.bounds[i].intersects(&r)).collect()
    }

    /// Objets dont la boîte contient ou touche le point `p` à `tol` près.
    pub fn query_point(&self, p: Pt, tol: f64) -> Vec<usize> {
        self.query(Rect::new(p.x - tol, p.y - tol, 2.0 * tol, 2.0 * tol))
    }
}

/// Fournit la recherche d'objet par identifiant pour les fonctions `*_with`.
pub fn getter<'a>(page: &'a Page, ids: &'a HashMap<Id, usize>) -> impl Fn(&Id) -> Option<&'a Object> {
    move |id| ids.get(id).map(|&i| &page.objects[i])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::{Connector, End};
    use crate::geom::Frame;
    use crate::model::{BoxGeom, Kind, Label};
    use crate::style::{Color, Style};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Object {
        Object::new_box(BoxGeom::Rect, Frame::new(x, y, w, h), Style { fill: Some(Color::rgb(1, 2, 3)), ..Style::default() }, Label::default())
    }

    fn grid_page(n: usize) -> Page {
        let mut p = Page::new("t");
        let cols = (n as f64).sqrt().ceil() as usize;
        for i in 0..n {
            p.objects.push(rect((i % cols) as f64 * 200.0, (i / cols) as f64 * 200.0, 100.0, 80.0));
        }
        p
    }

    #[test]
    fn query_matches_a_linear_scan_and_keeps_z_order() {
        let p = grid_page(900);
        let ids = id_index(&p);
        let idx = SpatialIndex::build(&p, &ids);
        for r in
            [Rect::new(0.0, 0.0, 450.0, 450.0), Rect::new(2900.0, 3100.0, 700.0, 300.0), Rect::new(-500.0, -500.0, 50.0, 50.0), Rect::new(-1e6, -1e6, 2e6, 2e6)]
        {
            let want: Vec<usize> = (0..p.objects.len()).filter(|&i| p.bounds(&p.objects[i]).intersects(&r)).collect();
            assert_eq!(idx.query(r), want, "{r:?}");
        }
    }

    #[test]
    fn large_objects_are_always_found() {
        let mut p = grid_page(100);
        p.objects.push(rect(-5000.0, -5000.0, 100_000.0, 100_000.0));
        let ids = id_index(&p);
        let idx = SpatialIndex::build(&p, &ids);
        let hit = idx.query_point(Pt::new(1234.0, 777.0), 1.0);
        assert!(hit.contains(&100), "{hit:?}");
    }

    #[test]
    fn connectors_are_indexed_with_their_resolved_path() {
        let mut p = Page::new("t");
        let a = rect(0.0, 0.0, 50.0, 50.0);
        let b = rect(1000.0, 0.0, 50.0, 50.0);
        let (ia, ib) = (a.id.clone(), b.id.clone());
        p.objects.extend([a, b]);
        p.objects.push(Object::new(Kind::Connector(Connector::new(End::bound(ia), End::bound(ib))), Style::default()));
        let ids = id_index(&p);
        let idx = SpatialIndex::build(&p, &ids);
        assert!(idx.query_point(Pt::new(500.0, 25.0), 1.0).contains(&2), "la flèche traverse le milieu");
        assert!(!idx.query_point(Pt::new(500.0, 500.0), 1.0).contains(&2));
    }

    #[test]
    fn empty_page_and_degenerate_boxes() {
        let p = Page::new("t");
        let idx = SpatialIndex::build(&p, &id_index(&p));
        assert!(idx.is_empty() && idx.query(Rect::new(0.0, 0.0, 10.0, 10.0)).is_empty());
        let mut p = Page::new("t");
        p.objects.push(rect(10.0, 10.0, 0.0, 0.0));
        let idx = SpatialIndex::build(&p, &id_index(&p));
        assert_eq!(idx.query_point(Pt::new(10.0, 10.0), 0.5), vec![0]);
    }

    #[test]
    fn pick_with_candidates_equals_pick() {
        let p = grid_page(400);
        let ids = id_index(&p);
        let idx = SpatialIndex::build(&p, &ids);
        let get = getter(&p, &ids);
        for pt in [Pt::new(50.0, 40.0), Pt::new(2050.0, 1640.0), Pt::new(150.0, 150.0), Pt::new(-30.0, -30.0)] {
            let cands = idx.query_point(pt, 6.0);
            assert_eq!(p.pick_with(pt, 6.0, &get, Some(&cands)), p.pick(pt, 6.0), "{pt:?}");
        }
    }

    #[test]
    fn marquee_with_index_equals_linear_marquee() {
        let p = grid_page(400);
        let ids = id_index(&p);
        let idx = SpatialIndex::build(&p, &ids);
        for (r, contain) in [(Rect::new(-10.0, -10.0, 650.0, 450.0), true), (Rect::new(150.0, 150.0, 500.0, 500.0), false)] {
            let mut want = p.objects_in_rect(r, contain);
            let mut got = p.objects_in_rect_idx(r, contain, &idx);
            want.sort();
            got.sort();
            assert_eq!(got, want);
        }
    }
}
