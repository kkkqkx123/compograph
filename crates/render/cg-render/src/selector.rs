//! Typed element selectors without a string language.
//!
//! Selectors combine target kind, identity, class subset, attribute equality,
//! and ancestry into one intersection match. Callers build them with the
//! constructors below; no textual parsing exists.

use std::collections::{BTreeSet, HashMap};

use cg_graph::{DataValue, GraphStore, NodeIndex};

/// Which element kinds a selector may match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectorTarget {
    Node,
    Edge,
    #[default]
    Any,
}

/// Typed match over nodes and edges.
///
/// Every set condition must hold at once. Missing attributes never match.
#[derive(Clone, Debug, Default)]
pub struct ElementSelector {
    target: SelectorTarget,
    node_id: Option<NodeIndex>,
    edge_endpoints: Option<(NodeIndex, NodeIndex)>,
    classes: BTreeSet<String>,
    attrs: HashMap<String, DataValue>,
    ancestor: Option<NodeIndex>,
}

impl ElementSelector {
    /// Matches nothing; used to guard empty bulk operations.
    pub fn never() -> Self {
        Self {
            target: SelectorTarget::Any,
            ..Self::default()
        }
    }

    /// Matches every node.
    pub fn any_node() -> Self {
        Self {
            target: SelectorTarget::Node,
            ..Self::default()
        }
    }

    /// Matches every edge.
    pub fn any_edge() -> Self {
        Self {
            target: SelectorTarget::Edge,
            ..Self::default()
        }
    }

    /// Matches one node by identity.
    pub fn node(id: NodeIndex) -> Self {
        Self {
            target: SelectorTarget::Node,
            node_id: Some(id),
            ..Self::default()
        }
    }

    /// Matches one directed edge by endpoints.
    pub fn edge(source: NodeIndex, target: NodeIndex) -> Self {
        Self {
            target: SelectorTarget::Edge,
            edge_endpoints: Some((source, target)),
            ..Self::default()
        }
    }

    /// Requires every listed class.
    pub fn with_classes(mut self, classes: impl IntoIterator<Item = String>) -> Self {
        self.classes.extend(classes);
        self
    }

    /// Requires one attribute equality.
    pub fn with_attr(mut self, key: impl Into<String>, value: DataValue) -> Self {
        self.attrs.insert(key.into(), value);
        self
    }

    /// Requires the node to descend from `ancestor`.
    pub fn with_ancestor(mut self, ancestor: NodeIndex) -> Self {
        self.ancestor = Some(ancestor);
        self
    }

    /// True when the selector carries no condition at all.
    pub fn is_empty(&self) -> bool {
        self.target == SelectorTarget::Any
            && self.node_id.is_none()
            && self.edge_endpoints.is_none()
            && self.classes.is_empty()
            && self.attrs.is_empty()
            && self.ancestor.is_none()
    }

    /// True when `node` satisfies the selector against `store`.
    pub fn matches_node(&self, store: &GraphStore, node: NodeIndex) -> bool {
        if self.is_empty() {
            return false;
        }
        match self.target {
            SelectorTarget::Edge => return false,
            SelectorTarget::Node | SelectorTarget::Any => {}
        }
        if self.edge_endpoints.is_some() {
            return false;
        }
        if let Some(wanted) = self.node_id {
            if wanted != node {
                return false;
            }
        }
        if store.graph().node_weight(node).is_none() {
            return false;
        }
        let held = store.node_classes(node);
        if !self.classes.iter().all(|name| held.contains(name)) {
            return false;
        }
        for (key, wanted) in &self.attrs {
            if store.node_attr(node, key) != Some(wanted.clone()) {
                return false;
            }
        }
        if let Some(ancestor) = self.ancestor {
            if !store.ancestors_of(node).contains(&ancestor) {
                return false;
            }
        }
        true
    }

