//! Layered element styling with per-element bypass overrides.
//!
//! The main styles live in [`StyleSheet`] as plain defaults. [`StyleMapper`]
//! derives patches from typed predicates over labels, degrees, or algorithm
//! result sets, without any string selector language. [`BypassStore`] holds
//! transient per-element overrides for selection, hover, and algorithm
//! highlights, so highlights never mutate the main styles.

use std::collections::{HashMap, HashSet};

use cg_graph::NodeIndex;

/// Fill of nodes carrying no mapping or bypass.
pub const DEFAULT_NODE_FILL: u32 = 0x4a9eff;

/// Outline of nodes carrying no mapping or bypass.
pub const DEFAULT_NODE_STROKE: u32 = 0x2c5f9e;

/// Fill applied through the bypass to selected or highlighted nodes.
pub const SELECTED_NODE_FILL: u32 = 0xff9f2e;

/// Fill applied through the bypass to the hovered node.
pub const HOVER_NODE_FILL: u32 = 0x8fc2ff;

/// Stroke of edges carrying no mapping or bypass.
pub const DEFAULT_EDGE_TINT: u32 = 0x8a93a6;

/// Tint applied through the bypass to highlighted edges.
pub const HIGHLIGHT_EDGE_TINT: u32 = 0xff9f2e;

/// Resolved appearance of one node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeStyle {
    pub fill: u32,
    pub stroke: u32,
    pub stroke_width: f32,
    pub opacity: f32,
    pub label_size: f32,
    pub scale: f32,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            fill: DEFAULT_NODE_FILL,
            stroke: DEFAULT_NODE_STROKE,
            stroke_width: 1.5,
            opacity: 1.0,
            label_size: 12.0,
            scale: 1.0,
        }
    }
}

/// Resolved appearance of one edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeStyle {
    pub tint: u32,
    pub width: f32,
    pub opacity: f32,
    pub arrow_scale: f32,
    pub label_size: f32,
}

impl Default for EdgeStyle {
    fn default() -> Self {
        Self {
            tint: DEFAULT_EDGE_TINT,
            width: 1.5,
            opacity: 1.0,
            arrow_scale: 1.0,
            label_size: 11.0,
        }
    }
}

/// Partial node appearance; set fields override the base on resolution.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeStylePatch {
    pub fill: Option<u32>,
    pub stroke: Option<u32>,
    pub stroke_width: Option<f32>,
    pub opacity: Option<f32>,
    pub label_size: Option<f32>,
    pub scale: Option<f32>,
}

impl NodeStylePatch {
    /// True when the patch carries no override.
    pub fn is_empty(&self) -> bool {
        self.fill.is_none()
            && self.stroke.is_none()
            && self.stroke_width.is_none()
            && self.opacity.is_none()
            && self.label_size.is_none()
            && self.scale.is_none()
    }

    /// Patch selecting a node through the bypass channel.
    pub fn selected() -> Self {
        Self {
            fill: Some(SELECTED_NODE_FILL),
            ..Self::default()
        }
    }

    /// Patch marking the hovered node through the bypass channel.
    pub fn hovered() -> Self {
        Self {
            fill: Some(HOVER_NODE_FILL),
            ..Self::default()
        }
    }

    /// Patch tinting a node, used for result group coloring.
    pub fn tinted(fill: u32) -> Self {
        Self {
            fill: Some(fill),
            ..Self::default()
        }
    }

    /// Patch resizing a node, used for score-driven size mapping.
    pub fn rescaled(scale: f32) -> Self {
        Self {
            scale: Some(scale),
            ..Self::default()
        }
    }

    fn apply_to(&self, base: &NodeStyle) -> NodeStyle {
        NodeStyle {
            fill: self.fill.unwrap_or(base.fill),
            stroke: self.stroke.unwrap_or(base.stroke),
            stroke_width: self.stroke_width.unwrap_or(base.stroke_width),
            opacity: self.opacity.unwrap_or(base.opacity),
            label_size: self.label_size.unwrap_or(base.label_size),
            scale: self.scale.unwrap_or(base.scale),
        }
    }
}

