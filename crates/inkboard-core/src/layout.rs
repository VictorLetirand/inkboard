//! Automatic graph layout (layered algorithm, Sugiyama style).
//!
//! Used for imported diagrams (Mermaid) and for the
//! "automatic layout" command on shapes already linked by connectors.

use crate::connector::Route;
use crate::editor::Editor;
use crate::geom::{Pt, Rect};
use crate::model::{Id, Kind};
use crate::num;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    TopDown,
    BottomUp,
    LeftRight,
    RightLeft,
}

#[derive(Clone, Copy, Debug)]
pub struct NodeSpec {
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// Center of each node (same order as the input).
    pub centers: Vec<Pt>,
    /// For each edge: waypoints from `from` to `to`, ends (centers) included.
    /// Empty for a loop or an invalid edge.
    pub routes: Vec<Vec<Pt>>,
}

#[derive(Clone, Copy)]
struct Item {
    a: f64, // size along the rank axis
    b: f64, // cross size
    rank: usize,
}

const NODE_SEP: f64 = 48.0;
const RANK_SEP: f64 = 84.0;

/// Edges to reverse so that the graph has no cycle (iterative DFS): `reversed[i]` for `edges[i]`; `valid`: the edges to consider.
fn break_cycles(n: usize, edges: &[(usize, usize)], valid: &[usize]) -> Vec<bool> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &i in valid {
        adj[edges[i].0].push(i);
    }
    let mut state = vec![0u8; n];
    let mut reversed = vec![false; edges.len()];
    for s in 0..n {
        if state[s] != 0 {
            continue;
        }
        let mut stack = vec![(s, 0usize)];
        state[s] = 1;
        while let Some(&mut (u, ref mut k)) = stack.last_mut() {
            if *k < adj[u].len() {
                let ei = adj[u][*k];
                *k += 1;
                let v = edges[ei].1;
                match state[v] {
                    1 => reversed[ei] = true,
                    0 => {
                        state[v] = 1;
                        stack.push((v, 0));
                    }
                    _ => {}
                }
            } else {
                state[u] = 2;
                stack.pop();
            }
        }
    }
    reversed
}

/// Rank of each node: longest path from a source (Kahn); `oriented`: acyclic edges `(from, to, _)`.
fn assign_ranks(items: &mut [Item], oriented: &[(usize, usize, usize)]) {
    let n = items.len();
    let mut indeg = vec![0usize; n];
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(u, v, _) in oriented {
        indeg[v] += 1;
        out[u].push(v);
    }
    let mut queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut qi = 0;
    while qi < queue.len() {
        let u = queue[qi];
        qi += 1;
        for &v in &out[u] {
            items[v].rank = items[v].rank.max(items[u].rank + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                queue.push(v);
            }
        }
    }
}

/// Order within the ranks: barycenters of the neighbors, alternating downward and upward sweeps.
fn order_ranks(layers: &mut [Vec<usize>], up: &[Vec<usize>], down: &[Vec<usize>], total: usize) {
    let max_rank = layers.len() - 1;
    let mut pos = vec![0usize; total];
    let reindex = |l: &[usize], pos: &mut [usize]| l.iter().enumerate().for_each(|(i, &id)| pos[id] = i);
    for l in layers.iter() {
        reindex(l, &mut pos);
    }
    let bary = |id: usize, nb: &[Vec<usize>], pos: &[usize]| -> f64 {
        if nb[id].is_empty() { num::f64_of(pos[id]) } else { nb[id].iter().map(|&x| num::f64_of(pos[x])).sum::<f64>() / num::f64_of(nb[id].len()) }
    };
    for it in 0..8 {
        let ranks: Vec<usize> = if it % 2 == 0 { (1..=max_rank).collect() } else { (0..max_rank).rev().collect() };
        for r in ranks {
            let nb = if it % 2 == 0 { up } else { down };
            let keys: HashMap<usize, f64> = layers[r].iter().map(|&id| (id, bary(id, nb, &pos))).collect();
            layers[r].sort_by(|p, q| keys[p].total_cmp(&keys[q]));
            reindex(&layers[r], &mut pos);
        }
    }
}

