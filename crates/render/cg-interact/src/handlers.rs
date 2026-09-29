//! Pure translation of pointer positions into graph state changes.
//!
//! These helpers stay free of gpui event types so they can be unit tested
//! headlessly; the application converts platform events into the model-space
//! points these functions consume.

use cg_geometry::{nearest_point_index, point_hits_node};
use cg_graph::{NodeIndex, Positions};
use cg_render::SpatialIndex;
use cg_types::{Point2, Vec2};

/// Half extent of a node body in model units, matching the paint plan.
pub const NODE_HALF_EXTENT: f32 = 12.0;

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
}
