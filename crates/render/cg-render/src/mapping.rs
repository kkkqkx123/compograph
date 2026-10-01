//! Numeric data mapping onto continuous visual channels.

use std::collections::HashMap;

use cg_graph::DataValue;

use crate::appearance::{EdgeStylePatch, NodeStylePatch};
use crate::fill::{NodeFill, lerp_rgb};

/// Numeric attribute mapped linearly onto the node fill color.
#[derive(Clone, Debug)]
pub struct NodeColorMap {
    /// Attribute key holding the numeric value.
    pub key: String,
    /// Source interval; out-of-range values clamp to the ends.
    pub src_min: f64,
    /// Source interval; out-of-range values clamp to the ends.
    pub src_max: f64,
    /// Fill at the lower source bound.
    pub start: u32,
    /// Fill at the upper source bound.
    pub end: u32,
}

impl NodeColorMap {
    pub(crate) fn patch_for(&self, attrs: &HashMap<String, DataValue>) -> Option<NodeStylePatch> {
        let value = match attrs.get(&self.key) {
            Some(DataValue::Number(number)) if number.is_finite() => *number,
            _ => return None,
        };
        Some(NodeStylePatch {
            fill: Some(NodeFill::solid(color_map(
                value,
                self.src_min,
                self.src_max,
                self.start,
                self.end,
            ))),
            ..NodeStylePatch::default()
        })
    }
}

/// Numeric attribute mapped linearly onto the edge line color.
#[derive(Clone, Debug)]
pub struct EdgeColorMap {
    /// Attribute key holding the numeric value.
    pub key: String,
    /// Source interval; out-of-range values clamp to the ends.
    pub src_min: f64,
    /// Source interval; out-of-range values clamp to the ends.
    pub src_max: f64,
    /// Tint at the lower source bound.
    pub start: u32,
    /// Tint at the upper source bound.
    pub end: u32,
}

impl EdgeColorMap {
    pub(crate) fn patch_for(&self, attrs: &HashMap<String, DataValue>) -> Option<EdgeStylePatch> {
        let value = match attrs.get(&self.key) {
            Some(DataValue::Number(number)) if number.is_finite() => *number,
            _ => return None,
        };
        Some(EdgeStylePatch {
            tint: Some(color_map(
                value,
                self.src_min,
                self.src_max,
                self.start,
                self.end,
            )),
            ..EdgeStylePatch::default()
        })
    }
}

/// Linear mapping of `value` from the source interval onto a color gradient.
///
/// Out-of-range values clamp to the ends. Degenerate or non-finite inputs
/// fall back to the start color.
pub fn color_map(value: f64, src_min: f64, src_max: f64, start: u32, end: u32) -> u32 {
    if !value.is_finite() || !src_min.is_finite() || !src_max.is_finite() || src_max <= src_min {
        return start;
    }
    let ratio = ((value - src_min) / (src_max - src_min)).clamp(0.0, 1.0);
    lerp_rgb(start, end, ratio as f32)
}

/// Linear mapping of `value` from the source interval onto the destination.
///
/// Out-of-range values clamp to the ends. Degenerate or non-finite inputs
/// fall back to the lower destination bound.
pub fn linear_map(value: f64, src_min: f64, src_max: f64, dst_min: f32, dst_max: f32) -> f32 {    if !value.is_finite() || !src_min.is_finite() || !src_max.is_finite() || src_max <= src_min {
        return dst_min;
    }
    let ratio = ((value - src_min) / (src_max - src_min)).clamp(0.0, 1.0);
    (dst_min + ratio as f32 * (dst_max - dst_min)).clamp(dst_min.min(dst_max), dst_min.max(dst_max))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::{EdgeStyle, NodeStyle};
    use crate::bypass::BypassStore;
    use crate::edge_rules::EdgeMapper;
    use crate::node_rules::{NodeDataTables, StyleMapper};
    use cg_graph::NodeIndex;
    use std::collections::BTreeSet;

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
    fn color_maps_cover_empty_clamped_and_degenerate_inputs() {
        use crate::appearance::StyleSheet;
        use crate::text::{LabelAlign, LabelBackground};

        let sheet = StyleSheet::default();
        let mut mapper = StyleMapper::new();
        mapper.add_color_map(NodeColorMap {
            key: "score".into(),
            src_min: 0.0,
            src_max: 100.0,
            start: 0x000000,
            end: 0xFFFFFF,
        });
        let bypass = BypassStore::new();
        let node = NodeIndex::new(0);
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
        assert_eq!(absent.fill, NodeFill::solid(crate::fill::DEFAULT_NODE_FILL));
        let mut low = HashMap::new();
        low.insert("score".to_string(), DataValue::Number(0.0));
        let bottom =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None,
                0,
                NodeDataTables { attrs: &low, classes: &BTreeSet::new() },
            );
        assert_eq!(bottom.fill, NodeFill::solid(0x000000));
        let mut high = HashMap::new();
        high.insert("score".to_string(), DataValue::Number(400.0));
        let top =
            bypass.resolve_node_with_data(&sheet, &mapper, node, None,
                0,
                NodeDataTables { attrs: &high, classes: &BTreeSet::new() },
            );
        assert_eq!(top.fill, NodeFill::solid(0xFFFFFF));
        assert_eq!(color_map(f64::NAN, 0.0, 100.0, 0x000000, 0xFFFFFF), 0x000000);
        assert_eq!(color_map(50.0, 1.0, 1.0, 0x112233, 0xFFFFFF), 0x112233);
        let mut edge_mapper = EdgeMapper::new();
        edge_mapper.add_color_map(EdgeColorMap {
            key: "w".into(),
            src_min: 0.0,
            src_max: 1.0,
            start: 0x000000,
            end: 0xFFFFFF,
        });
        let mut half = HashMap::new();
        half.insert("w".to_string(), DataValue::Number(0.5));
        let mid = bypass.resolve_edge_with_data(
            &sheet,
            &edge_mapper,
            node,
            NodeIndex::new(1),
            &half,
            &BTreeSet::new(),
        );
        assert_eq!(mid.tint, 0x808080);
        let plain = bypass.resolve_edge_with_data(
            &sheet,
            &edge_mapper,
            node,
            NodeIndex::new(1),
            &HashMap::new(),
            &BTreeSet::new(),
        );
        assert_eq!(plain.tint, crate::appearance::DEFAULT_EDGE_TINT);
        let _ = (LabelAlign::Center, LabelBackground::None);
        let _ = (NodeStyle::default(), EdgeStyle::default());
    }
}
