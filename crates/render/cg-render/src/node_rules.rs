//! Typed node styling rules evaluated in order.

use std::collections::{BTreeSet, HashMap, HashSet};

use cg_graph::{DataValue, NodeIndex};

use crate::appearance::{NodeStyle, NodeStylePatch};
use crate::mapping::{NodeColorMap, linear_map};

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
    ColorMap(NodeColorMap),
}

/// Attribute and class tables feeding data-driven node resolution.
///
/// The two tables travel together because rule matching reads both for one
/// node; grouping them keeps resolution signatures small as new selector
/// dimensions arrive.
#[derive(Clone, Copy, Debug)]
pub struct NodeDataTables<'a> {
    pub attrs: &'a HashMap<String, DataValue>,
    pub classes: &'a BTreeSet<String>,
}

/// Ordered node rules evaluated against labels, degrees, and result sets.
///
/// This mapper is the production rule channel behind the bypass store; it
/// covers degree, result-set and numeric-range predicates that the capped
/// string selector subset cannot spell. String-query rules belong to the
/// selector sheet channel instead of being folded in here.
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

    /// Appends a color data mapping; later entries win over earlier ones.
    pub fn add_color_map(&mut self, map: NodeColorMap) {
        self.rules.push(NodeStyleRule::ColorMap(map));
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
                NodeStyleRule::ColorMap(map) => {
                    if let Some(patch) = map.patch_for(attrs) {
                        resolved = patch.apply_to(&resolved);
                    }
                }
            }
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::StyleSheet;
    use crate::bypass::BypassStore;
    use crate::fill::{DEFAULT_NODE_FILL, NodeFill};

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
            bypass.resolve_node_with_data(&sheet, &mapper, node, None,
                0,
                NodeDataTables { attrs: &hit, classes: &BTreeSet::new() },
            );
        assert_eq!(resolved.fill, NodeFill::solid(0x222222));
        let missing = bypass.resolve_node_with_data(
            &sheet,
            &mapper,
            node,
            None,
            0,
            NodeDataTables {
                attrs: &HashMap::new(),
                classes: &BTreeSet::new(),
            },
        );
        assert_eq!(missing.fill, NodeFill::solid(DEFAULT_NODE_FILL));
        let mut wrong = HashMap::new();
        wrong.insert("kind".to_string(), DataValue::Number(1.0));
        let mismatched =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None,
                0,
                NodeDataTables { attrs: &wrong, classes: &BTreeSet::new() },
            );
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
            bypass.resolve_node_with_data(&sheet, &mapper, node, None,
                0,
                NodeDataTables { attrs: &low, classes: &BTreeSet::new() },
            );
        assert_eq!(bottom.scale, 0.5);
        assert_eq!(bottom.opacity, 0.2);
        let mut high = HashMap::new();
        high.insert("score".to_string(), DataValue::Number(200.0));
        let top =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None,
                0,
                NodeDataTables { attrs: &high, classes: &BTreeSet::new() },
            );
        assert_eq!(top.scale, 2.0);
        assert_eq!(top.opacity, 1.0);
        let absent = bypass.resolve_node_with_data(
            &sheet,
            &mapper,
            node,
            None,
            0,
            NodeDataTables {
                attrs: &HashMap::new(),
                classes: &BTreeSet::new(),
            },
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
            bypass.resolve_node_with_data(&sheet, &mapper, node, None,
                0,
                NodeDataTables { attrs: &HashMap::new(), classes: &both },
            );
        assert_eq!(hit.fill, NodeFill::solid(0x333333));
        let single: BTreeSet<String> = ["a".into()].into_iter().collect();
        let miss =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None,
                0,
                NodeDataTables { attrs: &HashMap::new(), classes: &single },
            );
        assert_eq!(miss.fill, NodeFill::solid(DEFAULT_NODE_FILL));
    }
}