/// Coordinates across the ranks: packed side by side, then pulled toward the neighbors (alternating sweeps, no overlap).
fn cross_coords(items: &[Item], layers: &[Vec<usize>], up: &[Vec<usize>], down: &[Vec<usize>]) -> Vec<f64> {
    let (total, max_rank) = (items.len(), layers.len() - 1);
    let mut cx = vec![0.0f64; total];
    for l in layers {
        let mut x = 0.0;
        for &id in l {
            let w = items[id].b;
            x += w / 2.0;
            cx[id] = x;
            x += w / 2.0 + NODE_SEP;
        }
        let shift = (x - NODE_SEP) / 2.0;
        for &id in l {
            cx[id] -= shift;
        }
    }
    let settle = |l: &[usize], desire: &dyn Fn(usize) -> f64, cx: &mut [f64]| {
        let m = l.len();
        let d: Vec<f64> = l.iter().map(|&i| desire(i)).collect();
        let w: Vec<f64> = l.iter().map(|&i| items[i].b).collect();
        let (mut x1, mut x2) = (vec![0.0; m], vec![0.0; m]);
        for i in 0..m {
            x1[i] = if i > 0 { d[i].max(x1[i - 1] + f64::midpoint(w[i - 1], w[i]) + NODE_SEP) } else { d[i] };
        }
        for i in (0..m).rev() {
            x2[i] = if i + 1 < m { d[i].min(x2[i + 1] - f64::midpoint(w[i], w[i + 1]) - NODE_SEP) } else { d[i] };
        }
        for (i, &id) in l.iter().enumerate() {
            cx[id] = f64::midpoint(x1[i], x2[i]);
        }
    };
    for _ in 0..6 {
        for layer in &layers[1..] {
            let cur = cx.clone();
            settle(layer, &|id| if up[id].is_empty() { cur[id] } else { up[id].iter().map(|&x| cur[x]).sum::<f64>() / num::f64_of(up[id].len()) }, &mut cx);
        }
        for r in (0..max_rank).rev() {
            let cur = cx.clone();
            settle(
                &layers[r],
                &|id| if down[id].is_empty() { cur[id] } else { down[id].iter().map(|&x| cur[x]).sum::<f64>() / num::f64_of(down[id].len()) },
                &mut cx,
            );
        }
    }
    cx
}

/// Computes a readable layout: ranks by longest path, cycles broken,
/// long edges passing through relay nodes, crossing reduction by barycenters.
pub fn layered(nodes: &[NodeSpec], edges: &[(usize, usize)], dir: Direction) -> Layout {
    let n = nodes.len();
    if n == 0 {
        return Layout::default();
    }
    let horiz = matches!(dir, Direction::LeftRight | Direction::RightLeft);
    let mut items: Vec<Item> = nodes.iter().map(|s| Item { a: if horiz { s.w } else { s.h }, b: if horiz { s.h } else { s.w }, rank: 0 }).collect();

    // valid edges (index into `edges`)
    let valid: Vec<usize> = (0..edges.len()).filter(|&i| edges[i].0 != edges[i].1 && edges[i].0 < n && edges[i].1 < n).collect();

    // 1. break the cycles
    let reversed = break_cycles(n, edges, &valid);
    let oriented: Vec<(usize, usize, usize)> =
        valid.iter().map(|&i| if reversed[i] { (edges[i].1, edges[i].0, i) } else { (edges[i].0, edges[i].1, i) }).collect();

    // 2. ranks
    assign_ranks(&mut items, &oriented);

    // 3. relay nodes for the edges that skip ranks
    let mut chains: Vec<(usize, Vec<usize>)> = Vec::new(); // (original edge index, oriented chain)
    for &(u, v, ei) in &oriented {
        let mut chain = vec![u];
        for r in items[u].rank + 1..items[v].rank {
            items.push(Item { a: 0.0, b: 12.0, rank: r });
            chain.push(items.len() - 1);
        }
        chain.push(v);
        chains.push((ei, chain));
    }
    let total = items.len();
    let max_rank = items.iter().map(|i| i.rank).max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, it) in items.iter().enumerate() {
        layers[it.rank].push(i);
    }
    let mut up: Vec<Vec<usize>> = vec![Vec::new(); total];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); total];
    for (_, chain) in &chains {
        for w in chain.windows(2) {
            down[w[0]].push(w[1]);
            up[w[1]].push(w[0]);
        }
    }

    // 4. order within ranks
    order_ranks(&mut layers, &up, &down, total);

    // 5. cross coordinates
    let cx = cross_coords(&items, &layers, &up, &down);

    // 6. coordinates along the ranks
    let mut ra = vec![0.0f64; max_rank + 1];
    let mut prev = 0.0;
    for (r, l) in layers.iter().enumerate() {
        let m = l.iter().map(|&i| items[i].a).fold(0.0, f64::max);
        ra[r] = if r == 0 { m / 2.0 } else { ra[r - 1] + prev / 2.0 + RANK_SEP + m / 2.0 };
        prev = m;
    }
    let place = |id: usize| -> Pt {
        let (a, b) = (ra[items[id].rank], cx[id]);
        match dir {
            Direction::TopDown => Pt::new(b, a),
            Direction::BottomUp => Pt::new(b, -a),
            Direction::LeftRight => Pt::new(a, b),
            Direction::RightLeft => Pt::new(-a, b),
        }
    };
    let mut routes = vec![Vec::new(); edges.len()];
    for (ei, chain) in &chains {
        let mut pts: Vec<Pt> = chain.iter().map(|&i| place(i)).collect();
        if reversed[*ei] {
            pts.reverse();
        }
        routes[*ei] = pts;
    }
    Layout { centers: (0..n).map(place).collect(), routes }
}

