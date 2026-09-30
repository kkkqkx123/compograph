//! Shared per-edge geometry for bulk and single-edge planning.
//!
//! Both plan entries resolve self loops, haystack fan-outs, Manhattan routes
//! and parallel Bezier curves through these helpers, so visual decisions stay
//! in one place while culling and traversal remain with the callers.

use cg_geometry::{bezier_control_for_edge, haystack_endpoints, manhattan_route, parallel_offsets};
use cg_graph::NodeIndex;
use cg_types::Point2;

use crate::style::EdgeStyle;

use super::plans::{EdgePaintOptions, NODE_SIDE, PARALLEL_STEP, PaintedEdge};

/// True when the bundle resolves as haystack fan-out lines.
pub(crate) fn should_use_haystack(options: EdgePaintOptions, bundle_len: usize) -> bool {
    if options.ortho.is_some() || options.taxi.is_some() {
        return false;
    }
    if options.force_haystack {
        cg_geometry::use_haystack(bundle_len, true)
    } else {
        bundle_len >= options.aggregate_threshold.max(1)
    }
}

/// True when the edge routes as a Manhattan polyline.
pub(crate) fn should_use_manhattan(options: EdgePaintOptions) -> bool {
    options.ortho.is_some() || options.taxi.is_some()
}

/// Bends of the Manhattan route selected by `options`, if any.
///
/// Taxi routes carry their single corner in the first bend; two-bend
/// orthogonal routes fill both. Degenerate axes fall back to no bends,
///
/// leaving the edge straight.
pub(crate) fn manhattan_bends(
    start: Point2,
    end: Point2,
    options: EdgePaintOptions,
) -> (Option<Point2>, Option<Point2>) {
    match manhattan_route(start, end, options.ortho, options.taxi).as_slice() {
        [_, mid_a, mid_b, _] => (Some(*mid_a), Some(*mid_b)),
        [_, mid, _] => (Some(*mid), None),
        _ => (None, None),
    }
}

/// Signed parallel offset for one directed pair, mirrored by direction.
pub(crate) fn directed_offset(
    source: NodeIndex,
    target: NodeIndex,
    slot: usize,
    bundle_len: usize,
) -> f32 {
    let mut offset = parallel_offsets(bundle_len, PARALLEL_STEP)
        .get(slot)
        .copied()
        .unwrap_or(0.0);
    if source.index() > target.index() {
        offset = -offset;
    }
    offset
}

/// Bezier control for one directed pair, or nothing when straight.
pub(crate) fn bezier_control(
    start: Point2,
    end: Point2,
    source: NodeIndex,
    target: NodeIndex,
    slot: usize,
    bundle_len: usize,
    options: EdgePaintOptions,
) -> Option<Point2> {
    let offset = directed_offset(source, target, slot, bundle_len);
    if offset == 0.0 || !options.level.draws_curves() {
        return None;
    }
    Some(bezier_control_for_edge(start, end, offset))
}

/// Fanned haystack endpoints for one bundle slot.
pub(crate) fn haystack_span(
    start: Point2,
    end: Point2,
    source: NodeIndex,
    target: NodeIndex,
    slot: usize,
) -> (Point2, Point2) {
    haystack_endpoints(
        start,
        end,
        source.index() as u32,
        target.index() as u32,
        slot as u32,
        NODE_SIDE,
    )
}

/// Painted self loop anchored at `screen` with its stacking ordinal applied.
pub(crate) fn loop_edge(
    source: NodeIndex,
    target: NodeIndex,
    screen: Point2,
    loop_stack: usize,
    style: EdgeStyle,
) -> PaintedEdge {
    let ctrls = cg_geometry::self_loop_controls(screen, NODE_SIDE, loop_stack);
    PaintedEdge {
        source,
        target,
        start: screen,
        end: screen,
        ctrl: None,
        loop_ctrls: Some(ctrls),
        bends: Vec::new(),
        aggregated: false,
        tint: style.tint,
        width: style.width,
        opacity: style.opacity,
        arrow: style.arrow,
        arrow_scale: style.arrow_scale,
    }
}

/// Painted haystack edge between fanned endpoints.
pub(crate) fn haystack_edge(
    source: NodeIndex,
    target: NodeIndex,
    start: Point2,
    end: Point2,
    style: EdgeStyle,
) -> PaintedEdge {
    let mut edge = PaintedEdge::straight(
        source,
        target,
        start,
        end,
        style.tint,
        style.width,
        style.opacity,
    )
    .with_arrow(style.arrow, style.arrow_scale);
    edge.aggregated = true;
    edge
}

/// Painted Manhattan edge carrying its bend table.
pub(crate) fn manhattan_edge(
    source: NodeIndex,
    target: NodeIndex,
    start: Point2,
    end: Point2,
    bends: Vec<Point2>,
    style: EdgeStyle,
) -> PaintedEdge {
    PaintedEdge {
        source,
        target,
        start,
        end,
        ctrl: None,
        loop_ctrls: None,
        bends,
        aggregated: false,
        tint: style.tint,
        width: style.width,
        opacity: style.opacity,
        arrow: style.arrow,
        arrow_scale: style.arrow_scale,
    }
}