/// Partial edge appearance; set fields override the base on resolution.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeStylePatch {
    pub tint: Option<u32>,
    pub width: Option<f32>,
    pub opacity: Option<f32>,
    pub arrow_scale: Option<f32>,
    pub label_size: Option<f32>,
}

impl EdgeStylePatch {
    /// True when the patch carries no override.
    pub fn is_empty(&self) -> bool {
        self.tint.is_none()
            && self.width.is_none()
            && self.opacity.is_none()
            && self.arrow_scale.is_none()
            && self.label_size.is_none()
    }

    /// Patch highlighting an edge through the bypass channel.
    pub fn highlighted() -> Self {
        Self {
            tint: Some(HIGHLIGHT_EDGE_TINT),
            width: Some(3.0),
            ..Self::default()
        }
    }

    /// Patch thickening an edge, used for spanning-tree emphasis.
    pub fn widened() -> Self {
        Self {
            width: Some(3.5),
            ..Self::default()
        }
    }

    fn apply_to(&self, base: &EdgeStyle) -> EdgeStyle {
        EdgeStyle {
            tint: self.tint.unwrap_or(base.tint),
            width: self.width.unwrap_or(base.width),
            opacity: self.opacity.unwrap_or(base.opacity),
            arrow_scale: self.arrow_scale.unwrap_or(base.arrow_scale),
            label_size: self.label_size.unwrap_or(base.label_size),
        }
    }
}

/// Default styles shared by every element.
#[derive(Clone, Debug, Default)]
pub struct StyleSheet {
    pub node: NodeStyle,
    pub edge: EdgeStyle,
}

/// Typed node condition; later rules in the mapper win over earlier ones.
#[derive(Clone, Debug)]
pub enum NodePredicate {
    /// Matches every node.
    Any,
    /// Matches nodes whose label equals the given text.
    LabelIs(String),
    /// Matches nodes with at least the given number of incident edges.
    DegreeAtLeast(usize),
    /// Matches nodes contained in an algorithm result set.
    InSet(HashSet<NodeIndex>),
}

impl NodePredicate {
    fn matches(&self, node: NodeIndex, label: Option<&str>, degree: usize) -> bool {
        match self {
            NodePredicate::Any => true,
            NodePredicate::LabelIs(wanted) => label == Some(wanted.as_str()),
            NodePredicate::DegreeAtLeast(minimum) => degree >= *minimum,
            NodePredicate::InSet(members) => members.contains(&node),
        }
    }
}

/// One conditional node restyling.
#[derive(Clone, Debug)]
pub struct NodeRule {
    pub predicate: NodePredicate,
    pub patch: NodeStylePatch,
}

/// Ordered node rules evaluated against labels, degrees, and result sets.
#[derive(Clone, Debug, Default)]
pub struct StyleMapper {
    rules: Vec<NodeRule>,
}

impl StyleMapper {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Appends a rule; rules appended later take precedence on overlap.
    pub fn add_rule(&mut self, predicate: NodePredicate, patch: NodeStylePatch) {
        self.rules.push(NodeRule { predicate, patch });
    }

    /// Base style with every matching rule applied in order.
    pub fn resolve_node(
        &self,
        base: &NodeStyle,
        node: NodeIndex,
        label: Option<&str>,
        degree: usize,
    ) -> NodeStyle {
        let mut resolved = *base;
        for rule in &self.rules {
            if rule.predicate.matches(node, label, degree) {
                resolved = rule.patch.apply_to(&resolved);
            }
        }
        resolved
    }
}

/// Typed edge condition; later rules in the mapper win over earlier ones.
#[derive(Clone, Debug)]
pub enum EdgePredicate {
    /// Matches every edge.
    Any,
    /// Matches edges touching the given node in either direction.
    IncidentTo(NodeIndex),
    /// Matches edges contained in an algorithm result set.
    InSet(HashSet<(NodeIndex, NodeIndex)>),
}

impl EdgePredicate {
    fn matches(&self, source: NodeIndex, target: NodeIndex) -> bool {
        match self {
            EdgePredicate::Any => true,
            EdgePredicate::IncidentTo(node) => source == *node || target == *node,
            EdgePredicate::InSet(members) => members.contains(&(source, target)),
        }
    }
}

