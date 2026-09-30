//! Deterministic edge aggregation for dense graphs.
//!
//! Two shapes replace per-edge Bezier geometry when a bundle grows dense:
//! haystack lines with hashed fan-out and orthogonal polylines with at most
//! two bends. Both stay deterministic so repeated renders agree exactly.

use cg_types::{Point2, Vec2};

/// Radius of the haystack fan around a node anchor, in node-side multiples.
pub const HAYSTACK_RADIUS_SCALE: f32 = 0.9;

/// Extra per-edge spread inside the fan, in model units.
pub const HAYSTACK_SPREAD: f32 = 6.0;

/// Bundle size from which haystack fan-out is well-defined.
///
/// This is the shape-level floor used when a caller forces haystack mode.
/// The density policy deciding when bundles switch lives in the render layer
/// and keeps its own higher threshold; the two must not be merged.
pub const HAYSTACK_BUNDLE_THRESHOLD: usize = 4;

/// Maximum bends of an orthogonal polyline.
pub const ORTHO_MAX_BENDS: usize = 2;

/// Preferred routing direction of an orthogonal edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrthoDirection {
    /// Route along the longer endpoint delta.
    #[default]
    Auto,
    /// Leave horizontally first.
    HorizontalFirst,
    /// Leave vertically first.
    VerticalFirst,
}

