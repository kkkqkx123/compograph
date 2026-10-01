//! Typed edge styling rules evaluated in order.

use std::collections::{BTreeSet, HashMap, HashSet};

use cg_graph::{DataValue, NodeIndex};

use crate::appearance::{EdgeStyle, EdgeStylePatch};
use crate::mapping::{EdgeColorMap, linear_map};

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
    ColorMap(EdgeColorMap),
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

    /// Appends a color data mapping; later entries win over earlier ones.
    pub fn add_color_map(&mut self, map: EdgeColorMap) {
        self.rules.push(EdgeStyleRule::ColorMap(map));
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
                EdgeStyleRule::ColorMap(map) => {
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
    use crate::appearance::{DEFAULT_EDGE_TINT, HIGHLIGHT_EDGE_TINT, StyleSheet};
    use crate::bypass::BypassStore;
    use cg_graph::NodeIndex;

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
}
