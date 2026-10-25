//! Quality of the automatic layout, measured on the final drawing: edge crossings, edges crossing a node they do not link, total edge length and size.
//!
//! Every graph is made by a seeded generator, so the numbers are the same on every run. `cargo test -p inkboard-core --test layout_quality -- --nocapture`
//! prints the table; the assertions are the bounds the layout must keep (they were set from the figures of the first, simpler algorithm).

use inkboard_core::geom::{Pt, Rect};
use inkboard_core::layout::{Direction, Layout, NodeSpec, layered};
use std::time::Instant;

/// Small deterministic generator (the numbers must not depend on a crate).
struct Rng(u64);
impl Rng {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        usize::try_from((self.0 >> 33) % u64::try_from(below.max(1)).unwrap()).unwrap()
    }
}

struct Graph {
    name: &'static str,
    sizes: Vec<NodeSpec>,
    edges: Vec<(usize, usize)>,
}

fn sizes(n: usize, rng: &mut Rng) -> Vec<NodeSpec> {
    (0..n).map(|_| NodeSpec { w: 90.0 + f64::from(u32::try_from(rng.next(5)).unwrap()) * 25.0, h: 52.0 }).collect()
}

/// Random edges; `acyclic`: only from a lower to a higher number.
fn random(name: &'static str, n: usize, per_node: usize, acyclic: bool, seed: u64) -> Graph {
    let mut rng = Rng(seed);
    let mut edges = Vec::new();
    for i in 0..n {
        for _ in 0..per_node {
            let j = rng.next(n);
            if j == i || (acyclic && j < i) {
                continue;
            }
            edges.push((i, j));
        }
    }
    Graph { name, sizes: sizes(n, &mut rng), edges }
}

/// A dependency graph in layers: most edges to the next layer, some skipping layers (what a software architecture looks like).
fn dependencies(name: &'static str, layers: usize, per_layer: usize, seed: u64) -> Graph {
    let mut rng = Rng(seed);
    let mut edges = Vec::new();
    for l in 0..layers - 1 {
        for i in 0..per_layer {
            let from = l * per_layer + i;
            for _ in 0..2 {
                edges.push((from, (l + 1) * per_layer + rng.next(per_layer)));
            }
            if rng.next(4) == 0 && l + 2 < layers {
                let to_layer = l + 2 + rng.next((layers - l - 2).min(3));
                edges.push((from, to_layer * per_layer + rng.next(per_layer)));
            }
        }
    }
    Graph { name, sizes: sizes(layers * per_layer, &mut rng), edges }
}

fn tree(name: &'static str, n: usize, branching: usize) -> Graph {
    let mut rng = Rng(5);
    Graph { name, sizes: sizes(n, &mut rng), edges: (1..n).map(|i| ((i - 1) / branching, i)).collect() }
}

#[derive(Debug, Default, Clone, Copy)]
struct Metrics {
    crossings: usize,
    through_nodes: usize,
    length: f64,
    width: f64,
    height: f64,
    millis: u128,
    overlaps: usize,
}

fn orient(a: Pt, b: Pt, c: Pt) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Segments cross strictly inside both (a shared end point is not a crossing).
fn cross(p: (Pt, Pt), q: (Pt, Pt)) -> bool {
    let (d1, d2, d3, d4) = (orient(q.0, q.1, p.0), orient(q.0, q.1, p.1), orient(p.0, p.1, q.0), orient(p.0, p.1, q.1));
    let eps = 1e-6;
    ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps)) && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps))
}

fn segment_hits(a: Pt, b: Pt, r: Rect) -> bool {
    let inside = |p: Pt| p.x > r.x && p.x < r.right() && p.y > r.y && p.y < r.bottom();
    if inside(a) || inside(b) {
        return true;
    }
    let corners = [Pt::new(r.x, r.y), Pt::new(r.right(), r.y), Pt::new(r.right(), r.bottom()), Pt::new(r.x, r.bottom())];
    (0..4).any(|i| cross((a, b), (corners[i], corners[(i + 1) % 4])))
}

