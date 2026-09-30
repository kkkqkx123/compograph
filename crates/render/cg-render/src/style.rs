//! Layered element styling with per-element bypass overrides.
//!
//! The main styles live in [`StyleSheet`] as plain defaults. [`StyleMapper`]
//! derives patches from typed predicates over labels, degrees, or algorithm
//! result sets, without any string selector language. [`BypassStore`] holds
//! transient per-element overrides for selection, hover, and algorithm
//! highlights, so highlights never mutate the main styles.

use std::collections::{BTreeSet, HashMap, HashSet};

use cg_graph::{DataValue, NodeIndex};

use crate::arrows::ArrowKind;
use crate::image::NodeImage;
use crate::shapes::NodeShape;

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

/// Linear fill of a node body.
///
/// The fill runs from `start` to `end` along `angle_deg`. A solid look is the
/// degenerate form with both ends equal. Angles follow the canvas gradient
/// convention: zero runs top to bottom and values grow clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeFill {
    pub start: u32,
    pub end: u32,
    pub angle_deg: f32,
}

impl NodeFill {
    /// Solid fill with one color for both ends.
    pub fn solid(color: u32) -> Self {
        Self {
            start: color,
            end: color,
            angle_deg: 0.0,
        }
    }

    /// Linear fill from `start` to `end` along `angle_deg`.
    pub fn gradient(start: u32, end: u32, angle_deg: f32) -> Self {
        Self {
            start,
            end,
            angle_deg: clamp_fill_angle(angle_deg),
        }
    }

    /// True when both ends share one color.
    pub fn is_solid(&self) -> bool {
        self.start == self.end
    }

    /// Gradient angle clamped to the valid range.
    pub fn angle(&self) -> f32 {
        clamp_fill_angle(self.angle_deg)
    }

    /// Solid color used when detail levels cannot afford gradients.
    pub fn solid_fallback(&self) -> u32 {
        self.start
    }

    /// Interpolated color at `t` in the closed unit interval.
    pub fn sample(&self, t: f32) -> u32 {
        let ratio = t.clamp(0.0, 1.0);
        lerp_rgb(self.start, self.end, ratio)
    }

    /// Interpolated color halfway between both ends.
    pub fn midpoint(&self) -> u32 {
        self.sample(0.5)
    }
}

impl Default for NodeFill {
    fn default() -> Self {
        Self::solid(DEFAULT_NODE_FILL)
    }
}

/// Clamps a gradient angle to degrees in the closed range.
fn clamp_fill_angle(angle: f32) -> f32 {
    if !angle.is_finite() {
        return 0.0;
    }
    angle.clamp(0.0, 360.0)
}

/// Linear interpolation of two packed colors per channel.
fn lerp_rgb(start: u32, end: u32, t: f32) -> u32 {
    let sr = ((start >> 16) & 0xFF) as f32;
    let sg = ((start >> 8) & 0xFF) as f32;
    let sb = (start & 0xFF) as f32;
    let er = ((end >> 16) & 0xFF) as f32;
    let eg = ((end >> 8) & 0xFF) as f32;
    let eb = (end & 0xFF) as f32;
    let r = (sr + (er - sr) * t).round().clamp(0.0, 255.0) as u32;
    let g = (sg + (eg - sg) * t).round().clamp(0.0, 255.0) as u32;
    let b = (sb + (eb - sb) * t).round().clamp(0.0, 255.0) as u32;
    (r << 16) | (g << 8) | b
}

/// Resolved appearance of one node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeStyle {
    pub fill: NodeFill,
    pub stroke: u32,
    pub stroke_width: f32,
    pub opacity: f32,
    pub label_size: f32,
    pub scale: f32,
    pub shape: NodeShape,
    pub image: Option<NodeImage>,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            fill: NodeFill::default(),
            stroke: DEFAULT_NODE_STROKE,
            stroke_width: 1.5,
            opacity: 1.0,
            label_size: 12.0,
            scale: 1.0,
            shape: NodeShape::Square,
            image: None,
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
    pub arrow: ArrowKind,
}

impl Default for EdgeStyle {
    fn default() -> Self {
        Self {
            tint: DEFAULT_EDGE_TINT,
            width: 1.5,
            opacity: 1.0,
            arrow_scale: 1.0,
            label_size: 11.0,
            arrow: ArrowKind::Triangle,
        }
    }
}

