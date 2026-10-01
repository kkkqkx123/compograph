//! Editable graph storage built on petgraph's stable index graph.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences, IntoNodeIdentifiers};
use petgraph::{Directed, Direction};

use gpui::Context;

use crate::attrs::DataValue;
use crate::events::GraphChangeEvent;
use crate::view::GraphView;

/// Application-level payload attached to a node.
#[derive(Clone, Debug)]
pub struct NodeData {
    pub label: String,
}

impl fmt::Display for NodeData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.label)
    }
}

/// Application-level payload attached to an edge.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeData {
    pub weight: f32,
}

impl PartialOrd for EdgeData {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.weight.partial_cmp(&other.weight)
    }
}

impl fmt::Display for EdgeData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.weight.fmt(formatter)
    }
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
    pub(crate) graph: StableGraph<NodeData, EdgeData, Directed>,
    pub(crate) node_attr_table: HashMap<NodeIndex, HashMap<String, DataValue>>,
    pub(crate) edge_attr_table: HashMap<EdgeIndex, HashMap<String, DataValue>>,
    pub(crate) node_class_table: HashMap<NodeIndex, BTreeSet<String>>,
    pub(crate) edge_class_table: HashMap<EdgeIndex, BTreeSet<String>>,
    pub(crate) parents: HashMap<NodeIndex, NodeIndex>,
    pub(crate) children: HashMap<NodeIndex, BTreeSet<NodeIndex>>,
    pub(crate) collapsed: HashSet<NodeIndex>,
}

impl GraphStore {
    pub fn new() -> Self {
        Self {
            graph: StableGraph::default(),
            node_attr_table: HashMap::new(),
            edge_attr_table: HashMap::new(),
            node_class_table: HashMap::new(),
            edge_class_table: HashMap::new(),
            parents: HashMap::new(),
            children: HashMap::new(),
            collapsed: HashSet::new(),
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

    /// True when `node` is still present in the graph.
    pub fn contains_node(&self, node: NodeIndex) -> bool {
        self.graph.node_weight(node).is_some()
    }

    /// Endpoints of `edge` as (source, target), if it is still present.
    pub fn edge_endpoints(&self, edge: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        self.graph.edge_endpoints(edge)
    }

    /// First edge from `source` to `target` in index order, if one exists.
    ///
    /// Parallel edges share one slot in paint and selection plans, so callers
    /// read the first match rather than tracking identities.
    pub fn find_edge(&self, source: NodeIndex, target: NodeIndex) -> Option<EdgeIndex> {
        self.graph
            .edge_references()
            .find(|edge| edge.source() == source && edge.target() == target)
            .map(|edge| edge.id())
    }

    /// Weight of the first edge from `source` to `target`, if one exists.
    ///
    /// Parallel edges share one label slot in the paint plan, so the plan
    /// reads the first match in index order rather than tracking identities.
    pub fn edge_weight(&self, source: NodeIndex, target: NodeIndex) -> Option<f32> {
        self.graph
            .edge_references()
            .find(|edge| edge.source() == source && edge.target() == target)
            .map(|edge| edge.weight().weight)
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
            self.drop_node_attrs(node);
            self.drop_node_classes(node);
            self.detach_compound(node);
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
            self.drop_edge_attrs(edge);
            self.drop_edge_classes(edge);
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
        self.clear_attr_tables();
        self.clear_class_tables();
        self.clear_compound();
        cx.emit(GraphChangeEvent::StructureReset);
        cx.notify();
    }
}

impl GraphView for GraphStore {
    fn node_ids(&self) -> Vec<NodeIndex> {
        if self.parents.is_empty() && self.collapsed.is_empty() {
            return self.graph.node_identifiers().collect();
        }
        self.visible_node_ids()
    }

    fn node_count(&self) -> usize {
        if self.parents.is_empty() && self.collapsed.is_empty() {
            return self.graph.node_count();
        }
        self.visible_node_ids().len()
    }

    fn edge_count(&self) -> usize {
        if self.parents.is_empty() && self.collapsed.is_empty() {
            return self.graph.edge_count();
        }
        self.visible_edges().len()
    }

    fn edges(&self) -> Vec<(NodeIndex, NodeIndex)> {
        if self.parents.is_empty() && self.collapsed.is_empty() {
            return self
                .graph
                .edge_references()
                .map(|edge| (edge.source(), edge.target()))
                .collect();
        }
        self.visible_edges()
    }

    fn degree(&self, node: NodeIndex) -> usize {
        if !self.is_visible(node) {
            return 0;
        }
        if self.parents.is_empty() && self.collapsed.is_empty() {
            let outgoing = self.graph.edges(node).count();
            let incoming = self.graph.edges_directed(node, Direction::Incoming).count();
            let loops = self
                .graph
                .edges(node)
                .filter(|edge| edge.source() == node && edge.target() == node)
                .count();
            return outgoing + incoming - loops;
        }
        let visible = self.visible_edges();
        let mut count = 0usize;
        for (source, target) in &visible {
            if *source == node || *target == node {
                count += 1;
            }
        }
        count
    }

    fn neighbors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        if !self.is_visible(node) {
            return Vec::new();
        }
        if self.parents.is_empty() && self.collapsed.is_empty() {
            let mut result: Vec<NodeIndex> = self
                .graph
                .edges(node)
                .map(|edge| edge.target())
                .chain(
                    self.graph
                        .edges_directed(node, Direction::Incoming)
                        .map(|edge| edge.source()),
                )
                .collect();
            result.sort_unstable_by_key(|neighbour| neighbour.index());
            result.dedup_by_key(|neighbour| neighbour.index());
            return result;
        }
        let mut result = Vec::new();
        for (source, target) in self.visible_edges() {
            if source == node && self.is_visible(target) {
                result.push(target);
            } else if target == node && self.is_visible(source) {
                result.push(source);
            }
        }
        result.sort_unstable_by_key(|neighbour| neighbour.index());
        result.dedup_by_key(|neighbour| neighbour.index());
        result
    }

    fn successors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        if !self.is_visible(node) {
            return Vec::new();
        }
        if self.parents.is_empty() && self.collapsed.is_empty() {
            return GraphStore::successors(self, node)
                .into_iter()
                .map(|(neighbour, _)| neighbour)
                .collect();
        }
        let mut result: Vec<NodeIndex> = self
            .visible_edges()
            .into_iter()
            .filter(|(source, _)| *source == node)
            .map(|(_, target)| target)
            .collect();
        result.sort_unstable_by_key(|neighbour| neighbour.index());
        result.dedup_by_key(|neighbour| neighbour.index());
        result
    }

