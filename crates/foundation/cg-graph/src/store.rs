//! Editable graph storage built on petgraph's stable index graph.

use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences, IntoNodeIdentifiers};
use petgraph::{Directed, Direction};

use gpui::Context;

use crate::events::GraphChangeEvent;
use crate::view::GraphView;

/// Application-level payload attached to a node.
#[derive(Clone, Debug)]
pub struct NodeData {
    pub label: String,
}

/// Application-level payload attached to an edge.
#[derive(Clone, Debug)]
pub struct EdgeData {
    pub weight: f32,
}

/// Owns the graph structure behind the application.
///
/// Node and edge indices stay valid across removals, which keeps cached
/// positions and UI state aligned with the structure they were computed for.
///
/// Every mutation takes a [`Context`] and announces the change through
/// [`GraphChangeEvent`], so downstream stages such as layout and rendering can
/// react without polling the structure.
pub struct GraphStore {
    graph: StableGraph<NodeData, EdgeData, Directed>,
}

impl GraphStore {
    pub fn new() -> Self {
        Self {
            graph: StableGraph::default(),
        }
    }

    /// Read-only access for internal use and algorithm consumers that need the
    /// full petgraph surface.
    ///
    /// Prefer the purpose-built queries below; they keep the rest of the
    /// workspace independent of the concrete graph type.
    pub fn graph(&self) -> &StableGraph<NodeData, EdgeData, Directed> {
        &self.graph
    }

    /// Identifiers of every node currently in the graph.
    ///
    /// Order is petgraph's internal index order, which is stable across
    /// removals because the store keeps a [`StableGraph`].
    pub fn node_ids(&self) -> impl Iterator<Item = NodeIndex> + '_ {
        self.graph.node_identifiers()
    }

    /// Number of nodes currently in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of edges currently in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Application payload of `node`, if it is still present.
    pub fn node_data(&self, node: NodeIndex) -> Option<&NodeData> {
        self.graph.node_weight(node)
    }

    /// Endpoints of `edge` as (source, target), if it is still present.
    pub fn edge_endpoints(&self, edge: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        self.graph.edge_endpoints(edge)
    }

    /// Sorted out-neighbours of `node`, carrying the connecting edge indices.
    pub fn successors(&self, node: NodeIndex) -> Vec<(NodeIndex, EdgeIndex)> {
        sort_neighbours(
            self.graph
                .edges(node)
                .filter(|edge| edge.source() == node)
                .map(|edge| (edge.target(), edge.id())),
        )
    }

    /// Sorted in-neighbours of `node`, carrying the connecting edge indices.
    pub fn predecessors(&self, node: NodeIndex) -> Vec<(NodeIndex, EdgeIndex)> {
        sort_neighbours(
            self.graph
                .edges_directed(node, Direction::Incoming)
                .map(|edge| (edge.source(), edge.id())),
        )
    }

    pub fn add_node(&mut self, cx: &mut Context<Self>, label: impl Into<String>) -> NodeIndex {
        let node = self.graph.add_node(NodeData {
            label: label.into(),
        });
        cx.emit(GraphChangeEvent::NodeAdded(node));
        cx.notify();
        node
    }

    pub fn remove_node(&mut self, cx: &mut Context<Self>, node: NodeIndex) -> Option<NodeData> {
        let removed = self.graph.remove_node(node);
        if removed.is_some() {
            cx.emit(GraphChangeEvent::NodeRemoved(node));
            cx.notify();
        }
        removed
    }

    pub fn add_edge(
        &mut self,
        cx: &mut Context<Self>,
        source: NodeIndex,
        target: NodeIndex,
        weight: f32,
    ) -> EdgeIndex {
        let edge = self.graph.add_edge(source, target, EdgeData { weight });
        cx.emit(GraphChangeEvent::EdgeAdded(edge));
        cx.notify();
        edge
    }

    pub fn remove_edge(&mut self, cx: &mut Context<Self>, edge: EdgeIndex) -> Option<EdgeData> {
        let removed = self.graph.remove_edge(edge);
        if removed.is_some() {
            cx.emit(GraphChangeEvent::EdgeRemoved(edge));
            cx.notify();
        }
        removed
    }

    /// Drops every node and edge in one step.
    ///
    /// A single event is emitted instead of one per removed element, which
    /// keeps bulk reloads from flooding subscribers.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.graph.clear();
        cx.emit(GraphChangeEvent::StructureReset);
        cx.notify();
    }
}

impl GraphView for GraphStore {
    fn node_ids(&self) -> Vec<NodeIndex> {
        self.graph.node_identifiers().collect()
    }

    fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    fn edges(&self) -> Vec<(NodeIndex, NodeIndex)> {
        self.graph
            .edge_references()
            .map(|edge| (edge.source(), edge.target()))
            .collect()
    }
}

impl Default for GraphStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Collects neighbour pairs into a deterministic order.
///
/// petgraph does not promise an iteration order for edges, so query results
/// are sorted by (neighbour, edge) index. Deterministic order keeps layout and
/// paint output reproducible, which matters for visual tests.
fn sort_neighbours(
    pairs: impl Iterator<Item = (NodeIndex, EdgeIndex)>,
) -> Vec<(NodeIndex, EdgeIndex)> {
    let mut result: Vec<_> = pairs.collect();
    result.sort_unstable_by_key(|(neighbour, edge)| (neighbour.index(), edge.index()));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a->b->d, a->c->d without a gpui context by using the raw graph.
    fn diamond() -> (StableGraph<NodeData, EdgeData, Directed>, [NodeIndex; 4]) {
        let mut graph: StableGraph<NodeData, EdgeData, Directed> = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        let d = graph.add_node(NodeData { label: "d".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, d, EdgeData { weight: 2.0 });
        graph.add_edge(a, c, EdgeData { weight: 4.0 });
        graph.add_edge(c, d, EdgeData { weight: 1.0 });
        (graph, [a, b, c, d])
    }

    #[test]
    fn neighbours_report_direction_and_stay_sorted() {
        let (graph, [a, b, c, d]) = diamond();
        let store = GraphStore { graph };
        assert_eq!(
            store.successors(a),
            vec![(b, EdgeIndex::new(0)), (c, EdgeIndex::new(2))]
        );
        assert_eq!(store.successors(d), Vec::new());
        assert_eq!(
            store.predecessors(d),
            vec![(b, EdgeIndex::new(1)), (c, EdgeIndex::new(3))]
        );
        assert_eq!(store.predecessors(a), Vec::new());
    }

    #[test]
    fn node_ids_and_counts_track_the_structure() {
        let (graph, [a, b, c, d]) = diamond();
        let store = GraphStore { graph };
        assert_eq!(store.node_ids().collect::<Vec<_>>(), vec![a, b, c, d]);
        assert_eq!(store.node_count(), 4);
        assert_eq!(store.edge_count(), 4);
        assert_eq!(
            store.node_data(a).map(|data| data.label.as_str()),
            Some("a")
        );
        assert!(store.node_data(NodeIndex::new(99)).is_none());
    }

    #[test]
    fn edge_endpoints_resolve_existing_edges_only() {
        let (graph, [a, b, _, _]) = diamond();
        let store = GraphStore { graph };
        assert_eq!(store.edge_endpoints(EdgeIndex::new(0)), Some((a, b)));
        assert_eq!(store.edge_endpoints(EdgeIndex::new(99)), None);
    }
}
