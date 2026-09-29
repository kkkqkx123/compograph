//! Pure translation of pointer positions into graph state changes.
//!
//! These helpers stay free of gpui event types so they can be unit tested
//! headlessly; the application converts platform events into the model-space
//! points these functions consume.

use std::collections::HashMap;

use cg_geometry::{
    BEZIER_HIT_SAMPLES, bezier_control_for_edge, nearest_point_index, parallel_offsets,
    point_hits_node, polyline_intersects_rect, sample_quadratic_bezier, segment_intersects_rect,
    self_loop_polyline,
};
use cg_graph::{GraphView, NodeIndex, Positions};
use cg_render::{NODE_SIDE, PARALLEL_STEP, SpatialIndex};
use cg_types::{Point2, Rect, Vec2};

/// Half extent of a node body in model units, derived from the paint plan.
pub const NODE_HALF_EXTENT: f32 = NODE_SIDE / 2.0;

/// Pointer tolerance for node grabs, in model units.
pub const NODE_GRAB_TOLERANCE: f32 = 6.0;

/// Finds the node under `world_point`, if any.
///
/// Candidates come from the spatial index; the nearest candidate within the
/// node body wins. Returns the hit node and the grab offset from its center.
pub fn press_hit(
    world_point: Point2,
    positions: &Positions,
    index: &SpatialIndex,
    radius: f32,
) -> Option<(NodeIndex, Vec2)> {
    let candidates = index.query_point(world_point, radius);
    let mut points = Vec::with_capacity(candidates.len());
    for node in &candidates {
        if let Some(position) = positions.get(node) {
            points.push(*position);
        }
    }
    let slot = nearest_point_index(world_point, &points)?;
    let node = candidates.get(slot).copied()?;
    let center = positions.get(&node).copied()?;
    if point_hits_node(world_point, center, NODE_HALF_EXTENT, NODE_GRAB_TOLERANCE) {
        Some((node, world_point - center))
    } else {
        None
    }
}

/// New center of a dragged node from the pointer and the initial grab offset.
pub fn drag_position(pointer_world: Point2, grab_offset: Vec2) -> Point2 {
    Point2::new(
        pointer_world.x - grab_offset.x,
        pointer_world.y - grab_offset.y,
    )
}

/// Normalized selection rectangle spanning a viewport drag.
///
/// Corners arrive in any order; the result always has non-negative size.
pub fn normalize_drag(start: Point2, current: Point2) -> Rect {
    Rect::from_corners(start, current)
}

/// Nodes whose bodies touch `rect`, in index order.
///
/// The spatial index narrows candidates and the node body square decides, so
/// partially covered nodes count as selected.
pub fn nodes_in_rect(
    positions: &Positions,
    index: &SpatialIndex,
    rect: Rect,
    half_extent: f32,
) -> Vec<NodeIndex> {
    let mut found: Vec<NodeIndex> = index
        .query_rect(rect)
        .into_iter()
        .filter(|node| {
            positions
                .get(node)
                .map(|center| {
                    let body = Rect::new(
                        Point2::new(center.x - half_extent, center.y - half_extent),
                        Vec2::new(half_extent * 2.0, half_extent * 2.0),
                    );
                    body.intersects(rect)
                })
                .unwrap_or(false)
        })
        .collect();
    found.sort_unstable_by_key(|node| node.index());
    found
}

/// True when the edge between `start` and `end` touches `rect`.
///
/// Curved edges are flattened with the same sampling as hit testing, so box
/// selection and pointer hits agree on curved shapes.
pub fn edge_hits_rect(start: Point2, ctrl: Option<Point2>, end: Point2, rect: Rect) -> bool {
    match ctrl {
        None => segment_intersects_rect(start, end, rect),
        Some(mid) => {
            let samples = sample_quadratic_bezier(start, mid, end, BEZIER_HIT_SAMPLES);
            polyline_intersects_rect(&samples, rect)
        }
    }
}

