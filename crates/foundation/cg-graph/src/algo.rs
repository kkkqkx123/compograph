//! Thin wrappers over petgraph algorithms.
//!
//! The wrappers keep call sites free of petgraph generics: each function takes
//! the graph the store exposes and returns plain, owned Rust data. They stay
//! free of gpui types so the algorithms can be exercised in headless tests.

use std::collections::HashMap;

use petgraph::Directed;
use petgraph::algo::{dijkstra, page_rank, tarjan_scc};
use petgraph::stable_graph::{NodeIndex, StableGraph};

use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep wrapper signatures
/// readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Shortest-path distances from `start`, using edge weights as costs.
///
/// Nodes unreachable from `start` are absent from the map. Distances use the
/// same sign convention as petgraph; negative weights are not supported by
/// Dijkstra and are the caller's responsibility.
pub fn shortest_paths(graph: &Graph, start: NodeIndex) -> HashMap<NodeIndex, f32> {
    dijkstra(graph, start, None, |edge| edge.weight().weight)
        .into_iter()
        .collect()
}

/// Shortest-path distance between two nodes, if one exists.
pub fn shortest_path_cost(graph: &Graph, start: NodeIndex, goal: NodeIndex) -> Option<f32> {
    dijkstra(graph, start, Some(goal), |edge| edge.weight().weight)
        .get(&goal)
        .copied()
}

/// Strongly connected components, each as a list of node indices.
///
/// Components are returned in reverse topological order, matching petgraph.
pub fn strongly_connected_components(graph: &Graph) -> Vec<Vec<NodeIndex>> {
    tarjan_scc(graph)
}

/// PageRank scores, parallel to the node order of [`Graph::node_indices`].
///
/// Returns an empty vector for an empty graph. `damping` must lie in `[0, 1]`.
pub fn rank_nodes(graph: &Graph, damping: f32, iterations: usize) -> Vec<f32> {
    page_rank(graph, damping, iterations)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the diamond used across these tests: a -> b -> d, a -> c -> d,
    /// with the two paths costing 3 and 5 respectively.
    fn diamond() -> (Graph, NodeIndex, NodeIndex, NodeIndex, NodeIndex) {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        let d = graph.add_node(NodeData { label: "d".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, d, EdgeData { weight: 2.0 });
        graph.add_edge(a, c, EdgeData { weight: 4.0 });
        graph.add_edge(c, d, EdgeData { weight: 1.0 });
        (graph, a, b, c, d)
    }

    #[test]
    fn shortest_paths_pick_the_cheaper_route() {
        let (graph, a, b, c, d) = diamond();
        let distances = shortest_paths(&graph, a);
        assert_eq!(distances.get(&a), Some(&0.0));
        assert_eq!(distances.get(&b), Some(&1.0));
        assert_eq!(distances.get(&c), Some(&4.0));
        assert_eq!(distances.get(&d), Some(&3.0));
        assert_eq!(shortest_path_cost(&graph, a, d), Some(3.0));
    }

    #[test]
    fn unreachable_nodes_are_absent_from_the_distance_map() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let lonely = graph.add_node(NodeData {
            label: "lonely".into(),
        });
        let distances = shortest_paths(&graph, a);
        assert!(!distances.contains_key(&lonely));
        assert_eq!(shortest_path_cost(&graph, a, lonely), None);
    }

    #[test]
    fn scc_groups_a_cycle_and_leaves_singletons() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, a, EdgeData { weight: 1.0 });
        graph.add_edge(b, c, EdgeData { weight: 1.0 });

        let components = strongly_connected_components(&graph);
        let cycle = components
            .iter()
            .find(|component| component.len() == 2)
            .expect("a two-node cycle exists");
        assert!(cycle.contains(&a) && cycle.contains(&b));
        assert!(components.iter().any(|component| component == &vec![c]));
    }

    #[test]
    fn scc_of_an_empty_graph_is_empty() {
        let graph: Graph = StableGraph::default();
        assert!(strongly_connected_components(&graph).is_empty());
    }

    #[test]
    fn rank_nodes_returns_one_score_per_node() {
        let (graph, _, _, _, _) = diamond();
        let ranks = rank_nodes(&graph, 0.85, 20);
        assert_eq!(ranks.len(), graph.node_count());
        assert!(ranks.iter().all(|rank| rank.is_finite() && *rank >= 0.0));
    }

    #[test]
    fn ranking_an_empty_graph_yields_no_scores() {
        let graph: Graph = StableGraph::default();
        assert!(rank_nodes(&graph, 0.85, 10).is_empty());
    }
}
