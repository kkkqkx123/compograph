//! Capped selector language shared by queries and style matching.
//!
//! The language covers types, identifiers, class names, attribute
//! comparisons, a store-evaluable state subset and comma combinations.
//! Meta comparisons and adjacency combinators are rejected with a positioned
//! error instead of partial support. Parsed queries are plain owned values,
//! so callers cache them freely.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use petgraph::stable_graph::NodeIndex;

use crate::store::GraphStore;
use crate::view::GraphView;

/// Which element kinds one selector group may match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectorTarget {
    Node,
    Edge,
    #[default]
    Any,
}

/// Comparison applied to one attribute value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrOp {
    Exists,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Literal value carried by an attribute comparison.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectorValue {
    Text(String),
    Number(f64),
    Flag(bool),
}

/// Store-evaluable element state; selection-like states live with the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateFilter {
    Loop,
    Orphan,
    Child,
    Parent,
    Collapsed,
    Visible,
    Hidden,
}

/// One attribute condition inside a selector group.
#[derive(Clone, Debug, PartialEq)]
pub struct AttrCondition {
    pub key: String,
    pub op: AttrOp,
    pub value: Option<SelectorValue>,
}

/// One comma-separated selector group; every condition must hold at once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectorGroup {
    pub target: SelectorTarget,
    pub id: Option<String>,
    pub classes: BTreeSet<String>,
    pub attrs: Vec<AttrCondition>,
    pub states: Vec<StateFilter>,
}

/// Parsed selector query; groups combine with OR.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectorQuery {
    pub groups: Vec<SelectorGroup>,
}

/// Failure to parse a selector; `position` is the byte offset of the cause.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub position: usize,
    pub reason: String,
}

impl ParseError {
    fn at(position: usize, reason: impl Into<String>) -> Self {
        Self {
            position,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid selector at byte {}: {}",
            self.position, self.reason
        )
    }
}

impl std::error::Error for ParseError {}

/// Parses `text` into a reusable query value.
pub fn parse_selector(text: &str) -> Result<SelectorQuery, ParseError> {
    SelectorParser::new(text).parse()
}

/// Small parse-once cache for hot selector strings.
#[derive(Debug, Default)]
pub struct SelectorCache {
    entries: HashMap<String, SelectorQuery>,
}

impl SelectorCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Returns the cached query for `text`, parsing on first use.
    pub fn resolve(&mut self, text: &str) -> Result<SelectorQuery, ParseError> {
        if let Some(query) = self.entries.get(text) {
            return Ok(query.clone());
        }
        let query = parse_selector(text)?;
        self.entries.insert(text.to_string(), query.clone());
        Ok(query)
    }

    /// Number of cached queries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no query has been cached yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

struct SelectorParser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    position: usize,
}

