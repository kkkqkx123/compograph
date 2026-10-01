//! Point hit testing and drag geometry for pointer gestures.

use cg_graph::{NodeIndex, Positions};
use cg_render::{NODE_SIDE, NodeShape, SpatialIndex, point_hits_shape};
use cg_types::{Point2, Rect, Vec2};

/// Half extent of a node body in model units, derived from the paint plan.
pub const NODE_HALF_EXTENT: f32 = NODE_SIDE / 2.0;

/// Pointer tolerance for node grabs, in model units.
pub const NODE_GRAB_TOLERANCE: f32 = 6.0;

/// Finds the node under `world_point` honoring per-node shapes.
///
/// Candidates are tested nearest first; the square body stays the conservative
/// outer envelope inside [`point_hits_shape`], and box selection shares the
/// same shape test, so taps and box selects agree.
pub fn press_hit_shaped(
    world_point: Point2,
    positions: &Positions,
    index: &SpatialIndex,
    radius: f32,
    shape_of: impl Fn(NodeIndex) -> NodeShape,
) -> Option<(NodeIndex, Vec2)> {
    let candidates = index.query_point(world_point, radius);
    let mut ordered: Vec<(f32, NodeIndex, Point2)> = Vec::new();
    for node in candidates {
        if let Some(center) = positions.get(&node).copied() {
            let distance = (world_point - center).length_squared();
            ordered.push((distance, node, center));
        }
    }
    ordered.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, node, center) in ordered {
        if point_hits_shape(
            shape_of(node),
            world_point,
            center,
            NODE_HALF_EXTENT,
            NODE_GRAB_TOLERANCE,
        ) {
            return Some((node, world_point - center));
        }
    }
    None
}

/// Node under `world_point` honoring per-node shapes, for hover highlighting.
pub fn hover_node_shaped(
    world_point: Point2,
    positions: &Positions,
    index: &SpatialIndex,
    radius: f32,
    shape_of: impl Fn(NodeIndex) -> NodeShape,
) -> Option<NodeIndex> {
    press_hit_shaped(world_point, positions, index, radius, shape_of).map(|(node, _)| node)
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
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 0.0));
        let index = indexed(&positions);
        let hit = press_hit_shaped(Point2::new(103.0, 1.0), &positions, &index, 24.0, |_| {
            NodeShape::Square
        });
        assert!(hit.map(|(node, _)| node) == Some(NodeIndex::new(1)));
    }

    #[test]
    fn press_misses_empty_space() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        assert!(
            press_hit_shaped(Point2::new(200.0, 200.0), &positions, &index, 24.0, |_| {
                NodeShape::Square
            })
            .is_none()
        );
    }

    #[test]
    fn drag_keeps_the_grab_offset() {
        let moved = drag_position(Point2::new(10.0, 10.0), Vec2::new(2.0, 1.0));
        assert_eq!(moved, Point2::new(8.0, 9.0));
    }

    #[test]
    fn drag_normalization_orders_corners() {
        let rect = normalize_drag(Point2::new(9.0, 1.0), Point2::new(2.0, 7.0));
        assert_eq!(rect.origin, Point2::new(2.0, 1.0));
        assert_eq!(rect.size, Vec2::new(7.0, 6.0));
    }

    #[test]
    fn shaped_press_rejects_triangle_corners() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        let corner = Point2::new(-11.0, -11.0);
        assert!(
            press_hit_shaped(corner, &positions, &index, 24.0, |_| NodeShape::Square).is_some()
        );
        assert!(
            press_hit_shaped(corner, &positions, &index, 24.0, |_| NodeShape::Triangle).is_none()
        );
        assert!(
            press_hit_shaped(Point2::ZERO, &positions, &index, 24.0, |_| {
                NodeShape::Triangle
            })
            .is_some()
        );
    }

    #[test]
    fn hover_agrees_with_press_hits() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        let shape_of = |_| NodeShape::Square;
        assert_eq!(
            hover_node_shaped(Point2::new(2.0, 1.0), &positions, &index, 24.0, shape_of),
            Some(NodeIndex::new(0))
        );
        assert_eq!(
            hover_node_shaped(
                Point2::new(200.0, 200.0),
                &positions,
                &index,
                24.0,
                shape_of
            ),
            None
        );
    }
}
