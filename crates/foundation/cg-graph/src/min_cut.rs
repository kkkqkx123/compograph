//! Deterministic global minimum cut over the store graph.
//!
//! The cut is self researched with the Stoer-Wagner method: petgraph 0.8.3
//! offers no minimum cut, and randomization is avoided so repeated runs agree,
//! matching the deterministic geometry policy. The directed store is read as
//! undirected, parallel edges count separately through summed weights, and
//! self loops are ignored because they never cross a partition.
//!
//! The result carries the cut weight plus one endpoint pair per crossing
//! physical edge, so parallel pairs appear once per edge. Pairs keep the
//! smaller endpoint first and arrive sorted, matching [`crate::algo::bridges`],
//! which lets the cut highlight outcome reuse them directly. Weights are the
//! caller's responsibility as with Dijkstra: negative values participate
//! arithmetically and callers seeking classical cuts pass non-negative maps.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};

use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep signatures readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Global minimum cut: total weight plus crossing endpoint pairs.
///
/// `edges` holds one entry per crossing physical edge with the smaller
/// endpoint first, sorted by endpoint indices. Disconnected, empty, and
/// single-node graphs yield a zero cut with no edges.
#[derive(Clone, Debug, PartialEq)]
pub struct MinCut {
    pub weight: f32,
    pub edges: Vec<(NodeIndex, NodeIndex)>,
}

