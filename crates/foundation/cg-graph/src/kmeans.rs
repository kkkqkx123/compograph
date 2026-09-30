//! Deterministic k-means clustering over layout positions.
//!
//! Initial centers are the positions of the first nodes in index order, so no
//! random source is involved and repeated runs agree. Each round assigns every
//! node to its nearest center and then moves each center to the mean of its
//! members. Empty clusters keep their previous center during iteration and
//! are dropped from the final output, so callers never observe vacant slots.
//!
//! Results are plain index groups with inner and outer order sorted. Failures
//! report the first relevant node in index order and never panic. Empty
//! graphs yield no groups.

use std::collections::HashMap;

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};

use crate::positions::Positions;
use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep signatures readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Groups nodes into `k` clusters by position similarity.
///
/// `k` must lie within one and the node count, and `max_iterations` must be
/// at least one. Every node must have a finite position. The caller pairs the
/// position snapshot with its own generation guard when coordinates may move.
pub fn kmeans_clusters(
    graph: &Graph,
    positions: &Positions,
    k: usize,
    max_iterations: usize,
) -> Result<Vec<Vec<NodeIndex>>, NodeIndex> {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    if order.is_empty() {
        return Ok(Vec::new());
    }
    if k == 0 || k > order.len() || max_iterations == 0 {
        let first = order.first().copied().unwrap_or(NodeIndex::new(0));
        return Err(first);
    }
    let mut points: Vec<(NodeIndex, f32, f32)> = Vec::with_capacity(order.len());
    for node in &order {
        match positions.get(node) {
            Some(point) if point.x.is_finite() && point.y.is_finite() => {
                points.push((*node, point.x, point.y));
            }
            _ => return Err(*node),
        }
    }
    let mut centers: Vec<(f32, f32)> = points.iter().take(k).map(|(_, x, y)| (*x, *y)).collect();
    let mut assignment = vec![0usize; points.len()];
    for _ in 0..max_iterations {
        let mut changed = false;
        for (ordinal, (_, x, y)) in points.iter().enumerate() {
            let mut best = 0usize;
            let mut best_distance = squared_distance(*x, *y, centers[0].0, centers[0].1);
            for (slot, center) in centers.iter().enumerate().skip(1) {
                let distance = squared_distance(*x, *y, center.0, center.1);
                if distance < best_distance {
                    best_distance = distance;
                    best = slot;
                }
            }
            if assignment[ordinal] != best {
                assignment[ordinal] = best;
                changed = true;
            }
        }
        let mut sums = vec![(0.0f32, 0.0f32, 0usize); centers.len()];
        for (ordinal, (_, x, y)) in points.iter().enumerate() {
            let slot = assignment[ordinal];
            sums[slot].0 += *x;
            sums[slot].1 += *y;
            sums[slot].2 += 1;
        }
        for (slot, (sum_x, sum_y, count)) in sums.iter().enumerate() {
            if *count > 0 {
                let total = *count as f32;
                centers[slot] = (sum_x / total, sum_y / total);
            }
        }
        if !changed {
            break;
        }
    }
    let mut buckets: HashMap<usize, Vec<NodeIndex>> = HashMap::new();
    for (ordinal, (node, _, _)) in points.iter().enumerate() {
        buckets.entry(assignment[ordinal]).or_default().push(*node);
    }
    Ok(sorted_groups(buckets.into_values().collect()))
}