fn measure(g: &Graph, dir: Direction) -> (Layout, Metrics) {
    let start = Instant::now();
    let lay = layered(&g.sizes, &g.edges, dir);
    let millis = start.elapsed().as_millis();
    let rects: Vec<Rect> = lay.centers.iter().zip(&g.sizes).map(|(c, s)| Rect::new(c.x - s.w / 2.0, c.y - s.h / 2.0, s.w, s.h)).collect();
    let mut m = Metrics { millis, ..Metrics::default() };
    let (mut min, mut max) = (Pt::new(f64::INFINITY, f64::INFINITY), Pt::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for r in &rects {
        min = Pt::new(min.x.min(r.x), min.y.min(r.y));
        max = Pt::new(max.x.max(r.right()), max.y.max(r.bottom()));
    }
    m.width = max.x - min.x;
    m.height = max.y - min.y;
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            m.overlaps += usize::from(rects[i].intersects(&rects[j]));
        }
    }
    // each edge as the polyline of its route; the end points are pulled to the border of the nodes, like the drawing does
    let polylines: Vec<Vec<(Pt, Pt)>> = lay.routes.iter().map(|r| r.windows(2).map(|w| (w[0], w[1])).collect()).collect();
    for (k, segs) in polylines.iter().enumerate() {
        for &(a, b) in segs {
            m.length += a.dist(b);
            for (n, r) in rects.iter().enumerate() {
                if n != g.edges[k].0 && n != g.edges[k].1 {
                    let shrunk = Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0);
                    m.through_nodes += usize::from(segment_hits(a, b, shrunk));
                }
            }
        }
    }
    for k in 0..polylines.len() {
        for l in k + 1..polylines.len() {
            for &p in &polylines[k] {
                for &q in &polylines[l] {
                    m.crossings += usize::from(cross(p, q));
                }
            }
        }
    }
    (lay, m)
}

fn graphs() -> Vec<Graph> {
    vec![
        random("cyclic 40 nodes / 3 out", 40, 3, false, 7),
        random("DAG 60 nodes / 2 out", 60, 2, true, 11),
        random("DAG 30 nodes / 3 out", 30, 3, true, 3),
        dependencies("layers 6 x 8", 6, 8, 21),
        dependencies("layers 8 x 5", 8, 5, 4),
        tree("tree 40 / 3", 40, 3),
        tree("tree 25 / 2", 25, 2),
    ]
}

/// Figures of the first, simpler algorithm (depth-first cycle breaking, longest-path ranks, barycenter sweeps): crossings, edges crossing a node, length.
const FIRST_ALGORITHM: [(&str, usize, usize, f64); 7] = [
    ("cyclic 40 nodes / 3 out", 447, 5, 194_672.0),
    ("DAG 60 nodes / 2 out", 81, 17, 32_697.0),
    ("DAG 30 nodes / 3 out", 105, 10, 25_953.0),
    ("layers 6 x 8", 199, 22, 45_054.0),
    ("layers 8 x 5", 109, 6, 34_959.0),
    ("tree 40 / 3", 0, 0, 12_999.0),
    ("tree 25 / 2", 5, 0, 5_788.0),
];

#[test]
fn quality_table_and_bounds() {
    eprintln!("{:<28} {:>9} {:>9} {:>10} {:>8} {:>8} {:>6}", "graph", "crossings", "thru node", "length", "width", "height", "ms");
    let (mut through, mut through_before) = (0, 0);
    for g in graphs() {
        let (_, m) = measure(&g, Direction::TopDown);
        eprintln!("{:<28} {:>9} {:>9} {:>10.0} {:>8.0} {:>8.0} {:>6}", g.name, m.crossings, m.through_nodes, m.length, m.width, m.height, m.millis);
        assert_eq!(m.overlaps, 0, "{}: nodes overlap", g.name);
        let (_, crossings, thru, length) = *FIRST_ALGORITHM.iter().find(|f| f.0 == g.name).expect("figures of the first algorithm");
        assert!(m.crossings <= crossings, "{}: {} crossings, the first algorithm had {crossings}", g.name, m.crossings);
        assert!(m.length <= length + 1.0, "{}: length {:.0}, the first algorithm had {length:.0}", g.name, m.length);
        through += m.through_nodes;
        through_before += thru;
    }
    assert!(through <= through_before, "edges crossing a node: {through}, the first algorithm had {through_before}");
}

#[test]
fn the_aim_is_met_on_the_graph_that_was_unreadable() {
    // the random cyclic graph of 40 modules was drawn 4 400 high with 194 000 of edge length: it must now be much more compact
    let g = random("cyclic 40 nodes / 3 out", 40, 3, false, 7);
    let (_, m) = measure(&g, Direction::TopDown);
    assert!(m.height < 3_000.0, "height {}", m.height);
    assert!(m.length < 130_000.0, "length {}", m.length);
}