/// True when the self loop anchored at `center` touches `rect`.
///
/// The loop is flattened with the shared loop geometry, so selection matches
/// what the canvas paints.
pub fn loop_hits_rect(center: Point2, node_side: f32, ordinal: usize, rect: Rect) -> bool {
    polyline_intersects_rect(&self_loop_polyline(center, node_side, ordinal), rect)
}

/// Directed edges touching `rect`, in (source, target) order.
///
/// Straight and curved edges share the paint plan bundling, and self loops
/// reuse the painted loop geometry, so box selection matches the canvas.
/// Parallel edges in one direction collapse to a single entry.
pub fn edges_in_rect(
    graph: &dyn GraphView,
    positions: &Positions,
    rect: Rect,
    node_side: f32,
) -> Vec<(NodeIndex, NodeIndex)> {
    let mut edges = graph.edges();
    edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    let mut bundle_of: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        bundle_of
            .entry(bundle_key(*source, *target))
            .or_default()
            .push(ordinal);
    }
    let mut loops_seen: HashMap<usize, usize> = HashMap::new();
    let mut found = Vec::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        if source == target {
            let Some(center) = positions.get(source) else {
                continue;
            };
            let seen = loops_seen.get(&source.index()).copied().unwrap_or(0);
            loops_seen.insert(source.index(), seen + 1);
            if loop_hits_rect(*center, node_side, seen, rect) {
                found.push((*source, *target));
            }
            continue;
        }
        let (Some(start), Some(end)) = (positions.get(source), positions.get(target)) else {
            continue;
        };
        let bundle = bundle_of
            .get(&bundle_key(*source, *target))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let slot = bundle
            .iter()
            .position(|member| *member == ordinal)
            .unwrap_or(0);
        let offsets = parallel_offsets(bundle.len(), PARALLEL_STEP);
        let mut offset = offsets.get(slot).copied().unwrap_or(0.0);
        if source.index() > target.index() {
            offset = -offset;
        }
        let ctrl = if offset == 0.0 {
            None
        } else {
            Some(bezier_control_for_edge(*start, *end, offset))
        };
        if edge_hits_rect(*start, ctrl, *end, rect) {
            found.push((*source, *target));
        }
    }
    found.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    found.dedup();
    found
}

fn bundle_key(source: NodeIndex, target: NodeIndex) -> (usize, usize) {
    let (a, b) = (source.index(), target.index());
    if a <= b { (a, b) } else { (b, a) }
}

/// Node under `world_point`, if any, for hover highlighting.
///
/// This shares the press-hit geometry without the grab offset, so hover and
/// click agree on the target.
pub fn hover_node(
    world_point: Point2,
    positions: &Positions,
    index: &SpatialIndex,
    radius: f32,
) -> Option<NodeIndex> {
    press_hit(world_point, positions, index, radius).map(|(node, _)| node)
}

