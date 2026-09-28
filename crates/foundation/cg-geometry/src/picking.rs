//! Hit-testing predicates for pointer-driven selection.

use cg_types::{Point2, Rect};

/// Shortest distance from `point` to the segment between `a` and `b`.
///
/// Falls back to the point distance when the segment degenerates.
pub fn distance_to_segment(point: Point2, a: Point2, b: Point2) -> f32 {
    let ab = b - a;
    let length_squared = ab.length_squared();
    if length_squared <= f32::EPSILON {
        return (point - a).length();
    }
    let t = (((point - a).x * ab.x + (point - a).y * ab.y) / length_squared).clamp(0.0, 1.0);
    let closest = a + ab * t;
    (point - closest).length()
}

/// True when the segment between `a` and `b` intersects the rectangle.
///
/// Uses the separating-axis test on the four rectangle edges.
pub fn segment_intersects_rect(a: Point2, b: Point2, rect: Rect) -> bool {
    if rect.contains(a) || rect.contains(b) {
        return true;
    }
    let min = rect.origin;
    let max = rect.origin + rect.size;
    let corners = [
        min,
        Point2::new(max.x, min.y),
        max,
        Point2::new(min.x, max.y),
    ];
    corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .any(|(u, v)| segments_intersect(a, b, *u, *v))
}

/// True when segments `a1`-`a2` and `b1`-`b2` cross each other.
pub fn segments_intersect(a1: Point2, a2: Point2, b1: Point2, b2: Point2) -> bool {
    let d1 = cross(b1, b2, a1);
    let d2 = cross(b1, b2, a2);
    let d3 = cross(a1, a2, b1);
    let d4 = cross(a1, a2, b2);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    d1 == 0.0 && on_segment(b1, b2, a1)
        || d2 == 0.0 && on_segment(b1, b2, a2)
        || d3 == 0.0 && on_segment(a1, a2, b1)
        || d4 == 0.0 && on_segment(a1, a2, b2)
}

fn cross(origin: Point2, a: Point2, b: Point2) -> f32 {
    let u = a - origin;
    let v = b - origin;
    u.x * v.y - u.y * v.x
}

fn on_segment(a: Point2, b: Point2, point: Point2) -> bool {
    point.x >= a.x.min(b.x)
        && point.x <= a.x.max(b.x)
        && point.y >= a.y.min(b.y)
        && point.y <= a.y.max(b.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_types::Vec2;

    #[test]
    fn distance_to_segment_clamps_to_endpoints() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(10.0, 0.0);
        assert_eq!(distance_to_segment(Point2::new(-3.0, 0.0), a, b), 3.0);
        assert_eq!(distance_to_segment(Point2::new(14.0, 0.0), a, b), 4.0);
        assert_eq!(distance_to_segment(Point2::new(5.0, 2.0), a, b), 2.0);
    }

    #[test]
    fn box_selection_catches_crossing_segment() {
        let rect = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
        assert!(segment_intersects_rect(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            rect
        ));
        assert!(!segment_intersects_rect(
            Point2::new(0.0, 5.0),
            Point2::new(10.0, 5.0),
            rect
        ));
    }
}
