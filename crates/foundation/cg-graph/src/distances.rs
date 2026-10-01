//! Cluster distance metrics over layout positions.
//!
//! The metric set generalizes single-linkage grouping beyond Euclidean space:
//! callers pick a metric explicitly and group points whose pairwise distance
//! does not exceed a threshold. The existing hierarchical entry stays as the
//! Euclidean special case, while this module carries the parameterized form.
//!
//! Results are plain index groups with inner and outer order sorted, so
//! repeated runs agree. Failures report the first relevant node in index
//! order and never panic. Empty graphs yield no groups.

use std::collections::HashMap;

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};

use cg_types::Point2;

use crate::positions::Positions;
use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep signatures readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Distance metric between two positions.
///
/// All three are deterministic and need no parameters, so callers name the
/// geometry explicitly instead of relying on a silent default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClusterMetric {
    Euclidean,
    Manhattan,
    Chebyshev,
}

/// Distance between two points under `metric`.
pub fn point_distance(first: Point2, second: Point2, metric: ClusterMetric) -> f32 {
    let dx = (first.x - second.x).abs();
    let dy = (first.y - second.y).abs();
    match metric {
        ClusterMetric::Euclidean => dx.hypot(dy),
        ClusterMetric::Manhattan => dx + dy,
        ClusterMetric::Chebyshev => dx.max(dy),
    }
}

/// Groups nodes whose metric linkage distance does not exceed `threshold`.
///
/// `threshold` must be finite and non-negative. Every node must have a finite
/// position. Isolated nodes form singleton groups unless another position
/// lies within the threshold under the chosen metric.
pub fn metric_clusters(
    graph: &Graph,
    positions: &Positions,
    metric: ClusterMetric,
    threshold: f32,
) -> Result<Vec<Vec<NodeIndex>>, NodeIndex> {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    if order.is_empty() {
        return Ok(Vec::new());
    }
    if !threshold.is_finite() || threshold < 0.0 {
        let first = order.first().copied().unwrap_or(NodeIndex::new(0));
        return Err(first);
    }
    let mut points: Vec<(NodeIndex, Point2)> = Vec::with_capacity(order.len());
    for node in &order {
        match positions.get(node) {
            Some(point) if point.x.is_finite() && point.y.is_finite() => {
                points.push((*node, *point));
            }
            _ => return Err(*node),
        }
    }
    let mut sets = DisjointSets::new(points.len());
    for left in 0..points.len() {
        for right in (left + 1)..points.len() {
            if point_distance(points[left].1, points[right].1, metric) <= threshold {
                sets.union(left, right);
            }
        }
    }
    let mut buckets: HashMap<usize, Vec<NodeIndex>> = HashMap::new();
    for (ordinal, (node, _)) in points.iter().enumerate() {
        let root = sets.find(ordinal);
        buckets.entry(root).or_default().push(*node);
    }
    Ok(sorted_groups(buckets.into_values().collect()))
}

/// Disjoint-set union with path compression and union by rank.
struct DisjointSets {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl DisjointSets {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, member: usize) -> usize {
        let mut root = member;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut cursor = member;
        while self.parent[cursor] != root {
            let next = self.parent[cursor];
            self.parent[cursor] = root;
            cursor = next;
        }
        root
    }

    fn union(&mut self, first: usize, second: usize) {
        let left = self.find(first);
        let right = self.find(second);
        if left == right {
            return;
        }
        if self.rank[left] < self.rank[right] {
            self.parent[left] = right;
        } else if self.rank[left] > self.rank[right] {
            self.parent[right] = left;
        } else {
            self.parent[right] = left;
            self.rank[left] = self.rank[left].saturating_add(1);
        }
    }
}

/// Sorts each group by index and orders groups by their first member.
///
/// Empty groups are dropped so callers never observe vacant slots.
fn sorted_groups(mut groups: Vec<Vec<NodeIndex>>) -> Vec<Vec<NodeIndex>> {
    for group in groups.iter_mut() {
        group.sort_unstable_by_key(|node| node.index());
    }
    groups.retain(|group| !group.is_empty());
    groups
        .sort_unstable_by_key(|group| group.first().map(|node| node.index()).unwrap_or(usize::MAX));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as StdHashMap;

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
    fn point_distances_match_their_geometries() {
        let first = Point2::new(0.0, 0.0);
        let second = Point2::new(3.0, 4.0);
        assert_eq!(point_distance(first, second, ClusterMetric::Euclidean), 5.0);
        assert_eq!(point_distance(first, second, ClusterMetric::Manhattan), 7.0);
        assert_eq!(point_distance(first, second, ClusterMetric::Chebyshev), 4.0);
        assert_eq!(point_distance(first, first, ClusterMetric::Euclidean), 0.0);
    }

    #[test]
    fn every_metric_splits_two_clouds() {
        let (graph, positions) = two_clusters();
        for metric in [
            ClusterMetric::Euclidean,
            ClusterMetric::Manhattan,
            ClusterMetric::Chebyshev,
        ] {
            let groups = metric_clusters(&graph, &positions, metric, 50.0).expect("valid input");
            assert_eq!(groups.len(), 2, "metric splits into two");
            for group in &groups {
                assert_eq!(group.len(), 3);
            }
            covers_each_node_once(&graph, &groups);
        }
    }

    #[test]
    fn small_threshold_keeps_singletons() {
        let (graph, positions) = two_clusters();
        let groups = metric_clusters(&graph, &positions, ClusterMetric::Euclidean, 0.5)
            .expect("valid input");
        assert_eq!(groups.len(), 6);
        covers_each_node_once(&graph, &groups);
    }

    #[test]
    fn empty_graph_yields_no_groups() {
        let graph: Graph = StableGraph::default();
        let positions: Positions = StdHashMap::new();
        let groups =
            metric_clusters(&graph, &positions, ClusterMetric::Euclidean, 10.0).expect("empty");
        assert!(groups.is_empty());
    }

    #[test]
    fn repeated_runs_agree_in_order() {
        let (graph, positions) = two_clusters();
        let first = metric_clusters(&graph, &positions, ClusterMetric::Manhattan, 50.0)
            .expect("valid input");
        for _ in 0..5 {
            let next = metric_clusters(&graph, &positions, ClusterMetric::Manhattan, 50.0)
                .expect("valid input");
            assert_eq!(first, next);
        }
    }

    #[test]
    fn invalid_threshold_and_missing_positions_report_a_node() {
        let (graph, positions) = two_clusters();
        let bad = metric_clusters(&graph, &positions, ClusterMetric::Euclidean, f32::NAN)
            .expect_err("nan");
        assert!(graph.node_weight(bad).is_some());
        let negative = metric_clusters(&graph, &positions, ClusterMetric::Euclidean, -1.0)
            .expect_err("negative");
        assert!(graph.node_weight(negative).is_some());
        let infinite = metric_clusters(&graph, &positions, ClusterMetric::Euclidean, f32::INFINITY)
            .expect_err("infinite");
        assert!(graph.node_weight(infinite).is_some());
        let mut sparse = positions;
        let missing = graph.node_indices().next().expect("a node exists");
        sparse.remove(&missing);
        let absent = metric_clusters(&graph, &sparse, ClusterMetric::Euclidean, 50.0)
            .expect_err("missing position");
        assert_eq!(absent, missing);
    }
}
