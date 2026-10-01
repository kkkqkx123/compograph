//! Deterministic affinity propagation over layout positions.
//!
//! Similarity is the negative squared Euclidean distance, and the shared
//! preference is the median of the off-diagonal similarities, so well
//! separated point clouds naturally elect one exemplar each without any
//! caller supplied seed. Responsibility and availability messages start at
//! zero and are damped every round, which keeps repeated runs identical.
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

/// Groups nodes by affinity propagation over their positions.
///
/// `damping` must be finite within half inclusive and one exclusive, and
/// `max_iterations` must be at least one. Every node must have a finite
/// position. Isolated nodes join their nearest exemplar by similarity, so no
/// separate singleton rule exists. Dense storage suits small and medium
/// graphs; very large graphs belong in a background task with a prior size
/// check by the caller.
pub fn affinity_clusters(
    graph: &Graph,
    positions: &Positions,
    damping: f32,
    max_iterations: usize,
) -> Result<Vec<Vec<NodeIndex>>, NodeIndex> {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    if order.is_empty() {
        return Ok(Vec::new());
    }
    if !damping.is_finite() || damping < 0.5 || damping >= 1.0 || max_iterations == 0 {
        let first = order.first().copied().unwrap_or(NodeIndex::new(0));
        return Err(first);
    }
    let mut points: Vec<(NodeIndex, f64, f64)> = Vec::with_capacity(order.len());
    for node in &order {
        match positions.get(node) {
            Some(point) if point.x.is_finite() && point.y.is_finite() => {
                points.push((*node, f64::from(point.x), f64::from(point.y)));
            }
            _ => return Err(*node),
        }
    }
    if points.len() == 1 {
        return Ok(vec![vec![points[0].0]]);
    }
    let size = points.len();
    let damping = f64::from(damping);
    let mut similarity = vec![0.0f64; size * size];
    let mut off_diagonal: Vec<f64> = Vec::with_capacity(size * (size - 1));
    for left in 0..size {
        for right in 0..size {
            if left == right {
                continue;
            }
            let dx = points[left].1 - points[right].1;
            let dy = points[left].2 - points[right].2;
            let value = -(dx * dx + dy * dy);
            similarity[left * size + right] = value;
            off_diagonal.push(value);
        }
    }
    let preference = median_of(&mut off_diagonal);
    for diagonal in 0..size {
        similarity[diagonal * size + diagonal] = preference;
    }
    let mut responsibility = vec![0.0f64; size * size];
    let mut availability = vec![0.0f64; size * size];
    for _ in 0..max_iterations {
        update_responsibility(
            &similarity,
            &availability,
            &mut responsibility,
            size,
            damping,
        );
        update_availability(&responsibility, &mut availability, size, damping);
    }
    let exemplars = elect_exemplars(&responsibility, &availability, size);
    Ok(assign_to_exemplars(&similarity, &order, &exemplars, size))
}

/// Median of the values, sorting in place; zero when empty.
fn median_of(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        (values[middle - 1] + values[middle]) / 2.0
    }
}

/// Damped responsibility update with row-wise top-two maxima.
fn update_responsibility(
    similarity: &[f64],
    availability: &[f64],
    responsibility: &mut [f64],
    size: usize,
    damping: f64,
) {
    for row in 0..size {
        let mut best = f64::NEG_INFINITY;
        let mut second = f64::NEG_INFINITY;
        let mut arg_best = 0usize;
        for column in 0..size {
            let value = availability[row * size + column] + similarity[row * size + column];
            if value > best {
                second = best;
                best = value;
                arg_best = column;
            } else if value > second {
                second = value;
            }
        }
        for column in 0..size {
            let target = if column == arg_best { second } else { best };
            let fresh = similarity[row * size + column] - target;
            let slot = row * size + column;
            responsibility[slot] = damping * responsibility[slot] + (1.0 - damping) * fresh;
        }
    }
}

