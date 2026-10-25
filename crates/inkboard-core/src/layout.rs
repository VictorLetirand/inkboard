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

/// Room between two relay nodes (the lines of long edges running side by side), and between a relay node and a real one.
const RELAY_SEP: f64 = 18.0;
const MIXED_SEP: f64 = 40.0;
/// Up to this many items (nodes and relays), the crossing reduction is tried from three starting orders.
const MULTI_START_MAX_ITEMS: usize = 1_500;

/// State of the greedy cycle breaking: the graph that is left, as degrees.
struct Peeling {
    out_edges: Vec<Vec<usize>>,
    in_edges: Vec<Vec<usize>>,
    out_deg: Vec<isize>,
    in_deg: Vec<isize>,
    removed: Vec<bool>,
    head: Vec<usize>,
    tail: Vec<usize>,
}

impl Peeling {
    /// Takes node `u` out of what is left; it goes to the head of the order (it only has edges going out of it left) or to the tail.
    fn take(&mut self, u: usize, to_head: bool, edges: &[(usize, usize)]) {
        self.removed[u] = true;
        if to_head {
            self.head.push(u);
        } else {
            self.tail.push(u);
        }
        for &e in &self.out_edges[u] {
            self.in_deg[edges[e].1] -= 1;
        }
        for &e in &self.in_edges[u] {
            self.out_deg[edges[e].0] -= 1;
        }
    }
}

/// Edges to reverse so that the graph has no cycle: the greedy heuristic of Eades, Lin and Smyth (sinks go last, sources first, otherwise the node
/// whose out-edges outnumber its in-edges the most goes first), which reverses few edges and none in an acyclic graph.
/// `reversed[i]` for `edges[i]`; `valid`: the edges to consider.
fn break_cycles(n: usize, edges: &[(usize, usize)], valid: &[usize]) -> Vec<bool> {
    let mut g = Peeling {
        out_edges: vec![Vec::new(); n],
        in_edges: vec![Vec::new(); n],
        out_deg: vec![0; n],
        in_deg: vec![0; n],
        removed: vec![false; n],
        head: Vec::with_capacity(n),
        tail: Vec::new(),
    };
    for &i in valid {
        g.out_edges[edges[i].0].push(i);
        g.in_edges[edges[i].1].push(i);
        g.out_deg[edges[i].0] += 1;
        g.in_deg[edges[i].1] += 1;
    }
    let mut left = n;
    while left > 0 {
        let mut progress = true;
        while progress {
            progress = false;
            for u in 0..n {
                if g.removed[u] {
                    continue;
                }
                if g.out_deg[u] == 0 {
                    g.take(u, false, edges);
                } else if g.in_deg[u] == 0 {
                    g.take(u, true, edges);
                } else {
                    continue;
                }
                left -= 1;
                progress = true;
            }
        }
        if left == 0 {
            break;
        }
        let pick = (0..n).filter(|&u| !g.removed[u]).max_by_key(|&u| (g.out_deg[u] - g.in_deg[u], std::cmp::Reverse(u)));
        if let Some(u) = pick {
            g.take(u, true, edges);
            left -= 1;
        }
    }
    let mut place = vec![0usize; n];
    for (k, &u) in g.head.iter().chain(g.tail.iter().rev()).enumerate() {
        place[u] = k;
    }
    let mut reversed = vec![false; edges.len()];
    for &i in valid {
        reversed[i] = place[edges[i].0] > place[edges[i].1];
    }
    reversed
}

/// Rank of each node: longest path from a source (Kahn), then shortened (see [`compact_ranks`]); `oriented`: acyclic edges `(from, to, _)`.
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
    compact_ranks(items, oriented);
}

/// Shortens the edges: the longest-path ranking leaves every source on the first rank, so a node that only points far down stretches its edges
/// over the whole drawing. A node whose edges going out outnumber those coming in moves down as far as its successors allow, one whose edges coming in
/// outnumber the others moves up as far as its predecessors allow (repeated until nothing moves). An edge never gets shorter than one rank.
fn compact_ranks(items: &mut [Item], oriented: &[(usize, usize, usize)]) {
    let n = items.len();
    let (mut preds, mut succs): (Vec<Vec<usize>>, Vec<Vec<usize>>) = (vec![Vec::new(); n], vec![Vec::new(); n]);
    for &(u, v, _) in oriented {
        preds[v].push(u);
        succs[u].push(v);
    }
    for _ in 0..40 {
        let mut moved = false;
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(items[i].rank));
        for pass in 0..2 {
            for &u in &order {
                let (indeg, outdeg) = (preds[u].len(), succs[u].len());
                let target = match outdeg.cmp(&indeg) {
                    std::cmp::Ordering::Greater => succs[u].iter().map(|&v| items[v].rank).min().map(|r| r - 1),
                    std::cmp::Ordering::Less => preds[u].iter().map(|&v| items[v].rank).max().map(|r| r + 1),
                    std::cmp::Ordering::Equal => None,
                };
                if let Some(t) = target.filter(|&t| t != items[u].rank) {
                    items[u].rank = t;
                    moved = true;
                }
            }
            if pass == 0 {
                order.reverse();
            }
        }
        if !moved {
            break;
        }
    }
    if let Some(first) = items.iter().map(|i| i.rank).min() {
        for i in items.iter_mut() {
            i.rank -= first;
        }
    }
}

