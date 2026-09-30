//! Free-form element attributes shared by data mapping and selectors.
//!
//! Attributes live in side tables on the store so the core node and edge
//! payloads stay untouched. Each table maps an element to its key table.

use std::collections::HashMap;

use petgraph::stable_graph::{EdgeIndex, NodeIndex};

use crate::events::GraphChangeEvent;
use crate::store::GraphStore;

/// Free-form value carried by one attribute key.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json-io", derive(serde::Serialize, serde::Deserialize))]
pub enum DataValue {
    Text(String),
    Number(f64),
    Flag(bool),
}

impl DataValue {
    /// Numeric view of the value; text and flags have none.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            DataValue::Number(value) => Some(*value),
            DataValue::Text(_) | DataValue::Flag(_) => None,
        }
    }

    /// Text view of the value; numbers and flags have none.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            DataValue::Text(value) => Some(value.as_str()),
            DataValue::Number(_) | DataValue::Flag(_) => None,
        }
    }

    /// Flag view of the value; numbers and text have none.
    pub fn as_flag(&self) -> Option<bool> {
        match self {
            DataValue::Flag(value) => Some(*value),
            DataValue::Text(_) | DataValue::Number(_) => None,
        }
    }
}

/// True when `key` may name an attribute.
pub fn valid_attr_key(key: &str) -> bool {
    !key.is_empty() && !key.chars().any(|point| point.is_whitespace())
}

impl GraphStore {
    /// Attribute table of `node`; empty when absent or removed.
    pub fn node_attrs(&self, node: NodeIndex) -> HashMap<String, DataValue> {
        self.node_attr_table.get(&node).cloned().unwrap_or_default()
    }

    /// Single attribute of `node`, if present.
    pub fn node_attr(&self, node: NodeIndex, key: &str) -> Option<DataValue> {
        self.node_attr_table
            .get(&node)
            .and_then(|table| table.get(key).cloned())
    }

    /// Stores one node attribute; rejects blank keys and missing nodes.
    pub fn set_node_attr(
        &mut self,
        cx: &mut gpui::Context<Self>,
        node: NodeIndex,
        key: impl Into<String>,
        value: DataValue,
    ) -> bool {
        let key = key.into();
        if !valid_attr_key(&key) || self.graph.node_weight(node).is_none() {
            return false;
        }
        self.node_attr_table
            .entry(node)
            .or_default()
            .insert(key, value);
        cx.emit(GraphChangeEvent::NodeAttrChanged(node));
        cx.notify();
        true
    }

    /// Removes one node attribute; false when absent.
    pub fn remove_node_attr(
        &mut self,
        cx: &mut gpui::Context<Self>,
        node: NodeIndex,
        key: &str,
    ) -> bool {
        let removed = self
            .node_attr_table
            .get_mut(&node)
            .is_some_and(|table| table.remove(key).is_some());
        if removed {
            cx.emit(GraphChangeEvent::NodeAttrChanged(node));
            cx.notify();
        }
        removed
    }

    /// Attribute table of `edge`; empty when absent or removed.
    pub fn edge_attrs(&self, edge: EdgeIndex) -> HashMap<String, DataValue> {
        self.edge_attr_table.get(&edge).cloned().unwrap_or_default()
    }

    /// Single attribute of `edge`, if present.
    pub fn edge_attr(&self, edge: EdgeIndex, key: &str) -> Option<DataValue> {
        self.edge_attr_table
            .get(&edge)
            .and_then(|table| table.get(key).cloned())
    }

    /// Stores one edge attribute; rejects blank keys and missing edges.
    pub fn set_edge_attr(
        &mut self,
        cx: &mut gpui::Context<Self>,
        edge: EdgeIndex,
        key: impl Into<String>,
        value: DataValue,
    ) -> bool {
        let key = key.into();
        if !valid_attr_key(&key) || self.graph.edge_weight(edge).is_none() {
            return false;
        }
        self.edge_attr_table
            .entry(edge)
            .or_default()
            .insert(key, value);
        cx.emit(GraphChangeEvent::EdgeAttrChanged(edge));
        cx.notify();
        true
    }

    /// Removes one edge attribute; false when absent.
    pub fn remove_edge_attr(
        &mut self,
        cx: &mut gpui::Context<Self>,
        edge: EdgeIndex,
        key: &str,
    ) -> bool {
        let removed = self
            .edge_attr_table
            .get_mut(&edge)
            .is_some_and(|table| table.remove(key).is_some());
        if removed {
            cx.emit(GraphChangeEvent::EdgeAttrChanged(edge));
            cx.notify();
        }
        removed
    }

    pub(crate) fn drop_node_attrs(&mut self, node: NodeIndex) {
        self.node_attr_table.remove(&node);
    }

    pub(crate) fn drop_edge_attrs(&mut self, edge: EdgeIndex) {
        self.edge_attr_table.remove(&edge);
    }

    pub(crate) fn clear_attr_tables(&mut self) {
        self.node_attr_table.clear();
        self.edge_attr_table.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_views_stay_typed() {
        assert_eq!(DataValue::Number(2.0).as_number(), Some(2.0));
        assert_eq!(DataValue::Text("a".into()).as_text(), Some("a"));
        assert_eq!(DataValue::Flag(true).as_flag(), Some(true));
        assert_eq!(DataValue::Text("a".into()).as_number(), None);
        assert_eq!(DataValue::Number(1.0).as_text(), None);
        assert_eq!(DataValue::Number(1.0).as_flag(), None);
    }

    #[test]
    fn blank_keys_are_rejected() {
        assert!(!valid_attr_key(""));
        assert!(!valid_attr_key("has space"));
        assert!(valid_attr_key("weight_1"));
    }
}
