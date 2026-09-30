//! Viewport culling math shared by node and edge plan passes.
//!
//! All tests here are geometric: world rectangles derive from the camera,
//! screen tests use a fixed margin, and hit testing elsewhere consumes the
//! same flattened paths the painters store.

use cg_graph::NodeIndex;
use cg_types::{Point2, Rect, Vec2};

use crate::camera::Camera;

use super::plans::{NODE_SIDE, PARALLEL_STEP};

/// Margin around the viewport still scheduled for painting.
const CULL_MARGIN: f32 = 32.0;

/// Model-space rectangle currently visible through the camera.
pub fn world_viewport_rect(camera: &Camera, viewport: Vec2) -> Rect {
    let top_left = camera.viewport_to_world(viewport, Point2::ZERO);
    let bottom_right = camera.viewport_to_world(viewport, Point2::new(viewport.x, viewport.y));
    Rect::from_corners(top_left, bottom_right)
}

pub(crate) fn unordered_key(source: NodeIndex, target: NodeIndex) -> (usize, usize) {
    let (a, b) = (source.index(), target.index());
    if a <= b { (a, b) } else { (b, a) }
}

/// Screen cull margin expressed in model units at the current zoom.
pub(crate) fn world_margin_for(camera: &Camera) -> f32 {
    CULL_MARGIN / camera.zoom.max(f32::EPSILON)
}

/// Extra model-space spread a bundle may fan out to around its endpoints.
pub(crate) fn spread_for(bundle_len: usize) -> f32 {
    NODE_SIDE + PARALLEL_STEP * bundle_len as f32
}

pub(crate) fn point_in_grown_rect(point: Point2, rect: Rect, grow: f32) -> bool {
    point.x >= rect.origin.x - grow
        && point.y >= rect.origin.y - grow
        && point.x <= rect.origin.x + rect.size.x + grow
        && point.y <= rect.origin.y + rect.size.y + grow
}

pub(crate) fn segment_in_grown_rect(a: Point2, b: Point2, rect: Rect, grow: f32) -> bool {
    let min_x = a.x.min(b.x);
    let min_y = a.y.min(b.y);
    let max_x = a.x.max(b.x);
    let max_y = a.y.max(b.y);
    max_x >= rect.origin.x - grow
        && max_y >= rect.origin.y - grow
        && min_x <= rect.origin.x + rect.size.x + grow
        && min_y <= rect.origin.y + rect.size.y + grow
}

pub(crate) fn edge_visible(
    start: Point2,
    end: Point2,
    ctrl: Option<Point2>,
    viewport: Vec2,
) -> bool {
    let mut min_x = start.x.min(end.x);
    let mut min_y = start.y.min(end.y);
    let mut max_x = start.x.max(end.x);
    let mut max_y = start.y.max(end.y);
    if let Some(mid) = ctrl {
        min_x = min_x.min(mid.x);
        min_y = min_y.min(mid.y);
        max_x = max_x.max(mid.x);
        max_y = max_y.max(mid.y);
    }
    bounds_visible(min_x, min_y, max_x, max_y, viewport)
}

pub(crate) fn polyline_visible(line: &[Point2], viewport: Vec2) -> bool {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in line {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    bounds_visible(min_x, min_y, max_x, max_y, viewport)
}

pub(crate) fn loop_visible(anchor: Point2, ctrls: [Point2; 2], viewport: Vec2) -> bool {
    let min_x = anchor.x.min(ctrls[0].x.min(ctrls[1].x));
    let min_y = anchor.y.min(ctrls[0].y.min(ctrls[1].y));
    let max_x = anchor.x.max(ctrls[0].x.max(ctrls[1].x));
    let max_y = anchor.y.max(ctrls[0].y.max(ctrls[1].y));
    bounds_visible(min_x, min_y, max_x, max_y, viewport)
}

pub(crate) fn bounds_visible(
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    viewport: Vec2,
) -> bool {
    max_x >= -CULL_MARGIN
        && max_y >= -CULL_MARGIN
        && min_x <= viewport.x + CULL_MARGIN
        && min_y <= viewport.y + CULL_MARGIN
}