/// Number of crossings between the edges of each pair of neighboring ranks, for the order `pos` (position of each item in its rank).
fn count_crossings(layers: &[Vec<usize>], down: &[Vec<usize>], pos: &[usize]) -> usize {
    let mut total = 0;
    for r in 0..layers.len().saturating_sub(1) {
        // the edges leaving rank r, by position of the upper end then of the lower end: crossings are inversions of the lower ends
        let mut pairs: Vec<(usize, usize)> = layers[r].iter().flat_map(|&u| down[u].iter().map(move |&v| (pos[u], pos[v]))).collect();
        pairs.sort_unstable();
        let mut tree = vec![0usize; layers[r + 1].len() + 1];
        for (seen, &(_, lower)) in pairs.iter().enumerate() {
            let mut at = lower + 1;
            let mut not_after = 0;
            while at > 0 {
                not_after += tree[at];
                at -= at & at.wrapping_neg();
            }
            total += seen - not_after;
            at = lower + 1;
            while at < tree.len() {
                tree[at] += 1;
                at += at & at.wrapping_neg();
            }
        }
    }
    total
}

/// Order within the ranks: a depth-first order to start with (what is linked stays together), then barycenters of the neighbors alternating
/// downward and upward sweeps, each followed by swaps of neighbors that remove crossings; the order with the fewest crossings is kept.
fn order_ranks(layers: &mut [Vec<usize>], up: &[Vec<usize>], down: &[Vec<usize>], total: usize) {
    let given = layers.to_vec();
    let mut pos = vec![0usize; total];
    let reindex = |l: &[usize], pos: &mut [usize]| l.iter().enumerate().for_each(|(i, &id)| pos[id] = i);
    // depth-first order from the first rank, then from the nodes nothing leads to
    let mut seen = vec![false; total];
    let mut rank_of = vec![0usize; total];
    for (r, l) in layers.iter().enumerate() {
        for &id in l {
            rank_of[id] = r;
        }
    }
    let mut dfs: Vec<usize> = vec![usize::MAX; total];
    let mut next = 0;
    let roots: Vec<usize> = layers.iter().flatten().copied().filter(|&id| up[id].is_empty()).collect();
    for root in roots.into_iter().chain(layers.iter().flatten().copied()) {
        let mut stack = vec![root];
        while let Some(u) = stack.pop() {
            if std::mem::replace(&mut seen[u], true) {
                continue;
            }
            dfs[u] = next;
            next += 1;
            stack.extend(down[u].iter().rev().copied());
        }
    }
    for l in layers.iter_mut() {
        l.sort_by_key(|&id| dfs[id]);
        reindex(l, &mut pos);
    }
    // the sweeps are run from the depth-first order, from its reverse and from the order given; the fewest crossings win (a huge graph only gets
    // the first one: each run costs more than linear time)
    let reversed: Vec<Vec<usize>> = layers.iter().map(|l| l.iter().rev().copied().collect()).collect();
    let mut best: Option<(usize, Vec<Vec<usize>>)> = None;
    let starts = [layers.to_vec(), reversed, given];
    for start in starts.into_iter().take(if total <= MULTI_START_MAX_ITEMS { 3 } else { 1 }) {
        let (crossings, order) = improve(start, up, down, total);
        if best.as_ref().is_none_or(|b| crossings < b.0) {
            best = Some((crossings, order));
        }
    }
    if let Some((_, order)) = best {
        for (l, o) in layers.iter_mut().zip(order) {
            *l = o;
        }
    }
}

