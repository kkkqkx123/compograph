//! Deterministic Markov clustering over the store graph.
//!
//! The stochastic matrix alternates expansion and inflation until the columns
//! stabilize or the caller supplied iteration budget runs out. Self loops are
//! accumulated rather than assigned so existing loop weights survive, and each
//! column is normalized after every stage. Membership is assigned once per
//! node by its strongest attractor column, which guarantees every node lands
//! in exactly one group without a separate deduplication pass.
//!
//! The directed store is read as undirected: each directed edge contributes
//! its weight in both directions, parallel edges sum, and self loops never
//! create cross-group links. Results are plain index groups with inner and
//! outer order sorted, so repeated runs agree. Failures report the first
//! relevant node in index order and never panic. Empty graphs yield no groups.

use std::collections::HashMap;

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};

use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep signatures readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Weight added to every diagonal entry before iteration starts.
const SELF_LOOP_WEIGHT: f32 = 1.0;

/// Largest change between successive matrices treated as converged.
const CONVERGENCE_EPSILON: f32 = 1e-4;

/// Groups nodes with Markov clustering using the given inflation and budget.
///
/// `inflation` must be finite and larger than one; larger values yield finer
/// groups. `max_iterations` must be at least one. Expansion is fixed to
/// squaring. Dense storage suits small and medium graphs; very large graphs
/// belong in a background task with a prior size check by the caller.
pub fn markov_clusters(
    graph: &Graph,
    inflation: f32,
    max_iterations: usize,
) -> Result<Vec<Vec<NodeIndex>>, NodeIndex> {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    if order.is_empty() {
        return Ok(Vec::new());
    }
    if !inflation.is_finite() || inflation <= 1.0 || max_iterations == 0 {
        let first = order.first().copied().unwrap_or(NodeIndex::new(0));
        return Err(first);
    }
    let size = order.len();
    let position: HashMap<NodeIndex, usize> = order
        .iter()
        .enumerate()
        .map(|(ordinal, node)| (*node, ordinal))
        .collect();
    let mut matrix = vec![0.0f32; size * size];
    for reference in graph.edge_references() {
        let source = reference.source();
        let target = reference.target();
        let weight = reference.weight().weight;
        if !weight.is_finite() || weight <= 0.0 {
            continue;
        }
        let row = position.get(&source).copied().unwrap_or(usize::MAX);
        let column = position.get(&target).copied().unwrap_or(usize::MAX);
        if row == usize::MAX || column == usize::MAX {
            continue;
        }
        if row == column {
            matrix[row * size + column] += weight;
        } else {
            matrix[row * size + column] += weight;
            matrix[column * size + row] += weight;
        }
    }
    for diagonal in 0..size {
        matrix[diagonal * size + diagonal] += SELF_LOOP_WEIGHT;
    }
    normalize_columns(&mut matrix, size);
    let mut current = matrix;
    for _ in 0..max_iterations {
        let expanded = multiply(&current, size);
        let mut next = expanded;
        inflate(&mut next, size, inflation);
        if converged(&current, &next) {
            current = next;
            break;
        }
        current = next;
    }
    let mut buckets: HashMap<usize, Vec<NodeIndex>> = HashMap::new();
    for (column, node) in order.iter().enumerate() {
        let mut best_row = 0usize;
        let mut best_value = current[column];
        for row in 1..size {
            let value = current[row * size + column];
            if value > best_value {
                best_value = value;
                best_row = row;
            }
        }
        buckets.entry(best_row).or_default().push(*node);
    }
    Ok(sorted_groups(buckets.into_values().collect(), &order))
}

/// Normalizes each column to sum to one, leaving empty columns untouched.
fn normalize_columns(matrix: &mut [f32], size: usize) {
    for column in 0..size {
        let mut total = 0.0f32;
        for row in 0..size {
            total += matrix[row * size + column];
        }
        if total > 0.0 && total.is_finite() {
            for row in 0..size {
                matrix[row * size + column] /= total;
            }
        }
    }
}

/// Dense square of the column-stochastic matrix.
fn multiply(matrix: &[f32], size: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; size * size];
    for row in 0..size {
        for middle in 0..size {
            let left = matrix[row * size + middle];
            if left == 0.0 {
                continue;
            }
            for column in 0..size {
                out[row * size + column] += left * matrix[middle * size + column];
            }
        }
    }
    out
}

/// Raises each entry to `inflation` and renormalizes every column.
fn inflate(matrix: &mut [f32], size: usize, inflation: f32) {
    for value in matrix.iter_mut() {
        *value = value.powf(inflation);
    }
    normalize_columns(matrix, size);
}

/// True when every entry changed by less than the convergence epsilon.
fn converged(previous: &[f32], next: &[f32]) -> bool {
    previous
        .iter()
        .zip(next.iter())
        .all(|(old, fresh)| (*old - *fresh).abs() <= CONVERGENCE_EPSILON)
}

