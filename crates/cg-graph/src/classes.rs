//! Ordered class sets for batch styling and queries.
//!
//! Classes live in side tables on the store. Traversal order is dictionary
//! order so repeated runs agree regardless of insertion order.

use std::collections::{BTreeSet, HashSet};

use petgraph::stable_graph::{EdgeIndex, NodeIndex};

use crate::events::GraphChangeEvent;
use crate::store::GraphStore;

/// True when `name` may be used as a class.
pub fn valid_class_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|point| point.is_whitespace())
}

impl GraphStore {
    /// Ordered class snapshot of `node`; empty when absent.
    pub fn node_classes(&self, node: NodeIndex) -> BTreeSet<String> {
        self.node_class_table
            .get(&node)
            .cloned()
            .unwrap_or_default()
    }

    /// True when `node` carries `class`.
    pub fn node_has_class(&self, node: NodeIndex, class: &str) -> bool {
        self.node_class_table
            .get(&node)
            .is_some_and(|set| set.contains(class))
    }

    /// Adds one class to `node`; false for illegal names or missing nodes.
    pub fn add_node_class(
        &mut self,
        cx: &mut gpui::Context<Self>,
        node: NodeIndex,
        class: impl Into<String>,
    ) -> bool {
        let class = class.into();
        if !valid_class_name(&class) || self.graph.node_weight(node).is_none() {
            return false;
        }
        if self
            .node_class_table
            .get(&node)
            .is_some_and(|set| set.contains(&class))
        {
            return false;
        }
        self.before_mutation();
        self.node_class_table.entry(node).or_default().insert(class);
        self.announce(cx, GraphChangeEvent::NodeClassChanged(node));
        true
    }

    /// Removes one class from `node`; false when absent.
    pub fn remove_node_class(
        &mut self,
        cx: &mut gpui::Context<Self>,
        node: NodeIndex,
        class: &str,
    ) -> bool {
        let present = self
            .node_class_table
            .get(&node)
            .is_some_and(|set| set.contains(class));
        if !present {
            return false;
        }
        self.before_mutation();
        let removed = self
            .node_class_table
            .get_mut(&node)
            .is_some_and(|set| set.remove(class));
        if removed {
            self.announce(cx, GraphChangeEvent::NodeClassChanged(node));
        }
        removed
    }

    /// Nodes carrying `class`, in index order.
    pub fn nodes_with_class(&self, class: &str) -> Vec<NodeIndex> {
        let mut found: Vec<NodeIndex> = self
            .node_class_table
            .iter()
            .filter(|(_, set)| set.contains(class))
            .map(|(node, _)| *node)
            .filter(|node| self.graph.node_weight(*node).is_some() && self.is_visible(*node))
            .collect();
        found.sort_unstable_by_key(|node| node.index());
        found
    }

    /// Nodes carrying every class in `classes`, in index order.
    pub fn nodes_with_all_classes(&self, classes: &BTreeSet<String>) -> Vec<NodeIndex> {
        if classes.is_empty() {
            return self.visible_node_ids();
        }
        let mut found: Vec<NodeIndex> = self
            .visible_node_ids()
            .into_iter()
            .filter(|node| {
                self.node_class_table
                    .get(node)
                    .is_some_and(|set| classes.iter().all(|wanted| set.contains(wanted)))
            })
            .collect();
        found.sort_unstable_by_key(|node| node.index());
        found
    }

    /// Ordered class snapshot of `edge`; empty when absent.
    pub fn edge_classes(&self, edge: EdgeIndex) -> BTreeSet<String> {
        self.edge_class_table
            .get(&edge)
            .cloned()
            .unwrap_or_default()
    }

    /// True when `edge` carries `class`.
    pub fn edge_has_class(&self, edge: EdgeIndex, class: &str) -> bool {
        self.edge_class_table
            .get(&edge)
            .is_some_and(|set| set.contains(class))
    }

    /// Adds one class to `edge`; false for illegal names or missing edges.
    pub fn add_edge_class(
        &mut self,
        cx: &mut gpui::Context<Self>,
        edge: EdgeIndex,
        class: impl Into<String>,
    ) -> bool {
        let class = class.into();
        if !valid_class_name(&class) || self.graph.edge_weight(edge).is_none() {
            return false;
        }
        if self
            .edge_class_table
            .get(&edge)
            .is_some_and(|set| set.contains(&class))
        {
            return false;
        }
        self.before_mutation();
        self.edge_class_table.entry(edge).or_default().insert(class);
        self.announce(cx, GraphChangeEvent::EdgeClassChanged(edge));
        true
    }

    /// Removes one class from `edge`; false when absent.
    pub fn remove_edge_class(
        &mut self,
        cx: &mut gpui::Context<Self>,
        edge: EdgeIndex,
        class: &str,
    ) -> bool {
        let present = self
            .edge_class_table
            .get(&edge)
            .is_some_and(|set| set.contains(class));
        if !present {
            return false;
        }
        self.before_mutation();
        let removed = self
            .edge_class_table
            .get_mut(&edge)
            .is_some_and(|set| set.remove(class));
        if removed {
            self.announce(cx, GraphChangeEvent::EdgeClassChanged(edge));
        }
        removed
    }

    /// Visible edges carrying `class`, in endpoint order.
    pub fn edges_with_class(&self, class: &str) -> Vec<(NodeIndex, NodeIndex)> {
        let visible: HashSet<(NodeIndex, NodeIndex)> = self.visible_edges().into_iter().collect();
        let mut found: Vec<(NodeIndex, NodeIndex)> = self
            .edge_class_table
            .iter()
            .filter(|(_, set)| set.contains(class))
            .filter_map(|(edge, _)| self.graph.edge_endpoints(*edge))
            .filter(|endpoints| visible.contains(endpoints))
            .collect();
        found.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        found.dedup();
        found
    }

    pub(crate) fn drop_node_classes(&mut self, node: NodeIndex) {
        self.node_class_table.remove(&node);
    }

    pub(crate) fn drop_edge_classes(&mut self, edge: EdgeIndex) {
        self.edge_class_table.remove(&edge);
    }

    pub(crate) fn clear_class_tables(&mut self) {
        self.node_class_table.clear();
        self.edge_class_table.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_names_reject_blank_and_whitespace() {
        assert!(!valid_class_name(""));
        assert!(!valid_class_name("has space"));
        assert!(valid_class_name("hub"));
        assert!(valid_class_name("Hub"));
    }
}