/// Sweeps from the order `layers`: barycenters of the neighbors, downward then upward, each followed by [`transpose`]. Returns the fewest crossings
/// met and the order that gave them.
fn improve(mut layers: Vec<Vec<usize>>, up: &[Vec<usize>], down: &[Vec<usize>], total: usize) -> (usize, Vec<Vec<usize>>) {
    let max_rank = layers.len() - 1;
    let mut pos = vec![0usize; total];
    let reindex = |l: &[usize], pos: &mut [usize]| l.iter().enumerate().for_each(|(i, &id)| pos[id] = i);
    for l in &layers {
        reindex(l, &mut pos);
    }
    let bary = |id: usize, nb: &[Vec<usize>], pos: &[usize]| -> f64 {
        if nb[id].is_empty() { num::f64_of(pos[id]) } else { nb[id].iter().map(|&x| num::f64_of(pos[x])).sum::<f64>() / num::f64_of(nb[id].len()) }
    };
    let mut best = layers.clone();
    let mut best_crossings = count_crossings(&layers, down, &pos);
    let mut stalled = 0;
    for it in 0..40 {
        if best_crossings == 0 || stalled >= 6 {
            break;
        }
        let ranks: Vec<usize> = if it % 2 == 0 { (1..=max_rank).collect() } else { (0..max_rank).rev().collect() };
        for r in ranks {
            let nb = if it % 2 == 0 { up } else { down };
            let keys: HashMap<usize, f64> = layers[r].iter().map(|&id| (id, bary(id, nb, &pos))).collect();
            layers[r].sort_by(|p, q| keys[p].total_cmp(&keys[q]));
            reindex(&layers[r], &mut pos);
        }
        transpose(&mut layers, up, down, &mut pos);
        let crossings = count_crossings(&layers, down, &pos);
        if crossings < best_crossings {
            best_crossings = crossings;
            best.clone_from(&layers);
            stalled = 0;
        } else {
            stalled += 1;
        }
    }
    (best_crossings, best)
}