    fn predecessors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        if !self.is_visible(node) {
            return Vec::new();
        }
        if self.parents.is_empty() && self.collapsed.is_empty() {
            return GraphStore::predecessors(self, node)
                .into_iter()
                .map(|(neighbour, _)| neighbour)
                .collect();
        }
        let mut result: Vec<NodeIndex> = self
            .visible_edges()
            .into_iter()
            .filter(|(_, target)| *target == node)
            .map(|(source, _)| source)
            .collect();
        result.sort_unstable_by_key(|neighbour| neighbour.index());
        result.dedup_by_key(|neighbour| neighbour.index());
        result
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

    fn store_of(graph: StableGraph<NodeData, EdgeData, Directed>) -> GraphStore {
        GraphStore {
            graph,
            node_attr_table: HashMap::new(),
            edge_attr_table: HashMap::new(),
            node_class_table: HashMap::new(),
            edge_class_table: HashMap::new(),
            parents: HashMap::new(),
            children: HashMap::new(),
            collapsed: HashSet::new(),
        }
    }

    #[test]
    fn neighbours_report_direction_and_stay_sorted() {
        let (graph, [a, b, c, d]) = diamond();
        let store = store_of(graph);
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
        let store = store_of(graph);
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
    fn edge_weight_reads_the_first_match_in_index_order() {
        let (graph, [a, b, _, _]) = diamond();
        let store = store_of(graph);
        assert_eq!(store.edge_weight(a, b), Some(1.0));
        assert_eq!(store.edge_weight(b, a), None);
    }

    #[test]
    fn view_adjacency_covers_both_directions() {
        let (graph, [a, b, c, d]) = diamond();
        let store = store_of(graph);
        let view: &dyn GraphView = &store;
        assert_eq!(view.degree(a), 2);
        assert_eq!(view.degree(d), 2);
        assert_eq!(view.successors(a), vec![b, c]);
        assert_eq!(view.predecessors(d).len(), 2);
        assert_eq!(view.neighbors(d).len(), 2);
    }

    #[test]
    fn edge_endpoints_resolve_existing_edges_only() {
        let (graph, [a, b, _, _]) = diamond();
        let store = store_of(graph);
        assert_eq!(store.edge_endpoints(EdgeIndex::new(0)), Some((a, b)));
        assert_eq!(store.edge_endpoints(EdgeIndex::new(99)), None);
    }

    #[test]
    fn existence_and_edge_lookup_use_purpose_queries() {
        let (graph, [a, b, _, _]) = diamond();
        let store = store_of(graph);
        assert!(store.contains_node(a));
        assert!(store.contains_node(b));
        assert!(!store.contains_node(NodeIndex::new(99)));
        assert_eq!(store.find_edge(a, b), Some(EdgeIndex::new(0)));
        assert_eq!(store.find_edge(b, a), None);
    }
}