/// Squared Euclidean distance without a square root for comparisons.
fn squared_distance(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

/// Sorts each group by index and orders groups by their first member.
///
/// Empty groups are dropped so callers never observe vacant slots.
fn sorted_groups(mut groups: Vec<Vec<NodeIndex>>) -> Vec<Vec<NodeIndex>> {
    for group in groups.iter_mut() {
        group.sort_unstable_by_key(|node| node.index());
    }
    groups.retain(|group| !group.is_empty());
    groups.sort_unstable_by_key(|group| group.first().map(|node| node.index()).unwrap_or(usize::MAX));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_types::Point2;

    fn labelled(label: &str) -> NodeData {
        NodeData {
            label: label.into(),
        }
    }

    fn weighted(weight: f32) -> EdgeData {
        EdgeData { weight }
    }

    fn two_clusters() -> (Graph, Positions) {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(labelled("a"));
        let b = graph.add_node(labelled("b"));
        let c = graph.add_node(labelled("c"));
        let d = graph.add_node(labelled("d"));
        let e = graph.add_node(labelled("e"));
        let f = graph.add_node(labelled("f"));
        graph.add_edge(a, b, weighted(1.0));
        graph.add_edge(b, c, weighted(1.0));
        graph.add_edge(d, e, weighted(1.0));
        graph.add_edge(e, f, weighted(1.0));
        graph.add_edge(c, d, weighted(0.2));
        let positions: Positions = [
            (a, Point2::new(0.0, 0.0)),
            (b, Point2::new(1.0, 0.0)),
            (c, Point2::new(0.0, 1.0)),
            (d, Point2::new(100.0, 100.0)),
            (e, Point2::new(101.0, 100.0)),
            (f, Point2::new(100.0, 101.0)),
        ]
        .into_iter()
        .collect();
        (graph, positions)
    }

    fn covers_each_node_once(graph: &Graph, groups: &[Vec<NodeIndex>]) {
        let mut seen: Vec<usize> = groups
            .iter()
            .flat_map(|group| group.iter().map(|node| node.index()))
            .collect();
        seen.sort_unstable();
        let mut expected: Vec<usize> = graph.node_indices().map(|node| node.index()).collect();
        expected.sort_unstable();
        assert_eq!(seen, expected);
    }

    #[test]
    fn two_centers_split_two_clusters() {
        let (graph, positions) = two_clusters();
        let groups = kmeans_clusters(&graph, &positions, 2, 20).expect("valid input");
        assert_eq!(groups.len(), 2);
        for group in &groups {
            assert_eq!(group.len(), 3);
        }
        covers_each_node_once(&graph, &groups);
        let first: Vec<usize> = groups[0].iter().map(|node| node.index()).collect();
        assert!(first == vec![0, 1, 2] || first == vec![3, 4, 5]);
    }

    #[test]
    fn empty_graph_yields_no_groups() {
        let graph: Graph = StableGraph::default();
        let positions: Positions = HashMap::new();
        let groups = kmeans_clusters(&graph, &positions, 2, 10).expect("empty");
        assert!(groups.is_empty());
    }

    #[test]
    fn repeated_runs_agree_in_order() {
        let (graph, positions) = two_clusters();
        let first = kmeans_clusters(&graph, &positions, 2, 20).expect("valid input");
        for _ in 0..10 {
            let next = kmeans_clusters(&graph, &positions, 2, 20).expect("valid input");
            assert_eq!(first, next);
        }
    }

    #[test]
    fn output_holds_no_empty_groups() {
        let (graph, positions) = two_clusters();
        let groups = kmeans_clusters(&graph, &positions, 3, 20).expect("valid input");
        assert!(!groups.is_empty());
        for group in &groups {
            assert!(!group.is_empty());
        }
        covers_each_node_once(&graph, &groups);
    }

    #[test]
    fn invalid_k_and_missing_positions_report_a_node() {
        let (graph, positions) = two_clusters();
        let zero = kmeans_clusters(&graph, &positions, 0, 10).expect_err("zero k");
        assert!(graph.node_weight(zero).is_some());
        let overflow = kmeans_clusters(&graph, &positions, 7, 10).expect_err("k above count");
        assert!(graph.node_weight(overflow).is_some());
        let idle = kmeans_clusters(&graph, &positions, 2, 0).expect_err("empty budget");
        assert!(graph.node_weight(idle).is_some());
        let mut sparse = positions;
        let missing = graph.node_indices().next().expect("a node exists");
        sparse.remove(&missing);
        let absent = kmeans_clusters(&graph, &sparse, 2, 10).expect_err("missing position");
        assert_eq!(absent, missing);
    }
}
