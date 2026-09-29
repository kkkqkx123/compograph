//! Hit-testing predicates for pointer-driven selection.

use cg_types::{Point2, Rect};

use crate::curves::sample_quadratic_bezier;

/// Pointer tolerance for node hits, in model units.
pub const NODE_HIT_TOLERANCE: f32 = 2.0;

/// Pointer tolerance for edge hits, in model units.
pub const EDGE_HIT_TOLERANCE: f32 = 8.0;

/// Sample count used when approximating a Bezier edge by segments.
pub const BEZIER_HIT_SAMPLES: usize = 16;

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

/// True when `point` falls inside the square node body around `center`.
///
/// The body half extent grows by `tolerance` so near misses still select.
pub fn point_hits_node(point: Point2, center: Point2, half_extent: f32, tolerance: f32) -> bool {
    let half = half_extent + tolerance;
    (point.x - center.x).abs() <= half && (point.y - center.y).abs() <= half
}

/// Index of the nearest candidate to `point`, if any candidate exists.
///
/// Candidates stay index-agnostic so this crate never names graph identifiers.
pub fn nearest_point_index(point: Point2, candidates: &[Point2]) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let distance = (*candidate - point).length_squared();
        let replace = best.map(|(_, known)| distance < known).unwrap_or(true);
        if replace {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

/// Shortest distance from `point` to the polyline through `points`.
///
/// Returns none for an empty polyline and the endpoint distance for a single
/// point, so callers never divide by an implicit segment count.
pub fn distance_to_polyline(point: Point2, points: &[Point2]) -> Option<f32> {
    let mut head = points.first()?;
    if points.len() == 1 {
        return Some((*head - point).length());
    }
    let mut best = f32::INFINITY;
    for next in points.iter().skip(1) {
        best = best.min(distance_to_segment(point, *head, *next));
        head = next;
    }
    Some(best)
}

/// Shortest distance from `point` to a quadratic Bezier edge.
///
/// The curve is discretised into [`BEZIER_HIT_SAMPLES`] segments, matching the
/// sampling strategy of the reference implementation.
pub fn distance_to_bezier(start: Point2, ctrl: Point2, end: Point2, point: Point2) -> f32 {
    let samples = sample_quadratic_bezier(start, ctrl, end, BEZIER_HIT_SAMPLES);
    distance_to_polyline(point, &samples).unwrap_or(f32::INFINITY)
}

/// True when `point` lies within `tolerance` of the straight edge `a`-`b`.
pub fn edge_hit(point: Point2, a: Point2, b: Point2, tolerance: f32) -> bool {
    distance_to_segment(point, a, b) <= tolerance
}

/// True when `point` lies within `tolerance` of the Bezier edge.
pub fn bezier_hit(start: Point2, ctrl: Point2, end: Point2, point: Point2, tolerance: f32) -> bool {
    distance_to_bezier(start, ctrl, end, point) <= tolerance
}

/// True when the polyline through `points` touches `rect`.
///
/// A hit means either a sample point inside the rectangle or a segment
/// crossing its border, so curved edges flattened into polylines reuse the
/// same box-selection path as straight edges.
pub fn polyline_intersects_rect(points: &[Point2], rect: Rect) -> bool {
    if points.is_empty() {
        return false;
    }
    if points.iter().any(|point| rect.contains(*point)) {
        return true;
    }
    points
        .windows(2)
        .any(|pair| segment_intersects_rect(pair[0], pair[1], rect))
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

    #[test]
    fn node_hit_expands_body_by_tolerance() {
        let center = Point2::new(10.0, 10.0);
        assert!(point_hits_node(
            Point2::new(10.0, 10.0),
            center,
            12.0,
            NODE_HIT_TOLERANCE
        ));
        assert!(point_hits_node(
            Point2::new(23.0, 10.0),
            center,
            12.0,
            NODE_HIT_TOLERANCE
        ));
        assert!(!point_hits_node(
            Point2::new(40.0, 10.0),
            center,
            12.0,
            NODE_HIT_TOLERANCE
        ));
    }

    #[test]
    fn nearest_candidate_prefers_the_closer_point() {
        let candidates = vec![Point2::new(0.0, 0.0), Point2::new(9.0, 0.0)];
        assert_eq!(
            nearest_point_index(Point2::new(7.0, 0.0), &candidates),
            Some(1)
        );
        assert_eq!(nearest_point_index(Point2::new(0.0, 0.0), &[]), None);
    }

    #[test]
    fn polyline_distance_covers_empty_and_degenerate_cases() {
        let point = Point2::new(3.0, 4.0);
        assert_eq!(distance_to_polyline(point, &[]), None);
        assert_eq!(
            distance_to_polyline(point, &[Point2::new(0.0, 0.0)]),
            Some(5.0)
        );
        let polyline = vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)];
        assert_eq!(
            distance_to_polyline(Point2::new(5.0, 2.0), &polyline),
            Some(2.0)
        );
    }

    #[test]
    fn edge_and_bezier_hits_follow_tolerance() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(10.0, 0.0);
        assert!(edge_hit(Point2::new(5.0, 2.0), a, b, EDGE_HIT_TOLERANCE));
        assert!(!edge_hit(Point2::new(5.0, 20.0), a, b, EDGE_HIT_TOLERANCE));
        let ctrl = Point2::new(5.0, 10.0);
        assert!(bezier_hit(
            a,
            ctrl,
            b,
            Point2::new(5.0, 5.0),
            EDGE_HIT_TOLERANCE
        ));
        assert!(!bezier_hit(
            a,
            ctrl,
            b,
            Point2::new(5.0, -10.0),
            EDGE_HIT_TOLERANCE
        ));
    }

    #[test]
    fn polyline_rect_hit_covers_inside_crossing_and_empty() {
        let rect = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
        let crossing = vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)];
        assert!(polyline_intersects_rect(&crossing, rect));
        let inside = vec![Point2::new(4.5, 0.0), Point2::new(5.0, 0.5)];
        assert!(polyline_intersects_rect(&inside, rect));
        let outside = vec![Point2::new(0.0, 5.0), Point2::new(10.0, 5.0)];
        assert!(!polyline_intersects_rect(&outside, rect));
        assert!(!polyline_intersects_rect(&[], rect));
    }

    #[test]
    fn self_loop_polyline_has_finite_hit_distance() {
        use crate::curves::self_loop_polyline;

        let center = Point2::new(0.0, 0.0);
        let polyline = self_loop_polyline(center, 24.0, 0);
        let near = distance_to_polyline(Point2::new(0.0, -20.0), &polyline);
        assert!(near.map(|distance| distance.is_finite()).unwrap_or(false));
        let far = distance_to_polyline(Point2::new(0.0, 200.0), &polyline);
        assert!(
            far.map(|distance| distance > EDGE_HIT_TOLERANCE)
                .unwrap_or(false)
        );
    }
}
