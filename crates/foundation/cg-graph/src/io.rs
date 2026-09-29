//! File import and export for the graph structure with node positions.
//!
//! The JSON schema stores a node table (stable identifier, label, optional
//! coordinates) alongside an edge table (endpoint identifiers, weight), so one
//! file carries both structure and layout. Import and export are pure
//! functions over owned data: the document sits between the store and the
//! file, which keeps round-trip tests free of any gpui context.
//!
//! DOT export reuses petgraph's own formatter. DOT import uses a small
//! hand-written parser covering the subset this application writes plus the
//! common shapes emitted by external tools, so a round trip needs no optional
//! petgraph feature.

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

/// Parses a DOT document into a transferable graph document.
///
/// The parser covers the subset this application emits plus the common shapes
/// external tools produce: a `strict? (di)?graph` header, node statements with
/// attribute blocks, `->` and `--` edges, `;`/`,` separators, `//`, `#` and
/// `/* */` comments, and both quoted and bare identifiers. Node identifiers
/// become dense ids in first-seen order; `label` defaults to the node name,
/// `weight` defaults to one, and `pos="x,y"` fills the optional position.
///
/// Unsupported constructs report an error naming the offending token instead
/// of panicking, so callers can surface a readable message.
pub fn import_dot(encoded: &str) -> Result<GraphDocument, IoError> {
    DotParser::new(encoded).parse()
}

/// One node or edge statement awaiting resolution into the document.
enum DotStatement {
    /// A bare node reference; attributes may follow on the same statement.
    Node { name: String, attrs: Attributes },
    /// A directed edge between two named endpoints.
    Edge {
        source: String,
        target: String,
        attrs: Attributes,
    },
}

/// Attribute block captured from a statement, as key/value string pairs.
type Attributes = HashMap<String, String>;

/// Dense node table plus edge list while the parser walks the input.
struct DotDocument {
    order: Vec<String>,
    ids: HashMap<String, usize>,
    nodes: Vec<NodeEntry>,
    edges: Vec<EdgeEntry>,
}

