//! DOT import and export for graph documents.

use std::collections::HashMap;

use petgraph::Directed;
use petgraph::dot::Dot;
use petgraph::stable_graph::StableGraph;

use super::document::{EdgeEntry, GraphDocument, IoError, NodeEntry};
use crate::store::{EdgeData, NodeData};

type Graph = StableGraph<NodeData, EdgeData, Directed>;

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
            ..NodeEntry::default()
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
            ..EdgeEntry::default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::positions::Positions;

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