    /// True when the directed edge satisfies the selector against `store`.
    pub fn matches_edge(&self, store: &GraphStore, source: NodeIndex, target: NodeIndex) -> bool {
        if self.is_empty() {
            return false;
        }
        match self.target {
            SelectorTarget::Node => return false,
            SelectorTarget::Edge | SelectorTarget::Any => {}
        }
        if self.node_id.is_some() || self.ancestor.is_some() {
            return false;
        }
        if let Some((wanted_source, wanted_target)) = self.edge_endpoints {
            if wanted_source != source || wanted_target != target {
                return false;
            }
        }
        let edge = store
            .graph()
            .edge_indices()
            .find(|edge| store.edge_endpoints(*edge) == Some((source, target)));
        let Some(found) = edge else {
            return false;
        };
        let held = store.edge_classes(found);
        if !self.classes.iter().all(|name| held.contains(name)) {
            return false;
        }
        for (key, wanted) in &self.attrs {
            if store.edge_attr(found, key) != Some(wanted.clone()) {
                return false;
            }
        }
        true
    }

    /// Visible nodes of `store` matching the selector, in index order.
    pub fn select_nodes(&self, store: &GraphStore) -> Vec<NodeIndex> {
        let mut found: Vec<NodeIndex> = store
            .visible_node_ids()
            .into_iter()
            .filter(|node| self.matches_node(store, *node))
            .collect();
        found.sort_unstable_by_key(|node| node.index());
        found
    }

    /// Visible edges of `store` matching the selector, in endpoint order.
    pub fn select_edges(&self, store: &GraphStore) -> Vec<(NodeIndex, NodeIndex)> {
        let mut found: Vec<(NodeIndex, NodeIndex)> = store
            .visible_edges()
            .into_iter()
            .filter(|(source, target)| self.matches_edge(store, *source, *target))
            .collect();
        found.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        found.dedup();
        found
    }
}

/// Ordered selector stylesheet with node and edge patch lists.
#[derive(Clone, Debug, Default)]
pub struct SelectorSheet {
    nodes: Vec<(ElementSelector, crate::style::NodeStylePatch)>,
    edges: Vec<(ElementSelector, crate::style::EdgeStylePatch)>,
}

impl SelectorSheet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a node rule; later rules win over earlier ones.
    pub fn add_node_rule(
        &mut self,
        selector: ElementSelector,
        patch: crate::style::NodeStylePatch,
    ) {
        self.nodes.push((selector, patch));
    }

    /// Appends an edge rule; later rules win over earlier ones.
    pub fn add_edge_rule(
        &mut self,
        selector: ElementSelector,
        patch: crate::style::EdgeStylePatch,
    ) {
        self.edges.push((selector, patch));
    }

    /// Node style with every matching selector applied in order.
    pub fn resolve_node(
        &self,
        store: &GraphStore,
        base: &crate::style::NodeStyle,
        node: NodeIndex,
    ) -> crate::style::NodeStyle {
        let mut resolved = base.clone();
        for (selector, patch) in &self.nodes {
            if selector.matches_node(store, node) {
                resolved = patch.apply_to_style(&resolved);
            }
        }
        resolved
    }

    /// Edge style with every matching selector applied in order.
    pub fn resolve_edge(
        &self,
        store: &GraphStore,
        base: &crate::style::EdgeStyle,
        source: NodeIndex,
        target: NodeIndex,
    ) -> crate::style::EdgeStyle {
        let mut resolved = *base;
        for (selector, patch) in &self.edges {
            if selector.matches_edge(store, source, target) {
                resolved = patch.apply_to_edge(&resolved);
            }
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_graph::GraphStore;

    #[test]
    fn empty_selector_matches_nothing() {
        let store = GraphStore::new();
        let selector = ElementSelector::never();
        assert!(selector.is_empty());
        assert!(selector.select_nodes(&store).is_empty());
        assert!(selector.select_edges(&store).is_empty());
        assert!(!selector.matches_node(&store, NodeIndex::new(0)));
    }

    #[test]
    fn any_node_selector_matches_without_conditions() {
        let selector = ElementSelector::any_node();
        assert!(!selector.is_empty());
    }

    #[test]
    fn target_kinds_stay_exclusive() {
        let node_only = ElementSelector::any_node();
        let edge_only = ElementSelector::any_edge();
        let store = GraphStore::new();
        assert!(!node_only.matches_edge(&store, NodeIndex::new(0), NodeIndex::new(1)));
        assert!(!edge_only.matches_node(&store, NodeIndex::new(0)));
    }
}