impl<'a> SelectorParser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            bytes: text.as_bytes(),
            position: 0,
        }
    }

    fn parse(mut self) -> Result<SelectorQuery, ParseError> {
        self.skip_spaces();
        if self.exhausted() {
            return Err(ParseError::at(0, "selector is empty"));
        }
        let mut groups = Vec::new();
        loop {
            groups.push(self.parse_group()?);
            self.skip_spaces();
            if self.exhausted() {
                break;
            }
            if self.consume(b',') {
                self.skip_spaces();
                if self.exhausted() {
                    return Err(ParseError::at(self.position, "trailing comma needs a group"));
                }
                continue;
            }
            return Err(ParseError::at(
                self.position,
                "expected a comma between selector groups",
            ));
        }
        Ok(SelectorQuery { groups })
    }

    fn parse_group(&mut self) -> Result<SelectorGroup, ParseError> {
        let mut group = SelectorGroup::default();
        if self.consume(b'*') {
            group.target = SelectorTarget::Any;
        } else if let Some(target) = self.consume_type() {
            group.target = target;
        }
        loop {
            self.skip_spaces();
            if self.exhausted() || self.peek() == Some(b',') {
                break;
            }
            match self.peek() {
                Some(b'#') => {
                    self.position += 1;
                    group.id = Some(self.parse_name("identifier")?);
                }
                Some(b'.') => {
                    self.position += 1;
                    let name = self.parse_name("class name")?;
                    group.classes.insert(name);
                }
                Some(b'[') => {
                    self.position += 1;
                    group.attrs.push(self.parse_attr()?);
                }
                Some(b':') => {
                    self.position += 1;
                    group.states.push(self.parse_state()?);
                }
                Some(b'>') | Some(b'+') | Some(b'~') => {
                    return Err(ParseError::at(
                        self.position,
                        "combinators are out of scope; use comma combinations",
                    ));
                }
                Some(b'-') if self.peek2() == Some(b'>') => {
                    return Err(ParseError::at(
                        self.position,
                        "edge combinators are out of scope; use comma combinations",
                    ));
                }
                _ => {
                    return Err(ParseError::at(
                        self.position,
                        "expected an identifier, class, attribute or state",
                    ));
                }
            }
        }
        Ok(group)
    }

    fn consume_type(&mut self) -> Option<SelectorTarget> {
        for (word, target) in [
            ("node", SelectorTarget::Node),
            ("edge", SelectorTarget::Edge),
        ] {
            if self.text[self.position..].starts_with(word)
                && self
                    .text[self.position + word.len()..]
                    .chars()
                    .next()
                    .is_none_or(|next| !is_name_char(next))
            {
                self.position += word.len();
                return Some(target);
            }
        }
        None
    }

    fn parse_name(&mut self, what: &str) -> Result<String, ParseError> {
        let start = self.position;
        while self
            .text[self.position..]
            .chars()
            .next()
            .is_some_and(is_name_char)
        {
            self.position += self.text[self.position..].chars().next().map_or(0, |c| c.len_utf8());
        }
        if start == self.position {
            return Err(ParseError::at(start, format!("expected a {what}")));
        }
        Ok(self.text[start..self.position].to_string())
    }

    fn parse_attr(&mut self) -> Result<AttrCondition, ParseError> {
        self.skip_spaces();
        if self.peek() == Some(b'[') {
            return Err(ParseError::at(
                self.position,
                "meta selectors are out of scope",
            ));
        }
        if self.peek() == Some(b'!') {
            return Err(ParseError::at(
                self.position,
                "negated existence tests are out of scope",
            ));
        }
        let key = self.parse_name("attribute key")?;
        self.skip_spaces();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(AttrCondition {
                key,
                op: AttrOp::Exists,
                value: None,
            });
        }
        let op = self.parse_attr_op()?;
        self.skip_spaces();
        let value = self.parse_value()?;
        self.skip_spaces();
        if self.peek() != Some(b']') {
            return Err(ParseError::at(self.position, "expected a closing bracket"));
        }
        self.position += 1;
        Ok(AttrCondition {
            key,
            op,
            value: Some(value),
        })
    }

    fn parse_attr_op(&mut self) -> Result<AttrOp, ParseError> {
        let start = self.position;
        let op = if self.consume_str("!=") {
            AttrOp::Ne
        } else if self.consume_str(">=") {
            AttrOp::Ge
        } else if self.consume_str("<=") {
            AttrOp::Le
        } else if self.consume(b'=') {
            AttrOp::Eq
        } else if self.consume(b'>') {
            AttrOp::Gt
        } else if self.consume(b'<') {
            AttrOp::Lt
        } else {
            return Err(ParseError::at(start, "expected an attribute operator"));
        };
        Ok(op)
    }

    fn parse_value(&mut self) -> Result<SelectorValue, ParseError> {
        let start = self.position;
        match self.peek() {
            Some(b'"') | Some(b'\'') => {
                let quote = self.bytes[self.position];
                self.position += 1;
                let content_start = self.position;
                while !self.exhausted() && self.bytes[self.position] != quote {
                    self.position += 1;
                }
                if self.exhausted() {
                    return Err(ParseError::at(start, "unterminated quoted value"));
                }
                let value = self.text[content_start..self.position].to_string();
                self.position += 1;
                Ok(SelectorValue::Text(value))
            }
            Some(_) => {
                let token_start = self.position;
                while self.peek().is_some_and(|byte| {
                    byte != b']'
                        && byte != b','
                        && !byte.is_ascii_whitespace()
                }) {
                    self.position += 1;
                }
                let token = &self.text[token_start..self.position];
                if token.is_empty() {
                    return Err(ParseError::at(start, "expected an attribute value"));
                }
                if let Ok(number) = token.parse::<f64>() {
                    return Ok(SelectorValue::Number(number));
                }
                match token {
                    "true" => Ok(SelectorValue::Flag(true)),
                    "false" => Ok(SelectorValue::Flag(false)),
                    _ => Ok(SelectorValue::Text(token.to_string())),
                }
            }
            None => Err(ParseError::at(start, "expected an attribute value")),
        }
    }

    fn parse_state(&mut self) -> Result<StateFilter, ParseError> {
        let start = self.position;
        let name = self.parse_name("state name")?;
        match name.as_str() {
            "loop" => Ok(StateFilter::Loop),
            "orphan" => Ok(StateFilter::Orphan),
            "child" => Ok(StateFilter::Child),
            "parent" => Ok(StateFilter::Parent),
            "collapsed" => Ok(StateFilter::Collapsed),
            "visible" => Ok(StateFilter::Visible),
            "hidden" => Ok(StateFilter::Hidden),
            _ => Err(ParseError::at(start, format!("unknown state :{name}"))),
        }
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.bytes.get(self.position + 1).copied()
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn consume_str(&mut self, word: &str) -> bool {
        if self.text[self.position..].starts_with(word) {
            self.position += word.len();
            true
        } else {
            false
        }
    }

    fn exhausted(&self) -> bool {
        self.position >= self.bytes.len()
    }
}