impl DotDocument {
    fn new() -> Self {
        Self {
            order: Vec::new(),
            ids: HashMap::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Returns the dense id for `name`, registering a new node on first sight.
    fn intern(&mut self, name: &str) -> usize {
        if let Some(id) = self.ids.get(name) {
            return *id;
        }
        let id = self.order.len();
        self.order.push(name.to_string());
        self.ids.insert(name.to_string(), id);
        self.nodes.push(NodeEntry {
            id,
            label: name.to_string(),
            position: None,
        });
        id
    }

    /// Applies `label`/`pos` attributes of a node statement.
    fn apply_node(&mut self, statement: DotStatement) {
        let DotStatement::Node { name, attrs } = statement else {
            return;
        };
        let id = self.intern(&name);
        let Some(entry) = self.nodes.get_mut(id) else {
            return;
        };
        if let Some(label) = attrs.get("label") {
            entry.label = label.clone();
        }
        if let Some(position) = attrs.get("pos").and_then(|raw| parse_position(raw)) {
            entry.position = Some(position);
        }
    }

    /// Applies an edge statement, interning both endpoints.
    fn apply_edge(&mut self, statement: DotStatement) {
        let DotStatement::Edge {
            source,
            target,
            attrs,
        } = statement
        else {
            return;
        };
        let source_id = self.intern(&source);
        let target_id = self.intern(&target);
        // Prefer an explicit `weight`; fall back to a numeric `label`, which is
        // how petgraph's own formatter renders an edge weight during export.
        let weight = attrs
            .get("weight")
            .and_then(|raw| raw.parse::<f32>().ok())
            .or_else(|| attrs.get("label").and_then(|raw| raw.parse::<f32>().ok()))
            .filter(|value| value.is_finite())
            .unwrap_or(1.0);
        self.edges.push(EdgeEntry {
            source: source_id,
            target: target_id,
            weight,
        });
    }

    fn finish(self) -> GraphDocument {
        GraphDocument {
            nodes: self.nodes,
            edges: self.edges,
        }
    }
}

/// Reads `x,y` (optionally suffixed with `!`) into a coordinate pair.
fn parse_position(raw: &str) -> Option<[f32; 2]> {
    let cleaned = raw.trim().trim_end_matches('!');
    let (x, y) = cleaned.split_once(',')?;
    let x = x.trim().parse::<f32>().ok()?;
    let y = y.trim().parse::<f32>().ok()?;
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

/// Minimal DOT parser over a character cursor.
struct DotParser {
    chars: Vec<char>,
    position: usize,
    document: DotDocument,
}

impl DotParser {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            position: 0,
            document: DotDocument::new(),
        }
    }

    fn parse(mut self) -> Result<GraphDocument, IoError> {
        self.skip_trivia();
        // An optional `strict` prefix heads both directed and undirected graphs.
        self.consume_keyword("strict");
        self.skip_trivia();
        if !self.consume_keyword("digraph") {
            self.expect_keyword("graph")?;
        }
        self.skip_trivia();
        // Optional graph name before the body brace.
        if !self.at('{') {
            self.read_identifier()?;
            self.skip_trivia();
        }
        self.expect('{')?;
        self.parse_body()?;
        self.skip_trivia();
        self.expect('}')?;

        let document = self.document.finish();
        document.validate()?;
        Ok(document)
    }

    /// Parses statements until the closing brace of the current block.
    fn parse_body(&mut self) -> Result<(), IoError> {
        loop {
            self.skip_trivia();
            if self.at('}') || self.position >= self.chars.len() {
                return Ok(());
            }
            if self.consume(';') || self.consume(',') || self.consume('{') {
                continue;
            }
            self.parse_statement()?;
            self.skip_trivia();
            // Statements end with `;`, `,` or the next `}`.
            let _ = self.consume(';') || self.consume(',');
        }
    }

    /// Parses one statement: node, edge chain, or attribute assignment.
    fn parse_statement(&mut self) -> Result<(), IoError> {
        // Skip default-attribute statements such as `node [shape=circle]`.
        if self.peek_keyword("node") || self.peek_keyword("edge") || self.peek_keyword("graph") {
            self.read_identifier()?;
            self.skip_trivia();
            let _ = self.read_attribute_block()?;
            return Ok(());
        }
        // A bare key=value assignment at statement level is ignored.
        if self.is_assignment_ahead() {
            self.read_identifier()?;
            self.skip_trivia();
            self.consume('=');
            self.skip_trivia();
            let _ = self.read_value()?;
            return Ok(());
        }

        let first = self.read_identifier()?;
        self.skip_trivia();
        if self.at('-') {
            self.parse_edge_chain(first)
        } else {
            let attrs = self.read_attribute_block()?;
            self.document
                .apply_node(DotStatement::Node { name: first, attrs });
            Ok(())
        }
    }

    /// Parses `a -> b -> c [attrs]`, emitting one edge per hop.
    fn parse_edge_chain(&mut self, first: String) -> Result<(), IoError> {
        let mut endpoints = vec![first];
        loop {
            self.skip_trivia();
            if !self.consume_edge_operator()? {
                break;
            }
            self.skip_trivia();
            endpoints.push(self.read_identifier()?);
        }
        self.skip_trivia();
        let attrs = self.read_attribute_block()?;
        for window in endpoints.windows(2) {
            let (Some(source), Some(target)) = (window.first(), window.get(1)) else {
                continue;
            };
            self.document.apply_edge(DotStatement::Edge {
                source: source.clone(),
                target: target.clone(),
                attrs: attrs.clone(),
            });
        }
        Ok(())
    }

    /// Consumes `->` or `--`, reporting which operator was present.
    fn consume_edge_operator(&mut self) -> Result<bool, IoError> {
        if self.consume_str("->") || self.consume_str("--") {
            return Ok(true);
        }
        if self.at('-') {
            return Err(self.error("expected '->' or '--'"));
        }
        Ok(false)
    }

    /// Reads an optional `[ key=value, ... ]` block.
    fn read_attribute_block(&mut self) -> Result<Attributes, IoError> {
        let mut attrs = Attributes::new();
        self.skip_trivia();
        if !self.consume('[') {
            return Ok(attrs);
        }
        loop {
            self.skip_trivia();
            if self.consume(']') {
                break;
            }
            let key = self.read_identifier()?;
            self.skip_trivia();
            let value = if self.consume('=') {
                self.skip_trivia();
                self.read_value()?
            } else {
                String::new()
            };
            attrs.insert(key, value);
            self.skip_trivia();
            let _ = self.consume(',') || self.consume(';');
        }
        Ok(attrs)
    }

    /// True when the upcoming tokens read as `identifier =` without consuming.
    fn is_assignment_ahead(&self) -> bool {
        let mut cursor = self.position;
        while let Some(current) = self.chars.get(cursor).copied() {
            if current.is_whitespace() {
                cursor += 1;
            } else {
                break;
            }
        }
        while let Some(current) = self.chars.get(cursor).copied() {
            if current.is_whitespace()
                || matches!(current, '{' | '}' | '[' | ']' | ';' | ',' | '=' | '"' | '-')
            {
                break;
            }
            cursor += 1;
        }
        while let Some(current) = self.chars.get(cursor).copied() {
            if current.is_whitespace() {
                cursor += 1;
            } else {
                break;
            }
        }
        self.chars.get(cursor).copied() == Some('=')
    }

    /// Reads an attribute value: quoted string, number, or bare identifier.
    fn read_value(&mut self) -> Result<String, IoError> {
        self.skip_trivia();
        if self.at('"') {
            return self.read_quoted();
        }
        self.read_identifier()
    }

    /// Reads a double-quoted string, resolving escapes and `\"`.
    fn read_quoted(&mut self) -> Result<String, IoError> {
        self.expect('"')?;
        let mut value = String::new();
        while let Some(current) = self.bump() {
            match current {
                '"' => return Ok(value),
                '\\' => {
                    if let Some(escaped) = self.bump() {
                        match escaped {
                            'n' => value.push('\n'),
                            't' => value.push('\t'),
                            'r' => value.push('\r'),
                            other => value.push(other),
                        }
                    }
                }
                other => value.push(other),
            }
        }
        Err(self.error("unterminated string"))
    }

    /// Reads a quoted string or a bare identifier as a name.
    fn read_identifier(&mut self) -> Result<String, IoError> {
        self.skip_trivia();
        if self.at('"') {
            return self.read_quoted();
        }
        let mut name = String::new();
        while let Some(current) = self.chars.get(self.position).copied() {
            if current.is_whitespace()
                || matches!(current, '{' | '}' | '[' | ']' | ';' | ',' | '=' | '"')
                || current == '-'
            {
                break;
            }
            name.push(current);
            self.position += 1;
        }
        if name.is_empty() {
            return Err(self.error("expected an identifier"));
        }
        Ok(name)
    }

    /// Consumes `keyword` when it stands alone as the next token.
    fn consume_keyword(&mut self, keyword: &str) -> bool {
        if !self.peek_keyword(keyword) {
            return false;
        }
        self.position += keyword.chars().count();
        true
    }

    /// Consumes `keyword`, reporting an error when it is absent.
    fn expect_keyword(&mut self, keyword: &str) -> Result<(), IoError> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{keyword}'")))
        }
    }

    /// True when `keyword` appears as a whole token at the cursor.
    fn peek_keyword(&self, keyword: &str) -> bool {
        let remaining: String = self.chars[self.position..].iter().collect();
        if !remaining.starts_with(keyword) {
            return false;
        }
        match remaining.chars().nth(keyword.chars().count()) {
            None => true,
            Some(next) => !next.is_alphanumeric() && next != '_',
        }
    }

    /// Skips whitespace and `//`, `#`, `/* */` comments.
    fn skip_trivia(&mut self) {
        loop {
            while let Some(current) = self.chars.get(self.position).copied() {
                if current.is_whitespace() {
                    self.position += 1;
                } else {
                    break;
                }
            }
            if self.consume_str("//") || self.consume('#') {
                while let Some(current) = self.chars.get(self.position).copied() {
                    if current == '\n' {
                        break;
                    }
                    self.position += 1;
                }
                continue;
            }
            if self.consume_str("/*") {
                while self.position < self.chars.len() && !self.consume_str("*/") {
                    self.position += 1;
                }
                continue;
            }
            return;
        }
    }

    fn at(&self, expected: char) -> bool {
        self.chars.get(self.position).copied() == Some(expected)
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.at(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn consume_str(&mut self, expected: &str) -> bool {
        let remaining: String = self.chars[self.position..].iter().collect();
        if remaining.starts_with(expected) {
            self.position += expected.chars().count();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), IoError> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn bump(&mut self) -> Option<char> {
        let current = self.chars.get(self.position).copied();
        if current.is_some() {
            self.position += 1;
        }
        current
    }

    /// Builds an error carrying the line number of the cursor.
    fn error(&self, message: &str) -> IoError {
        let line = self.chars[..self.position.min(self.chars.len())]
            .iter()
            .filter(|character| **character == '\n')
            .count()
            + 1;
        IoError::invalid(format!("dot parse error on line {line}: {message}"))
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
    fn dot_export_mentions_nodes_and_round_trips_through_import() {
        let (graph, _) = sample();
        let encoded = export_dot(&graph);
        assert!(encoded.contains("digraph"));

        let document = import_dot(&encoded).expect("dot import reads petgraph output");
        assert_eq!(document.nodes.len(), 3);
        assert_eq!(document.edges.len(), 1);
        assert!((document.edges[0].weight - 2.5).abs() < 1e-5);
        let labels: Vec<&str> = document
            .nodes
            .iter()
            .map(|entry| entry.label.as_str())
            .collect();
        assert!(labels.contains(&"a") && labels.contains(&"b") && labels.contains(&"solo"));
        assert!(document.validate().is_ok());
    }

    #[test]
    fn dot_import_reads_names_edges_and_attributes() {
        let encoded = r#"
            strict digraph demo {
                // a leading comment
                a [label="Alpha", pos="1,2!"];
                b;
                a -> b [weight=3.5];
                b -> c [label="edge"];
                c;
            }
        "#;
        let document = import_dot(encoded).expect("dot parses");
        assert_eq!(document.nodes.len(), 3);
        assert_eq!(document.edges.len(), 2);
        let alpha = document
            .nodes
            .iter()
            .find(|entry| entry.label == "Alpha")
            .expect("label attribute wins over the name");
        assert_eq!(alpha.position, Some([1.0, 2.0]));
        assert!((document.edges[0].weight - 3.5).abs() < 1e-5);
        assert!((document.edges[1].weight - 1.0).abs() < 1e-5);
    }

    #[test]
    fn dot_import_accepts_undirected_edges_and_comments() {
        let encoded = "# hash comment\ngraph {\n  /* block comment */\n  a -- b;\n  b -- c\n}\n";
        let document = import_dot(encoded).expect("undirected dot parses");
        assert_eq!(document.nodes.len(), 3);
        assert_eq!(document.edges.len(), 2);
    }

    #[test]
    fn dot_import_handles_edge_chains_and_default_attributes() {
        let encoded = "digraph { node [shape=circle]; a -> b -> c [weight=2]; }";
        let document = import_dot(encoded).expect("chain parses");
        assert_eq!(document.nodes.len(), 3);
        assert_eq!(document.edges.len(), 2);
        for edge in &document.edges {
            assert!((edge.weight - 2.0).abs() < 1e-5);
        }
    }

    #[test]
    fn dot_import_handles_isolated_and_empty_graphs() {
        let solo = import_dot("digraph { lonely; }").expect("isolated node parses");
        assert_eq!(solo.nodes.len(), 1);
        assert!(solo.edges.is_empty());

        let empty = import_dot("digraph { }").expect("empty body parses");
        assert!(empty.nodes.is_empty() && empty.edges.is_empty());
    }

    #[test]
    fn dot_import_reports_malformed_input_without_panicking() {
        assert!(import_dot("").is_err());
        assert!(import_dot("not a graph").is_err());
        assert!(import_dot("digraph { a -> }").is_err());
        assert!(import_dot("digraph { a -> b ").is_err());
        let failure = import_dot("digraph { \"unterminated }").expect_err("bad string");
        assert!(failure.message().contains("line"));
    }
}