/// Rearranges the selected shapes according to the connectors that link them.
/// The connectors concerned receive waypoints to go around the intermediate ranks.
pub fn auto_layout(editor: &mut Editor, ids: &[Id], dir: Direction) {
    let page = editor.page().clone();
    let nodes: Vec<&crate::model::Object> = page.objects.iter().filter(|o| ids.contains(&o.id) && o.is_attachable() && !page.is_locked(o)).collect();
    if nodes.len() < 2 {
        return;
    }
    let index: HashMap<&Id, usize> = nodes.iter().enumerate().map(|(i, o)| (&o.id, i)).collect();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut edge_ids: Vec<Id> = Vec::new();
    for o in &page.objects {
        if let Kind::Connector(c) = &o.kind
            && let (Some(a), Some(b)) = (c.start.target().and_then(|t| index.get(t)), c.end.target().and_then(|t| index.get(t)))
        {
            edges.push((*a, *b));
            edge_ids.push(o.id.clone());
        }
    }
    let specs: Vec<NodeSpec> = nodes
        .iter()
        .map(|o| {
            let r = o.frame().map(|f| f.aabb()).unwrap_or_default();
            NodeSpec { w: r.w, h: r.h }
        })
        .collect();
    let lay = layered(&specs, &edges, dir);
    let origin = nodes.iter().filter_map(|o| o.frame()).map(|f| f.aabb()).reduce(|a, b| a.union(&b)).unwrap_or(Rect::default());
    let bb = Rect::from_points(
        lay.centers.iter().zip(&specs).flat_map(|(c, s)| [Pt::new(c.x - s.w / 2.0, c.y - s.h / 2.0), Pt::new(c.x + s.w / 2.0, c.y + s.h / 2.0)]),
    )
    .unwrap_or_default();
    let shift = Pt::new(origin.x - bb.x, origin.y - bb.y);
    let node_ids: Vec<Id> = nodes.iter().map(|o| o.id.clone()).collect();
    editor.transact(|e| {
        for (i, id) in node_ids.iter().enumerate() {
            let target = lay.centers[i] + shift;
            e.update(id, |o| {
                if let Some(f) = o.frame() {
                    o.translate(Pt::new(target.x - f.center().x, target.y - f.center().y));
                }
            });
        }
        for (k, id) in edge_ids.iter().enumerate() {
            let route = &lay.routes[k];
            let via: Vec<Pt> = if route.len() > 2 { route[1..route.len() - 1].iter().map(|p| *p + shift).collect() } else { Vec::new() };
            e.update(id, |o| {
                if let Kind::Connector(c) = &mut o.kind {
                    c.via = via;
                    if c.route == Route::Elbow {
                        c.route = Route::Straight;
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(w: f64, h: f64) -> NodeSpec {
        NodeSpec { w, h }
    }

    fn overlaps(l: &Layout, nodes: &[NodeSpec]) -> bool {
        for i in 0..nodes.len() {
            for j in i + 1..nodes.len() {
                let (a, b) = (l.centers[i], l.centers[j]);
                if (a.x - b.x).abs() < f64::midpoint(nodes[i].w, nodes[j].w) && (a.y - b.y).abs() < f64::midpoint(nodes[i].h, nodes[j].h) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn chain_is_laid_out_top_down() {
        let nodes = vec![spec(100.0, 50.0); 3];
        let l = layered(&nodes, &[(0, 1), (1, 2)], Direction::TopDown);
        assert!(l.centers[0].y < l.centers[1].y && l.centers[1].y < l.centers[2].y);
        assert!(!overlaps(&l, &nodes));
    }

    #[test]
    fn left_right_uses_x_for_ranks() {
        let nodes = vec![spec(100.0, 50.0); 2];
        let l = layered(&nodes, &[(0, 1)], Direction::LeftRight);
        assert!(l.centers[0].x < l.centers[1].x);
        assert!((l.centers[0].y - l.centers[1].y).abs() < 1e-6);
    }

    #[test]
    fn cycles_do_not_hang_and_routes_keep_direction() {
        let nodes = vec![spec(80.0, 40.0); 3];
        let edges = [(0, 1), (1, 2), (2, 0), (1, 1)];
        let l = layered(&nodes, &edges, Direction::TopDown);
        assert!(!overlaps(&l, &nodes));
        // the back edge 2→0: the route does start at the center of node 2 and arrives at node 0
        let r = &l.routes[2];
        assert!(r.first().unwrap().dist(l.centers[2]) < 1e-6);
        assert!(r.last().unwrap().dist(l.centers[0]) < 1e-6);
        assert!(l.routes[3].is_empty(), "a loop has no route");
    }

    #[test]
    fn long_edges_get_relay_points_and_no_overlap_in_diamond() {
        let nodes = vec![spec(100.0, 50.0); 5];
        // 0→1→2→3 and 0→3 (long edge) and 0→4
        let edges = [(0, 1), (1, 2), (2, 3), (0, 3), (0, 4)];
        let l = layered(&nodes, &edges, Direction::TopDown);
        assert_eq!(l.routes[3].len(), 4, "0→3 skips two ranks: two relays");
        assert!(!overlaps(&l, &nodes));
        // relays do not land on a node
        for p in &l.routes[3][1..3] {
            for (c, s) in l.centers.iter().zip(&nodes) {
                assert!((p.x - c.x).abs() > s.w / 2.0 || (p.y - c.y).abs() > s.h / 2.0);
            }
        }
    }

    #[test]
    fn disconnected_and_empty_inputs() {
        assert_eq!(layered(&[], &[], Direction::TopDown).centers, [] as [crate::geom::Pt; 0]);
        let nodes = vec![spec(60.0, 30.0); 4];
        let l = layered(&nodes, &[], Direction::TopDown);
        assert!(!overlaps(&l, &nodes));
        let l = layered(&nodes, &[(9, 0)], Direction::TopDown); // invalid edge ignored
        assert_eq!(l.routes[0], [] as [crate::geom::Pt; 0]);
    }

    #[test]
    fn wide_fanout_is_centered_on_parent() {
        let nodes = vec![spec(100.0, 50.0); 4];
        let l = layered(&nodes, &[(0, 1), (0, 2), (0, 3)], Direction::TopDown);
        let mid = f64::midpoint(l.centers[1].x, l.centers[3].x);
        assert!((l.centers[0].x - mid).abs() < 1.0);
        assert!(!overlaps(&l, &nodes));
    }

    #[test]
    fn auto_layout_moves_nodes_and_adds_via_points() {
        use crate::geom::Frame;
        use crate::model::{BoxGeom, Document, Label, Object};
        use crate::style::Style;
        let mut e = Editor::new(Document::default());
        let ids: Vec<Id> = (0..4)
            .map(|i| e.insert(Object::new_box(BoxGeom::Rect, Frame::new(f64::from(i) * 10.0, 0.0, 100.0, 50.0), Style::default(), Label::default())))
            .collect();
        let mut all = ids.clone();
        for (a, b) in [(0, 1), (1, 2), (2, 3), (0, 3)] {
            all.push(e.connect(&ids[a], &ids[b]));
        }
        auto_layout(&mut e, &all, Direction::TopDown);
        let y: Vec<f64> = ids.iter().map(|i| e.page().get(i).unwrap().frame().unwrap().y).collect();
        assert!(y[0] < y[1] && y[1] < y[2] && y[2] < y[3]);
        let long = e.page().objects.iter().rev().find_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).unwrap();
        assert_eq!(long.via.len(), 2);
        assert!(e.undo());
        assert_eq!(e.page().get(&ids[3]).unwrap().frame().unwrap().y, 0.0);
    }
}