fn is_name_char(next: char) -> bool {
    next.is_ascii_alphanumeric() || next == '_' || next == '-'
}

impl SelectorQuery {
    /// Visible nodes matching any group, in index order.
    pub fn select_nodes(&self, store: &GraphStore) -> Vec<NodeIndex> {
        let mut found: Vec<NodeIndex> = store
            .visible_node_ids()
            .into_iter()
            .filter(|node| self.matches_node(store, *node))
            .collect();
        found.sort_unstable_by_key(|node| node.index());
        found
    }

    /// Visible edges matching any group, in endpoint order without duplicates.
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

    /// True when `node` matches any group of the query.
    pub fn matches_node(&self, store: &GraphStore, node: NodeIndex) -> bool {
        self.groups
            .iter()
            .any(|group| group.matches_node(store, node))
    }

    /// True when the directed edge matches any group of the query.
    pub fn matches_edge(
        &self,
        store: &GraphStore,
        source: NodeIndex,
        target: NodeIndex,
    ) -> bool {
        self.groups
            .iter()
            .any(|group| group.matches_edge(store, source, target))
    }
}

impl SelectorGroup {
    fn matches_node(&self, store: &GraphStore, node: NodeIndex) -> bool {
        match self.target {
            SelectorTarget::Edge => return false,
            SelectorTarget::Node | SelectorTarget::Any => {}
        }
        if !store.contains_node(node) {
            return false;
        }
        if let Some(wanted) = &self.id {
            let label = store.node_data(node).map(|data| data.label.as_str());
            if label != Some(wanted.as_str()) {
                return false;
            }
        }
        let held = store.node_classes(node);
        if !self.classes.iter().all(|name| held.contains(name)) {
            return false;
        }
        for condition in &self.attrs {
            if !attr_matches(&node_attr_lookup(store, node, condition), condition) {
                return false;
            }
        }
        self.states
            .iter()
            .all(|state| node_state_matches(store, node, *state))
    }

