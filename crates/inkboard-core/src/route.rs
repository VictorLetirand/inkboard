//! Orthogonal routing around obstacles (A* search on the grid formed by the obstacle edges).

use crate::geom::{Pt, Rect};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Cost of a change of direction: favors paths with few bends.
const BEND_PENALTY: f64 = 40.0;
/// Maximum number of nodes explored before giving up (the fallback routing then takes over).
const MAX_NODES: usize = 60_000;

fn dedup_sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    v
}

/// Does the horizontal or vertical segment `[p, q]` cross the inside of `r`?
fn segment_hits(p: Pt, q: Pt, r: &Rect) -> bool {
    const E: f64 = 1e-6;
    let (x0, x1, y0, y1) = (p.x.min(q.x), p.x.max(q.x), p.y.min(q.y), p.y.max(q.y));
    x1 > r.x + E && x0 < r.right() - E && y1 > r.y + E && y0 < r.bottom() - E
}

/// True when one of the path's segments crosses the inside of an obstacle (oblique segments are tested by sampling).
#[must_use]
pub fn crosses(path: &[Pt], obstacles: &[Rect]) -> bool {
    path.windows(2).any(|w| {
        let axis_aligned = (w[0].x - w[1].x).abs() < 1e-9 || (w[0].y - w[1].y).abs() < 1e-9;
        obstacles.iter().any(|r| {
            if axis_aligned {
                segment_hits(w[0], w[1], r)
            } else {
                (0..=32).any(|i| {
                    let p = w[0].lerp(w[1], f64::from(i) / 32.0);
                    p.x > r.x + 1e-6 && p.x < r.right() - 1e-6 && p.y > r.y + 1e-6 && p.y < r.bottom() - 1e-6
                })
            }
        })
    })
}

