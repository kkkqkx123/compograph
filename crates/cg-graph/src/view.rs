//! Read-only view over graph structure for layout and rendering.
//!
//! Layout engines and paint planners program against this trait instead of the
//! concrete store, so they can run headlessly against lightweight mocks and
//! stay independent of the petgraph storage details.

use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use petgraph::{Directed, Direction};

use crate::store::{EdgeData, NodeData};

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

    /// Number of incident edges of `node`, counting both directions.
    fn degree(&self, node: NodeIndex) -> usize;

    /// Distinct neighbours of `node` in either direction, in index order.
    fn neighbors(&self, node: NodeIndex) -> Vec<NodeIndex>;

    /// Distinct out-neighbours of `node`, in index order.
    fn successors(&self, node: NodeIndex) -> Vec<NodeIndex>;

    /// Distinct in-neighbours of `node`, in index order.
    fn predecessors(&self, node: NodeIndex) -> Vec<NodeIndex>;
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

impl GraphView for StableGraph<NodeData, EdgeData, Directed> {
    fn node_ids(&self) -> Vec<NodeIndex> {
        use petgraph::visit::IntoNodeIdentifiers;
        self.node_identifiers().collect()
    }

    fn node_count(&self) -> usize {
        self.node_count()
    }

    fn edge_count(&self) -> usize {
        self.edge_count()
    }

    fn edges(&self) -> Vec<(NodeIndex, NodeIndex)> {
        use petgraph::visit::IntoEdgeReferences;
        self.edge_references()
            .map(|edge| (edge.source(), edge.target()))
            .collect()
    }

    fn degree(&self, node: NodeIndex) -> usize {
        let outgoing = self.edges(node).count();
        let incoming = self.edges_directed(node, Direction::Incoming).count();
        let loops = self
            .edges(node)
            .filter(|edge| edge.source() == node && edge.target() == node)
            .count();
        outgoing + incoming - loops
    }

    fn neighbors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let mut result: Vec<NodeIndex> = self
            .edges(node)
            .map(|edge| edge.target())
            .chain(
                self.edges_directed(node, Direction::Incoming)
                    .map(|edge| edge.source()),
            )
            .collect();
        result.sort_unstable_by_key(|neighbour| neighbour.index());
        result.dedup_by_key(|neighbour| neighbour.index());
        result
    }

    fn successors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let mut result: Vec<NodeIndex> = self.edges(node).map(|edge| edge.target()).collect();
        result.sort_unstable_by_key(|neighbour| neighbour.index());
        result.dedup_by_key(|neighbour| neighbour.index());
        result
    }

    fn predecessors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let mut result: Vec<NodeIndex> = self
            .edges_directed(node, Direction::Incoming)
            .map(|edge| edge.source())
            .collect();
        result.sort_unstable_by_key(|neighbour| neighbour.index());
        result.dedup_by_key(|neighbour| neighbour.index());
        result
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

    fn degree(&self, node: NodeIndex) -> usize {
        let index = node.index();
        self.edges
            .iter()
            .filter(|(source, target)| *source == index || *target == index)
            .count()
    }

    fn neighbors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let index = node.index();
        let mut result: Vec<NodeIndex> = self
            .edges
            .iter()
            .filter_map(|(source, target)| {
                if *source == index {
                    Some(NodeIndex::new(*target))
                } else if *target == index {
                    Some(NodeIndex::new(*source))
                } else {
                    None
                }
            })
            .collect();
        result.sort_unstable_by_key(|node| node.index());
        result.dedup_by_key(|node| node.index());
        result
    }

    fn successors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let index = node.index();
        let mut result: Vec<NodeIndex> = self
            .edges
            .iter()
            .filter(|(source, _)| *source == index)
            .map(|(_, target)| NodeIndex::new(*target))
            .collect();
        result.sort_unstable_by_key(|node| node.index());
        result.dedup_by_key(|node| node.index());
        result
    }

    fn predecessors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let index = node.index();
        let mut result: Vec<NodeIndex> = self
            .edges
            .iter()
            .filter(|(_, target)| *target == index)
            .map(|(source, _)| NodeIndex::new(*source))
            .collect();
        result.sort_unstable_by_key(|node| node.index());
        result.dedup_by_key(|node| node.index());
        result
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

    #[test]
    fn adjacency_queries_stay_sorted_and_directed() {
        let mut graph = MockGraph::chain(3);
        graph.push_edge(2, 0);
        assert_eq!(graph.degree(NodeIndex::new(1)), 2);
        assert_eq!(
            graph.neighbors(NodeIndex::new(1)),
            vec![NodeIndex::new(0), NodeIndex::new(2)]
        );
        assert_eq!(graph.successors(NodeIndex::new(1)), vec![NodeIndex::new(2)]);
        assert_eq!(
            graph.predecessors(NodeIndex::new(1)),
            vec![NodeIndex::new(0)]
        );
        assert_eq!(graph.successors(NodeIndex::new(2)), vec![NodeIndex::new(0)]);
        assert_eq!(
            graph.predecessors(NodeIndex::new(0)),
            vec![NodeIndex::new(2)]
        );
    }

    #[test]
    fn self_loops_count_towards_degree() {
        let mut graph = MockGraph::isolated(1);
        graph.push_edge(0, 0);
        assert_eq!(graph.degree(NodeIndex::new(0)), 1);
        assert_eq!(graph.neighbors(NodeIndex::new(0)), vec![NodeIndex::new(0)]);
    }
}