    fn matches_edge(&self, store: &GraphStore, source: NodeIndex, target: NodeIndex) -> bool {
        match self.target {
            SelectorTarget::Node => return false,
            SelectorTarget::Edge | SelectorTarget::Any => {}
        }
        if self.id.is_some() {
            return false;
        }
        let Some(found) = store.find_edge(source, target) else {
            return false;
        };
        let held = store.edge_classes(found);
        if !self.classes.iter().all(|name| held.contains(name)) {
            return false;
        }
        for condition in &self.attrs {
            if !attr_matches(&edge_attr_lookup(store, found, condition), condition) {
                return false;
            }
        }
        self.states
            .iter()
            .all(|state| edge_state_matches(store, source, target, found, *state))
    }
}

enum LookupValue {
    Missing,
    Value(crate::attrs::DataValue),
    Label(String),
}

fn node_attr_lookup(
    store: &GraphStore,
    node: NodeIndex,
    condition: &AttrCondition,
) -> LookupValue {
    if condition.key == "id" || condition.key == "label" {
        return match store.node_data(node) {
            Some(data) => LookupValue::Label(data.label.clone()),
            None => LookupValue::Missing,
        };
    }
    match store.node_attr(node, &condition.key) {
        Some(value) => LookupValue::Value(value),
        None => LookupValue::Missing,
    }
}

fn edge_attr_lookup(
    store: &GraphStore,
    edge: crate::EdgeIndex,
    condition: &AttrCondition,
) -> LookupValue {
    // Weight lives in the typed edge payload rather than the attribute
    // table, so it bridges here the way node labels bridge above. Missing
    // edges still report missing, keeping the never-matches-missing rule.
    if condition.key == "weight" {
        return match store.edge_data(edge) {
            Some(data) => LookupValue::Value(crate::attrs::DataValue::Number(data.weight as f64)),
            None => LookupValue::Missing,
        };
    }
    match store.edge_attr(edge, &condition.key) {
        Some(value) => LookupValue::Value(value),
        None => LookupValue::Missing,
    }
}

fn attr_matches(lookup: &LookupValue, condition: &AttrCondition) -> bool {
    match condition.op {
        AttrOp::Exists => !matches!(lookup, LookupValue::Missing),
        AttrOp::Eq | AttrOp::Ne | AttrOp::Gt | AttrOp::Ge | AttrOp::Lt | AttrOp::Le => {
            let Some(wanted) = &condition.value else {
                return false;
            };
            compare_values(lookup, wanted, condition.op)
        }
    }
}

fn compare_values(lookup: &LookupValue, wanted: &SelectorValue, op: AttrOp) -> bool {
    let held = match lookup {
        LookupValue::Missing => return false,
        LookupValue::Value(value) => value,
        LookupValue::Label(text) => {
            return compare_text(text, wanted, op);
        }
    };
    match (held, wanted) {
        (crate::attrs::DataValue::Number(left), SelectorValue::Number(right)) => {
            compare_ordered(*left, *right, op)
        }
        (crate::attrs::DataValue::Text(left), SelectorValue::Text(right)) => {
            compare_text_ordered(left, right, op)
        }
        (crate::attrs::DataValue::Flag(left), SelectorValue::Flag(right)) => match op {
            AttrOp::Eq => left == right,
            AttrOp::Ne => left != right,
            _ => false,
        },
        _ => matches!(op, AttrOp::Ne),
    }
}

fn compare_text(held: &str, wanted: &SelectorValue, op: AttrOp) -> bool {
    match wanted {
        SelectorValue::Text(right) => compare_text_ordered(held, right, op),
        _ => matches!(op, AttrOp::Ne),
    }
}

