//! Centrality scores over the read-only graph view.
//!
//! The trio is self researched: petgraph offers no centrality beyond
//! PageRank, so degree, closeness and betweenness are implemented here
//! against [`GraphView`] rather than petgraph generics. That keeps the
//! scores usable from layout and panel code without leaking storage types,
//! and headless tests can run them against [`MockGraph`].
//!
//! All three read the store as undirected: traversal follows neighbours in
//! either direction, matching the cut searches in [`crate::algo`]. Scores
//! arrive as plain vectors parallel to [`node_order`], the same convention
//! as [`rank_nodes`]. Isolated nodes score zero; empty graphs
//! yield empty vectors.

use std::collections::{HashMap, VecDeque};

use petgraph::Directed;
use petgraph::algo::page_rank;
use petgraph::stable_graph::{NodeIndex, StableGraph};

use crate::store::{EdgeData, NodeData};
use crate::view::GraphView;

/// Node identifiers in the order the centrality vectors follow.
///
/// Sorted by index for determinism; every trio function aligns its output
/// with this order so callers can zip scores back to nodes.
pub fn node_order(graph: &dyn GraphView) -> Vec<NodeIndex> {
    let mut ids = graph.node_ids();
    ids.sort_unstable_by_key(|node| node.index());
    ids
}

/// Degree centrality: distinct-neighbour count normalized by `n - 1`.
///
/// Neighbours collapse parallel edges and ignore self loops, so the hub of a
/// star scores exactly one while leaves share `1 / (n - 1)`. Isolated nodes
/// score zero, and graphs with fewer than two nodes yield only zeros.
pub fn degree_centrality(graph: &dyn GraphView) -> Vec<f32> {
    let order = node_order(graph);
    let count = order.len();
    if count < 2 {
        return vec![0.0; count];
    }
    let scale = (count - 1) as f32;
    order
        .iter()
        .map(|node| {
            graph
                .neighbors(*node)
                .into_iter()
                .filter(|neighbour| *neighbour != *node)
                .count() as f32
                / scale
        })
        .collect()
}

/// Closeness centrality over undirected distances.
///
/// Each score is the Wasserman-Faust improved value: the reachable share
/// `reachable / (n - 1)` times `reachable / distance_sum`, so disconnected
/// graphs degrade gracefully instead of dividing by zero. Unreachable nodes
/// simply do not contribute, and nodes reaching nothing score zero.
pub fn closeness_centrality(graph: &dyn GraphView) -> Vec<f32> {
    let order = node_order(graph);
    let count = order.len();
    if count < 2 {
        return vec![0.0; count];
    }
    let adjacency = undirected_index_adjacency(graph, &order);
    let scale = (count - 1) as f32;
    order
        .iter()
        .enumerate()
        .map(|(source, _)| {
            let distances = bfs_distances(&adjacency, source);
            let mut reachable = 0usize;
            let mut total = 0u32;
            for (target, distance) in distances.iter().enumerate() {
                if target != source && *distance >= 0 {
                    reachable += 1;
                    total += *distance as u32;
                }
            }
            if reachable == 0 || total == 0 {
                0.0
            } else {
                let reachable = reachable as f32;
                (reachable / scale) * (reachable / total as f32)
            }
        })
        .collect()
}

/// Betweenness centrality via the Brandes accumulation over undirected paths.
///
/// Every undirected edge is walked as two directed arcs and each unordered
/// node pair is therefore counted twice; the single division by
/// `(n - 1) * (n - 2)` folds both the double counting and the `[0, 1]`
/// normalization into one step. Graphs with fewer than three nodes yield
/// only zeros.
pub fn betweenness_centrality(graph: &dyn GraphView) -> Vec<f32> {
    let order = node_order(graph);
    let count = order.len();
    if count < 3 {
        return vec![0.0; count];
    }
    let adjacency = undirected_index_adjacency(graph, &order);
    let mut raw = vec![0.0f64; count];
    for source in 0..count {
        let mut stack = Vec::new();
        let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); count];
        let mut paths = vec![0.0f64; count];
        paths[source] = 1.0;
        let mut distance = vec![-1i32; count];
        distance[source] = 0;
        let mut queue = VecDeque::from([source]);
        while let Some(node) = queue.pop_front() {
            stack.push(node);
            for &next in &adjacency[node] {
                if distance[next] < 0 {
                    distance[next] = distance[node] + 1;
                    queue.push_back(next);
                }
                if distance[next] == distance[node] + 1 {
                    paths[next] += paths[node];
                    predecessors[next].push(node);
                }
            }
        }
        let mut dependency = vec![0.0f64; count];
        while let Some(node) = stack.pop() {
            for &parent in &predecessors[node] {
                if paths[node] > 0.0 {
                    dependency[parent] += paths[parent] / paths[node] * (1.0 + dependency[node]);
                }
            }
            if node != source {
                raw[node] += dependency[node];
            }
        }
    }
    let scale = ((count - 1) * (count - 2)) as f64;
    raw.iter().map(|score| (score / scale) as f32).collect()
}

/// PageRank scores, parallel to the node order of the store graph.
///
/// Returns an empty vector for an empty graph. `damping` must lie in `[0, 1]`.
pub fn rank_nodes(
    graph: &StableGraph<NodeData, EdgeData, Directed>,
    damping: f32,
    iterations: usize,
) -> Vec<f32> {
    page_rank(graph, damping, iterations)
}