/// One conditional edge restyling.
#[derive(Clone, Debug)]
pub struct EdgeRule {
    pub predicate: EdgePredicate,
    pub patch: EdgeStylePatch,
}

/// Ordered edge rules evaluated against endpoints and result sets.
#[derive(Clone, Debug, Default)]
pub struct EdgeMapper {
    rules: Vec<EdgeRule>,
}

impl EdgeMapper {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Appends a rule; rules appended later take precedence on overlap.
    pub fn add_rule(&mut self, predicate: EdgePredicate, patch: EdgeStylePatch) {
        self.rules.push(EdgeRule { predicate, patch });
    }

    /// Base style with every matching rule applied in order.
    pub fn resolve_edge(
        &self,
        base: &EdgeStyle,
        source: NodeIndex,
        target: NodeIndex,
    ) -> EdgeStyle {
        let mut resolved = *base;
        for rule in &self.rules {
            if rule.predicate.matches(source, target) {
                resolved = rule.patch.apply_to(&resolved);
            }
        }
        resolved
    }
}

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
        let mapped = mapper.resolve_node(&sheet.node, node, label, degree);
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
        let mapped = mapper.resolve_edge(&sheet.edge, source, target);
        self.edge_bypass(source, target)
            .map(|patch| patch.apply_to(&mapped))
            .unwrap_or(mapped)
    }
}

/// Distinct fills for result groups such as strongly connected components.
///
/// Groups past the palette length share the overflow fill instead of cycling,
/// so the capped set stays visually distinct from the merged remainder.
pub const SCC_GROUP_FILLS: [u32; 8] = [
    0xff9f2e, 0x4a9eff, 0x3ecf6e, 0xb47bff, 0xff5d6c, 0x2ec4d6, 0xffd23e, 0x7a9e7e,
];

/// Fill of every group past the palette length.
pub const SCC_OVERFLOW_FILL: u32 = 0x8a93a6;

/// Fill for `group`, with overflow groups sharing one muted tone.
pub fn scc_fill(group: usize) -> u32 {
    SCC_GROUP_FILLS
        .get(group)
        .copied()
        .unwrap_or(SCC_OVERFLOW_FILL)
}

/// Smallest node scale produced by score mapping.
pub const MIN_RANK_SCALE: f32 = 0.75;

/// Largest node scale produced by score mapping.
pub const MAX_RANK_SCALE: f32 = 1.75;