fn compare_ordered(left: f64, right: f64, op: AttrOp) -> bool {
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    match op {
        AttrOp::Eq => left == right,
        AttrOp::Ne => left != right,
        AttrOp::Gt => left > right,
        AttrOp::Ge => left >= right,
        AttrOp::Lt => left < right,
        AttrOp::Le => left <= right,
        AttrOp::Exists => true,
    }
}

fn compare_text_ordered(left: &str, right: &str, op: AttrOp) -> bool {
    match op {
        AttrOp::Eq => left == right,
        AttrOp::Ne => left != right,
        AttrOp::Gt => left > right,
        AttrOp::Ge => left >= right,
        AttrOp::Lt => left < right,
        AttrOp::Le => left <= right,
        AttrOp::Exists => true,
    }
}

fn node_state_matches(store: &GraphStore, node: NodeIndex, state: StateFilter) -> bool {
    match state {
        StateFilter::Loop => store.successors(node).iter().any(|(next, _)| *next == node),
        StateFilter::Orphan => GraphView::degree(store, node) == 0,
        StateFilter::Child => store.parent_of(node).is_some(),
        StateFilter::Parent => store.is_container(node),
        StateFilter::Collapsed => store.is_collapsed(node),
        StateFilter::Visible => store.is_visible(node),
        StateFilter::Hidden => !store.is_visible(node),
    }
}