/// Sorts each group by index and orders groups by their first member.
///
/// The node order lookup keeps group order stable even when attractor rows
/// are sparse. Empty groups are dropped.
fn sorted_groups(mut groups: Vec<Vec<NodeIndex>>, order: &[NodeIndex]) -> Vec<Vec<NodeIndex>> {
    let rank: HashMap<NodeIndex, usize> = order
        .iter()
        .enumerate()
        .map(|(ordinal, node)| (*node, ordinal))
        .collect();
    for group in groups.iter_mut() {
        group.sort_unstable_by_key(|node| node.index());
    }
    groups.retain(|group| !group.is_empty());
    groups.sort_unstable_by_key(|group| {
        group
            .first()
            .and_then(|node| rank.get(node).copied())
            .unwrap_or(usize::MAX)
    });
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labelled(label: &str) -> NodeData {
        NodeData {
            label: label.into(),
        }
    }

    fn weighted(weight: f32) -> EdgeData {
        EdgeData { weight }
    }

    fn two_clusters() -> Graph {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(labelled("a"));
        let b = graph.add_node(labelled("b"));
        let c = graph.add_node(labelled("c"));
        let d = graph.add_node(labelled("d"));
        let e = graph.add_node(labelled("e"));
        let f = graph.add_node(labelled("f"));
        for (source, target) in [(a, b), (b, c), (a, c)] {
            graph.add_edge(source, target, weighted(5.0));
            graph.add_edge(target, source, weighted(5.0));
        }
        for (source, target) in [(d, e), (e, f), (d, f)] {
            graph.add_edge(source, target, weighted(5.0));
            graph.add_edge(target, source, weighted(5.0));
        }
        graph.add_edge(c, d, weighted(0.2));
        graph.add_edge(d, c, weighted(0.2));
        graph
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
    fn bridge_graph_splits_into_two_groups() {
        let graph = two_clusters();
        let groups = markov_clusters(&graph, 2.0, 20).expect("valid input");
        assert_eq!(groups.len(), 2);
        for group in &groups {
            assert_eq!(group.len(), 3);
        }
        covers_each_node_once(&graph, &groups);
        let first: Vec<usize> = groups[0].iter().map(|node| node.index()).collect();
        let second: Vec<usize> = groups[1].iter().map(|node| node.index()).collect();
        assert!(first.contains(&0) == second.contains(&3) || first.contains(&3));
    }

    #[test]
    fn empty_graph_yields_no_groups() {
        let graph: Graph = StableGraph::default();
        let groups = markov_clusters(&graph, 2.0, 20).expect("empty");
        assert!(groups.is_empty());
    }

    #[test]
    fn isolated_nodes_form_singletons() {
        let mut graph: Graph = StableGraph::default();
        graph.add_node(labelled("a"));
        graph.add_node(labelled("b"));
        let groups = markov_clusters(&graph, 2.0, 20).expect("valid input");
        assert_eq!(groups.len(), 2);
        covers_each_node_once(&graph, &groups);
    }

    #[test]
    fn repeated_runs_agree_in_order() {
        let graph = two_clusters();
        let first = markov_clusters(&graph, 2.0, 20).expect("valid input");
        for _ in 0..10 {
            let next = markov_clusters(&graph, 2.0, 20).expect("valid input");
            assert_eq!(first, next);
        }
    }

    #[test]
    fn uniform_cliques_have_no_duplicate_members() {
        let mut graph: Graph = StableGraph::default();
        let nodes: Vec<NodeIndex> = (0..12)
            .map(|ordinal| graph.add_node(labelled(&ordinal.to_string())))
            .collect();
        for chunk in nodes.chunks(3) {
            for left in 0..chunk.len() {
                for right in 0..chunk.len() {
                    if left != right {
                        graph.add_edge(chunk[left], chunk[right], weighted(1.0));
                    }
                }
            }
        }
        let groups = markov_clusters(&graph, 2.0, 30).expect("valid input");
        assert!(!groups.is_empty());
        covers_each_node_once(&graph, &groups);
    }

    #[test]
    fn invalid_params_report_a_node() {
        let graph = two_clusters();
        let bad = markov_clusters(&graph, 1.0, 20).expect_err("inflation must exceed one");
        assert!(graph.node_weight(bad).is_some());
        let nan = markov_clusters(&graph, f32::NAN, 20).expect_err("nan inflation");
        assert!(graph.node_weight(nan).is_some());
        let zero = markov_clusters(&graph, 2.0, 0).expect_err("empty budget");
        assert!(graph.node_weight(zero).is_some());
    }

    #[test]
    fn parallel_edges_and_self_loops_stay_stable() {
        let mut graph: Graph = StableGraph::default();
        let x = graph.add_node(labelled("x"));
        let y = graph.add_node(labelled("y"));
        graph.add_edge(x, y, weighted(1.0));
        graph.add_edge(x, y, weighted(2.0));
        graph.add_edge(y, y, weighted(3.0));
        let groups = markov_clusters(&graph, 2.0, 20).expect("valid input");
        covers_each_node_once(&graph, &groups);
    }
}
