//! Graph traversals from a start node.

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{Bfs, Dfs};

use crate::store::{EdgeData, NodeData};

type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Nodes in breadth-first order from `start` along outgoing edges.
///
/// A missing source yields an empty order rather than an error.
pub fn breadth_first_order(graph: &Graph, start: NodeIndex) -> Vec<NodeIndex> {
    if graph.node_weight(start).is_none() {
        return Vec::new();
    }
    let mut search = Bfs::new(graph, start);
    let mut order = Vec::new();
    while let Some(node) = search.next(graph) {
        order.push(node);
    }
    order
}

/// Nodes in depth-first order from `start` along outgoing edges.
///
/// A missing source yields an empty order rather than an error.
pub fn depth_first_order(graph: &Graph, start: NodeIndex) -> Vec<NodeIndex> {
    if graph.node_weight(start).is_none() {
        return Vec::new();
    }
    let mut search = Dfs::new(graph, start);
    let mut order = Vec::new();
    while let Some(node) = search.next(graph) {
        order.push(node);
    }
    order
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
    fn traversals_start_at_the_source_and_cover_reachable_nodes() {
        let (graph, a, _, _, d) = diamond();
        let breadth = breadth_first_order(&graph, a);
        assert_eq!(breadth.first(), Some(&a));
        assert_eq!(breadth.len(), graph.node_count());
        assert!(breadth.contains(&d));
        let depth = depth_first_order(&graph, a);
        assert_eq!(depth.first(), Some(&a));
        assert_eq!(depth.len(), graph.node_count());
        assert!(breadth_first_order(&graph, NodeIndex::new(99)).is_empty());
        assert!(depth_first_order(&graph, NodeIndex::new(99)).is_empty());
    }
}
