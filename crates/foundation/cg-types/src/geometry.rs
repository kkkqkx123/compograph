//! Points, vectors and rectangles used in both model space and screen space.

use std::ops::{Add, Mul, Sub};

/// A position expressed in two dimensions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point2 {
    pub x: f32,
    pub y: f32,
}

impl Point2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl Add<Vec2> for Point2 {
    type Output = Point2;

    fn add(self, rhs: Vec2) -> Point2 {
        Point2::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Point2 {
    type Output = Vec2;

    fn sub(self, rhs: Point2) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

/// A displacement between two positions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Euclidean length of the displacement.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Squared Euclidean length, avoiding the square root for comparisons.
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: f32) -> Vec2 {
        Vec2::new(self.x * rhs, self.y * rhs)
    }
}

/// An axis-aligned rectangle described by its top-left corner and extent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub origin: Point2,
    pub size: Vec2,
}

impl Rect {
    pub const fn new(origin: Point2, size: Vec2) -> Self {
        Self { origin, size }
    }

    /// True when the point lies inside the rectangle, borders included.
    pub fn contains(self, point: Point2) -> bool {
        point.x >= self.origin.x
            && point.x <= self.origin.x + self.size.x
            && point.y >= self.origin.y
            && point.y <= self.origin.y + self.size.y
    }

    /// Builds the normalized rectangle spanning two corner points.
    ///
    /// Drag corners arrive in any order, so the origin takes the minima and
    /// the size the absolute extents; the result always has non-negative size.
    pub fn from_corners(a: Point2, b: Point2) -> Self {
        let origin = Point2::new(a.x.min(b.x), a.y.min(b.y));
        let size = Vec2::new((a.x - b.x).abs(), (a.y - b.y).abs());
        Self { origin, size }
    }

    /// True when the two rectangles overlap, borders included.
    pub fn intersects(self, other: Rect) -> bool {
        let self_far = Point2::new(self.origin.x + self.size.x, self.origin.y + self.size.y);
        let other_far = Point2::new(other.origin.x + other.size.x, other.origin.y + other.size.y);
        self.origin.x <= other_far.x
            && other.origin.x <= self_far.x
            && self.origin.y <= other_far.y
            && other.origin.y <= self_far.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_border_points() {
        let rect = Rect::new(Point2::new(0.0, 0.0), Vec2::new(10.0, 10.0));
        assert!(rect.contains(Point2::new(5.0, 5.0)));
        assert!(rect.contains(Point2::new(10.0, 10.0)));
        assert!(!rect.contains(Point2::new(11.0, 5.0)));
    }

    #[test]
    fn rect_from_corners_normalizes_drag_order() {
        let rect = Rect::from_corners(Point2::new(8.0, 2.0), Point2::new(3.0, 9.0));
        assert_eq!(rect.origin, Point2::new(3.0, 2.0));
        assert_eq!(rect.size, Vec2::new(5.0, 7.0));
    }

    #[test]
    fn rect_intersects_edges_and_gaps() {
        let base = Rect::new(Point2::new(0.0, 0.0), Vec2::new(10.0, 10.0));
        let touching = Rect::new(Point2::new(10.0, 0.0), Vec2::new(5.0, 5.0));
        let apart = Rect::new(Point2::new(11.0, 0.0), Vec2::new(5.0, 5.0));
        assert!(base.intersects(touching));
        assert!(!base.intersects(apart));
    }

    #[test]
    fn vector_length_matches_hypotenuse() {
        assert_eq!(Vec2::new(3.0, 4.0).length(), 5.0);
    }

    #[test]
    fn point_displacement_composes_with_scaling() {
        let moved = Point2::new(1.0, 1.0) + Vec2::new(2.0, 3.0) * 2.0;
        assert_eq!(moved, Point2::new(5.0, 7.0));
    }
}