/// Deterministic global minimum cut under undirected semantics.
///
/// Every directed edge links its endpoints both ways with its stored weight;
/// parallel edges sum, self loops are skipped. Ties break towards smaller
/// ordinal indices, so repeated runs over the same structure agree.
pub fn global_min_cut(graph: &Graph) -> MinCut {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    let size = order.len();
    if size < 2 {
        return MinCut {
            weight: 0.0,
            edges: Vec::new(),
        };
    }
    let position: HashMap<NodeIndex, usize> = order
        .iter()
        .enumerate()
        .map(|(ordinal, node)| (*node, ordinal))
        .collect();
    let mut weights = vec![vec![0.0f32; size]; size];
    let mut crossings: Vec<(usize, usize, f32)> = Vec::new();
    for reference in graph.edge_references() {
        let source = reference.source();
        let target = reference.target();
        if source == target {
            continue;
        }
        let row = position.get(&source).copied().unwrap_or(usize::MAX);
        let column = position.get(&target).copied().unwrap_or(usize::MAX);
        if row == usize::MAX || column == usize::MAX {
            continue;
        }
        let weight = reference.weight().weight;
        weights[row][column] += weight;
        weights[column][row] += weight;
        crossings.push((row, column, weight));
    }
    if crossings.is_empty() {
        return MinCut {
            weight: 0.0,
            edges: Vec::new(),
        };
    }
    let mut members: Vec<Vec<usize>> = (0..size).map(|ordinal| vec![ordinal]).collect();
    let mut live = vec![true; size];
    let mut live_count = size;
    let mut best_weight = f32::INFINITY;
    let mut best_side: Vec<usize> = Vec::new();
    while live_count > 1 {
        let origin = (0..size).filter(|ordinal| live[*ordinal]).min();
        let Some(first) = origin else {
            break;
        };
        let mut added = vec![false; size];
        let mut reached = vec![0.0f32; size];
        let mut sequence = Vec::new();
        added[first] = true;
        sequence.push(first);
        for ordinal in 0..size {
            if live[ordinal] && !added[ordinal] {
                reached[ordinal] = weights[first][ordinal];
            }
        }
        while sequence.len() < live_count {
            let mut pick: Option<usize> = None;
            for ordinal in 0..size {
                if !live[ordinal] || added[ordinal] {
                    continue;
                }
                let take = match pick {
                    None => true,
                    Some(current) => reached[ordinal]
                        .partial_cmp(&reached[current])
                        .unwrap_or(Ordering::Equal)
                        == Ordering::Greater
                        || (reached[ordinal]
                            .partial_cmp(&reached[current])
                            .unwrap_or(Ordering::Equal)
                            == Ordering::Equal
                            && ordinal < current),
                };
                if take {
                    pick = Some(ordinal);
                }
            }
            let Some(next) = pick else {
                break;
            };
            added[next] = true;
            sequence.push(next);
            for ordinal in 0..size {
                if live[ordinal] && !added[ordinal] {
                    reached[ordinal] += weights[next][ordinal];
                }
            }
        }
        if sequence.len() < 2 {
            break;
        }
        let target = sequence[sequence.len() - 1];
        let source = sequence[sequence.len() - 2];
        let phase_weight = reached[target];
        if phase_weight
            .partial_cmp(&best_weight)
            .unwrap_or(Ordering::Equal)
            == Ordering::Less
        {
            best_weight = phase_weight;
            best_side = members[target].clone();
        }
        for ordinal in 0..size {
            if live[ordinal] {
                weights[source][ordinal] += weights[target][ordinal];
                weights[ordinal][source] += weights[ordinal][target];
            }
        }
        let mut merged = members[source].clone();
        merged.extend(members[target].iter().copied());
        merged.sort_unstable();
        members[source] = merged;
        members[target] = Vec::new();
        live[target] = false;
        live_count -= 1;
    }
    if best_weight.is_infinite() {
        return MinCut {
            weight: 0.0,
            edges: Vec::new(),
        };
    }
    let inside: HashSet<usize> = best_side.into_iter().collect();
    let mut edges = Vec::new();
    let mut total = 0.0f32;
    for (row, column, weight) in crossings {
        let left = inside.contains(&row);
        let right = inside.contains(&column);
        if left != right {
            let first = order[row];
            let second = order[column];
            if first.index() <= second.index() {
                edges.push((first, second));
            } else {
                edges.push((second, first));
            }
            total += weight;
        }
    }
    edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    MinCut {
        weight: total,
        edges,
    }
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

    fn chain3() -> (Graph, NodeIndex, NodeIndex, NodeIndex) {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(labelled("a"));
        let b = graph.add_node(labelled("b"));
        let c = graph.add_node(labelled("c"));
        graph.add_edge(a, b, weighted(1.0));
        graph.add_edge(b, c, weighted(1.0));
        (graph, a, b, c)
    }

    fn cut_is_consistent(graph: &Graph, cut: &MinCut) {
        let mut sorted = cut.edges.clone();
        sorted.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        assert_eq!(cut.edges, sorted);
        for (source, target) in &cut.edges {
            assert!(source.index() <= target.index());
            assert!(graph.node_weight(*source).is_some());
            assert!(graph.node_weight(*target).is_some());
        }
        assert!(cut.weight.is_finite());
        assert!(cut.weight >= 0.0);
    }

    #[test]
    fn chain_cut_is_one_unit_edge() {
        let (graph, _, _, _) = chain3();
        let cut = global_min_cut(&graph);
        assert!((cut.weight - 1.0).abs() < 1e-5);
        assert_eq!(cut.edges.len(), 1);
        cut_is_consistent(&graph, &cut);
    }

    #[test]
    fn ring_cut_isolates_one_node() {
        let mut graph: Graph = StableGraph::default();
        let nodes: Vec<NodeIndex> = (0..3)
            .map(|ordinal| graph.add_node(labelled(&ordinal.to_string())))
            .collect();
        graph.add_edge(nodes[0], nodes[1], weighted(1.0));
        graph.add_edge(nodes[1], nodes[2], weighted(1.0));
        graph.add_edge(nodes[2], nodes[0], weighted(1.0));
        let cut = global_min_cut(&graph);
        assert!((cut.weight - 2.0).abs() < 1e-5);
        assert_eq!(cut.edges.len(), 2);
        cut_is_consistent(&graph, &cut);
    }

    #[test]
    fn parallel_edges_count_separately() {
        let mut graph: Graph = StableGraph::default();
        let x = graph.add_node(labelled("x"));
        let y = graph.add_node(labelled("y"));
        graph.add_edge(x, y, weighted(1.0));
        graph.add_edge(x, y, weighted(2.0));
        let cut = global_min_cut(&graph);
        assert!((cut.weight - 3.0).abs() < 1e-5);
        assert_eq!(cut.edges.len(), 2);
        cut_is_consistent(&graph, &cut);
    }

    #[test]
    fn self_loops_never_join_the_cut() {
        let mut graph: Graph = StableGraph::default();
        let node = graph.add_node(labelled("only"));
        graph.add_edge(node, node, weighted(5.0));
        let cut = global_min_cut(&graph);
        assert_eq!(cut.weight, 0.0);
        assert!(cut.edges.is_empty());
    }

    #[test]
    fn empty_lonely_and_disconnected_graphs_cut_nothing() {
        let empty: Graph = StableGraph::default();
        let cut = global_min_cut(&empty);
        assert_eq!(cut.weight, 0.0);
        assert!(cut.edges.is_empty());
        let mut lonely: Graph = StableGraph::default();
        lonely.add_node(labelled("lonely"));
        let cut = global_min_cut(&lonely);
        assert_eq!(cut.weight, 0.0);
        assert!(cut.edges.is_empty());
        let mut split: Graph = StableGraph::default();
        let a = split.add_node(labelled("a"));
        let b = split.add_node(labelled("b"));
        split.add_edge(a, b, weighted(1.0));
        split.add_node(labelled("c"));
        split.add_node(labelled("d"));
        let cut = global_min_cut(&split);
        assert_eq!(cut.weight, 0.0);
        assert!(cut.edges.is_empty());
    }

    #[test]
    fn repeated_runs_agree() {
        let (graph, _, _, _) = chain3();
        let first = global_min_cut(&graph);
        let second = global_min_cut(&graph);
        assert_eq!(first, second);
    }
}