/// Zoom factor for a scroll wheel line delta.
///
/// Positive deltas zoom in one notch per line; the factor compounds, so small
/// trackpad deltas stay smooth while notched wheels move visibly.
pub fn wheel_zoom_factor(line_delta: f32) -> f32 {
    if !line_delta.is_finite() {
        return 1.0;
    }
    1.15f32.powf(-line_delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indexed(positions: &Positions) -> SpatialIndex {
        let mut index = SpatialIndex::new(32.0);
        index.rebuild(positions);
        index
    }

    #[test]
    fn press_finds_the_node_under_the_pointer() {
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 0.0));
        let index = indexed(&positions);
        let hit = press_hit(Point2::new(103.0, 1.0), &positions, &index, 24.0);
        assert!(hit.map(|(node, _)| node) == Some(NodeIndex::new(1)));
    }

    #[test]
    fn press_misses_empty_space() {
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        assert!(press_hit(Point2::new(200.0, 200.0), &positions, &index, 24.0).is_none());
    }

    #[test]
    fn drag_keeps_the_grab_offset() {
        let moved = drag_position(Point2::new(10.0, 10.0), Vec2::new(2.0, 1.0));
        assert_eq!(moved, Point2::new(8.0, 9.0));
    }

    #[test]
    fn wheel_factor_zooms_in_on_negative_delta() {
        assert!(wheel_zoom_factor(-1.0) > 1.0);
        assert!(wheel_zoom_factor(1.0) < 1.0);
        assert_eq!(wheel_zoom_factor(f32::NAN), 1.0);
    }

    #[test]
    fn drag_normalization_orders_corners() {
        let rect = normalize_drag(Point2::new(9.0, 1.0), Point2::new(2.0, 7.0));
        assert_eq!(rect.origin, Point2::new(2.0, 1.0));
        assert_eq!(rect.size, Vec2::new(7.0, 6.0));
    }

    #[test]
    fn rect_select_picks_nodes_by_body_overlap() {
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 0.0));
        positions.insert(NodeIndex::new(2), Point2::new(200.0, 0.0));
        let index = indexed(&positions);
        let rect = Rect::from_corners(Point2::new(-20.0, -20.0), Point2::new(112.0, 20.0));
        assert_eq!(
            nodes_in_rect(&positions, &index, rect, NODE_HALF_EXTENT),
            vec![NodeIndex::new(0), NodeIndex::new(1)]
        );
    }

    #[test]
    fn rect_select_judges_edges_by_intersection() {
        let rect = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
        assert!(edge_hits_rect(
            Point2::new(0.0, 0.0),
            None,
            Point2::new(10.0, 0.0),
            rect
        ));
        assert!(!edge_hits_rect(
            Point2::new(0.0, 5.0),
            None,
            Point2::new(10.0, 5.0),
            rect
        ));
        assert!(edge_hits_rect(
            Point2::new(0.0, 0.0),
            Some(Point2::new(5.0, 10.0)),
            Point2::new(10.0, 0.0),
            Rect::from_corners(Point2::new(3.0, 3.0), Point2::new(7.0, 7.0))
        ));
    }

    #[test]
    fn rect_select_catches_self_loops_above_the_node() {
        let center = Point2::new(0.0, 0.0);
        let above = Rect::from_corners(Point2::new(-30.0, -70.0), Point2::new(30.0, -14.0));
        assert!(loop_hits_rect(center, 24.0, 0, above));
        let below = Rect::from_corners(Point2::new(-30.0, 30.0), Point2::new(30.0, 70.0));
        assert!(!loop_hits_rect(center, 24.0, 0, below));
    }

    #[test]
    fn edges_in_rect_matches_straight_and_loop_edges() {
        use cg_graph::MockGraph;

        let mut graph = MockGraph::chain(3);
        graph.push_edge(2, 2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(10.0, 0.0));
        positions.insert(NodeIndex::new(2), Point2::new(20.0, 0.0));
        let crossing = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
        assert_eq!(
            edges_in_rect(&graph, &positions, crossing, NODE_SIDE),
            vec![(NodeIndex::new(0), NodeIndex::new(1))]
        );
        let above_loop = Rect::from_corners(Point2::new(-10.0, -70.0), Point2::new(50.0, -14.0));
        assert_eq!(
            edges_in_rect(&graph, &positions, above_loop, NODE_SIDE),
            vec![(NodeIndex::new(2), NodeIndex::new(2))]
        );
        let far = Rect::new(Point2::new(200.0, 200.0), Vec2::new(10.0, 10.0));
        assert!(edges_in_rect(&graph, &positions, far, NODE_SIDE).is_empty());
    }

    #[test]
    fn hover_agrees_with_press_hits() {
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        assert_eq!(
            hover_node(Point2::new(2.0, 1.0), &positions, &index, 24.0),
            Some(NodeIndex::new(0))
        );
        assert_eq!(
            hover_node(Point2::new(200.0, 200.0), &positions, &index, 24.0),
            None
        );
    }
}
