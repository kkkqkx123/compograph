//! Evaluation of quadratic Bezier curves used for curved edges.

use cg_types::Point2;

/// Point on the quadratic Bezier from `start` to `end` with control point
/// `ctrl`, evaluated at parameter `t` in `[0, 1]`.
pub fn quadratic_bezier(start: Point2, ctrl: Point2, end: Point2, t: f32) -> Point2 {
    let u = 1.0 - t;
    Point2::new(
        u * u * start.x + 2.0 * u * t * ctrl.x + t * t * end.x,
        u * u * start.y + 2.0 * u * t * ctrl.y + t * t * end.y,
    )
}

/// Samples the quadratic Bezier into `segments` straight pieces.
///
/// The returned polyline always includes both endpoints; a zero segment count
/// falls back to the endpoints so callers never receive an empty path.
pub fn sample_quadratic_bezier(
    start: Point2,
    ctrl: Point2,
    end: Point2,
    segments: usize,
) -> Vec<Point2> {
    let segments = segments.max(1);
    (0..=segments)
        .map(|step| quadratic_bezier(start, ctrl, end, step as f32 / segments as f32))
        .collect()
}

/// Control point for a single curved edge between `start` and `end`.
///
/// The control point sits on the perpendicular bisector at `offset` model
/// units, so parallel edges spread symmetrically around the straight line.
/// Degenerate endpoints fall back to a horizontal nudge to avoid NaNs.
pub fn bezier_control_for_edge(start: Point2, end: Point2, offset: f32) -> Point2 {
    use cg_types::Vec2;

    let mid = Point2::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0);
    let delta = end - start;
    let length = delta.length();
    if length <= f32::EPSILON {
        return mid + Vec2::new(offset, 0.0);
    }
    let normal = Vec2::new(-delta.y / length, delta.x / length);
    mid + normal * offset
}

/// Height of a self loop above the node top, as a multiple of `node_side`.
pub const SELF_LOOP_HEIGHT_SCALE: f32 = 1.5;

/// Half width of a self loop, as a multiple of `node_side`.
pub const SELF_LOOP_HALF_WIDTH_SCALE: f32 = 0.75;

/// Sample count used when flattening a self loop into a polyline.
pub const SELF_LOOP_SAMPLES: usize = 24;

/// Control points of the upward self loop anchored at `center`.
///
/// The loop leaves and returns to the node anchor and bulges upwards, so both
/// control points sit above the node body. Overlapping loops are spread by
/// `ordinal`, which scales the loop outward in whole steps.
pub fn self_loop_controls(center: Point2, node_side: f32, ordinal: usize) -> [Point2; 2] {
    let side = node_side.max(1.0);
    let growth = 1.0 + ordinal as f32 / 3.0;
    let half_width = side * SELF_LOOP_HALF_WIDTH_SCALE * growth;
    let height = side * (0.5 + SELF_LOOP_HEIGHT_SCALE) * growth;
    [
        Point2::new(center.x - half_width, center.y - height),
        Point2::new(center.x + half_width, center.y - height),
    ]
}

/// Point on the cubic Bezier with controls `ctrl_a` and `ctrl_b`.
pub fn cubic_bezier(start: Point2, ctrl_a: Point2, ctrl_b: Point2, end: Point2, t: f32) -> Point2 {
    let u = 1.0 - t;
    let a = u * u * u;
    let b = 3.0 * u * u * t;
    let c = 3.0 * u * t * t;
    let d = t * t * t;
    Point2::new(
        a * start.x + b * ctrl_a.x + c * ctrl_b.x + d * end.x,
        a * start.y + b * ctrl_a.y + c * ctrl_b.y + d * end.y,
    )
}

/// Samples the cubic Bezier into `segments` straight pieces.
pub fn sample_cubic_bezier(
    start: Point2,
    ctrl_a: Point2,
    ctrl_b: Point2,
    end: Point2,
    segments: usize,
) -> Vec<Point2> {
    let segments = segments.max(1);
    (0..=segments)
        .map(|step| cubic_bezier(start, ctrl_a, ctrl_b, end, step as f32 / segments as f32))
        .collect()
}

