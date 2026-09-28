//! Editable graph storage built on petgraph's stable index graph.

use petgraph::Directed;
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};

use gpui::Context;

use crate::events::GraphChangeEvent;

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

    /// Read-only access for layout, rendering and algorithm consumers.
    pub fn graph(&self) -> &StableGraph<NodeData, EdgeData, Directed> {
        &self.graph
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

impl Default for GraphStore {
    fn default() -> Self {
        Self::new()
    }
}