/// Damped availability update from positive responsibilities.
fn update_availability(
    responsibility: &[f64],
    availability: &mut [f64],
    size: usize,
    damping: f64,
) {
    for column in 0..size {
        let mut positive_sum = 0.0f64;
        for row in 0..size {
            if responsibility[row * size + column] > 0.0 {
                positive_sum += responsibility[row * size + column];
            }
        }
        let self_responsibility = responsibility[column * size + column];
        let self_positive = if self_responsibility > 0.0 {
            self_responsibility
        } else {
            0.0
        };
        for row in 0..size {
            let fresh = if row == column {
                positive_sum - self_positive
            } else {
                let row_positive = if responsibility[row * size + column] > 0.0 {
                    responsibility[row * size + column]
                } else {
                    0.0
                };
                let capped = self_responsibility + positive_sum - row_positive - self_positive;
                if capped > 0.0 { 0.0 } else { capped }
            };
            let slot = row * size + column;
            availability[slot] = damping * availability[slot] + (1.0 - damping) * fresh;
        }
    }
}

/// Exemplar columns with a positive self verdict, falling back to the best one.
fn elect_exemplars(responsibility: &[f64], availability: &[f64], size: usize) -> Vec<usize> {
    let mut exemplars = Vec::new();
    for column in 0..size {
        if responsibility[column * size + column] + availability[column * size + column] > 0.0 {
            exemplars.push(column);
        }
    }
    if exemplars.is_empty() {
        let mut best = 0usize;
        let mut best_value = f64::NEG_INFINITY;
        for column in 0..size {
            let value =
                responsibility[column * size + column] + availability[column * size + column];
            if value > best_value {
                best_value = value;
                best = column;
            }
        }
        exemplars.push(best);
    }
    exemplars
}

/// Assigns every node to its most similar exemplar, ties broken by order.
fn assign_to_exemplars(
    similarity: &[f64],
    order: &[NodeIndex],
    exemplars: &[usize],
    size: usize,
) -> Vec<Vec<NodeIndex>> {
    let mut ranked = exemplars.to_vec();
    ranked.sort_unstable();
    let mut buckets: HashMap<usize, Vec<NodeIndex>> = HashMap::new();
    for (row, node) in order.iter().enumerate() {
        let mut best = ranked[0];
        let mut best_value = similarity[row * size + best];
        for candidate in ranked.iter().skip(1) {
            let value = similarity[row * size + candidate];
            if value > best_value {
                best_value = value;
                best = *candidate;
            }
        }
        buckets.entry(best).or_default().push(*node);
    }
    sorted_groups(buckets.into_values().collect())
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
    fn two_clouds_elect_two_exemplars() {
        let (graph, positions) = two_clusters();
        let groups = affinity_clusters(&graph, &positions, 0.5, 100).expect("valid input");
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
        let positions: Positions = StdHashMap::new();
        let groups = affinity_clusters(&graph, &positions, 0.5, 50).expect("empty");
        assert!(groups.is_empty());
    }

    #[test]
    fn single_node_forms_one_group() {
        let mut graph: Graph = StableGraph::default();
        let only = graph.add_node(labelled("solo"));
        let positions: Positions = [(only, Point2::new(3.0, 4.0))].into_iter().collect();
        let groups = affinity_clusters(&graph, &positions, 0.7, 20).expect("valid input");
        assert_eq!(groups, vec![vec![only]]);
    }

    #[test]
    fn repeated_runs_agree_in_order() {
        let (graph, positions) = two_clusters();
        let first = affinity_clusters(&graph, &positions, 0.5, 100).expect("valid input");
        for _ in 0..5 {
            let next = affinity_clusters(&graph, &positions, 0.5, 100).expect("valid input");
            assert_eq!(first, next);
        }
    }

    #[test]
    fn invalid_params_and_missing_positions_report_a_node() {
        let (graph, positions) = two_clusters();
        let low = affinity_clusters(&graph, &positions, 0.2, 20).expect_err("damping below range");
        assert!(graph.node_weight(low).is_some());
        let high = affinity_clusters(&graph, &positions, 1.0, 20).expect_err("damping at one");
        assert!(graph.node_weight(high).is_some());
        let nan = affinity_clusters(&graph, &positions, f32::NAN, 20).expect_err("nan damping");
        assert!(graph.node_weight(nan).is_some());
        let idle = affinity_clusters(&graph, &positions, 0.5, 0).expect_err("empty budget");
        assert!(graph.node_weight(idle).is_some());
        let mut sparse = positions;
        let missing = graph.node_indices().next().expect("a node exists");
        sparse.remove(&missing);
        let absent = affinity_clusters(&graph, &sparse, 0.5, 20).expect_err("missing position");
        assert_eq!(absent, missing);
    }
}
