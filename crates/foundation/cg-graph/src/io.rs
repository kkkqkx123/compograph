//! File import and export for the graph structure with node positions.
//!
//! The JSON schema stores a node table (stable identifier, label, optional
//! coordinates) alongside an edge table (endpoint identifiers, weight), so one
//! file carries both structure and layout. Import and export are pure
//! functions over owned data: the document sits between the store and the
//! file, which keeps round-trip tests free of any gpui context.
//!
//! DOT export reuses petgraph's own formatter. DOT import needs petgraph's
//! optional parser, which this workspace does not enable, so it reports an
//! explicit error instead of silently dropping the graph.

use std::collections::{HashMap, HashSet};
use std::fmt;

use petgraph::Directed;
use petgraph::dot::Dot;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences, IntoNodeIdentifiers};

use crate::positions::Positions;
use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep signatures readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Failure to encode, decode, or validate a graph document.
#[derive(Clone, Debug, PartialEq)]
pub struct IoError {
    message: String,
}

impl IoError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
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

#[cfg(feature = "json-io")]
impl From<serde_json::Error> for IoError {
    fn from(error: serde_json::Error) -> Self {
        Self::invalid(format!("malformed graph json: {error}"))
    }
}

/// One node row of the JSON schema.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json-io", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeEntry {
    pub id: usize,
    pub label: String,
    pub position: Option<[f32; 2]>,
}

/// One edge row of the JSON schema.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json-io", derive(serde::Serialize, serde::Deserialize))]
pub struct EdgeEntry {
    pub source: usize,
    pub target: usize,
    pub weight: f32,
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
    /// referring to the same rows across exports.
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
            })
            .collect();
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
        let mut seen = HashSet::new();
        for entry in &self.nodes {
            if !seen.insert(entry.id) {
                return Err(IoError::invalid(format!("duplicate node id {}", entry.id)));
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

/// Encodes a collected document as JSON.
#[cfg(feature = "json-io")]
pub fn export_json(document: &GraphDocument) -> Result<String, IoError> {
    serde_json::to_string_pretty(document).map_err(IoError::from)
}

/// Decodes and validates a JSON document produced by [`export_json`].
#[cfg(feature = "json-io")]
pub fn import_json(encoded: &str) -> Result<GraphDocument, IoError> {
    let document: GraphDocument = serde_json::from_str(encoded)?;
    document.validate()?;
    Ok(document)
}

/// Renders the graph in DOT language using petgraph's own formatter.
pub fn export_dot(graph: &Graph) -> String {
    format!("{}", Dot::new(graph))
}

/// DOT import is unavailable without petgraph's optional parser.
///
/// Returning an explicit error keeps callers from mistaking an empty graph for
/// a successful import.
pub fn import_dot(_encoded: &str) -> Result<GraphDocument, IoError> {
    Err(IoError::unsupported(
        "dot import needs petgraph's dot_parser feature, which is disabled",
    ))
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

    #[cfg(feature = "json-io")]
    #[test]
    fn json_round_trip_preserves_structure_labels_weights_and_positions() {
        let (graph, positions) = sample();
        let document = GraphDocument::collect_from(&graph, &positions);
        let encoded = export_json(&document).expect("encoding works");
        let decoded = import_json(&encoded).expect("decoding works");
        assert_eq!(decoded, document);
    }

    #[cfg(feature = "json-io")]
    #[test]
    fn json_round_trip_covers_empty_and_isolated_graphs() {
        let empty: Graph = StableGraph::default();
        let empty_document = GraphDocument::collect_from(&empty, &Positions::new());
        let decoded = import_json(&export_json(&empty_document).expect("encoding works"))
            .expect("decoding works");
        assert!(decoded.nodes.is_empty() && decoded.edges.is_empty());

        let mut solo_graph: Graph = StableGraph::default();
        solo_graph.add_node(NodeData {
            label: "solo".into(),
        });
        let solo_document = GraphDocument::collect_from(&solo_graph, &Positions::new());
        let solo = import_json(&export_json(&solo_document).expect("encoding works"))
            .expect("decoding works");
        assert_eq!(solo.nodes.len(), 1);
        assert!(solo.edges.is_empty());
    }

    #[cfg(feature = "json-io")]
    #[test]
    fn malformed_json_reports_an_error() {
        assert!(import_json("{oops").is_err());
        assert!(
            import_json(r#"{"nodes":[],"edges":[{"source":0,"target":1,"weight":1}]}"#).is_err()
        );
    }

    #[test]
    fn dot_export_mentions_nodes_and_dot_import_stays_explicit() {
        let (graph, _) = sample();
        let encoded = export_dot(&graph);
        assert!(encoded.contains("digraph"));
        let failure = import_dot(&encoded).expect_err("dot import is disabled");
        assert!(failure.message().contains("dot_parser"));
    }
}