/// Painted user waypoint edge carrying the cleaned screen sequence.
pub(crate) fn waypoint_edge(
    source: NodeIndex,
    target: NodeIndex,
    start: Point2,
    end: Point2,
    bends: Vec<Point2>,
    style: EdgeStyle,
) -> PaintedEdge {
    PaintedEdge {
        source,
        target,
        start,
        end,
        ctrl: None,
        loop_ctrls: None,
        bends,
        aggregated: false,
        tint: style.tint,
        width: style.width,
        opacity: style.opacity,
        arrow: style.arrow,
        arrow_scale: style.arrow_scale,
    }
}

/// Cleans raw screen waypoints into interior bends.
///
/// Non-finite points drop, consecutive duplicates collapse, and points equal
/// to the endpoints drop so empty and single-point inputs degrade to a
/// straight line through the shared polyline builder.
pub(crate) fn clean_waypoints(start: Point2, raw: &[Point2], end: Point2) -> Vec<Point2> {
    let filtered: Vec<Point2> = raw
        .iter()
        .copied()
        .filter(|point| *point != start && *point != end)
        .collect();
    let line = cg_geometry::segmented_polyline(start, &filtered, end);
    if line.len() <= 2 {
        return Vec::new();
    }
    line[1..line.len() - 1].to_vec()
}

/// Painted straight or parallel-curved edge.
pub(crate) fn curved_edge(
    source: NodeIndex,
    target: NodeIndex,
    start: Point2,
    end: Point2,
    ctrl: Option<Point2>,
    style: EdgeStyle,
) -> PaintedEdge {
    PaintedEdge {
        source,
        target,
        start,
        end,
        ctrl,
        loop_ctrls: None,
        bends: Vec::new(),
        aggregated: false,
        tint: style.tint,
        width: style.width,
        opacity: style.opacity,
        arrow: style.arrow,
        arrow_scale: style.arrow_scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lod::DetailLevel;
    use cg_geometry::OrthoDirection;

    fn options() -> EdgePaintOptions {
        EdgePaintOptions::default()
    }

    #[test]
    fn haystack_decision_follows_threshold_and_overrides() {
        assert!(should_use_haystack(options(), 24));
        assert!(!should_use_haystack(options(), 2));
        let forced = EdgePaintOptions {
            force_haystack: true,
            ..options()
        };
        assert!(should_use_haystack(forced, 6));
        let ortho = EdgePaintOptions {
            ortho: Some(OrthoDirection::Auto),
            force_haystack: true,
            ..options()
        };
        assert!(!should_use_haystack(ortho, 64));
        assert!(should_use_manhattan(ortho));
        assert!(!should_use_manhattan(options()));
    }

    #[test]
    fn opposite_directions_mirror_offsets() {
        let forward = directed_offset(NodeIndex::new(0), NodeIndex::new(1), 0, 2);
        let backward = directed_offset(NodeIndex::new(1), NodeIndex::new(0), 0, 2);
        assert_eq!(forward, -backward);
    }

    #[test]
    fn simplified_level_drops_curve_controls() {
        let start = Point2::new(0.0, 0.0);
        let end = Point2::new(100.0, 0.0);
        let full = bezier_control(
            start,
            end,
            NodeIndex::new(0),
            NodeIndex::new(1),
            0,
            2,
            options(),
        );
        assert!(full.is_some());
        let simplified = EdgePaintOptions {
            level: DetailLevel::Simplified,
            ..options()
        };
        assert!(
            bezier_control(
                start,
                end,
                NodeIndex::new(0),
                NodeIndex::new(1),
                0,
                2,
                simplified
            )
            .is_none()
        );
    }

    #[test]
    fn taxi_bend_carries_a_single_corner() {
        let taxi = EdgePaintOptions {
            taxi: Some(OrthoDirection::HorizontalFirst),
            ..options()
        };
        let (bend_a, bend_b) =
            manhattan_bends(Point2::new(0.0, 0.0), Point2::new(100.0, 40.0), taxi);
        assert!(bend_a.is_some());
        assert!(bend_b.is_none());
    }

    #[test]
    fn waypoint_cleaning_drops_degenerate_points() {
        let start = Point2::new(0.0, 0.0);
        let end = Point2::new(100.0, 0.0);
        assert!(clean_waypoints(start, &[], end).is_empty());
        assert!(clean_waypoints(start, &[Point2::new(f32::NAN, 0.0)], end).is_empty());
        let single = clean_waypoints(start, &[Point2::new(50.0, 10.0)], end);
        assert_eq!(single, vec![Point2::new(50.0, 10.0)]);
        let collapsed = clean_waypoints(
            start,
            &[
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(0.0, 0.0),
            ],
            end,
        );
        assert_eq!(collapsed, vec![Point2::new(10.0, 0.0)]);
    }

    #[test]
    fn waypoint_edge_carries_the_cleaned_table() {
        use crate::style::EdgeStyle;

        let start = Point2::new(0.0, 0.0);
        let end = Point2::new(100.0, 0.0);
        let bends = clean_waypoints(
            start,
            &[Point2::new(30.0, 20.0), Point2::new(70.0, -20.0)],
            end,
        );
        let edge = waypoint_edge(
            NodeIndex::new(0),
            NodeIndex::new(1),
            start,
            end,
            bends,
            EdgeStyle::default(),
        );
        assert_eq!(edge.bends().len(), 2);
        assert_eq!(edge.polyline().len(), 4);
        assert!(edge.ctrl.is_none());
        assert!(!edge.aggregated);
    }
}