/// Partial node appearance; set fields override the base on resolution.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeStylePatch {
    pub fill: Option<NodeFill>,
    pub stroke: Option<u32>,
    pub stroke_width: Option<f32>,
    pub opacity: Option<f32>,
    pub label_size: Option<f32>,
    pub scale: Option<f32>,
    pub shape: Option<NodeShape>,
    pub image: Option<Option<NodeImage>>,
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
            && self.shape.is_none()
            && self.image.is_none()
    }

    /// Patch selecting a node through the bypass channel.
    pub fn selected() -> Self {
        Self {
            fill: Some(NodeFill::solid(SELECTED_NODE_FILL)),
            ..Self::default()
        }
    }

    /// Patch marking the hovered node through the bypass channel.
    pub fn hovered() -> Self {
        Self {
            fill: Some(NodeFill::solid(HOVER_NODE_FILL)),
            ..Self::default()
        }
    }

    /// Patch tinting a node, used for result group coloring.
    pub fn tinted(fill: u32) -> Self {
        Self {
            fill: Some(NodeFill::solid(fill)),
            ..Self::default()
        }
    }

    /// Patch filling a node with a linear gradient.
    pub fn gradient_fill(fill: NodeFill) -> Self {
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

    /// Patch reshaping a node.
    pub fn reshaped(shape: NodeShape) -> Self {
        Self {
            shape: Some(shape),
            ..Self::default()
        }
    }

    /// Patch showing a local background image on a node.
    pub fn with_image(image: NodeImage) -> Self {
        Self {
            image: Some(Some(image)),
            ..Self::default()
        }
    }

    /// Patch clearing any background image from a node.
    pub fn without_image() -> Self {
        Self {
            image: Some(None),
            ..Self::default()
        }
    }

    pub fn apply_to(&self, base: &NodeStyle) -> NodeStyle {
        NodeStyle {
            fill: self.fill.unwrap_or(base.fill),
            stroke: self.stroke.unwrap_or(base.stroke),
            stroke_width: self.stroke_width.unwrap_or(base.stroke_width),
            opacity: self.opacity.unwrap_or(base.opacity),
            label_size: self.label_size.unwrap_or(base.label_size),
            scale: self.scale.unwrap_or(base.scale),
            shape: self.shape.unwrap_or(base.shape),
            image: self.image.clone().unwrap_or_else(|| base.image.clone()),
        }
    }

    /// Applies the patch, exposed for selector sheets in the same crate family.
    pub fn apply_to_style(&self, base: &NodeStyle) -> NodeStyle {
        self.apply_to(base)
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
    pub arrow: Option<ArrowKind>,
}

impl EdgeStylePatch {
    /// True when the patch carries no override.
    pub fn is_empty(&self) -> bool {
        self.tint.is_none()
            && self.width.is_none()
            && self.opacity.is_none()
            && self.arrow_scale.is_none()
            && self.label_size.is_none()
            && self.arrow.is_none()
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

    /// Patch reheading an edge.
    pub fn reheaded(arrow: ArrowKind) -> Self {
        Self {
            arrow: Some(arrow),
            ..Self::default()
        }
    }

    pub fn apply_to(&self, base: &EdgeStyle) -> EdgeStyle {
        EdgeStyle {
            tint: self.tint.unwrap_or(base.tint),
            width: self.width.unwrap_or(base.width),
            opacity: self.opacity.unwrap_or(base.opacity),
            arrow_scale: self.arrow_scale.unwrap_or(base.arrow_scale),
            label_size: self.label_size.unwrap_or(base.label_size),
            arrow: self.arrow.unwrap_or(base.arrow),
        }
    }

    /// Applies the patch, exposed for selector sheets in the same crate family.
    pub fn apply_to_edge(&self, base: &EdgeStyle) -> EdgeStyle {
        self.apply_to(base)
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
    /// Matches nodes whose attribute equals the given value.
    AttrEquals { key: String, value: DataValue },
    /// Matches nodes whose numeric attribute falls in the closed interval.
    AttrInRange { key: String, min: f64, max: f64 },
    /// Matches nodes carrying the given class.
    HasClass(String),
    /// Matches nodes carrying every listed class.
    HasClasses(Vec<String>),
}

impl NodePredicate {
    fn matches(
        &self,
        node: NodeIndex,
        label: Option<&str>,
        degree: usize,
        attrs: &HashMap<String, DataValue>,
        classes: &BTreeSet<String>,
    ) -> bool {
        match self {
            NodePredicate::Any => true,
            NodePredicate::LabelIs(wanted) => label == Some(wanted.as_str()),
            NodePredicate::DegreeAtLeast(minimum) => degree >= *minimum,
            NodePredicate::InSet(members) => members.contains(&node),
            NodePredicate::AttrEquals { key, value } => attrs.get(key) == Some(value),
            NodePredicate::AttrInRange { key, min, max } => match attrs.get(key) {
                Some(DataValue::Number(number)) if number.is_finite() => {
                    number >= min && number <= max
                }
                _ => false,
            },
            NodePredicate::HasClass(wanted) => classes.contains(wanted),
            NodePredicate::HasClasses(wanted) => wanted.iter().all(|name| classes.contains(name)),
        }
    }
}

/// One conditional node restyling.
#[derive(Clone, Debug)]
pub struct NodeRule {
    pub predicate: NodePredicate,
    pub patch: NodeStylePatch,
}

/// Numeric attribute mapped linearly onto continuous style fields.
#[derive(Clone, Debug)]
pub struct NodeNumberMap {
    /// Attribute key holding the numeric value.
    pub key: String,
    /// Source interval; out-of-range values clamp to the ends.
    pub src_min: f64,
    /// Source interval; out-of-range values clamp to the ends.
    pub src_max: f64,
    /// Destination interval for the size multiplier.
    pub scale: Option<(f32, f32)>,
    /// Destination interval for the outline width.
    pub stroke_width: Option<(f32, f32)>,
    /// Destination interval for opacity.
    pub opacity: Option<(f32, f32)>,
    /// Destination interval for the label size.
    pub label_size: Option<(f32, f32)>,
}

impl NodeNumberMap {
    fn patch_for(&self, attrs: &HashMap<String, DataValue>) -> Option<NodeStylePatch> {
        let value = match attrs.get(&self.key) {
            Some(DataValue::Number(number)) if number.is_finite() => *number,
            _ => return None,
        };
        let mut patch = NodeStylePatch::default();
        if let Some((lo, hi)) = self.scale {
            patch.scale = Some(linear_map(value, self.src_min, self.src_max, lo, hi));
        }
        if let Some((lo, hi)) = self.stroke_width {
            patch.stroke_width = Some(linear_map(value, self.src_min, self.src_max, lo, hi));
        }
        if let Some((lo, hi)) = self.opacity {
            patch.opacity = Some(linear_map(value, self.src_min, self.src_max, lo, hi));
        }
        if let Some((lo, hi)) = self.label_size {
            patch.label_size = Some(linear_map(value, self.src_min, self.src_max, lo, hi));
        }
        if patch.is_empty() { None } else { Some(patch) }
    }
}

#[derive(Clone, Debug)]
enum NodeStyleRule {
    Predicate(NodeRule),
    NumberMap(NodeNumberMap),
}

/// Ordered node rules evaluated against labels, degrees, and result sets.
#[derive(Clone, Debug, Default)]
pub struct StyleMapper {
    rules: Vec<NodeStyleRule>,
}

impl StyleMapper {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Appends a rule; rules appended later take precedence on overlap.
    pub fn add_rule(&mut self, predicate: NodePredicate, patch: NodeStylePatch) {
        self.rules
            .push(NodeStyleRule::Predicate(NodeRule { predicate, patch }));
    }

    /// Appends a numeric data mapping; later entries win over earlier ones.
    pub fn add_number_map(&mut self, map: NodeNumberMap) {
        self.rules.push(NodeStyleRule::NumberMap(map));
    }

    /// Base style with every matching rule applied in order.
    pub fn resolve_node(
        &self,
        base: &NodeStyle,
        node: NodeIndex,
        label: Option<&str>,
        degree: usize,
    ) -> NodeStyle {
        self.resolve_node_with_data(base, node, label, degree, &HashMap::new(), &BTreeSet::new())
    }

    /// Full resolution carrying attribute and class tables.
    pub fn resolve_node_with_data(
        &self,
        base: &NodeStyle,
        node: NodeIndex,
        label: Option<&str>,
        degree: usize,
        attrs: &HashMap<String, DataValue>,
        classes: &BTreeSet<String>,
    ) -> NodeStyle {
        let mut resolved = base.clone();
        for rule in &self.rules {
            match rule {
                NodeStyleRule::Predicate(entry) => {
                    if entry.predicate.matches(node, label, degree, attrs, classes) {
                        resolved = entry.patch.apply_to(&resolved);
                    }
                }
                NodeStyleRule::NumberMap(map) => {
                    if let Some(patch) = map.patch_for(attrs) {
                        resolved = patch.apply_to(&resolved);
                    }
                }
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
    /// Matches edges whose attribute equals the given value.
    AttrEquals { key: String, value: DataValue },
    /// Matches edges whose numeric attribute falls in the closed interval.
    AttrInRange { key: String, min: f64, max: f64 },
    /// Matches edges carrying the given class.
    HasClass(String),
    /// Matches edges carrying every listed class.
    HasClasses(Vec<String>),
}

impl EdgePredicate {
    fn matches(
        &self,
        source: NodeIndex,
        target: NodeIndex,
        attrs: &HashMap<String, DataValue>,
        classes: &BTreeSet<String>,
    ) -> bool {
        match self {
            EdgePredicate::Any => true,
            EdgePredicate::IncidentTo(node) => source == *node || target == *node,
            EdgePredicate::InSet(members) => members.contains(&(source, target)),
            EdgePredicate::AttrEquals { key, value } => attrs.get(key) == Some(value),
            EdgePredicate::AttrInRange { key, min, max } => match attrs.get(key) {
                Some(DataValue::Number(number)) if number.is_finite() => {
                    number >= min && number <= max
                }
                _ => false,
            },
            EdgePredicate::HasClass(wanted) => classes.contains(wanted),
            EdgePredicate::HasClasses(wanted) => wanted.iter().all(|name| classes.contains(name)),
        }
    }
}

/// One conditional edge restyling.
#[derive(Clone, Debug)]
pub struct EdgeRule {
    pub predicate: EdgePredicate,
    pub patch: EdgeStylePatch,
}

/// Numeric attribute mapped linearly onto continuous edge fields.
#[derive(Clone, Debug)]
pub struct EdgeNumberMap {
    /// Attribute key holding the numeric value.
    pub key: String,
    /// Source interval; out-of-range values clamp to the ends.
    pub src_min: f64,
    /// Source interval; out-of-range values clamp to the ends.
    pub src_max: f64,
    /// Destination interval for the line width.
    pub width: Option<(f32, f32)>,
    /// Destination interval for opacity.
    pub opacity: Option<(f32, f32)>,
    /// Destination interval for the arrow scale.
    pub arrow_scale: Option<(f32, f32)>,
    /// Destination interval for the label size.
    pub label_size: Option<(f32, f32)>,
}

impl EdgeNumberMap {
    fn patch_for(&self, attrs: &HashMap<String, DataValue>) -> Option<EdgeStylePatch> {
        let value = match attrs.get(&self.key) {
            Some(DataValue::Number(number)) if number.is_finite() => *number,
            _ => return None,
        };
        let mut patch = EdgeStylePatch::default();
        if let Some((lo, hi)) = self.width {
            patch.width = Some(linear_map(value, self.src_min, self.src_max, lo, hi));
        }
        if let Some((lo, hi)) = self.opacity {
            patch.opacity = Some(linear_map(value, self.src_min, self.src_max, lo, hi));
        }
        if let Some((lo, hi)) = self.arrow_scale {
            patch.arrow_scale = Some(linear_map(value, self.src_min, self.src_max, lo, hi));
        }
        if let Some((lo, hi)) = self.label_size {
            patch.label_size = Some(linear_map(value, self.src_min, self.src_max, lo, hi));
        }
        if patch.is_empty() { None } else { Some(patch) }
    }
}

#[derive(Clone, Debug)]
enum EdgeStyleRule {
    Predicate(EdgeRule),
    NumberMap(EdgeNumberMap),
}

/// Ordered edge rules evaluated against endpoints and result sets.
#[derive(Clone, Debug, Default)]
pub struct EdgeMapper {
    rules: Vec<EdgeStyleRule>,
}

impl EdgeMapper {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Appends a rule; rules appended later take precedence on overlap.
    pub fn add_rule(&mut self, predicate: EdgePredicate, patch: EdgeStylePatch) {
        self.rules
            .push(EdgeStyleRule::Predicate(EdgeRule { predicate, patch }));
    }

    /// Appends a numeric data mapping; later entries win over earlier ones.
    pub fn add_number_map(&mut self, map: EdgeNumberMap) {
        self.rules.push(EdgeStyleRule::NumberMap(map));
    }

    /// Base style with every matching rule applied in order.
    pub fn resolve_edge(
        &self,
        base: &EdgeStyle,
        source: NodeIndex,
        target: NodeIndex,
    ) -> EdgeStyle {
        self.resolve_edge_with_data(base, source, target, &HashMap::new(), &BTreeSet::new())
    }

    /// Full resolution carrying attribute and class tables.
    pub fn resolve_edge_with_data(
        &self,
        base: &EdgeStyle,
        source: NodeIndex,
        target: NodeIndex,
        attrs: &HashMap<String, DataValue>,
        classes: &BTreeSet<String>,
    ) -> EdgeStyle {
        let mut resolved = *base;
        for rule in &self.rules {
            match rule {
                EdgeStyleRule::Predicate(entry) => {
                    if entry.predicate.matches(source, target, attrs, classes) {
                        resolved = entry.patch.apply_to(&resolved);
                    }
                }
                EdgeStyleRule::NumberMap(map) => {
                    if let Some(patch) = map.patch_for(attrs) {
                        resolved = patch.apply_to(&resolved);
                    }
                }
            }
        }
        resolved
    }
}

/// Linear mapping of `value` from the source interval onto the destination.
///
/// Out-of-range values clamp to the ends. Degenerate or non-finite inputs
/// fall back to the lower destination bound.
pub fn linear_map(value: f64, src_min: f64, src_max: f64, dst_min: f32, dst_max: f32) -> f32 {
    if !value.is_finite() || !src_min.is_finite() || !src_max.is_finite() || src_max <= src_min {
        return dst_min;
    }
    let ratio = ((value - src_min) / (src_max - src_min)).clamp(0.0, 1.0);
    (dst_min + ratio as f32 * (dst_max - dst_min)).clamp(dst_min.min(dst_max), dst_min.max(dst_max))
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
        self.resolve_node_with_data(
            sheet,
            mapper,
            node,
            label,
            degree,
            &HashMap::new(),
            &BTreeSet::new(),
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
        attrs: &HashMap<String, DataValue>,
        classes: &BTreeSet<String>,
    ) -> NodeStyle {
        let mapped =
            mapper.resolve_node_with_data(&sheet.node, node, label, degree, attrs, classes);
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
                fill: Some(NodeFill::solid(0x111111)),
                ..NodeStylePatch::default()
            },
        );
        mapper.add_rule(
            NodePredicate::LabelIs("hub".to_string()),
            NodeStylePatch {
                fill: Some(NodeFill::solid(0x222222)),
                ..NodeStylePatch::default()
            },
        );
        let bypass = BypassStore::new();
        let node = NodeIndex::new(4);
        let resolved = bypass.resolve_node(&sheet, &mapper, node, Some("hub"), 6);
        assert_eq!(resolved.fill, NodeFill::solid(0x222222));
        let unmatched = bypass.resolve_node(&sheet, &mapper, node, Some("leaf"), 0);
        assert_eq!(unmatched.fill, NodeFill::solid(DEFAULT_NODE_FILL));
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
        sheet.node.fill = NodeFill::solid(0x123456);
        let mapper = StyleMapper::new();
        let mut bypass = BypassStore::new();
        let node = NodeIndex::new(1);
        bypass.set_node(node, NodeStylePatch::selected());
        let highlighted = bypass.resolve_node(&sheet, &mapper, node, None, 0);
        assert_eq!(highlighted.fill, NodeFill::solid(SELECTED_NODE_FILL));
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

    #[test]
    fn shape_and_arrow_defaults_keep_legacy_look() {
        use crate::arrows::ArrowKind;
        use crate::shapes::NodeShape;

        assert_eq!(NodeStyle::default().shape, NodeShape::Square);
        assert_eq!(EdgeStyle::default().arrow, ArrowKind::Triangle);
        let reshaped = NodeStylePatch::reshaped(NodeShape::Circle).apply_to(&NodeStyle::default());
        assert_eq!(reshaped.shape, NodeShape::Circle);
        let reheaded = EdgeStylePatch::reheaded(ArrowKind::Diamond).apply_to(&EdgeStyle::default());
        assert_eq!(reheaded.arrow, ArrowKind::Diamond);
    }

    #[test]
    fn attr_equality_matches_typed_values_only() {
        let sheet = StyleSheet::default();
        let mut mapper = StyleMapper::new();
        mapper.add_rule(
            NodePredicate::AttrEquals {
                key: "kind".into(),
                value: DataValue::Text("hub".into()),
            },
            NodeStylePatch {
                fill: Some(NodeFill::solid(0x222222)),
                ..NodeStylePatch::default()
            },
        );
        let bypass = BypassStore::new();
        let node = NodeIndex::new(1);
        let mut hit = HashMap::new();
        hit.insert("kind".to_string(), DataValue::Text("hub".into()));
        let resolved =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None, 0, &hit, &BTreeSet::new());
        assert_eq!(resolved.fill, NodeFill::solid(0x222222));
        let missing = bypass.resolve_node_with_data(
            &sheet,
            &mapper,
            node,
            None,
            0,
            &HashMap::new(),
            &BTreeSet::new(),
        );
        assert_eq!(missing.fill, NodeFill::solid(DEFAULT_NODE_FILL));
        let mut wrong = HashMap::new();
        wrong.insert("kind".to_string(), DataValue::Number(1.0));
        let mismatched =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None, 0, &wrong, &BTreeSet::new());
        assert_eq!(mismatched.fill, NodeFill::solid(DEFAULT_NODE_FILL));
    }

    #[test]
    fn number_map_clamps_and_skips_missing() {
        let sheet = StyleSheet::default();
        let mut mapper = StyleMapper::new();
        mapper.add_number_map(NodeNumberMap {
            key: "score".into(),
            src_min: 0.0,
            src_max: 100.0,
            scale: Some((0.5, 2.0)),
            stroke_width: None,
            opacity: Some((0.2, 1.0)),
            label_size: None,
        });
        let bypass = BypassStore::new();
        let node = NodeIndex::new(0);
        let mut low = HashMap::new();
        low.insert("score".to_string(), DataValue::Number(0.0));
        let bottom =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None, 0, &low, &BTreeSet::new());
        assert_eq!(bottom.scale, 0.5);
        assert_eq!(bottom.opacity, 0.2);
        let mut high = HashMap::new();
        high.insert("score".to_string(), DataValue::Number(200.0));
        let top =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None, 0, &high, &BTreeSet::new());
        assert_eq!(top.scale, 2.0);
        assert_eq!(top.opacity, 1.0);
        let absent = bypass.resolve_node_with_data(
            &sheet,
            &mapper,
            node,
            None,
            0,
            &HashMap::new(),
            &BTreeSet::new(),
        );
        assert_eq!(absent.scale, 1.0);
    }

    #[test]
    fn class_predicates_require_membership() {
        let sheet = StyleSheet::default();
        let mut mapper = StyleMapper::new();
        mapper.add_rule(
            NodePredicate::HasClasses(vec!["a".into(), "b".into()]),
            NodeStylePatch {
                fill: Some(NodeFill::solid(0x333333)),
                ..NodeStylePatch::default()
            },
        );
        let bypass = BypassStore::new();
        let node = NodeIndex::new(0);
        let both: BTreeSet<String> = ["a".into(), "b".into(), "c".into()].into_iter().collect();
        let hit =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None, 0, &HashMap::new(), &both);
        assert_eq!(hit.fill, NodeFill::solid(0x333333));
        let single: BTreeSet<String> = ["a".into()].into_iter().collect();
        let miss =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None, 0, &HashMap::new(), &single);
        assert_eq!(miss.fill, NodeFill::solid(DEFAULT_NODE_FILL));
    }

    #[test]
    fn linear_map_covers_edges_and_degenerate_ranges() {
        assert_eq!(linear_map(0.0, 0.0, 100.0, 1.0, 5.0), 1.0);
        assert_eq!(linear_map(100.0, 0.0, 100.0, 1.0, 5.0), 5.0);
        assert_eq!(linear_map(50.0, 0.0, 100.0, 1.0, 5.0), 3.0);
        assert_eq!(linear_map(-10.0, 0.0, 100.0, 1.0, 5.0), 1.0);
        assert_eq!(linear_map(200.0, 0.0, 100.0, 1.0, 5.0), 5.0);
        assert_eq!(linear_map(50.0, 1.0, 1.0, 1.0, 5.0), 1.0);
        assert_eq!(linear_map(f64::NAN, 0.0, 100.0, 1.0, 5.0), 1.0);
    }

    #[test]
    fn edge_attr_rules_match_with_tables() {
        let sheet = StyleSheet::default();
        let mut mapper = EdgeMapper::new();
        mapper.add_rule(
            EdgePredicate::AttrEquals {
                key: "kind".into(),
                value: DataValue::Flag(true),
            },
            EdgeStylePatch::highlighted(),
        );
        let bypass = BypassStore::new();
        let a = NodeIndex::new(0);
        let b = NodeIndex::new(1);
        let mut hit = HashMap::new();
        hit.insert("kind".to_string(), DataValue::Flag(true));
        let matched = bypass.resolve_edge_with_data(&sheet, &mapper, a, b, &hit, &BTreeSet::new());
        assert_eq!(matched.tint, HIGHLIGHT_EDGE_TINT);
        let missed =
            bypass.resolve_edge_with_data(&sheet, &mapper, a, b, &HashMap::new(), &BTreeSet::new());
        assert_eq!(missed.tint, DEFAULT_EDGE_TINT);
    }

    #[test]
    fn solid_fill_stays_the_default_appearance() {
        assert_eq!(
            NodeStyle::default().fill,
            NodeFill::solid(DEFAULT_NODE_FILL)
        );
        assert!(NodeStyle::default().fill.is_solid());
        assert_eq!(NodeFill::solid(0x112233).solid_fallback(), 0x112233);
    }

    #[test]
    fn gradient_samples_endpoints_midpoint_and_clamps_angle() {
        let fill = NodeFill::gradient(0x000000, 0xFFFFFF, 90.0);
        assert!(!fill.is_solid());
        assert_eq!(fill.sample(0.0), 0x000000);
        assert_eq!(fill.sample(1.0), 0xFFFFFF);
        assert_eq!(fill.midpoint(), 0x808080);
        assert_eq!(fill.sample(-1.0), 0x000000);
        assert_eq!(fill.sample(2.0), 0xFFFFFF);
        assert_eq!(
            NodeFill::gradient(0x111111, 0x222222, f32::NAN).angle(),
            0.0
        );
        assert_eq!(NodeFill::gradient(0x111111, 0x222222, 500.0).angle(), 360.0);
        assert_eq!(NodeFill::gradient(0x111111, 0x222222, -20.0).angle(), 0.0);
    }

    #[test]
    fn gradient_patch_replaces_the_whole_fill() {
        let base = NodeStyle::default();
        let patch = NodeStylePatch::gradient_fill(NodeFill::gradient(0x111111, 0x222222, 90.0));
        assert!(!patch.is_empty());
        let resolved = patch.apply_to(&base);
        assert_eq!(resolved.fill.start, 0x111111);
        assert_eq!(resolved.fill.end, 0x222222);
    }

    #[test]
    fn image_defaults_to_none_and_patch_sets_and_clears() {
        use crate::image::{ImageFit, NodeImage};

        assert!(NodeStyle::default().image.is_none());
        assert!(NodeStylePatch::default().is_empty());
        let base = NodeStyle::default();
        let spec = NodeImage::new("/tmp/a.png", ImageFit::Cover);
        let set = NodeStylePatch::with_image(spec.clone()).apply_to(&base);
        assert_eq!(set.image, Some(spec));
        assert!(
            !NodeStylePatch::with_image(NodeImage::new("/tmp/a.png", ImageFit::Contain)).is_empty()
        );
        let mut with_image = base.clone();
        with_image.image = set.image;
        let cleared = NodeStylePatch::without_image().apply_to(&with_image);
        assert!(cleared.image.is_none());
        let untouched = NodeStylePatch::default().apply_to(&base);
        assert!(untouched.image.is_none());
    }
}
