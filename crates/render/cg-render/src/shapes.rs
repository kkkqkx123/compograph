//! Node body shapes shared by paint plans and hit testing.
//!
//! Vertex tables are framework independent relative coordinates centered at
//! the origin. Drawing and hit testing consume the same tables so a shape
//! selects exactly where it paints. The square keeps its fast rectangle path
//! and its hit test doubles as the conservative outer envelope for the other
//! shapes.

use cg_geometry::{
    distance_to_segment, point_hits_node, point_in_polygon, polygon_intersects_rect,
};
use cg_types::{Point2, Rect, Vec2};

/// Body shape of a node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeShape {
    /// Axis aligned square, the historical default.
    #[default]
    Square,
    /// Inscribed circle.
    Circle,
    /// Axis aligned ellipse wider than tall.
    Ellipse,
    /// Rectangle with rounded corners.
    RoundedRect,
    /// Upward pointing triangle.
    Triangle,
    /// Four point diamond.
    Diamond,
    /// Regular pentagon pointing up.
    Pentagon,
    /// Regular hexagon pointing up.
    Hexagon,
    /// Regular octagon pointing up.
    Octagon,
    /// Five point star pointing up.
    Star,
}

/// Shape used when the detail level cannot afford polygons.
pub fn shape_for_level(shape: NodeShape, minimal: bool) -> NodeShape {
    if minimal { NodeShape::Square } else { shape }
}

/// Relative vertices of `shape` for a body of `side` length.
///
/// Vertices wind counterclockwise around the origin without repeating the
/// first point. The square returns its four corners; the circle and ellipse
/// sample their outline; the rounded rectangle walks its edges with corner
/// arcs; the triangle and diamond return their corners.
pub fn node_polygon(shape: NodeShape, side: f32) -> Vec<Point2> {
    let side = side.max(4.0);
    let half = side / 2.0;
    match shape {
        NodeShape::Square => vec![
            Point2::new(-half, -half),
            Point2::new(half, -half),
            Point2::new(half, half),
            Point2::new(-half, half),
        ],
        NodeShape::Circle => circle_points(half, 24),
        NodeShape::Ellipse => ellipse_points(half, half * 0.62, 24),
        NodeShape::RoundedRect => rounded_rect_points(half, 4),
        NodeShape::Triangle => (0..3)
            .map(|ordinal| {
                let angle = -std::f32::consts::FRAC_PI_2
                    + ordinal as f32 * 2.0 * std::f32::consts::PI / 3.0;
                Point2::new(angle.cos() * half, angle.sin() * half)
            })
            .collect(),
        NodeShape::Diamond => vec![
            Point2::new(0.0, -half),
            Point2::new(half, 0.0),
            Point2::new(0.0, half),
            Point2::new(-half, 0.0),
        ],
        NodeShape::Pentagon => regular_polygon(5, half),
        NodeShape::Hexagon => regular_polygon(6, half),
        NodeShape::Octagon => regular_polygon(8, half),
        NodeShape::Star => star_points(half),
    }
}

/// True when `point` selects the body of `shape` around `center`.
///
/// The square test doubles as the conservative outer envelope: points outside
/// the square plus tolerance miss every shape without further checks. Circles
/// and ellipses use their radial equations; the remaining shapes use the ray
/// cast over their shared vertex tables.
pub fn point_hits_shape(
    shape: NodeShape,
    point: Point2,
    center: Point2,
    half: f32,
    tolerance: f32,
) -> bool {
    if !point_hits_node(point, center, half, tolerance) {
        return false;
    }
    match shape {
        NodeShape::Square | NodeShape::RoundedRect => true,
        NodeShape::Circle => {
            let delta = point - center;
            delta.length() <= half + tolerance
        }
        NodeShape::Ellipse => {
            let rx = half + tolerance;
            let ry = half * 0.62 + tolerance;
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let dx = (point.x - center.x) / rx;
            let dy = (point.y - center.y) / ry;
            dx * dx + dy * dy <= 1.0
        }
        NodeShape::Triangle
        | NodeShape::Diamond
        | NodeShape::Pentagon
        | NodeShape::Hexagon
        | NodeShape::Octagon
        | NodeShape::Star => {
            let side = half * 2.0;
            let relative: Vec<Point2> = node_polygon(shape, side)
                .into_iter()
                .map(|vertex| Point2::new(vertex.x + center.x, vertex.y + center.y))
                .collect();
            if point_in_polygon(point, &relative) {
                return true;
            }
            distance_to_polygon(point, &relative) <= tolerance
        }
    }
}

