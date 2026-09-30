//! Box-selection hit testing for painted edges.
//!
//! Straight edges use the segment test while curved edges and self loops are
//! flattened with the shared sampling, so selection matches the paint.

use cg_geometry::{
    BEZIER_HIT_SAMPLES, polyline_intersects_rect, sample_cubic_bezier, sample_quadratic_bezier,
    segment_intersects_rect,
};
use cg_types::Rect;

use super::plans::PaintedEdge;

/// True when the painted edge touches the screen-space selection `rect`.
pub fn painted_edge_hits(edge: &PaintedEdge, rect: Rect) -> bool {
    if let Some([ctrl_a, ctrl_b]) = edge.loop_ctrls {
        let samples = sample_cubic_bezier(edge.start, ctrl_a, ctrl_b, edge.end, BEZIER_HIT_SAMPLES);
        return polyline_intersects_rect(&samples, rect);
    }
    if !edge.bends.is_empty() {
        return polyline_intersects_rect(&edge.polyline(), rect);
    }
    match edge.ctrl {
        None => segment_intersects_rect(edge.start, edge.end, rect),
        Some(mid) => {
            let samples = sample_quadratic_bezier(edge.start, mid, edge.end, BEZIER_HIT_SAMPLES);
            polyline_intersects_rect(&samples, rect)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_graph::NodeIndex;
    use cg_types::{Point2, Vec2};

    use crate::arrows::ArrowKind;

    fn straight() -> PaintedEdge {
        PaintedEdge {
            source: NodeIndex::new(0),
            target: NodeIndex::new(1),
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
            ctrl: None,
            loop_ctrls: None,
            bends: Vec::new(),
            aggregated: false,
            tint: 0,
            width: 1.0,
            opacity: 1.0,
            arrow: ArrowKind::Triangle,
            arrow_scale: 1.0,
        }
    }

    #[test]
    fn straight_and_curved_hits_follow_paint() {
        let rect = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
        assert!(painted_edge_hits(&straight(), rect));
        let far = Rect::new(Point2::new(4.0, 50.0), Vec2::new(2.0, 2.0));
        assert!(!painted_edge_hits(&straight(), far));
        let curved = PaintedEdge {
            ctrl: Some(Point2::new(5.0, 10.0)),
            ..straight()
        };
        let bulge = Rect::new(Point2::new(3.0, 3.0), Vec2::new(4.0, 4.0));
        assert!(painted_edge_hits(&curved, bulge));
    }
}
