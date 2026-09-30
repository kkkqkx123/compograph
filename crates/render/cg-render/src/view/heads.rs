//! Arrowhead plans pointing along the edge end tangent.
//!
//! Every kind shares one orientation rule; the dot needs only a center.
//! Self loops point along the return tangent from the second loop control.

use cg_types::Vec2;

use crate::arrows::arrow_polygon;
use crate::lod::DetailLevel;

use super::plans::{ARROW_HALF_WIDTH, ARROW_LENGTH, PaintedArrow, PaintedEdge};

/// Arrowheads honoring the detail level.
///
/// Minimal detail drops arrowheads entirely; other levels point every edge
/// along its end tangent. Self loops point along the return tangent.
pub fn paint_arrows_for_level(edges: &[PaintedEdge], level: DetailLevel) -> Vec<PaintedArrow> {
    if level == DetailLevel::Minimal {
        return Vec::new();
    }
    paint_arrows(edges)
}

/// Arrowheads for every painted edge, pointing along the end tangent.
///
/// Self loops point along the return tangent from the second loop control.
pub fn paint_arrows(edges: &[PaintedEdge]) -> Vec<PaintedArrow> {
    edges.iter().map(paint_single_arrow).collect()
}

/// Arrowhead for one painted edge, pointing along its end tangent.
///
/// Self loops point along the return tangent from the second loop control.
/// The kind and scale ride on the edge plan; dots ignore the angle and center
/// on the endpoint.
pub fn paint_single_arrow(edge: &PaintedEdge) -> PaintedArrow {
    let direction = edge_direction(edge);
    let angle = direction.y.atan2(direction.x);
    let scale = edge.arrow_scale.max(0.1);
    let points = arrow_polygon(
        edge.arrow,
        edge.end,
        angle,
        ARROW_LENGTH * scale,
        ARROW_HALF_WIDTH * scale,
    );
    PaintedArrow {
        tip: edge.end,
        points,
        tint: edge.tint,
        kind: edge.arrow,
    }
}

/// Unit tangent of an edge at its endpoint.
fn edge_direction(edge: &PaintedEdge) -> Vec2 {
    let reference = edge
        .loop_ctrls
        .map(|ctrls| ctrls[1])
        .or(edge.ctrl)
        .or(edge.bend_b)
        .or(edge.bend_a)
        .unwrap_or(edge.start);
    let delta = edge.end - reference;
    let length = delta.length();
    if length <= f32::EPSILON {
        Vec2::new(1.0, 0.0)
    } else {
        Vec2::new(delta.x / length, delta.y / length)
    }
}

#[cfg(test)]
mod tests {
    use cg_graph::{MockGraph, NodeIndex, Positions};

    use super::*;
    use crate::arrows::ArrowKind;
    use crate::camera::Camera;
    use crate::style::EdgeStyle;
    use cg_types::{Point2, Vec2};

    fn viewport() -> Vec2 {
        Vec2::new(1024.0, 768.0)
    }

    fn camera() -> Camera {
        Camera::new(Point2::ZERO, 1.0)
    }

    #[test]
    fn arrow_tip_leads_along_the_edge() {
        use super::super::plans::{ARROW_HALF_WIDTH, ARROW_LENGTH};
        use crate::arrows::arrow_polygon;

        let corners = arrow_polygon(
            ArrowKind::Triangle,
            Point2::new(10.0, 0.0),
            0.0,
            ARROW_LENGTH,
            ARROW_HALF_WIDTH,
        );
        assert_eq!(corners[0], Point2::new(10.0, 0.0));
        assert!(corners[1].x < corners[0].x && corners[2].x < corners[0].x);
        assert!(corners[1].y > 0.0 && corners[2].y < 0.0);
    }

    #[test]
    fn self_loop_arrow_sits_on_the_loop_end() {
        use super::super::edges::paint_edges;

        let mut graph = MockGraph::isolated(1);
        graph.push_edge(0, 0);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
            EdgeStyle::default()
        });
        assert_eq!(edges.len(), 1);
        let arrows = paint_arrows(&edges);
        assert_eq!(arrows.len(), 1);
        assert!(arrows[0].tip == edges[0].end);
    }

    #[test]
    fn arrow_kinds_expand_to_vertex_tables() {
        use super::super::edges::paint_edges;

        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        for (kind, count) in [
            (ArrowKind::Triangle, 3),
            (ArrowKind::Dovetail, 4),
            (ArrowKind::Tee, 4),
            (ArrowKind::Dot, 12),
            (ArrowKind::Diamond, 4),
        ] {
            let edges = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
                EdgeStyle {
                    arrow: kind,
                    ..EdgeStyle::default()
                }
            });
            assert_eq!(edges.len(), 1);
            assert_eq!(edges[0].arrow, kind);
            let arrows = paint_arrows(&edges);
            assert_eq!(arrows.len(), 1);
            assert_eq!(arrows[0].kind, kind);
            assert_eq!(arrows[0].points.len(), count);
            assert_eq!(arrows[0].tip, edges[0].end);
        }
    }
}