#[test]
fn planar_graphs_are_drawn_without_crossing() {
    // a tree, a ladder and a grid-like mesh can be drawn without any crossing
    let mut rng = Rng(1);
    let ladder = Graph {
        name: "ladder",
        sizes: sizes(20, &mut rng),
        edges: (0..9).flat_map(|i| [(i, i + 1), (10 + i, 11 + i), (i, 10 + i)]).chain([(9, 19)]).collect(),
    };
    let grid = Graph {
        name: "grid",
        sizes: sizes(16, &mut rng),
        edges: (0..16).flat_map(|i| [(i % 4 != 3).then_some((i, i + 1)), (i < 12).then_some((i, i + 4))]).flatten().collect(),
    };
    for g in [tree("tree 31 / 2", 31, 2), tree("tree 13 / 3", 13, 3), ladder, grid] {
        let (_, m) = measure(&g, Direction::TopDown);
        assert_eq!(m.crossings, 0, "{}", g.name);
    }
}

#[test]
fn the_layout_is_the_same_every_time() {
    for g in graphs() {
        let (a, _) = measure(&g, Direction::TopDown);
        let (b, _) = measure(&g, Direction::TopDown);
        assert_eq!(a.centers, b.centers, "{}", g.name);
        assert_eq!(a.routes, b.routes, "{}", g.name);
    }
}

#[test]
fn every_direction_gives_a_drawing_without_overlap_and_the_ranks_follow_the_direction() {
    for g in graphs() {
        for dir in [Direction::TopDown, Direction::BottomUp, Direction::LeftRight, Direction::RightLeft] {
            let (lay, m) = measure(&g, dir);
            assert_eq!(m.overlaps, 0, "{} {dir:?}", g.name);
            if g.name == "tree 40 / 3" {
                // in a tree every edge goes from the parent to a child, along the direction
                for &(a, b) in &g.edges {
                    let (from, to) = (lay.centers[a], lay.centers[b]);
                    let ok = match dir {
                        Direction::TopDown => to.y > from.y,
                        Direction::BottomUp => to.y < from.y,
                        Direction::LeftRight => to.x > from.x,
                        Direction::RightLeft => to.x < from.x,
                    };
                    assert!(ok, "{dir:?}: {a} -> {b}");
                }
            }
        }
    }
}

#[test]
fn a_node_that_only_points_far_down_is_not_left_on_the_first_rank() {
    // 0 -> 1 -> 2 -> 3 and 4 -> 3: node 4 has no predecessor, but must sit just above 3, not at the top
    let nodes = vec![NodeSpec { w: 100.0, h: 50.0 }; 5];
    let lay = layered(&nodes, &[(0, 1), (1, 2), (2, 3), (4, 3)], Direction::TopDown);
    assert!((lay.centers[4].y - lay.centers[2].y).abs() < 1e-6, "node 4 is on the rank of node 2");
    assert_eq!(lay.routes[3].len(), 2, "so its edge is one rank long");
}

#[test]
fn few_edges_are_reversed_to_break_cycles() {
    // a complete cycle of 12 nodes: one edge must go back, the others keep their direction (down)
    let n = 12;
    let nodes = vec![NodeSpec { w: 80.0, h: 40.0 }; n];
    let edges: Vec<(usize, usize)> = (0..n).map(|i| (i, (i + 1) % n)).collect();
    let lay = layered(&nodes, &edges, Direction::TopDown);
    let up = edges.iter().filter(|&&(a, b)| lay.centers[b].y < lay.centers[a].y).count();
    assert_eq!(up, 1, "one edge goes back up");
    // an acyclic graph keeps every edge going down, whatever the order of the numbers
    let dag: Vec<(usize, usize)> = vec![(5, 3), (3, 0), (0, 7), (5, 7), (7, 1), (9, 5), (1, 2)];
    let nodes = vec![NodeSpec { w: 80.0, h: 40.0 }; 10];
    let lay = layered(&nodes, &dag, Direction::TopDown);
    for &(a, b) in &dag {
        assert!(lay.centers[b].y > lay.centers[a].y, "{a} -> {b}");
    }
}

#[test]
fn parallel_long_edges_run_closer_than_the_nodes() {
    // a chain 0 -> ... -> 9 and four more edges 0 -> 9 that skip eight ranks: four lines run side by side
    let n = 10;
    let nodes = vec![NodeSpec { w: 100.0, h: 50.0 }; n];
    let mut edges: Vec<(usize, usize)> = (0..9).map(|i| (i, i + 1)).collect();
    edges.extend([(0, 9), (0, 9), (0, 9), (0, 9)]);
    let lay = layered(&nodes, &edges, Direction::TopDown);
    let mut xs: Vec<f64> = (9..13).map(|k| lay.routes[k][4].x).collect();
    xs.sort_by(f64::total_cmp);
    let gaps: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.iter().all(|g| *g >= 12.0 - 1e-6), "relays never overlap: {gaps:?}");
    assert!(gaps.iter().all(|g| *g < 48.0), "lines of long edges run closer than two nodes would: {gaps:?}");
}