/// True when the body of `shape` around `center` touches `rect`.
///
/// The square envelope rejects exactly: every body is inscribed in it, so an
/// envelope miss misses every shape. Squares and rounded rectangles decide by
/// rectangle overlap, matching their point hit test; the remaining shapes use
/// the shared vertex tables, so a shape selects exactly where it paints.
pub fn shape_hits_rect(shape: NodeShape, center: Point2, half: f32, rect: Rect) -> bool {
    let body = Rect::new(
        Point2::new(center.x - half, center.y - half),
        Vec2::new(half * 2.0, half * 2.0),
    );
    if !body.intersects(rect) {
        return false;
    }
    match shape {
        NodeShape::Square | NodeShape::RoundedRect => true,
        _ => {
            let vertices: Vec<Point2> = node_polygon(shape, half * 2.0)
                .into_iter()
                .map(|vertex| Point2::new(vertex.x + center.x, vertex.y + center.y))
                .collect();
            polygon_intersects_rect(&vertices, rect)
        }
    }
}

fn circle_points(radius: f32, segments: usize) -> Vec<Point2> {
    (0..segments)
        .map(|ordinal| {
            let angle = ordinal as f32 * 2.0 * std::f32::consts::PI / segments as f32;
            Point2::new(angle.cos() * radius, angle.sin() * radius)
        })
        .collect()
}

fn regular_polygon(sides: usize, radius: f32) -> Vec<Point2> {
    (0..sides)
        .map(|ordinal| {
            let angle = -std::f32::consts::FRAC_PI_2
                + ordinal as f32 * 2.0 * std::f32::consts::PI / sides as f32;
            Point2::new(angle.cos() * radius, angle.sin() * radius)
        })
        .collect()
}

fn star_points(outer: f32) -> Vec<Point2> {
    let inner = outer * 0.45;
    (0..10)
        .map(|ordinal| {
            let radius = if ordinal % 2 == 0 { outer } else { inner };
            let angle = -std::f32::consts::FRAC_PI_2 + ordinal as f32 * std::f32::consts::PI / 5.0;
            Point2::new(angle.cos() * radius, angle.sin() * radius)
        })
        .collect()
}

fn ellipse_points(rx: f32, ry: f32, segments: usize) -> Vec<Point2> {
    (0..segments)
        .map(|ordinal| {
            let angle = ordinal as f32 * 2.0 * std::f32::consts::PI / segments as f32;
            Point2::new(angle.cos() * rx, angle.sin() * ry)
        })
        .collect()
}

fn rounded_rect_points(half: f32, arc_steps: usize) -> Vec<Point2> {
    let radius = (half * 0.4).min(half);
    let straight = half - radius;
    let corners = [
        (Point2::new(straight, straight), 0.0),
        (
            Point2::new(-straight, straight),
            std::f32::consts::FRAC_PI_2,
        ),
        (Point2::new(-straight, -straight), std::f32::consts::PI),
        (
            Point2::new(straight, -straight),
            3.0 * std::f32::consts::FRAC_PI_2,
        ),
    ];
    let mut points = Vec::new();
    for (center, base) in corners {
        for step in 0..=arc_steps {
            let angle = base + step as f32 * std::f32::consts::FRAC_PI_2 / arc_steps as f32;
            points.push(Point2::new(
                center.x + angle.cos() * radius,
                center.y + angle.sin() * radius,
            ));
        }
    }
    points
}