#[derive(Clone, Copy, PartialEq)]
struct State {
    cost: f64,
    est: f64,
    ix: usize,
    iy: usize,
    horizontal: Option<bool>,
}
impl Eq for State {}
impl Ord for State {
    fn cmp(&self, o: &Self) -> Ordering {
        (o.cost + o.est).total_cmp(&(self.cost + self.est)) // tas minimal
    }
}
impl PartialOrd for State {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Looks for an orthogonal path from `a` to `b` avoiding the `obstacles` (grown by `margin`).
/// `start_horizontal`: imposed initial direction (leaving a side of a shape).
/// Returns the path corners, or `None` when no path exists (e.g. enclosed start).
#[must_use]
pub fn route_around(a: Pt, b: Pt, start_horizontal: Option<bool>, obstacles: &[Rect], margin: f64) -> Option<Vec<Pt>> {
    type Key = (usize, usize, u8);
    let obs: Vec<Rect> = obstacles.iter().map(|r| r.expand(margin)).collect();
    let mut xs = vec![a.x, b.x];
    let mut ys = vec![a.y, b.y];
    for r in &obs {
        xs.extend([r.x, r.right()]);
        ys.extend([r.y, r.bottom()]);
    }
    let (xs, ys) = (dedup_sorted(xs), dedup_sorted(ys));
    let idx = |v: &[f64], t: f64| v.iter().position(|c| (*c - t).abs() < 1e-6);
    let (sx, sy, tx, ty) = (idx(&xs, a.x)?, idx(&ys, a.y)?, idx(&xs, b.x)?, idx(&ys, b.y)?);
    let node = |ix: usize, iy: usize| Pt::new(xs[ix], ys[iy]);
    let inside = |p: Pt| obs.iter().any(|r| p.x > r.x + 1e-6 && p.x < r.right() - 1e-6 && p.y > r.y + 1e-6 && p.y < r.bottom() - 1e-6);
    if inside(node(sx, sy)) || inside(node(tx, ty)) {
        return None;
    }
    let free = |p: Pt, q: Pt| !obs.iter().any(|r| segment_hits(p, q, r));
    let dir_key = |h: Option<bool>| match h {
        None => 0u8,
        Some(true) => 1,
        Some(false) => 2,
    };
    let mut best: HashMap<Key, f64> = HashMap::new();
    let mut prev: HashMap<Key, Key> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let est = |ix: usize, iy: usize| (xs[ix] - b.x).abs() + (ys[iy] - b.y).abs();
    heap.push(State { cost: 0.0, est: est(sx, sy), ix: sx, iy: sy, horizontal: start_horizontal });
    best.insert((sx, sy, dir_key(start_horizontal)), 0.0);
    let mut explored = 0;
    let mut goal: Option<Key> = None;
    while let Some(s) = heap.pop() {
        let k = (s.ix, s.iy, dir_key(s.horizontal));
        if best.get(&k).is_some_and(|c| *c < s.cost - 1e-9) {
            continue;
        }
        if s.ix == tx && s.iy == ty {
            goal = Some(k);
            break;
        }
        explored += 1;
        if explored > MAX_NODES {
            return None;
        }
        let here = node(s.ix, s.iy);
        let moves: [(isize, isize); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        for (dx, dy) in moves {
            let (Some(nx), Some(ny)) = (s.ix.checked_add_signed(dx), s.iy.checked_add_signed(dy)) else { continue };
            if nx >= xs.len() || ny >= ys.len() {
                continue;
            }
            let there = node(nx, ny);
            if !free(here, there) || inside(there) {
                continue;
            }
            let horizontal = dy == 0;
            let bend = if s.horizontal.is_some_and(|h| h != horizontal) { BEND_PENALTY } else { 0.0 };
            let cost = s.cost + here.dist(there) + bend;
            let nk = (nx, ny, dir_key(Some(horizontal)));
            if best.get(&nk).is_none_or(|c| cost < *c - 1e-9) {
                best.insert(nk, cost);
                prev.insert(nk, k);
                heap.push(State { cost, est: est(nx, ny), ix: nx, iy: ny, horizontal: Some(horizontal) });
            }
        }
    }
    let mut cur = goal?;
    let mut pts = vec![node(cur.0, cur.1)];
    while let Some(p) = prev.get(&cur) {
        cur = *p;
        pts.push(node(cur.0, cur.1));
    }
    pts.reverse();
    // remove aligned points and duplicates
    let mut out: Vec<Pt> = Vec::new();
    for p in pts {
        if out.last().is_some_and(|l| l.dist(p) < 1e-6) {
            continue;
        }
        while out.len() >= 2 {
            let (u, v) = (out[out.len() - 2], out[out.len() - 1]);
            if ((v - u).x * (p - v).y - (v - u).y * (p - v).x).abs() < 1e-6 {
                out.pop();
            } else {
                break;
            }
        }
        out.push(p);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bends(path: &[Pt]) -> usize {
        path.len().saturating_sub(2)
    }

    #[test]
    fn straight_when_nothing_is_in_the_way() {
        let p = route_around(Pt::new(0.0, 0.0), Pt::new(300.0, 0.0), Some(true), &[Rect::new(100.0, 200.0, 50.0, 50.0)], 10.0).unwrap();
        assert_eq!(p, vec![Pt::new(0.0, 0.0), Pt::new(300.0, 0.0)]);
    }

    #[test]
    fn goes_around_a_blocking_rectangle_with_few_bends() {
        let obstacle = Rect::new(100.0, -40.0, 100.0, 80.0);
        let p = route_around(Pt::new(0.0, 0.0), Pt::new(300.0, 0.0), Some(true), &[obstacle], 10.0).unwrap();
        assert_eq!(p[0], Pt::new(0.0, 0.0));
        assert_eq!(*p.last().unwrap(), Pt::new(300.0, 0.0));
        assert!(!crosses(&p, &[obstacle]), "{p:?}");
        assert!(!crosses(&p, &[obstacle.expand(9.0)]), "margin respected: {p:?}");
        assert!(bends(&p) <= 4, "{p:?}");
        for w in p.windows(2) {
            assert!((w[0].x - w[1].x).abs() < 1e-9 || (w[0].y - w[1].y).abs() < 1e-9, "oblique segment");
        }
    }

    #[test]
    fn navigates_a_maze_of_obstacles() {
        let walls = [Rect::new(100.0, -200.0, 20.0, 380.0), Rect::new(200.0, -180.0, 20.0, 380.0), Rect::new(300.0, -200.0, 20.0, 380.0)];
        let p = route_around(Pt::new(0.0, 0.0), Pt::new(400.0, 0.0), Some(true), &walls, 5.0).unwrap();
        assert!(!crosses(&p, &walls), "{p:?}");
        assert_eq!(*p.last().unwrap(), Pt::new(400.0, 0.0));
    }

    #[test]
    fn enclosed_start_or_end_has_no_route() {
        let ring = [Rect::new(-50.0, -50.0, 100.0, 100.0)];
        assert!(route_around(Pt::new(0.0, 0.0), Pt::new(300.0, 0.0), None, &ring, 5.0).is_none(), "start inside an obstacle");
        let cage = [
            Rect::new(-60.0, -60.0, 120.0, 20.0),
            Rect::new(-60.0, 40.0, 120.0, 20.0),
            Rect::new(-60.0, -60.0, 20.0, 120.0),
            Rect::new(40.0, -60.0, 20.0, 120.0),
        ];
        assert!(route_around(Pt::new(0.0, 0.0), Pt::new(300.0, 0.0), None, &cage, 5.0).is_none(), "enclosed start");
    }

    #[test]
    fn prefers_fewer_bends_over_slightly_shorter_paths() {
        // from (0,0) to (200,200) without obstacles: a single bend (an L), not a staircase
        let p = route_around(Pt::new(0.0, 0.0), Pt::new(200.0, 200.0), Some(true), &[], 10.0).unwrap();
        assert_eq!(bends(&p), 1, "{p:?}");
        assert_eq!(p[1], Pt::new(200.0, 0.0), "starts horizontally as requested");
    }

    #[test]
    fn crossing_detection() {
        let r = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert!(crosses(&[Pt::new(0.0, 20.0), Pt::new(50.0, 20.0)], &[r]));
        assert!(!crosses(&[Pt::new(0.0, 5.0), Pt::new(50.0, 5.0)], &[r]));
        assert!(!crosses(&[Pt::new(0.0, 10.0), Pt::new(50.0, 10.0)], &[r]), "running along an edge is not crossing it");
        assert!(crosses(&[Pt::new(0.0, 0.0), Pt::new(40.0, 40.0)], &[r]), "diagonale");
    }
}