#[test]
fn awkward_inputs_do_not_panic() {
    let node = NodeSpec { w: 70.0, h: 30.0 };
    // one node, loops, duplicate and opposite edges, invalid indices, a complete graph, isolated nodes
    assert_eq!(layered(&[node], &[(0, 0)], Direction::TopDown).centers.len(), 1);
    let two = vec![node; 2];
    let lay = layered(&two, &[(0, 1), (0, 1), (1, 0), (0, 0), (5, 1), (1, 9)], Direction::LeftRight);
    assert_eq!(lay.routes.len(), 6);
    assert!(lay.routes[3].is_empty() && lay.routes[4].is_empty() && lay.routes[5].is_empty());
    let n = 9;
    let complete: Vec<(usize, usize)> = (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j))).collect();
    let nodes = vec![node; n];
    let lay = layered(&nodes, &complete, Direction::TopDown);
    assert_eq!(lay.routes.iter().filter(|r| r.len() >= 2).count(), complete.len());
    let lay = layered(&vec![node; 30], &[(0, 1)], Direction::TopDown);
    assert_eq!(lay.centers.len(), 30);
}

mod random_graphs {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::RngSeed;

    fn config() -> ProptestConfig {
        let mut config = ProptestConfig { cases: 300, failure_persistence: None, ..ProptestConfig::default() };
        if matches!(config.rng_seed, RngSeed::Random) {
            config.rng_seed = RngSeed::Fixed(2027);
        }
        config
    }

    proptest! {
        #![proptest_config(config())]

        /// Whatever the graph: one center per node, no overlap, one route per valid edge that starts and ends at the right centers, and linked
        /// nodes are never on the same rank.
        #[test]
        fn any_graph_gets_a_sound_layout(
            n in 1usize..30,
            raw in proptest::collection::vec((0usize..34, 0usize..34), 0..90),
            horizontal in any::<bool>(),
        ) {
            let nodes: Vec<NodeSpec> = (0..n).map(|i| NodeSpec { w: 60.0 + f64::from(u32::try_from(i % 4).unwrap()) * 30.0, h: 40.0 }).collect();
            let dir = if horizontal { Direction::LeftRight } else { Direction::TopDown };
            let lay = layered(&nodes, &raw, dir);
            prop_assert_eq!(lay.centers.len(), n);
            prop_assert_eq!(lay.routes.len(), raw.len());
            for i in 0..n {
                for j in i + 1..n {
                    let (a, b) = (lay.centers[i], lay.centers[j]);
                    let apart = (a.x - b.x).abs() >= f64::midpoint(nodes[i].w, nodes[j].w) - 1e-6 || (a.y - b.y).abs() >= f64::midpoint(nodes[i].h, nodes[j].h) - 1e-6;
                    prop_assert!(apart, "nodes {} and {} overlap", i, j);
                }
            }
            for (k, &(a, b)) in raw.iter().enumerate() {
                let valid = a < n && b < n && a != b;
                let route = &lay.routes[k];
                if !valid {
                    prop_assert!(route.is_empty(), "edge {} is not valid", k);
                    continue;
                }
                prop_assert!(route.len() >= 2);
                prop_assert!(route[0].dist(lay.centers[a]) < 1e-6 && route[route.len() - 1].dist(lay.centers[b]) < 1e-6, "edge {} does not link its nodes", k);
                let rank_axis = |p: Pt| if horizontal { p.x } else { p.y };
                prop_assert!((rank_axis(lay.centers[a]) - rank_axis(lay.centers[b])).abs() > 1e-6, "edge {} links two nodes of one rank", k);
            }
        }
    }
}

#[test]
fn large_graphs_stay_fast() {
    // the largest flowchart the Mermaid import accepts: 1 000 nodes and 2 500 links
    for g in [random("cyclic 1000 / 2.5", 1000, 3, false, 99), random("DAG 1000 / 2.5", 1000, 3, true, 98), dependencies("layers 25 x 40", 25, 40, 5)] {
        let g = Graph { edges: g.edges.into_iter().take(2500).collect(), ..g };
        let start = Instant::now();
        let lay = layered(&g.sizes, &g.edges, Direction::TopDown);
        let took = start.elapsed();
        eprintln!("{:<28} {} nodes {} edges: {took:?}", g.name, g.sizes.len(), g.edges.len());
        assert_eq!(lay.centers.len(), g.sizes.len());
        assert!(took.as_secs() < 20, "{}: {took:?}", g.name);
    }
}