/// Undirected adjacency in ordinal space for the trio searches.
///
/// Neighbours collapse parallel edges through [`GraphView::neighbors`], and
/// self loops are dropped because they never shorten a path.
fn undirected_index_adjacency(graph: &dyn GraphView, order: &[NodeIndex]) -> Vec<Vec<usize>> {
    let position: HashMap<NodeIndex, usize> = order
        .iter()
        .enumerate()
        .map(|(ordinal, node)| (*node, ordinal))
        .collect();
    order
        .iter()
        .map(|node| {
            let mut neighbours: Vec<usize> = graph
                .neighbors(*node)
                .into_iter()
                .filter(|neighbour| *neighbour != *node)
                .filter_map(|neighbour| position.get(&neighbour).copied())
                .collect();
            neighbours.sort_unstable();
            neighbours.dedup();
            neighbours
        })
        .collect()
}

/// Unweighted distances from `source`, with `-1` marking unreachable nodes.
fn bfs_distances(adjacency: &[Vec<usize>], source: usize) -> Vec<i32> {
    let mut distance = vec![-1i32; adjacency.len()];
    distance[source] = 0;
    let mut queue = VecDeque::from([source]);
    while let Some(node) = queue.pop_front() {
        for &next in &adjacency[node] {
            if distance[next] < 0 {
                distance[next] = distance[node] + 1;
                queue.push_back(next);
            }
        }
    }
    distance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::MockGraph;

    /// Star with bidirectional spokes, so the store reads as one undirected
    /// hub with four leaves.
    fn star() -> MockGraph {
        let mut graph = MockGraph::isolated(5);
        for leaf in 1..5 {
            graph.push_edge(0, leaf);
            graph.push_edge(leaf, 0);
        }
        graph
    }

    /// Bidirectional line of five nodes, read as one undirected chain.
    fn chain5() -> MockGraph {
        let mut graph = MockGraph::isolated(5);
        for index in 0..4 {
            graph.push_edge(index, index + 1);
            graph.push_edge(index + 1, index);
        }
        graph
    }

    #[test]
    fn hub_outranks_leaves_on_every_measure() {
        let graph = star();
        for scores in [
            degree_centrality(&graph),
            closeness_centrality(&graph),
            betweenness_centrality(&graph),
        ] {
            assert_eq!(scores.len(), 5);
            assert!(scores.iter().all(|score| score.is_finite()));
            assert!(scores[0] > scores[1]);
            assert!(scores[1..].iter().all(|score| *score == scores[1]));
        }
        assert_eq!(degree_centrality(&graph)[0], 1.0);
        assert_eq!(betweenness_centrality(&graph)[0], 1.0);
    }

    #[test]
    fn chain_middle_outranks_endpoints_on_every_measure() {
        let graph = chain5();
        let middle = 2;
        for scores in [
            degree_centrality(&graph),
            closeness_centrality(&graph),
            betweenness_centrality(&graph),
        ] {
            assert_eq!(scores.len(), 5);
            assert!(scores[middle] > scores[0]);
            assert!(scores[middle] > scores[4]);
            assert_eq!(scores[0], scores[4]);
        }
    }

    #[test]
    fn empty_and_lonely_graphs_score_zero_without_failing() {
        let empty = MockGraph::empty();
        assert!(degree_centrality(&empty).is_empty());
        assert!(closeness_centrality(&empty).is_empty());
        assert!(betweenness_centrality(&empty).is_empty());
        let lonely = MockGraph::isolated(1);
        assert_eq!(degree_centrality(&lonely), vec![0.0]);
        assert_eq!(closeness_centrality(&lonely), vec![0.0]);
        assert_eq!(betweenness_centrality(&lonely), vec![0.0]);
        let pair = MockGraph::isolated(2);
        assert_eq!(betweenness_centrality(&pair), vec![0.0, 0.0]);
    }

    #[test]
    fn disconnected_graphs_stay_finite() {
        let mut graph = MockGraph::isolated(4);
        graph.push_edge(0, 1);
        graph.push_edge(1, 0);
        let closeness = closeness_centrality(&graph);
        assert!(closeness.iter().all(|score| score.is_finite()));
        assert!(closeness[0] > closeness[2]);
        assert_eq!(closeness[2], 0.0);
        let betweenness = betweenness_centrality(&graph);
        assert!(betweenness.iter().all(|score| score.is_finite()));
    }

    #[test]
    fn node_order_sorts_by_index() {
        let graph = chain5();
        let order = node_order(&graph);
        assert_eq!(
            order,
            vec![
                NodeIndex::new(0),
                NodeIndex::new(1),
                NodeIndex::new(2),
                NodeIndex::new(3),
                NodeIndex::new(4),
            ]
        );
    }

    fn rank_diamond() -> StableGraph<NodeData, EdgeData, Directed> {
        let mut graph: StableGraph<NodeData, EdgeData, Directed> = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        let d = graph.add_node(NodeData { label: "d".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, d, EdgeData { weight: 2.0 });
        graph.add_edge(a, c, EdgeData { weight: 4.0 });
        graph.add_edge(c, d, EdgeData { weight: 1.0 });
        graph
    }

    #[test]
    fn rank_nodes_returns_one_score_per_node() {
        let graph = rank_diamond();
        let ranks = rank_nodes(&graph, 0.85, 20);
        assert_eq!(ranks.len(), graph.node_count());
        assert!(ranks.iter().all(|rank| rank.is_finite() && *rank >= 0.0));
    }

    #[test]
    fn ranking_an_empty_graph_yields_no_scores() {
        let graph: StableGraph<NodeData, EdgeData, Directed> = StableGraph::default();
        assert!(rank_nodes(&graph, 0.85, 10).is_empty());
    }
}
