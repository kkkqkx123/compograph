//! Transfer document between the store and encoded files.

use std::collections::{HashMap, HashSet};
use std::fmt;

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences, IntoNodeIdentifiers};

use crate::attrs::DataValue;
use crate::positions::Positions;
use crate::store::{EdgeData, GraphStore, NodeData};

type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Failure to encode, decode, or validate a graph document.
#[derive(Clone, Debug, PartialEq)]
pub struct IoError {
    message: String,
}

impl IoError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Human-readable reason the operation failed.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for IoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for IoError {}

/// One node row of the JSON schema.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "json-io", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeEntry {
    pub id: usize,
    pub label: String,
    pub position: Option<[f32; 2]>,
    #[cfg_attr(feature = "json-io", serde(default))]
    pub attrs: HashMap<String, DataValue>,
    #[cfg_attr(feature = "json-io", serde(default))]
    pub classes: Vec<String>,
    #[cfg_attr(feature = "json-io", serde(default))]
    pub parent: Option<usize>,
    #[cfg_attr(feature = "json-io", serde(default))]
    pub collapsed: bool,
}

/// One edge row of the JSON schema.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "json-io", derive(serde::Serialize, serde::Deserialize))]
pub struct EdgeEntry {
    pub source: usize,
    pub target: usize,
    pub weight: f32,
    #[cfg_attr(feature = "json-io", serde(default))]
    pub attrs: HashMap<String, DataValue>,
    #[cfg_attr(feature = "json-io", serde(default))]
    pub classes: Vec<String>,
}

/// Owned snapshot of a graph and its layout, ready to encode or decode.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "json-io", derive(serde::Serialize, serde::Deserialize))]
pub struct GraphDocument {
    pub nodes: Vec<NodeEntry>,
    pub edges: Vec<EdgeEntry>,
}

impl GraphDocument {
    /// Collects node labels, weights, and positions into transferable rows.
    ///
    /// Identifiers are the stable graph indices, so an edited store keeps
    /// referring to the same rows across exports. Attribute tables, class
    /// sets, and hierarchy are left empty; use `collect_from_store` for the
    /// full snapshot.
    pub fn collect_from(graph: &Graph, positions: &Positions) -> Self {
        let mut nodes: Vec<NodeEntry> = graph
            .node_identifiers()
            .map(|node| {
                let label = graph
                    .node_weight(node)
                    .map(|data| data.label.clone())
                    .unwrap_or_default();
                let position = positions.get(&node).map(|point| [point.x, point.y]);
                NodeEntry {
                    id: node.index(),
                    label,
                    position,
                    ..NodeEntry::default()
                }
            })
            .collect();
        nodes.sort_by_key(|entry| entry.id);
        let mut edges: Vec<EdgeEntry> = graph
            .edge_references()
            .map(|edge| EdgeEntry {
                source: edge.source().index(),
                target: edge.target().index(),
                weight: edge.weight().weight,
                ..EdgeEntry::default()
            })
            .collect();
        edges.sort_by_key(|entry| (entry.source, entry.target));
        Self { nodes, edges }
    }

    /// Full snapshot including attributes, classes, and hierarchy.
    ///
    /// The plain text exchange keeps labels and weights only; this entry
    /// point is the one the JSON exchange uses.
    pub fn collect_from_store(store: &GraphStore, positions: &Positions) -> Self {
        let graph = store.graph();
        let mut nodes: Vec<NodeEntry> = graph
            .node_identifiers()
            .map(|node| {
                let label = graph
                    .node_weight(node)
                    .map(|data| data.label.clone())
                    .unwrap_or_default();
                let position = positions.get(&node).map(|point| [point.x, point.y]);
                let mut classes: Vec<String> = store.node_classes(node).into_iter().collect();
                classes.sort();
                NodeEntry {
                    id: node.index(),
                    label,
                    position,
                    attrs: store.node_attrs(node),
                    classes,
                    parent: store.parent_of(node).map(|parent| parent.index()),
                    collapsed: store.is_collapsed(node),
                }
            })
            .collect();
        nodes.sort_by_key(|entry| entry.id);
        let mut edge_ids: Vec<petgraph::stable_graph::EdgeIndex> = graph.edge_indices().collect();
        edge_ids.sort_by_key(|edge| edge.index());
        let mut edges: Vec<EdgeEntry> = Vec::new();
        for edge in edge_ids {
            let Some((source, target)) = graph.edge_endpoints(edge) else {
                continue;
            };
            let weight = graph
                .edge_weight(edge)
                .map(|data| data.weight)
                .unwrap_or(1.0);
            let mut classes: Vec<String> = store.edge_classes(edge).into_iter().collect();
            classes.sort();
            edges.push(EdgeEntry {
                source: source.index(),
                target: target.index(),
                weight,
                attrs: store.edge_attrs(edge),
                classes,
            });
        }
        edges.sort_by_key(|entry| (entry.source, entry.target));
        Self { nodes, edges }
    }

    /// Positions keyed by freshly created node indices.
    ///
    /// The caller supplies the mapping from document identifiers to the live
    /// indices produced while rebuilding the store, because removals can leave
    /// the document identifiers sparse.
    pub fn positions_for(&self, resolve: impl Fn(usize) -> Option<NodeIndex>) -> Positions {
        let mut positions = Positions::new();
        for entry in &self.nodes {
            if let (Some(position), Some(node)) = (entry.position, resolve(entry.id)) {
                positions.insert(node, cg_types::Point2::new(position[0], position[1]));
            }
        }
        positions
    }

