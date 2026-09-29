//! Read-only view over graph structure for layout and rendering.
//!
//! Layout engines and paint planners program against this trait instead of the
//! concrete store, so they can run headlessly against lightweight mocks and
//! stay independent of the petgraph storage details.

use petgraph::stable_graph::NodeIndex;

/// Structural queries a layout engine or paint planner may perform.
///
/// All results use owned vectors in deterministic index order, which keeps
/// layout output reproducible across runs.
pub trait GraphView {
    /// Identifiers of every node currently in the graph.
    fn node_ids(&self) -> Vec<NodeIndex>;

    /// Number of nodes currently in the graph.
    fn node_count(&self) -> usize;

    /// Number of edges currently in the graph.
    fn edge_count(&self) -> usize;

    /// Every directed edge as a (source, target) pair.
    fn edges(&self) -> Vec<(NodeIndex, NodeIndex)>;
}

/// In-memory graph used by headless layout tests.
///
/// Node identifiers are dense from zero, so tests can build scenarios without
/// a gpui context.
#[derive(Clone, Debug, Default)]
pub struct MockGraph {
    nodes: usize,
    edges: Vec<(usize, usize)>,
}

impl MockGraph {
    /// Empty graph with no nodes or edges.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Chain of `nodes` nodes linked in index order.
    pub fn chain(nodes: usize) -> Self {
        let mut graph = Self {
            nodes,
            edges: Vec::new(),
        };
        for index in 1..nodes {
            graph.edges.push((index - 1, index));
        }
        graph
    }

    /// Graph of `nodes` isolated nodes without edges.
    pub fn isolated(nodes: usize) -> Self {
        Self {
            nodes,
            edges: Vec::new(),
        }
    }

    /// Fully connected directed graph over `nodes` nodes, excluding self loops.
    pub fn clique(nodes: usize) -> Self {
        let mut edges = Vec::new();
        for source in 0..nodes {
            for target in 0..nodes {
                if source != target {
                    edges.push((source, target));
                }
            }
        }
        Self { nodes, edges }
    }

    /// Adds one directed edge; missing endpoints extend the node range.
    pub fn push_edge(&mut self, source: usize, target: usize) {
        self.nodes = self.nodes.max(source + 1).max(target + 1);
        self.edges.push((source, target));
    }
}

impl GraphView for MockGraph {
    fn node_ids(&self) -> Vec<NodeIndex> {
        (0..self.nodes).map(NodeIndex::new).collect()
    }

    fn node_count(&self) -> usize {
        self.nodes
    }

    fn edge_count(&self) -> usize {
        self.edges.len()
    }

    fn edges(&self) -> Vec<(NodeIndex, NodeIndex)> {
        self.edges
            .iter()
            .map(|(source, target)| (NodeIndex::new(*source), NodeIndex::new(*target)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_links_nodes_in_order() {
        let graph = MockGraph::chain(3);
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
        assert_eq!(
            graph.edges(),
            vec![
                (NodeIndex::new(0), NodeIndex::new(1)),
                (NodeIndex::new(1), NodeIndex::new(2)),
            ]
        );
    }

    #[test]
    fn pushed_edges_extend_the_node_range() {
        let mut graph = MockGraph::empty();
        graph.push_edge(2, 5);
        assert_eq!(graph.node_count(), 6);
        assert_eq!(graph.edges(), vec![(NodeIndex::new(2), NodeIndex::new(5))]);
    }
}
