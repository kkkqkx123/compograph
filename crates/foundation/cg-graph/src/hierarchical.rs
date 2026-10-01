//! Hierarchical clustering over layout positions.
//!
//! Each node starts alone and nearby groups merge until the closest pair is
//! farther apart than the caller supplied threshold. The minimum distance
//! rule merges through a disjoint-set union over sorted pairs, while the
//! maximum and mean rules merge the closest pair iteratively. All rules stay
//! deterministic and free of graph writes.
//!
//! Results are plain index groups with inner and outer order sorted, so
//! repeated runs agree. Failures report the first relevant node in index
//! order and never panic. Empty graphs yield no groups.

use std::collections::HashMap;

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};

use crate::positions::Positions;
use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep signatures readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// How the distance between two groups is measured during merging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linkage {
    Min,
    Max,
    Mean,
}

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
    hierarchical_clusters_with_linkage(graph, positions, threshold, Linkage::Min)
}

/// Groups nodes with the chosen linkage rule and threshold.
///
/// Shares the validation and ordering contract of [`hierarchical_clusters`].
/// The minimum rule delegates to the union fast path; the maximum and mean
/// rules merge the closest pair iteratively with deterministic tie breaks.
pub fn hierarchical_clusters_with_linkage(
    graph: &Graph,
    positions: &Positions,
    threshold: f32,
    linkage: Linkage,
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
    if linkage == Linkage::Min {
        return min_linkage_groups(&points, threshold);
    }
    agglomerative_groups(&points, threshold, linkage)
}

/// Union fast path for the minimum distance rule.
fn min_linkage_groups(
    points: &[(NodeIndex, f32, f32)],
    threshold: f32,
) -> Result<Vec<Vec<NodeIndex>>, NodeIndex> {
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

/// Iterative closest pair merging for the maximum and mean rules.
///
/// Each round merges the closest pair of groups while their linkage distance
/// stays within `threshold`. Ties break towards smaller member ordinals so
/// repeated runs agree.
fn agglomerative_groups(
    points: &[(NodeIndex, f32, f32)],
    threshold: f32,
    linkage: Linkage,
) -> Result<Vec<Vec<NodeIndex>>, NodeIndex> {
    let mut clusters: Vec<Vec<usize>> = (0..points.len()).map(|ordinal| vec![ordinal]).collect();
    loop {
        let mut best: Option<(usize, usize, f32)> = None;
        for left in 0..clusters.len() {
            for right in (left + 1)..clusters.len() {
                let distance = linkage_distance(points, &clusters[left], &clusters[right], linkage);
                let take = match best {
                    None => true,
                    Some((best_left, best_right, best_distance)) => {
                        distance < best_distance
                            || (distance == best_distance
                                && (left, right) < (best_left, best_right))
                    }
                };
                if take {
                    best = Some((left, right, distance));
                }
            }
        }
        let Some((left, right, distance)) = best else {
            break;
        };
        if distance > threshold {
            break;
        }
        let mut merged = clusters[left].clone();
        merged.extend(clusters[right].iter().copied());
        merged.sort_unstable();
        let (low, high) = if left < right { (left, right) } else { (right, left) };
        clusters[low] = merged;
        clusters.remove(high);
    }
    let groups: Vec<Vec<NodeIndex>> = clusters
        .into_iter()
        .map(|members| members.into_iter().map(|ordinal| points[ordinal].0).collect())
        .collect();
    Ok(sorted_groups(groups))
}

/// Distance between two groups under the chosen linkage rule.
fn linkage_distance(
    points: &[(NodeIndex, f32, f32)],
    first: &[usize],
    second: &[usize],
    linkage: Linkage,
) -> f32 {
    let mut min = f32::INFINITY;
    let mut max = 0.0f32;
    let mut total = 0.0f32;
    let mut count = 0usize;
    for left in first {
        for right in second {
            let dx = points[*left].1 - points[*right].1;
            let dy = points[*left].2 - points[*right].2;
            let distance = dx.hypot(dy);
            if distance < min {
                min = distance;
            }
            if distance > max {
                max = distance;
            }
            total += distance;
            count += 1;
        }
    }
    match linkage {
        Linkage::Min => min,
        Linkage::Max => max,
        Linkage::Mean => {
            if count == 0 {
                f32::INFINITY
            } else {
                total / count as f32
            }
        }
    }
}

/// Disjoint-set union with path compression and union by rank.
struct DisjointSets {    parent: Vec<usize>,
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
                let mut members: Vec<usize> = group.iter().map(|node| node.index()).collect();
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
        let positions: Positions = [(a, Point2::new(0.0, 0.0)), (b, Point2::new(100.0, 0.0))]
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
        let absent = hierarchical_clusters(&graph, &sparse, 50.0).expect_err("missing position");
        assert_eq!(absent, missing);
    }

    #[test]
    fn every_linkage_splits_two_clusters_at_medium_threshold() {
        let (graph, positions) = two_clusters();
        for linkage in [Linkage::Min, Linkage::Max, Linkage::Mean] {
            let groups = hierarchical_clusters_with_linkage(&graph, &positions, 50.0, linkage)
                .expect("valid input");
            assert_eq!(groups.len(), 2, "linkage {linkage:?} keeps two clusters");
            covers_each_node_once(&graph, &groups);
        }
    }

    #[test]
    fn max_linkage_is_stricter_than_min_on_a_chain() {
        let mut graph: Graph = StableGraph::default();
        let nodes: Vec<NodeIndex> = (0..3).map(|ordinal| graph.add_node(labelled(&ordinal.to_string()))).collect();
        let positions: Positions = [
            (nodes[0], Point2::new(0.0, 0.0)),
            (nodes[1], Point2::new(2.0, 0.0)),
            (nodes[2], Point2::new(3.0, 0.0)),
        ]
        .into_iter()
        .collect();
        let min = hierarchical_clusters_with_linkage(&graph, &positions, 2.5, Linkage::Min)
            .expect("valid input");
        assert_eq!(min.len(), 1);
        let max = hierarchical_clusters_with_linkage(&graph, &positions, 2.5, Linkage::Max)
            .expect("valid input");
        assert_eq!(max.len(), 2);
    }
}
