//! Node box selection by body shape with an optional label pass.

use std::collections::BTreeSet;

use cg_graph::{NodeIndex, Positions};
use cg_render::{
    LabelStyle, NodeShape, SpatialIndex, label_envelope_styled, node_label_origin, shape_hits_rect,
};
use cg_types::{Point2, Rect, Vec2};

/// Nodes whose bodies touch `rect`, in index order.
///
/// The spatial index narrows candidates and each body is tested with its own
/// shape, so box selection agrees with pointer hit testing: corners a tap
/// rejects stay unselected here as well. The query grows by the body extent
/// because the index keys on centers; the exact shape test still rejects
/// non-overlapping bodies, so growth only adds candidates.
pub fn nodes_in_rect(
    positions: &Positions,
    index: &SpatialIndex,
    rect: Rect,
    half_extent: f32,
    shape_of: impl Fn(NodeIndex) -> NodeShape,
) -> Vec<NodeIndex> {
    let grown = Rect::new(
        Point2::new(rect.origin.x - half_extent, rect.origin.y - half_extent),
        Vec2::new(
            rect.size.x + half_extent * 2.0,
            rect.size.y + half_extent * 2.0,
        ),
    );
    let mut found: Vec<NodeIndex> = index
        .query_rect(grown)
        .into_iter()
        .filter(|node| {
            positions
                .get(node)
                .map(|center| shape_hits_rect(shape_of(*node), *center, half_extent, rect))
                .unwrap_or(false)
        })
        .collect();
    found.sort_unstable_by_key(|node| node.index());
    found
}

/// Nodes whose bodies or label envelopes touch `rect`, in index order.
///
/// The body pass reuses [`nodes_in_rect`] so taps and box selects keep
/// agreeing; the label pass unions each label envelope computed with the same
/// origin and envelope helpers the canvas plans with.
pub fn nodes_in_rect_with_labels(
    positions: &Positions,
    index: &SpatialIndex,
    rect: Rect,
    half_extent: f32,
    shape_of: impl Fn(NodeIndex) -> NodeShape,
    label_of: impl Fn(NodeIndex) -> Option<(String, f32, LabelStyle)>,
) -> Vec<NodeIndex> {
    let mut found: BTreeSet<usize> = nodes_in_rect(positions, index, rect, half_extent, &shape_of)
        .into_iter()
        .map(|node| node.index())
        .collect();
    for (node, center) in positions {
        if found.contains(&node.index()) {
            continue;
        }
        let Some((text, size, style)) = label_of(*node) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let origin = node_label_origin(*center, half_extent * 2.0, &style);
        let envelope = label_envelope_styled(origin, &text, size, 0.0, &style);
        if rect.intersects(envelope) {
            found.insert(node.index());
        }
    }
    found.into_iter().map(NodeIndex::new).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hits::NODE_HALF_EXTENT;

    fn indexed(positions: &Positions) -> SpatialIndex {
        let mut index = SpatialIndex::new(32.0);
        index.rebuild(positions);
        index
    }

    #[test]
    fn rect_select_picks_nodes_by_body_overlap() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 0.0));
        positions.insert(NodeIndex::new(2), Point2::new(200.0, 0.0));
        let index = indexed(&positions);
        let rect = Rect::from_corners(Point2::new(-20.0, -20.0), Point2::new(112.0, 20.0));
        assert_eq!(
            nodes_in_rect(&positions, &index, rect, NODE_HALF_EXTENT, |_| {
                NodeShape::Square
            }),
            vec![NodeIndex::new(0), NodeIndex::new(1)]
        );
    }

    #[test]
    fn rect_select_rejects_corners_a_tap_rejects() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::ZERO);
        let index = indexed(&positions);
        let corner = Rect::from_corners(Point2::new(-12.0, -12.0), Point2::new(-8.0, -8.0));
        assert_eq!(
            nodes_in_rect(&positions, &index, corner, NODE_HALF_EXTENT, |_| {
                NodeShape::Square
            }),
            vec![NodeIndex::new(0)]
        );
        assert!(
            nodes_in_rect(&positions, &index, corner, NODE_HALF_EXTENT, |_| {
                NodeShape::Triangle
            })
            .is_empty()
        );
    }

    #[test]
    fn label_box_select_unions_the_text_envelope() {
        use cg_graph::MockGraph;
        use cg_render::NodeShape;

        let graph = MockGraph::isolated(1);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        let body_only = Rect::from_corners(Point2::new(-20.0, -20.0), Point2::new(-15.0, -15.0));
        assert!(
            nodes_in_rect(&positions, &index, body_only, NODE_HALF_EXTENT, |_| {
                NodeShape::Square
            })
            .is_empty()
        );
        let label_band = Rect::from_corners(Point2::new(-40.0, 14.0), Point2::new(40.0, 40.0));
        assert!(
            nodes_in_rect(&positions, &index, label_band, NODE_HALF_EXTENT, |_| {
                NodeShape::Square
            })
            .is_empty()
        );
        let with_labels = nodes_in_rect_with_labels(
            &positions,
            &index,
            label_band,
            NODE_HALF_EXTENT,
            |_| NodeShape::Square,
            |_| Some(("hello".to_string(), 12.0, LabelStyle::default())),
        );
        assert_eq!(with_labels, vec![NodeIndex::new(0)]);
        let without_labels = nodes_in_rect_with_labels(
            &positions,
            &index,
            label_band,
            NODE_HALF_EXTENT,
            |_| NodeShape::Square,
            |_| None,
        );
        assert!(without_labels.is_empty());
        let _ = graph;
    }
}
