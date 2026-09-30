//! JSON import and export for graph documents.

use super::document::{GraphDocument, IoError};

#[cfg(feature = "json-io")]
impl From<serde_json::Error> for IoError {
    fn from(error: serde_json::Error) -> Self {
        Self::invalid(format!("malformed graph json: {error}"))
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

#[cfg(test)]
mod tests {
    #[cfg(feature = "json-io")]
    use super::*;
    #[cfg(feature = "json-io")]
    use petgraph::stable_graph::StableGraph;

    #[cfg(feature = "json-io")]
    use crate::positions::Positions;
    #[cfg(feature = "json-io")]
    use crate::store::{EdgeData, NodeData};

    #[cfg(feature = "json-io")]
    fn sample() -> (
        StableGraph<NodeData, EdgeData, petgraph::Directed>,
        Positions,
    ) {
        let mut graph: StableGraph<NodeData, EdgeData, petgraph::Directed> =
            StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        graph.add_edge(a, b, EdgeData { weight: 2.5 });
        let positions: Positions = [
            (a, cg_types::Point2::new(10.0, 20.0)),
            (b, cg_types::Point2::new(30.0, 40.0)),
        ]
        .into_iter()
        .collect();
        (graph, positions)
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
        let empty: StableGraph<NodeData, EdgeData, petgraph::Directed> = StableGraph::default();
        let empty_document = GraphDocument::collect_from(&empty, &Positions::new());
        let decoded = import_json(&export_json(&empty_document).expect("encoding works"))
            .expect("decoding works");
        assert!(decoded.nodes.is_empty() && decoded.edges.is_empty());

        let mut solo_graph: StableGraph<NodeData, EdgeData, petgraph::Directed> =
            StableGraph::default();
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
}
