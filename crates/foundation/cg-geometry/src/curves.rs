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
}