/// Swaps neighbors of a rank while that removes crossings with the ranks above and below (at most a few passes).
fn transpose(layers: &mut [Vec<usize>], up: &[Vec<usize>], down: &[Vec<usize>], pos: &mut [usize]) {
    // crossings of the edges of `u` and `v` (u left of v) with the neighboring ranks, and when their places are exchanged
    let pairs = |u: usize, v: usize, pos: &[usize]| -> (usize, usize) {
        let (mut before, mut after) = (0, 0);
        for nb in [up, down] {
            for &x in &nb[u] {
                for &y in &nb[v] {
                    before += usize::from(pos[x] > pos[y]);
                    after += usize::from(pos[y] > pos[x]);
                }
            }
        }
        (before, after)
    };
    for _ in 0..6 {
        let mut improved = false;
        for l in layers.iter_mut() {
            for i in 0..l.len().saturating_sub(1) {
                let (u, v) = (l[i], l[i + 1]);
                let (before, after) = pairs(u, v, pos);
                if after < before {
                    l.swap(i, i + 1);
                    pos[u] = i + 1;
                    pos[v] = i;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Coordinates across the ranks: packed side by side, then pulled toward the neighbors (alternating sweeps, no overlap).
/// `real`: the first items are nodes, the following ones are relays of long edges (they sit closer to each other).
fn cross_coords(items: &[Item], real: usize, layers: &[Vec<usize>], up: &[Vec<usize>], down: &[Vec<usize>]) -> Vec<f64> {
    let (total, max_rank) = (items.len(), layers.len() - 1);
    let sep = |a: usize, b: usize| match (a >= real, b >= real) {
        (true, true) => RELAY_SEP,
        (false, false) => NODE_SEP,
        _ => MIXED_SEP,
    };
    let mut cx = vec![0.0f64; total];
    for l in layers {
        let mut x = 0.0;
        let mut last = None;
        for &id in l {
            let w = items[id].b;
            if let Some(prev) = last {
                x += sep(prev, id);
            }
            x += w / 2.0;
            cx[id] = x;
            x += w / 2.0;
            last = Some(id);
        }
        let shift = x / 2.0;
        for &id in l {
            cx[id] -= shift;
        }
    }
    let settle = |l: &[usize], desire: &dyn Fn(usize) -> f64, cx: &mut [f64]| {
        let m = l.len();
        let d: Vec<f64> = l.iter().map(|&i| desire(i)).collect();
        let w: Vec<f64> = l.iter().map(|&i| items[i].b).collect();
        let gap = |i: usize| f64::midpoint(w[i], w[i + 1]) + sep(l[i], l[i + 1]);
        let (mut x1, mut x2) = (vec![0.0; m], vec![0.0; m]);
        for i in 0..m {
            x1[i] = if i > 0 { d[i].max(x1[i - 1] + gap(i - 1)) } else { d[i] };
        }
        for i in (0..m).rev() {
            x2[i] = if i + 1 < m { d[i].min(x2[i + 1] - gap(i)) } else { d[i] };
        }
        for (i, &id) in l.iter().enumerate() {
            cx[id] = f64::midpoint(x1[i], x2[i]);
        }
    };
    // a relay has one neighbor above and one below: it is pulled by both, so that the line of a long edge runs straight; a node follows the rank
    // it is being placed after
    let pull = |id: usize, side: &[Vec<usize>], cur: &[f64]| -> f64 {
        let around: Vec<f64> = if id >= real { up[id].iter().chain(&down[id]).map(|&x| cur[x]).collect() } else { side[id].iter().map(|&x| cur[x]).collect() };
        if around.is_empty() { cur[id] } else { around.iter().sum::<f64>() / num::f64_of(around.len()) }
    };
    for _ in 0..10 {
        for layer in &layers[1..] {
            let cur = cx.clone();
            settle(layer, &|id| pull(id, up, &cur), &mut cx);
        }
        for r in (0..max_rank).rev() {
            let cur = cx.clone();
            settle(&layers[r], &|id| pull(id, down, &cur), &mut cx);
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
    let cx = cross_coords(&items, n, &layers, &up, &down);

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

    /// Crossings counted the slow way: every pair of edges between two neighboring ranks.
    fn brute_crossings(layers: &[Vec<usize>], down: &[Vec<usize>], pos: &[usize]) -> usize {
        let mut total = 0;
        for l in &layers[..layers.len().saturating_sub(1)] {
            let edges: Vec<(usize, usize)> = l.iter().flat_map(|&u| down[u].iter().map(move |&v| (pos[u], pos[v]))).collect();
            for (i, a) in edges.iter().enumerate() {
                for b in &edges[i + 1..] {
                    total += usize::from((a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1));
                }
            }
        }
        total
    }

    #[test]
    fn the_fast_crossing_count_agrees_with_the_slow_one() {
        let mut seed = 12345u64;
        let mut next = |m: usize| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            usize::try_from((seed >> 33) % u64::try_from(m).unwrap()).unwrap()
        };
        for _ in 0..200 {
            let ranks = 2 + next(4);
            let mut layers: Vec<Vec<usize>> = Vec::new();
            let mut total = 0;
            for _ in 0..ranks {
                let size = 1 + next(6);
                layers.push((total..total + size).collect());
                total += size;
            }
            let mut down = vec![Vec::new(); total];
            for r in 0..ranks - 1 {
                for _ in 0..next(12) {
                    let (u, v) = (layers[r][next(layers[r].len())], layers[r + 1][next(layers[r + 1].len())]);
                    down[u].push(v); // duplicates are allowed: parallel edges
                }
            }
            for l in &mut layers {
                for i in (1..l.len()).rev() {
                    l.swap(i, next(i + 1));
                }
            }
            let mut pos = vec![0; total];
            for l in &layers {
                for (i, &id) in l.iter().enumerate() {
                    pos[id] = i;
                }
            }
            assert_eq!(count_crossings(&layers, &down, &pos), brute_crossings(&layers, &down, &pos), "{layers:?} {down:?}");
        }
    }

    #[test]
    fn cycle_breaking_reverses_nothing_in_a_dag_and_little_otherwise() {
        let edges = [(3, 1), (1, 0), (0, 2), (3, 2), (2, 4)];
        let valid: Vec<usize> = (0..edges.len()).collect();
        assert!(break_cycles(5, &edges, &valid).iter().all(|r| !r));
        // two cycles sharing an edge: 0→1→2→0 and 1→2→3→1: reversing 1→2 alone breaks both
        let edges = [(0, 1), (1, 2), (2, 0), (2, 3), (3, 1)];
        let valid: Vec<usize> = (0..edges.len()).collect();
        let reversed = break_cycles(4, &edges, &valid);
        assert_eq!(reversed, [false, true, false, false, false], "only 1→2 is reversed");
        // whatever is reversed, what is left has no cycle
        let kept: Vec<(usize, usize)> = edges.iter().zip(&reversed).map(|(&(a, b), &r)| if r { (b, a) } else { (a, b) }).collect();
        let mut items: Vec<Item> = vec![Item { a: 1.0, b: 1.0, rank: 0 }; 4];
        let oriented: Vec<(usize, usize, usize)> = kept.iter().enumerate().map(|(i, &(a, b))| (a, b, i)).collect();
        assign_ranks(&mut items, &oriented);
        assert!(oriented.iter().all(|&(a, b, _)| items[a].rank < items[b].rank), "ranks follow every oriented edge");
    }

    #[test]
    fn compaction_never_makes_an_edge_shorter_than_one_rank_and_starts_at_zero() {
        let edges = [(0, 1), (1, 2), (2, 3), (4, 3), (5, 3), (0, 3)];
        let oriented: Vec<(usize, usize, usize)> = edges.iter().enumerate().map(|(i, &(a, b))| (a, b, i)).collect();
        let mut items: Vec<Item> = vec![Item { a: 1.0, b: 1.0, rank: 0 }; 6];
        assign_ranks(&mut items, &oriented);
        assert_eq!(items.iter().map(|i| i.rank).min(), Some(0));
        assert!(oriented.iter().all(|&(a, b, _)| items[b].rank > items[a].rank));
        assert_eq!((items[4].rank, items[5].rank), (2, 2), "the two sources are just above their target");
    }
}
