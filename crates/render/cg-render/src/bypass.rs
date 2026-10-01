//! Transient per-element style overrides for selection and highlights.

use std::collections::{BTreeSet, HashMap};

use cg_graph::{DataValue, NodeIndex};

use crate::appearance::{EdgeStyle, EdgeStylePatch, NodeStyle, NodeStylePatch, StyleSheet};
use crate::edge_rules::EdgeMapper;
use crate::node_rules::{NodeDataTables, StyleMapper};

/// Transient per-element overrides for selection and highlights.
///
/// Bypass entries sit on top of the mapped styles and are cleared without
/// touching the sheet or the mappers, so highlights never pollute the main
/// styles. Edge entries are keyed by directed endpoint pairs, so opposite
/// directions stay independent while parallel edges in one direction share
/// their bypass entry.
#[derive(Clone, Debug, Default)]
pub struct BypassStore {
    nodes: HashMap<NodeIndex, NodeStylePatch>,
    edges: HashMap<(NodeIndex, NodeIndex), EdgeStylePatch>,
}

impl BypassStore {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    /// Stores a node override; empty patches clear the entry instead.
    pub fn set_node(&mut self, node: NodeIndex, patch: NodeStylePatch) {
        if patch.is_empty() {
            self.nodes.remove(&node);
        } else {
            self.nodes.insert(node, patch);
        }
    }

    /// Stores a directed edge override shared by parallel edges of one pair.
    pub fn set_edge(&mut self, source: NodeIndex, target: NodeIndex, patch: EdgeStylePatch) {
        if patch.is_empty() {
            self.edges.remove(&(source, target));
        } else {
            self.edges.insert((source, target), patch);
        }
    }

    pub fn clear_node(&mut self, node: NodeIndex) {
        self.nodes.remove(&node);
    }

    pub fn clear_edge(&mut self, source: NodeIndex, target: NodeIndex) {
        self.edges.remove(&(source, target));
    }

    pub fn clear_all(&mut self) {
        self.nodes.clear();
        self.edges.clear();
    }

    pub fn node_bypass(&self, node: NodeIndex) -> Option<&NodeStylePatch> {
        self.nodes.get(&node)
    }

    pub fn edge_bypass(&self, source: NodeIndex, target: NodeIndex) -> Option<&EdgeStylePatch> {
        self.edges.get(&(source, target))
    }

    /// Full node resolution: sheet default, then mapper, then bypass.
    pub fn resolve_node(
        &self,
        sheet: &StyleSheet,
        mapper: &StyleMapper,
        node: NodeIndex,
        label: Option<&str>,
        degree: usize,
    ) -> NodeStyle {
        self.resolve_node_with_data(
            sheet,
            mapper,
            node,
            label,
            degree,
            NodeDataTables {
                attrs: &HashMap::new(),
                classes: &BTreeSet::new(),
            },
        )
    }

    /// Full node resolution carrying attribute and class tables.
    pub fn resolve_node_with_data(
        &self,
        sheet: &StyleSheet,
        mapper: &StyleMapper,
        node: NodeIndex,
        label: Option<&str>,
        degree: usize,
        tables: NodeDataTables<'_>,
    ) -> NodeStyle {
        let mapped = mapper.resolve_node_with_data(
            &sheet.node,
            node,
            label,
            degree,
            tables.attrs,
            tables.classes,
        );
        self.node_bypass(node)
            .map(|patch| patch.apply_to(&mapped))
            .unwrap_or(mapped)
    }

    /// Full edge resolution: sheet default, then mapper, then bypass.
    pub fn resolve_edge(
        &self,
        sheet: &StyleSheet,
        mapper: &EdgeMapper,
        source: NodeIndex,
        target: NodeIndex,
    ) -> EdgeStyle {
        self.resolve_edge_with_data(
            sheet,
            mapper,
            source,
            target,
            &HashMap::new(),
            &BTreeSet::new(),
        )
    }

    /// Full edge resolution carrying attribute and class tables.
    pub fn resolve_edge_with_data(
        &self,
        sheet: &StyleSheet,
        mapper: &EdgeMapper,
        source: NodeIndex,
        target: NodeIndex,
        attrs: &HashMap<String, DataValue>,
        classes: &BTreeSet<String>,
    ) -> EdgeStyle {
        let mapped = mapper.resolve_edge_with_data(&sheet.edge, source, target, attrs, classes);
        self.edge_bypass(source, target)
            .map(|patch| patch.apply_to(&mapped))
            .unwrap_or(mapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::{EdgeStyle, HIGHLIGHT_EDGE_TINT, NodeStyle};

    #[test]
    fn resolution_falls_back_to_the_sheet_defaults() {
        let sheet = StyleSheet::default();
        let mapper = StyleMapper::new();
        let edge_mapper = EdgeMapper::new();
        let bypass = BypassStore::new();
        let node = NodeIndex::new(0);
        assert_eq!(
            bypass.resolve_node(&sheet, &mapper, node, None, 0),
            NodeStyle::default()
        );
        assert_eq!(
            bypass.resolve_edge(&sheet, &edge_mapper, node, NodeIndex::new(1)),
            EdgeStyle::default()
        );
    }

    #[test]
    fn bypass_overrides_and_clears_without_touching_the_sheet() {
        use crate::fill::NodeFill;

        let mut sheet = StyleSheet::default();
        sheet.node.fill = NodeFill::solid(0x123456);
        let mapper = StyleMapper::new();
        let mut bypass = BypassStore::new();
        let node = NodeIndex::new(1);
        bypass.set_node(node, NodeStylePatch::selected());
        let highlighted = bypass.resolve_node(&sheet, &mapper, node, None, 0);
        assert_eq!(highlighted.fill, NodeFill::solid(crate::fill::SELECTED_NODE_FILL));
        bypass.clear_node(node);
        let restored = bypass.resolve_node(&sheet, &mapper, node, None, 0);
        assert_eq!(restored.fill, NodeFill::solid(0x123456));
        assert_eq!(sheet.node.fill, NodeFill::solid(0x123456));
    }

    #[test]
    fn directed_edges_keep_independent_bypass_entries() {
        let sheet = StyleSheet::default();
        let edge_mapper = EdgeMapper::new();
        let mut bypass = BypassStore::new();
        let a = NodeIndex::new(0);
        let b = NodeIndex::new(1);
        bypass.set_edge(a, b, EdgeStylePatch::highlighted());
        assert_eq!(
            bypass.resolve_edge(&sheet, &edge_mapper, a, b).tint,
            HIGHLIGHT_EDGE_TINT
        );
        assert_eq!(
            bypass.resolve_edge(&sheet, &edge_mapper, b, a),
            EdgeStyle::default()
        );
        bypass.set_edge(a, b, EdgeStylePatch::default());
        assert_eq!(
            bypass.resolve_edge(&sheet, &edge_mapper, a, b),
            EdgeStyle::default()
        );
    }
}