/// Polyline of the upward self loop anchored at `center`.
///
/// The first and last samples coincide at the anchor, matching the paint plan
/// convention that a self loop starts and ends on the same node.
pub fn self_loop_polyline(center: Point2, node_side: f32, ordinal: usize) -> Vec<Point2> {
    let [ctrl_a, ctrl_b] = self_loop_controls(center, node_side, ordinal);
    sample_cubic_bezier(center, ctrl_a, ctrl_b, center, SELF_LOOP_SAMPLES)
}
/// Symmetric perpendicular offsets for `count` parallel edges.
///
/// A single edge stays straight at zero; pairs split evenly around the line;
/// larger bundles step outwards in whole `step` multiples.
pub fn parallel_offsets(count: usize, step: f32) -> Vec<f32> {
    (0..count)
        .map(|index| (index as f32 - (count as f32 - 1.0) / 2.0) * step)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_reproduced_exactly() {
        let start = Point2::new(0.0, 0.0);
        let ctrl = Point2::new(10.0, 20.0);
        let end = Point2::new(30.0, 10.0);
        assert_eq!(quadratic_bezier(start, ctrl, end, 0.0), start);
        assert_eq!(quadratic_bezier(start, ctrl, end, 1.0), end);
    }

    #[test]
    fn midpoint_matches_blossom_value() {
        let start = Point2::new(0.0, 0.0);
        let ctrl = Point2::new(10.0, 10.0);
        let end = Point2::new(20.0, 0.0);
        let mid = quadratic_bezier(start, ctrl, end, 0.5);
        assert_eq!(mid, Point2::new(10.0, 5.0));
    }

    #[test]
    fn sampling_keeps_endpoints_and_midpoint() {
        let samples = sample_quadratic_bezier(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 10.0),
            Point2::new(20.0, 0.0),
            4,
        );
        assert_eq!(samples.len(), 5);
        assert_eq!(samples[0], Point2::new(0.0, 0.0));
        assert_eq!(samples[4], Point2::new(20.0, 0.0));
        assert_eq!(samples[2], Point2::new(10.0, 5.0));
    }

    #[test]
    fn control_point_offsets_along_the_normal() {
        let start = Point2::new(0.0, 0.0);
        let end = Point2::new(10.0, 0.0);
        assert_eq!(
            bezier_control_for_edge(start, end, 4.0),
            Point2::new(5.0, 4.0)
        );
        assert_eq!(
            bezier_control_for_edge(start, end, 0.0),
            Point2::new(5.0, 0.0)
        );
    }

    #[test]
    fn parallel_offsets_stay_symmetric() {
        assert_eq!(parallel_offsets(1, 8.0), vec![0.0]);
        assert_eq!(parallel_offsets(2, 8.0), vec![-4.0, 4.0]);
        assert_eq!(parallel_offsets(3, 8.0), vec![-8.0, 0.0, 8.0]);
    }

    #[test]
    fn self_loop_starts_and_ends_at_the_anchor() {
        let center = Point2::new(10.0, 20.0);
        let polyline = self_loop_polyline(center, 24.0, 0);
        assert_eq!(polyline.first(), Some(&center));
        assert_eq!(polyline.last(), Some(&center));
        assert!(polyline.len() > 2);
    }

    #[test]
    fn self_loop_controls_sit_above_the_node() {
        let center = Point2::new(0.0, 0.0);
        let [left, right] = self_loop_controls(center, 24.0, 0);
        assert!(left.y < -12.0 && right.y < -12.0);
        assert!(left.x < center.x && right.x > center.x);
        let top = self_loop_polyline(center, 24.0, 0)
            .iter()
            .map(|point| point.y)
            .fold(f32::INFINITY, f32::min);
        assert!(top < -12.0);
    }

    #[test]
    fn overlapping_self_loops_grow_outward() {
        let center = Point2::ZERO;
        let near = self_loop_controls(center, 24.0, 0);
        let far = self_loop_controls(center, 24.0, 3);
        assert!(far[0].y < near[0].y && far[1].y < near[1].y);
    }
}
