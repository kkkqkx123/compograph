//! Bidirectional mapping between backend stable ids and graph indices.
//!
//! Backends address elements with stable string ids while the store addresses
//! them with petgraph indices. [`IdMap`] keeps both directions in sync so
//! consumers never hand-roll parallel tables. The map never touches the store
//! itself: callers insert after adding elements and call [`IdMap::sweep`]
//! after removals or full resets to drop stale entries.

use std::collections::HashMap;

use petgraph::stable_graph::{EdgeIndex, NodeIndex};

use crate::store::GraphStore;

/// Two-way lookup between external string ids and internal indices.
#[derive(Clone, Debug, Default)]
pub struct IdMap {
    nodes: HashMap<String, NodeIndex>,
    node_names: HashMap<NodeIndex, String>,
    edges: HashMap<String, EdgeIndex>,
    edge_names: HashMap<EdgeIndex, String>,
}

impl IdMap {
    /// Empty map binding nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no id is bound in either direction.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// Number of bound node ids.
    pub fn node_len(&self) -> usize {
        self.nodes.len()
    }

    /// Number of bound edge ids.
    pub fn edge_len(&self) -> usize {
        self.edges.len()
    }

    /// Binds `id` to `node`, evicting any previous binding of either side.
    ///
    /// Rebinding keeps both tables consistent: the id's old index and the
    /// index's old id are both released before the new pair is stored.
    pub fn insert_node(&mut self, id: impl Into<String>, node: NodeIndex) {
        let id = id.into();
        if let Some(old_node) = self.nodes.insert(id.clone(), node) {
            if old_node != node {
                self.node_names.remove(&old_node);
            }
        }
        if let Some(old_id) = self.node_names.insert(node, id.clone()) {
            if old_id != id {
                self.nodes.remove(&old_id);
            }
        }
    }

    /// Index bound to `id`, if any.
    pub fn node(&self, id: &str) -> Option<NodeIndex> {
        self.nodes.get(id).copied()
    }

    /// External id bound to `node`, if any.
    pub fn node_id(&self, node: NodeIndex) -> Option<&str> {
        self.node_names.get(&node).map(String::as_str)
    }

    /// Releases both directions of `node`, returning its id when bound.
    pub fn remove_node(&mut self, node: NodeIndex) -> Option<String> {
        let id = self.node_names.remove(&node)?;
        self.nodes.remove(&id);
        Some(id)
    }

    /// Binds `id` to `edge`, evicting any previous binding of either side.
    pub fn insert_edge(&mut self, id: impl Into<String>, edge: EdgeIndex) {
        let id = id.into();
        if let Some(old_edge) = self.edges.insert(id.clone(), edge) {
            if old_edge != edge {
                self.edge_names.remove(&old_edge);
            }
        }
        if let Some(old_id) = self.edge_names.insert(edge, id.clone()) {
            if old_id != id {
                self.edges.remove(&old_id);
            }
        }
    }

    /// Index bound to `id`, if any.
    pub fn edge(&self, id: &str) -> Option<EdgeIndex> {
        self.edges.get(id).copied()
    }

    /// External id bound to `edge`, if any.
    pub fn edge_id(&self, edge: EdgeIndex) -> Option<&str> {
        self.edge_names.get(&edge).map(String::as_str)
    }

    /// Releases both directions of `edge`, returning its id when bound.
    pub fn remove_edge(&mut self, edge: EdgeIndex) -> Option<String> {
        let id = self.edge_names.remove(&edge)?;
        self.edges.remove(&id);
        Some(id)
    }

    /// Drops every entry whose index no longer resolves in `store`.
    ///
    /// Call after removals or full resets; the store itself is only read.
    pub fn sweep(&mut self, store: &GraphStore) {
        self.nodes.retain(|_, node| store.contains_node(*node));
        self.node_names.retain(|node, _| store.contains_node(*node));
        self.edges
            .retain(|_, edge| store.edge_endpoints(*edge).is_some());
        self.edge_names
            .retain(|edge, _| store.edge_endpoints(*edge).is_some());
    }

    /// Releases every binding.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.node_names.clear();
        self.edges.clear();
        self.edge_names.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use petgraph::Directed;
    use petgraph::stable_graph::StableGraph;

    use super::*;
    use crate::store::{EdgeData, NodeData};

    fn store_with_two_nodes() -> (GraphStore, NodeIndex, NodeIndex, EdgeIndex) {
        let mut graph: StableGraph<NodeData, EdgeData, Directed> = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let edge = graph.add_edge(a, b, EdgeData { weight: 1.0 });
        let store = GraphStore {
            graph,
            node_attr_table: HashMap::new(),
            edge_attr_table: HashMap::new(),
            node_class_table: HashMap::new(),
            edge_class_table: HashMap::new(),
            parents: HashMap::new(),
            children: HashMap::new(),
            collapsed: HashSet::new(),
        };
        (store, a, b, edge)
    }

    #[test]
    fn node_ids_round_trip_in_both_directions() {
        let (_, a, _, _) = store_with_two_nodes();
        let mut map = IdMap::new();
        assert!(map.is_empty());
        map.insert_node("backend-a", a);
        assert_eq!(map.node("backend-a"), Some(a));
        assert_eq!(map.node_id(a), Some("backend-a"));
        assert_eq!(map.node_len(), 1);
        assert!(!map.is_empty());
    }

    #[test]
    fn rebinding_evicts_both_stale_sides() {
        let (_, a, b, _) = store_with_two_nodes();
        let mut map = IdMap::new();
        map.insert_node("x", a);
        map.insert_node("y", b);
        map.insert_node("x", b);
        assert_eq!(map.node("x"), Some(b));
        assert_eq!(map.node_id(b), Some("x"));
        assert_eq!(map.node_id(a), None);
        assert_eq!(map.node("y"), None);
        assert_eq!(map.node_len(), 1);
    }

    #[test]
    fn removal_releases_both_directions() {
        let (_, a, _, _) = store_with_two_nodes();
        let mut map = IdMap::new();
        map.insert_node("gone", a);
        assert_eq!(map.remove_node(a), Some("gone".to_string()));
        assert_eq!(map.node("gone"), None);
        assert_eq!(map.node_id(a), None);
        assert_eq!(map.remove_node(a), None);
    }

    #[test]
    fn sweep_drops_entries_missing_from_the_store() {
        let (mut store, a, b, edge) = store_with_two_nodes();
        let mut map = IdMap::new();
        map.insert_node("keep", a);
        map.insert_node("stale-node", NodeIndex::new(99));
        map.insert_edge("keep-edge", edge);
        map.insert_edge("stale-edge", EdgeIndex::new(99));
        map.sweep(&store);
        assert_eq!(map.node("keep"), Some(a));
        assert_eq!(map.node("stale-node"), None);
        assert_eq!(map.edge("keep-edge"), Some(edge));
        assert_eq!(map.edge("stale-edge"), None);
        assert_eq!(map.node_id(b), None);
        store.graph.remove_node(b);
        map.sweep(&store);
        assert_eq!(map.node("keep"), Some(a));
        map.clear();
        assert!(map.is_empty());
    }
}