fn distance_to_polygon(point: Point2, vertices: &[Point2]) -> f32 {
    if vertices.is_empty() {
        return f32::INFINITY;
    }
    let mut best = f32::INFINITY;
    let mut previous = vertices[vertices.len() - 1];
    for current in vertices {
        best = best.min(distance_to_segment(point, *current, previous));
        previous = *current;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygons_close_and_mirror_cleanly() {
        for shape in [
            NodeShape::Square,
            NodeShape::Circle,
            NodeShape::Ellipse,
            NodeShape::RoundedRect,
            NodeShape::Triangle,
            NodeShape::Diamond,
            NodeShape::Pentagon,
            NodeShape::Hexagon,
            NodeShape::Octagon,
            NodeShape::Star,
        ] {
            let vertices = node_polygon(shape, 24.0);
            assert!(vertices.len() >= 3);
            assert!(vertices.first() != vertices.last());
            let sum_x: f32 = vertices.iter().map(|point| point.x).sum();
            let sum_y: f32 = vertices.iter().map(|point| point.y).sum();
            assert!(sum_x.abs() < 1e-2, "{shape:?} mirrors on x");
            assert!(sum_y.abs() < 1e-2, "{shape:?} mirrors on y");
        }
        assert_eq!(node_polygon(NodeShape::Square, 24.0).len(), 4);
        assert_eq!(node_polygon(NodeShape::Triangle, 24.0).len(), 3);
        assert_eq!(node_polygon(NodeShape::Diamond, 24.0).len(), 4);
        assert_eq!(node_polygon(NodeShape::Pentagon, 24.0).len(), 5);
        assert_eq!(node_polygon(NodeShape::Hexagon, 24.0).len(), 6);
        assert_eq!(node_polygon(NodeShape::Octagon, 24.0).len(), 8);
        assert_eq!(node_polygon(NodeShape::Star, 24.0).len(), 10);
    }

    #[test]
    fn default_shape_stays_square() {
        assert_eq!(NodeShape::default(), NodeShape::Square);
        assert_eq!(shape_for_level(NodeShape::Circle, true), NodeShape::Square);
        assert_eq!(
            shape_for_level(NodeShape::Diamond, false),
            NodeShape::Diamond
        );
    }

    #[test]
    fn rect_hits_agree_with_point_hits() {
        let center = Point2::ZERO;
        let over = Rect::from_corners(Point2::new(-2.0, -2.0), Point2::new(2.0, 2.0));
        let far = Rect::from_corners(Point2::new(40.0, 40.0), Point2::new(44.0, 44.0));
        for shape in [
            NodeShape::Square,
            NodeShape::Circle,
            NodeShape::Ellipse,
            NodeShape::RoundedRect,
            NodeShape::Triangle,
            NodeShape::Diamond,
            NodeShape::Pentagon,
            NodeShape::Hexagon,
            NodeShape::Octagon,
            NodeShape::Star,
        ] {
            assert!(shape_hits_rect(shape, center, 12.0, over));
            assert!(!shape_hits_rect(shape, center, 12.0, far));
        }
        // The tight corner misses the circle arc: the inscribed outline passes
        // through (-8.49, -8.49), so a wider rect would overlap the painted
        // body and selecting it would be correct. The square still covers the
        // corner, matching the tap behavior the test mirrors.
        let corner = Rect::from_corners(Point2::new(-12.0, -12.0), Point2::new(-11.0, -11.0));
        assert!(shape_hits_rect(NodeShape::Square, center, 12.0, corner));
        assert!(!shape_hits_rect(NodeShape::Triangle, center, 12.0, corner));
        assert!(!shape_hits_rect(NodeShape::Circle, center, 12.0, corner));
    }

    #[test]
    fn hits_accept_inside_and_reject_outside() {
        let center = Point2::new(0.0, 0.0);
        for shape in [
            NodeShape::Square,
            NodeShape::Circle,
            NodeShape::Ellipse,
            NodeShape::RoundedRect,
            NodeShape::Triangle,
            NodeShape::Diamond,
            NodeShape::Pentagon,
            NodeShape::Hexagon,
            NodeShape::Octagon,
            NodeShape::Star,
        ] {
            assert!(point_hits_shape(shape, center, center, 12.0, 2.0));
            assert!(!point_hits_shape(
                shape,
                Point2::new(40.0, 40.0),
                center,
                12.0,
                2.0
            ));
        }
        assert!(!point_hits_shape(
            NodeShape::Circle,
            Point2::new(11.0, 11.0),
            center,
            12.0,
            0.0
        ));
        assert!(point_hits_shape(
            NodeShape::Triangle,
            Point2::new(0.0, -6.0),
            center,
            12.0,
            2.0
        ));
    }
}
