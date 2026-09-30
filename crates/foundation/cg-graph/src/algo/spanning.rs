//! Minimum spanning tree searches over the store graph.

use petgraph::Directed;
use petgraph::algo::{min_spanning_tree, min_spanning_tree_prim};
use petgraph::data::Element;
use petgraph::stable_graph::{NodeIndex, StableGraph};

use crate::store::{EdgeData, NodeData};

type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Minimum spanning forest edges via Kruskal's algorithm.
///
/// Each entry carries its endpoints and weight. Disconnected components each
/// contribute their own tree, so a forest with `c` components holds
/// `nodes - c` edges. The graph is treated as undirected.
pub fn minimum_spanning_forest(graph: &Graph) -> Vec<(NodeIndex, NodeIndex, f32)> {
    let order: Vec<NodeIndex> = graph.node_indices().collect();
    min_spanning_tree(graph)
        .filter_map(|element| match element {
            Element::Edge {
                source,
                target,
                weight,
            } => {
                let endpoints = order.get(source).zip(order.get(target));
                endpoints.map(|(source, target)| (*source, *target, weight.weight))
            }
            Element::Node { .. } => None,
        })
        .collect()
}

/// Minimum spanning tree edges of one component via Prim's algorithm.
///
/// Only the component holding the first node is covered; remaining components
/// contribute no edges. Prefer [`minimum_spanning_forest`] when every
/// component must be represented. The graph is treated as undirected.
pub fn minimum_spanning_tree_single(graph: &Graph) -> Vec<(NodeIndex, NodeIndex, f32)> {
    let order: Vec<NodeIndex> = graph.node_indices().collect();
    min_spanning_tree_prim(graph)
        .filter_map(|element| match element {
            Element::Edge {
                source,
                target,
                weight,
            } => {
                let endpoints = order.get(source).zip(order.get(target));
                endpoints.map(|(source, target)| (*source, *target, weight.weight))
            }
            Element::Node { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn kruskal_forest_spans_every_component() {
        let (mut graph, _, _, _, _) = diamond();
        let lonely = graph.add_node(NodeData {
            label: "lonely".into(),
        });
        let forest = minimum_spanning_forest(&graph);
        assert_eq!(forest.len(), graph.node_count() - 2);
        assert!(
            !forest
                .iter()
                .any(|(source, target, _)| { *source == lonely || *target == lonely })
        );
        let weights: f32 = forest.iter().map(|(_, _, weight)| weight).sum();
        assert!((weights - 4.0).abs() < 1e-5);
    }

    #[test]
    fn prim_tree_covers_a_single_component() {
        let (graph, _, _, _, _) = diamond();
        let tree = minimum_spanning_tree_single(&graph);
        assert_eq!(tree.len(), graph.node_count() - 1);
        let empty: Graph = StableGraph::default();
        assert!(minimum_spanning_forest(&empty).is_empty());
        assert!(minimum_spanning_tree_single(&empty).is_empty());
    }
}
