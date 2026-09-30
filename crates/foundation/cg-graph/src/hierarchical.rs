//! Single-linkage hierarchical clustering over layout positions.
//!
//! Each node starts alone and pairs whose Euclidean distance does not exceed
//! the caller supplied threshold join the same group. The merge uses a
//! disjoint-set union over sorted pairs, which matches the threshold stopping
//! rule of agglomerative clustering with the minimum distance rule while
//! staying deterministic and free of graph writes.
//!
//! Only the minimum distance rule is supported: it needs no cluster size
//! bookkeeping and avoids the broken mean update of the reference
//! implementation. Results are plain index groups with inner and outer order
//! sorted, so repeated runs agree. Failures report the first relevant node in
//! index order and never panic. Empty graphs yield no groups.

use std::collections::HashMap;

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};

use crate::positions::Positions;
use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep signatures readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Groups nodes whose single-linkage distance does not exceed `threshold`.
///
/// `threshold` must be finite and non-negative. Every node must have a finite
/// position in `positions`. Isolated nodes form singleton groups unless
/// another position lies within the threshold.
pub fn hierarchical_clusters(
    graph: &Graph,
    positions: &Positions,
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
    let mut points: Vec<(NodeIndex, f32, f32)> = Vec::with_capacity(order.len());
    for node in &order {
        match positions.get(node) {
            Some(point) if point.x.is_finite() && point.y.is_finite() => {
                points.push((*node, point.x, point.y));
            }
            _ => return Err(*node),
        }
    }
    let mut sets = DisjointSets::new(points.len());
    let limit = threshold * threshold;
    for left in 0..points.len() {
        for right in (left + 1)..points.len() {
            let dx = points[left].1 - points[right].1;
            let dy = points[left].2 - points[right].2;
            if dx * dx + dy * dy <= limit {
                sets.union(left, right);
            }
        }
    }
    let mut buckets: HashMap<usize, Vec<NodeIndex>> = HashMap::new();
    for (ordinal, (node, _, _)) in points.iter().enumerate() {
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

    fn member_set(groups: &[Vec<NodeIndex>]) -> Vec<Vec<usize>> {
        let mut sets: Vec<Vec<usize>> = groups
            .iter()
            .map(|group| {
                let mut members: Vec<usize> =
                    group.iter().map(|node| node.index()).collect();
                members.sort_unstable();
                members
            })
            .collect();
        sets.sort_unstable();
        sets
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
    fn medium_threshold_splits_two_clusters() {
        let (graph, positions) = two_clusters();
        let groups = hierarchical_clusters(&graph, &positions, 50.0).expect("valid input");
        assert_eq!(groups.len(), 2);
        let sets = member_set(&groups);
        assert!(sets.contains(&vec![0, 1, 2]));
        assert!(sets.contains(&vec![3, 4, 5]));
        covers_each_node_once(&graph, &groups);
    }

    #[test]
    fn small_threshold_keeps_singletons() {
        let (graph, positions) = two_clusters();
        let groups = hierarchical_clusters(&graph, &positions, 0.5).expect("valid input");
        assert_eq!(groups.len(), 6);
        covers_each_node_once(&graph, &groups);
    }

    #[test]
    fn empty_graph_yields_no_groups() {
        let graph: Graph = StableGraph::default();
        let positions: Positions = HashMap::new();
        let groups = hierarchical_clusters(&graph, &positions, 10.0).expect("empty");
        assert!(groups.is_empty());
    }

    #[test]
    fn isolated_nodes_form_singletons() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(labelled("a"));
        let b = graph.add_node(labelled("b"));
        let positions: Positions = [
            (a, Point2::new(0.0, 0.0)),
            (b, Point2::new(100.0, 0.0)),
        ]
        .into_iter()
        .collect();
        let groups = hierarchical_clusters(&graph, &positions, 10.0).expect("valid input");
        assert_eq!(groups.len(), 2);
        covers_each_node_once(&graph, &groups);
    }

    #[test]
    fn repeated_runs_agree_in_order() {
        let (graph, positions) = two_clusters();
        let first = hierarchical_clusters(&graph, &positions, 50.0).expect("valid input");
        for _ in 0..10 {
            let next = hierarchical_clusters(&graph, &positions, 50.0).expect("valid input");
            assert_eq!(first, next);
        }
    }

    #[test]
    fn invalid_threshold_and_missing_positions_report_a_node() {
        let (graph, positions) = two_clusters();
        let bad = hierarchical_clusters(&graph, &positions, f32::NAN).expect_err("nan threshold");
        assert!(graph.node_weight(bad).is_some());
        let negative =
            hierarchical_clusters(&graph, &positions, -1.0).expect_err("negative threshold");
        assert!(graph.node_weight(negative).is_some());
        let infinite =
            hierarchical_clusters(&graph, &positions, f32::INFINITY).expect_err("infinite");
        assert!(graph.node_weight(infinite).is_some());
        let mut sparse = positions;
        let missing = graph.node_indices().next().expect("a node exists");
        sparse.remove(&missing);
        let absent =
            hierarchical_clusters(&graph, &sparse, 50.0).expect_err("missing position");
        assert_eq!(absent, missing);
    }
}