/// Node scale for `score` linearly mapped from the observed score range.
///
/// A flat range maps every node to the midpoint scale. Non-positive or
/// non-finite scales fall back to the neutral scale of one.
pub fn scale_for_rank(score: f32, min: f32, max: f32) -> f32 {
    if !score.is_finite() || !min.is_finite() || !max.is_finite() || max <= min {
        return 1.0;
    }
    let ratio = ((score - min) / (max - min)).clamp(0.0, 1.0);
    let scale = MIN_RANK_SCALE + ratio * (MAX_RANK_SCALE - MIN_RANK_SCALE);
    if scale > 0.0 && scale.is_finite() {
        scale
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn later_mapping_rules_win_over_earlier_ones() {
        let sheet = StyleSheet::default();
        let mut mapper = StyleMapper::new();
        mapper.add_rule(
            NodePredicate::DegreeAtLeast(1),
            NodeStylePatch {
                fill: Some(0x111111),
                ..NodeStylePatch::default()
            },
        );
        mapper.add_rule(
            NodePredicate::LabelIs("hub".to_string()),
            NodeStylePatch {
                fill: Some(0x222222),
                ..NodeStylePatch::default()
            },
        );
        let bypass = BypassStore::new();
        let node = NodeIndex::new(4);
        let resolved = bypass.resolve_node(&sheet, &mapper, node, Some("hub"), 6);
        assert_eq!(resolved.fill, 0x222222);
        let unmatched = bypass.resolve_node(&sheet, &mapper, node, Some("leaf"), 0);
        assert_eq!(unmatched.fill, DEFAULT_NODE_FILL);
    }

    #[test]
    fn result_set_predicate_matches_members_only() {
        let sheet = StyleSheet::default();
        let mut mapper = StyleMapper::new();
        mapper.add_rule(
            NodePredicate::InSet(HashSet::from([NodeIndex::new(2)])),
            NodeStylePatch {
                opacity: Some(0.4),
                ..NodeStylePatch::default()
            },
        );
        let bypass = BypassStore::new();
        let member = bypass.resolve_node(&sheet, &mapper, NodeIndex::new(2), None, 0);
        assert_eq!(member.opacity, 0.4);
        let outsider = bypass.resolve_node(&sheet, &mapper, NodeIndex::new(3), None, 0);
        assert_eq!(outsider.opacity, 1.0);
    }

    #[test]
    fn bypass_overrides_and_clears_without_touching_the_sheet() {
        let mut sheet = StyleSheet::default();
        sheet.node.fill = 0x123456;
        let mapper = StyleMapper::new();
        let mut bypass = BypassStore::new();
        let node = NodeIndex::new(1);
        bypass.set_node(node, NodeStylePatch::selected());
        let highlighted = bypass.resolve_node(&sheet, &mapper, node, None, 0);
        assert_eq!(highlighted.fill, SELECTED_NODE_FILL);
        bypass.clear_node(node);
        let restored = bypass.resolve_node(&sheet, &mapper, node, None, 0);
        assert_eq!(restored.fill, 0x123456);
        assert_eq!(sheet.node.fill, 0x123456);
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

    #[test]
    fn later_edge_mapping_rules_win_over_earlier_ones() {
        let sheet = StyleSheet::default();
        let mut mapper = EdgeMapper::new();
        mapper.add_rule(EdgePredicate::Any, EdgeStylePatch::widened());
        mapper.add_rule(
            EdgePredicate::IncidentTo(NodeIndex::new(7)),
            EdgeStylePatch::highlighted(),
        );
        let bypass = BypassStore::new();
        let matched = bypass.resolve_edge(&sheet, &mapper, NodeIndex::new(7), NodeIndex::new(9));
        assert_eq!(matched.tint, HIGHLIGHT_EDGE_TINT);
        let unmatched = bypass.resolve_edge(&sheet, &mapper, NodeIndex::new(1), NodeIndex::new(2));
        assert_eq!(unmatched.tint, DEFAULT_EDGE_TINT);
        assert_eq!(unmatched.width, 3.5);
    }

    #[test]
    fn edge_result_set_predicate_matches_members_only() {
        let sheet = StyleSheet::default();
        let mut mapper = EdgeMapper::new();
        mapper.add_rule(
            EdgePredicate::InSet(HashSet::from([(NodeIndex::new(2), NodeIndex::new(3))])),
            EdgeStylePatch::highlighted(),
        );
        let bypass = BypassStore::new();
        let member = bypass.resolve_edge(&sheet, &mapper, NodeIndex::new(2), NodeIndex::new(3));
        assert_eq!(member.tint, HIGHLIGHT_EDGE_TINT);
        let reversed = bypass.resolve_edge(&sheet, &mapper, NodeIndex::new(3), NodeIndex::new(2));
        assert_eq!(reversed.tint, DEFAULT_EDGE_TINT);
    }

    #[test]
    fn rank_scales_span_the_configured_range() {
        assert_eq!(scale_for_rank(0.0, 0.0, 0.0), 1.0);
        assert_eq!(scale_for_rank(0.0, 0.0, 1.0), MIN_RANK_SCALE);
        assert_eq!(scale_for_rank(1.0, 0.0, 1.0), MAX_RANK_SCALE);
        assert_eq!(scale_for_rank(f32::NAN, 0.0, 1.0), 1.0);
        assert_eq!(scc_fill(0), SCC_GROUP_FILLS[0]);
        assert_eq!(scc_fill(SCC_GROUP_FILLS.len()), SCC_OVERFLOW_FILL);
        let patch = NodeStylePatch::rescaled(1.5);
        assert!(!patch.is_empty());
        let applied = patch.apply_to(&NodeStyle::default());
        assert_eq!(applied.scale, 1.5);
    }
}