/// Deterministic fan direction of one haystack edge.
///
/// The angle derives from hashing both endpoint identifiers, so the same edge
/// always fans the same way without runtime randomness.
pub fn haystack_angle(source: u32, target: u32, ordinal: u32) -> f32 {
    let mut hash = (source as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    hash ^= (target as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash ^= (ordinal as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(0x85EB_CA6B);
    let unit = ((hash >> 11) % 1_000_000) as f32 / 1_000_000.0;
    unit * std::f32::consts::TAU
}

/// Haystack endpoints fanned around the node anchors.
///
/// Each endpoint stays within `radius` of its anchor, offset along the hashed
/// fan direction plus a small ordinal spread so parallel edges do not overlap.
pub fn haystack_endpoints(
    source: Point2,
    target: Point2,
    source_id: u32,
    target_id: u32,
    ordinal: u32,
    node_side: f32,
) -> (Point2, Point2) {
    let radius = node_side.max(1.0) * HAYSTACK_RADIUS_SCALE;
    let angle_a = haystack_angle(source_id, target_id, ordinal);
    let angle_b = haystack_angle(target_id, source_id, ordinal);
    let spread_a = (ordinal as f32) * HAYSTACK_SPREAD / 4.0;
    let spread_b = (ordinal as f32) * HAYSTACK_SPREAD / 4.0;
    let offset_a = Vec2::new(angle_a.cos(), angle_a.sin()) * (radius * 0.5 + spread_a * 0.25);
    let offset_b = Vec2::new(angle_b.cos(), angle_b.sin()) * (radius * 0.5 + spread_b * 0.25);
    let clamped_a = clamp_offset(offset_a, radius);
    let clamped_b = clamp_offset(offset_b, radius);
    (source + clamped_a, target + clamped_b)
}

fn clamp_offset(offset: Vec2, radius: f32) -> Vec2 {
    let length = offset.length();
    if length > radius && length > 0.0 {
        offset * (radius / length)
    } else {
        offset
    }
}

/// Orthogonal polyline from `start` to `end` with at most two bends.
///
/// The returned points always start with `start` and end with `end`; interior
/// points are the bends. Auto mode routes along the longer delta.
pub fn ortho_polyline(start: Point2, end: Point2, direction: OrthoDirection) -> Vec<Point2> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx == 0.0 || dy == 0.0 {
        return vec![start, end];
    }
    let horizontal_first = match direction {
        OrthoDirection::HorizontalFirst => true,
        OrthoDirection::VerticalFirst => false,
        OrthoDirection::Auto => dx.abs() >= dy.abs(),
    };
    if horizontal_first {
        let mid_x = (start.x + end.x) / 2.0;
        vec![
            start,
            Point2::new(mid_x, start.y),
            Point2::new(mid_x, end.y),
            end,
        ]
    } else {
        let mid_y = (start.y + end.y) / 2.0;
        vec![
            start,
            Point2::new(start.x, mid_y),
            Point2::new(end.x, mid_y),
            end,
        ]
    }
}

/// True when a bundle of `count` edges should use haystack rendering.
pub fn use_haystack(count: usize, forced: bool) -> bool {
    forced || count >= HAYSTACK_BUNDLE_THRESHOLD
}

/// Single-corner Manhattan route from `start` to `end`.
///
/// Returns `start`, one axis-aligned corner, and `end`; degenerate axes fall
/// back to a straight segment. Auto mode leaves along the longer delta,
/// matching [`ortho_polyline`]. The paint plan stores the corner as its first
/// bend and hit testing flattens the same points, so drawing and selection
/// share one sampling.
pub fn taxi_polyline(start: Point2, end: Point2, direction: OrthoDirection) -> Vec<Point2> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx == 0.0 || dy == 0.0 {
        return vec![start, end];
    }
    let horizontal_first = match direction {
        OrthoDirection::HorizontalFirst => true,
        OrthoDirection::VerticalFirst => false,
        OrthoDirection::Auto => dx.abs() >= dy.abs(),
    };
    let corner = if horizontal_first {
        Point2::new(end.x, start.y)
    } else {
        Point2::new(start.x, end.y)
    };
    vec![start, corner, end]
}

/// Manhattan route points selected by the paint options, cleaned for flattening.
///
/// Taxi wins when both directions are set, matching the paint plan. Returns
/// an empty vector when neither route is selected so callers keep their
/// default straight or curved path. The paint plan and box selection both run
/// through this selector, so drawing and selection flatten identical points.
pub fn manhattan_route(
    start: Point2,
    end: Point2,
    ortho: Option<OrthoDirection>,
    taxi: Option<OrthoDirection>,
) -> Vec<Point2> {
    let line = if let Some(direction) = taxi {
        taxi_polyline(start, end, direction)
    } else if let Some(direction) = ortho {
        ortho_polyline(start, end, direction)
    } else {
        return Vec::new();
    };
    let via: Vec<Point2> = line
        .iter()
        .skip(1)
        .take(line.len().saturating_sub(2))
        .copied()
        .collect();
    segmented_polyline(start, &via, end)
}

/// Polyline through `start`, cleaned `via` points, and `end`.
///
/// Non-finite waypoints are dropped and consecutive duplicates collapse, so
/// callers can forward raw waypoint lists without pre-scrubbing. Both
/// endpoints are always kept; an empty waypoint list yields the straight
/// segment. The paint plan runs its taxi and orthogonal bends through this
/// builder and hit testing flattens the same output.
pub fn segmented_polyline(start: Point2, via: &[Point2], end: Point2) -> Vec<Point2> {
    let mut line = Vec::with_capacity(via.len() + 2);
    line.push(start);
    for point in via {
        if !point.x.is_finite() || !point.y.is_finite() {
            continue;
        }
        if *point != line[line.len() - 1] {
            line.push(*point);
        }
    }
    if end != line[line.len() - 1] {
        line.push(end);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haystack_stays_near_the_anchors() {
        let source = Point2::new(0.0, 0.0);
        let target = Point2::new(100.0, 0.0);
        let (a, b) = haystack_endpoints(source, target, 3, 7, 1, 24.0);
        let radius = 24.0 * HAYSTACK_RADIUS_SCALE + 1.0;
        assert!((a - source).length() <= radius);
        assert!((b - target).length() <= radius);
    }

    #[test]
    fn haystack_is_deterministic() {
        let source = Point2::new(0.0, 0.0);
        let target = Point2::new(50.0, 20.0);
        let first = haystack_endpoints(source, target, 3, 7, 2, 24.0);
        let second = haystack_endpoints(source, target, 3, 7, 2, 24.0);
        assert_eq!(first, second);
    }

    #[test]
    fn haystack_spreads_parallel_edges() {
        let source = Point2::ZERO;
        let target = Point2::new(100.0, 0.0);
        let (a0, _) = haystack_endpoints(source, target, 1, 2, 0, 24.0);
        let (a1, _) = haystack_endpoints(source, target, 1, 2, 1, 24.0);
        assert_ne!(a0, a1);
    }

    #[test]
    fn ortho_polyline_caps_bends() {
        let line = ortho_polyline(
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 40.0),
            OrthoDirection::Auto,
        );
        assert!(line.len() <= ORTHO_MAX_BENDS + 2);
        assert_eq!(line.first(), Some(&Point2::new(0.0, 0.0)));
        assert_eq!(line.last(), Some(&Point2::new(100.0, 40.0)));
        let vertical = ortho_polyline(
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 40.0),
            OrthoDirection::VerticalFirst,
        );
        assert_eq!(vertical[1], Point2::new(0.0, 20.0));
    }

    #[test]
    fn ortho_degenerate_axis_stays_straight() {
        let line = ortho_polyline(
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 50.0),
            OrthoDirection::Auto,
        );
        assert_eq!(line.len(), 2);
    }

    #[test]
    fn haystack_threshold_switches_bundles() {
        assert!(!use_haystack(3, false));
        assert!(use_haystack(4, false));
        assert!(use_haystack(1, true));
    }

    #[test]
    fn taxi_corners_stay_axis_aligned() {
        let start = Point2::new(0.0, 0.0);
        let end = Point2::new(100.0, 40.0);
        assert_eq!(
            taxi_polyline(start, end, OrthoDirection::HorizontalFirst),
            vec![start, Point2::new(100.0, 0.0), end]
        );
        assert_eq!(
            taxi_polyline(start, end, OrthoDirection::VerticalFirst),
            vec![start, Point2::new(0.0, 40.0), end]
        );
        assert_eq!(
            taxi_polyline(start, end, OrthoDirection::Auto),
            vec![start, Point2::new(100.0, 0.0), end]
        );
        let tall = taxi_polyline(start, Point2::new(10.0, 90.0), OrthoDirection::Auto);
        assert_eq!(tall[1], Point2::new(0.0, 90.0));
    }

    #[test]
    fn taxi_degenerate_axes_stay_straight() {
        let line = taxi_polyline(
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 50.0),
            OrthoDirection::Auto,
        );
        assert_eq!(line.len(), 2);
    }

    #[test]
    fn manhattan_route_prefers_taxi_and_stays_empty_without_options() {
        let start = Point2::new(0.0, 0.0);
        let end = Point2::new(100.0, 40.0);
        assert!(manhattan_route(start, end, None, None).is_empty());
        assert_eq!(
            manhattan_route(start, end, None, Some(OrthoDirection::HorizontalFirst)),
            vec![start, Point2::new(100.0, 0.0), end]
        );
        assert_eq!(
            manhattan_route(
                start,
                end,
                Some(OrthoDirection::VerticalFirst),
                Some(OrthoDirection::HorizontalFirst)
            ),
            vec![start, Point2::new(100.0, 0.0), end]
        );
        let ortho = manhattan_route(start, end, Some(OrthoDirection::Auto), None);
        assert_eq!(ortho.len(), 4);
    }

    #[test]
    fn segmented_polyline_cleans_waypoints_but_keeps_endpoints() {
        let start = Point2::new(0.0, 0.0);
        let end = Point2::new(100.0, 0.0);
        assert_eq!(segmented_polyline(start, &[], end), vec![start, end]);
        let cleaned = segmented_polyline(
            start,
            &[
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(f32::NAN, 0.0),
                Point2::new(50.0, 10.0),
            ],
            end,
        );
        assert_eq!(
            cleaned,
            vec![start, Point2::new(10.0, 0.0), Point2::new(50.0, 10.0), end]
        );
    }
}