    /// Rejects duplicate identifiers and edges dangling past the node table.
    pub fn validate(&self) -> Result<(), IoError> {
        use crate::compound::MAX_COMPOUND_DEPTH;
        let mut seen = HashSet::new();
        for entry in &self.nodes {
            if !seen.insert(entry.id) {
                return Err(IoError::invalid(format!("duplicate node id {}", entry.id)));
            }
        }
        let mut parent_of: HashMap<usize, usize> = HashMap::new();
        for entry in &self.nodes {
            if let Some(parent) = entry.parent {
                if !seen.contains(&parent) {
                    return Err(IoError::invalid(format!(
                        "node {} names an unknown parent {}",
                        entry.id, parent
                    )));
                }
                if parent == entry.id {
                    return Err(IoError::invalid(format!(
                        "node {} cannot parent itself",
                        entry.id
                    )));
                }
                parent_of.insert(entry.id, parent);
            }
            for value in entry.attrs.values() {
                if let DataValue::Number(number) = value
                    && !number.is_finite()
                {
                    return Err(IoError::invalid("attribute numbers must be finite"));
                }
            }
        }
        for entry in &self.nodes {
            let mut cursor = entry.id;
            let mut depth = 0usize;
            let mut chain: HashSet<usize> = HashSet::new();
            chain.insert(cursor);
            while let Some(parent) = parent_of.get(&cursor).copied() {
                if !chain.insert(parent) {
                    return Err(IoError::invalid(format!(
                        "node {} closes a parent cycle",
                        entry.id
                    )));
                }
                depth += 1;
                if depth > MAX_COMPOUND_DEPTH {
                    return Err(IoError::invalid(format!(
                        "node {} exceeds the compound depth limit",
                        entry.id
                    )));
                }
                cursor = parent;
            }
        }
        let mut has_child: HashSet<usize> = HashSet::new();
        for child in parent_of.keys() {
            if let Some(parent) = parent_of.get(child) {
                has_child.insert(*parent);
            }
        }
        for entry in &self.nodes {
            if entry.collapsed && !has_child.contains(&entry.id) {
                return Err(IoError::invalid(format!(
                    "node {} is marked collapsed without children",
                    entry.id
                )));
            }
        }
        for entry in &self.edges {
            if !seen.contains(&entry.source) || !seen.contains(&entry.target) {
                return Err(IoError::invalid(format!(
                    "edge {} -> {} names an unknown node",
                    entry.source, entry.target
                )));
            }
            if !entry.weight.is_finite() {
                return Err(IoError::invalid("edge weights must be finite"));
            }
        }
        Ok(())
    }
}

/// Maps document identifiers to live indices in identifier order.
///
/// Import rebuilds nodes in sorted order, so this helper mirrors the mapping
/// the application builds while inserting them.
pub fn remap_positions(document: &GraphDocument, order: &[NodeIndex]) -> Positions {
    let mut by_id: HashMap<usize, NodeIndex> = HashMap::new();
    let mut sorted: Vec<usize> = document.nodes.iter().map(|entry| entry.id).collect();
    sorted.sort_unstable();
    for (entry_id, node) in sorted.into_iter().zip(order.iter().copied()) {
        by_id.insert(entry_id, node);
    }
    document.positions_for(|id| by_id.get(&id).copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::GraphStore;

    fn sample() -> (Graph, Positions) {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let lonely = graph.add_node(NodeData {
            label: "solo".into(),
        });
        graph.add_edge(a, b, EdgeData { weight: 2.5 });
        let positions: Positions = [
            (a, cg_types::Point2::new(10.0, 20.0)),
            (b, cg_types::Point2::new(30.0, 40.0)),
        ]
        .into_iter()
        .collect();
        let _ = lonely;
        (graph, positions)
    }

    #[test]
    fn collect_covers_nodes_edges_and_sparse_positions() {
        let (graph, positions) = sample();
        let document = GraphDocument::collect_from(&graph, &positions);
        assert_eq!(document.nodes.len(), 3);
        assert_eq!(document.edges.len(), 1);
        assert_eq!(document.edges[0].weight, 2.5);
        let solo = document
            .nodes
            .iter()
            .find(|entry| entry.label == "solo")
            .expect("isolated node is kept");
        assert!(solo.position.is_none());
        assert!(document.validate().is_ok());
    }

    #[test]
    fn validation_rejects_duplicates_dangling_edges_and_bad_weights() {
        let (graph, positions) = sample();
        let mut document = GraphDocument::collect_from(&graph, &positions);
        let mut doubled = document.clone();
        doubled.nodes.push(doubled.nodes[0].clone());
        assert!(doubled.validate().is_err());
        document.edges.push(EdgeEntry {
            source: 999,
            target: 0,
            weight: 1.0,
            ..EdgeEntry::default()
        });
        assert!(document.validate().is_err());
        let mut bad_weight = GraphDocument::collect_from(&graph, &positions);
        bad_weight.edges[0].weight = f32::NAN;
        assert!(bad_weight.validate().is_err());
    }

    #[test]
    fn remap_positions_follows_insertion_order() {
        let (graph, positions) = sample();
        let document = GraphDocument::collect_from(&graph, &positions);
        let order: Vec<NodeIndex> = document
            .nodes
            .iter()
            .map(|entry| NodeIndex::new(entry.id))
            .collect();
        let mapped = remap_positions(&document, &order);
        assert_eq!(mapped.len(), positions.len());
        for (node, point) in &positions {
            assert_eq!(mapped.get(node), Some(point));
        }
    }

    #[test]
    fn store_graph_type_matches_the_document_collector() {
        let store = GraphStore::new();
        let document = GraphDocument::collect_from(store.graph(), &Positions::new());
        assert!(document.nodes.is_empty() && document.edges.is_empty());
        assert!(document.validate().is_ok());
    }
}