fn edge_state_matches(
    store: &GraphStore,
    source: NodeIndex,
    target: NodeIndex,
    found: crate::EdgeIndex,
    state: StateFilter,
) -> bool {
    match state {
        StateFilter::Loop => source == target,
        StateFilter::Visible => {
            store.edge_endpoints(found).is_some()
                && store.is_visible(source)
                && store.is_visible(target)
        }
        StateFilter::Hidden => {
            store.edge_endpoints(found).is_some()
                && (!store.is_visible(source) || !store.is_visible(target))
        }
        StateFilter::Orphan
        | StateFilter::Child
        | StateFilter::Parent
        | StateFilter::Collapsed => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::DataValue;
    use crate::store::{EdgeData, NodeData};
    use petgraph::Directed;
    use petgraph::stable_graph::StableGraph;
    use std::collections::{BTreeSet, HashMap};

    fn labeled_store() -> (GraphStore, NodeIndex, NodeIndex, NodeIndex) {
        let mut graph: StableGraph<NodeData, EdgeData, Directed> = StableGraph::default();
        let main = graph.add_node(NodeData { label: "main".into() });
        let hub = graph.add_node(NodeData { label: "hub".into() });
        let leaf = graph.add_node(NodeData { label: "leaf".into() });
        graph.add_edge(main, hub, EdgeData { weight: 2.0 });
        graph.add_edge(hub, leaf, EdgeData { weight: 7.5 });
        graph.add_edge(leaf, leaf, EdgeData { weight: 1.0 });
        let mut store = GraphStore {
            graph,
            ..GraphStore::new()
        };
        store.node_attr_table.insert(
            hub,
            HashMap::from([("score".to_string(), DataValue::Number(9.0))]),
        );
        store
            .node_class_table
            .insert(hub, BTreeSet::from(["hub".to_string()]));
        (store, main, hub, leaf)
    }

    #[test]
    fn empty_and_combinator_queries_report_positions() {
        assert!(parse_selector("").is_err());
        assert!(parse_selector("   ").is_err());
        let trailing = parse_selector("node,").expect_err("trailing comma fails");
        assert!(trailing.position > 0);
        let combinator = parse_selector("node > edge").expect_err("child combinator fails");
        assert!(!combinator.reason.is_empty());
        let meta = parse_selector("[[degree > 1]]").expect_err("meta fails");
        assert!(!meta.reason.is_empty());
        let unknown = parse_selector("node:fast").expect_err("unknown state fails");
        assert!(unknown.reason.contains("fast"));
    }

    #[test]
    fn capped_subset_parses_to_typed_groups() {
        let query = parse_selector("node.hub, edge[weight >= 2]").expect("valid query parses");
        assert_eq!(query.groups.len(), 2);
        assert_eq!(query.groups[0].target, SelectorTarget::Node);
        assert!(query.groups[0].classes.contains("hub"));
        assert_eq!(query.groups[1].target, SelectorTarget::Edge);
        assert_eq!(query.groups[1].attrs.len(), 1);
        let id_query = parse_selector("#main").expect("id parses");
        assert_eq!(id_query.groups[0].id.as_deref(), Some("main"));
        let state_query = parse_selector("node:orphan:visible").expect("states parse");
        assert_eq!(state_query.groups[0].states.len(), 2);
    }

    #[test]
    fn display_reports_position_and_reason() {
        let error = ParseError::at(7, "expected a comma between selector groups");
        assert!(error.to_string().contains('7'));
    }

    #[test]
    fn cache_parses_once_and_reuses() {
        let mut cache = SelectorCache::new();
        assert!(cache.is_empty());
        let first = cache.resolve("node.hub").expect("first parse works");
        let second = cache.resolve("node.hub").expect("cached parse works");
        assert_eq!(first, second);
        assert_eq!(cache.len(), 1);
        assert!(cache.resolve("node > edge").is_err());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn evaluation_covers_id_class_attr_and_state() {
        let (store, main, hub, leaf) = labeled_store();
        assert_eq!(
            parse_selector("#main").expect("id").select_nodes(&store),
            vec![main]
        );
        assert_eq!(
            parse_selector(".hub").expect("class").select_nodes(&store),
            vec![hub]
        );
        assert_eq!(
            parse_selector("[score >= 5]")
                .expect("range")
                .select_nodes(&store),
            vec![hub]
        );
        assert_eq!(
            parse_selector("[label = \"leaf\"]")
                .expect("label key")
                .select_nodes(&store),
            vec![leaf]
        );
        assert!(
            parse_selector("[missing != 1]")
                .expect("inequality")
                .select_nodes(&store)
                .is_empty()
        );
        assert_eq!(
            parse_selector("node:orphan")
                .expect("orphan")
                .select_nodes(&store),
            Vec::new()
        );
        assert_eq!(
            parse_selector("edge:loop")
                .expect("loop edge")
                .select_edges(&store),
            vec![(leaf, leaf)]
        );
        assert_eq!(
            parse_selector("#main, #leaf")
                .expect("comma")
                .select_nodes(&store),
            vec![main, leaf]
        );
        assert_eq!(
            parse_selector("*").expect("wildcard").select_nodes(&store),
            vec![main, hub, leaf]
        );
    }

    #[test]
    fn edge_identity_and_target_kinds_stay_exclusive() {        let (store, main, hub, leaf) = labeled_store();
        let nodes = parse_selector("node").expect("node type");
        assert_eq!(nodes.select_nodes(&store).len(), 3);
        assert!(nodes.select_edges(&store).is_empty());
        let edges = parse_selector("edge").expect("edge type");
        assert_eq!(edges.select_edges(&store).len(), 3);
        assert!(edges.select_nodes(&store).is_empty());
        assert!(!edges.matches_node(&store, main));
        assert!(!nodes.matches_edge(&store, main, hub));
        let _ = leaf;
    }

    #[test]
    fn edge_weight_bridges_the_typed_payload() {
        let (store, main, hub, leaf) = labeled_store();
        assert_eq!(
            parse_selector("edge[weight >= 2]")
                .expect("weight bridges")
                .select_edges(&store),
            vec![(main, hub), (hub, leaf)]
        );
        assert!(
            parse_selector("edge[weight > 100]")
                .expect("heavy")
                .select_edges(&store)
                .is_empty()
        );
        assert!(
            parse_selector("edge[missing = 1]")
                .expect("missing key")
                .select_edges(&store)
                .is_empty()
        );
        let _ = leaf;
    }
}
