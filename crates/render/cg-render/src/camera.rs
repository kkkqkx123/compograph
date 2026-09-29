//! Viewport mapping between model coordinates and screen pixels.

use cg_types::{Point2, Vec2};

/// Pan and zoom state of the graph canvas.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Model-space point displayed at the center of the viewport.
    pub center: Point2,
    /// Scale factor from model units to logical pixels.
    pub zoom: f32,
}

/// Smallest allowed zoom, keeping the graph from collapsing to a dot.
pub const MIN_ZOOM: f32 = 0.1;

/// Largest allowed zoom, keeping single nodes from filling the screen.
pub const MAX_ZOOM: f32 = 8.0;

impl Camera {
    pub fn new(center: Point2, zoom: f32) -> Self {
        Self { center, zoom }
    }

    /// Maps a model-space point to viewport-relative pixels.
    pub fn world_to_viewport(&self, viewport_size: Vec2, point: Point2) -> Point2 {
        let half = viewport_size * 0.5;
        let offset = point - self.center;
        Point2::new(half.x + offset.x * self.zoom, half.y + offset.y * self.zoom)
    }

    /// Inverse of [`Camera::world_to_viewport`], used by hit testing.
    pub fn viewport_to_world(&self, viewport_size: Vec2, point: Point2) -> Point2 {
        let half = viewport_size * 0.5;
        let offset = Vec2::new(point.x - half.x, point.y - half.y);
        Point2::new(
            self.center.x + offset.x / self.zoom,
            self.center.y + offset.y / self.zoom,
        )
    }

    /// Zooms around a viewport anchor so the model point under the cursor stays put.
    ///
    /// The factor multiplies the current zoom and the result is clamped to the
    /// supported range; a non-positive factor leaves the camera untouched.
    pub fn zoom_at(&mut self, viewport_size: Vec2, anchor: Point2, factor: f32) {
        if factor <= 0.0 || !factor.is_finite() {
            return;
        }
        let world_before = self.viewport_to_world(viewport_size, anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let half = viewport_size * 0.5;
        let offset = Vec2::new(anchor.x - half.x, anchor.y - half.y);
        self.center = Point2::new(
            world_before.x - offset.x / self.zoom,
            world_before.y - offset.y / self.zoom,
        );
    }

    /// Moves the visible center by a model-space displacement.
    pub fn pan_by(&mut self, delta: Vec2) {
        self.center = self.center + delta;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_mapping_round_trips_through_world() {
        let camera = Camera::new(Point2::new(10.0, 20.0), 1.5);
        let size = Vec2::new(800.0, 600.0);
        let world = Point2::new(42.0, -7.0);
        let viewport = camera.world_to_viewport(size, world);
        let back = camera.viewport_to_world(size, viewport);
        assert!((world.x - back.x).abs() < 1e-4);
        assert!((world.y - back.y).abs() < 1e-4);
    }

    #[test]
    fn zoom_scales_model_offsets() {
        let camera = Camera::new(Point2::ZERO, 2.0);
        let size = Vec2::new(100.0, 100.0);
        let viewport = camera.world_to_viewport(size, Point2::new(5.0, 0.0));
        assert_eq!(viewport, Point2::new(60.0, 50.0));
    }

    #[test]
    fn zoom_anchor_keeps_the_cursor_model_point_still() {
        let mut camera = Camera::new(Point2::ZERO, 1.0);
        let size = Vec2::new(800.0, 600.0);
        let anchor = Point2::new(500.0, 200.0);
        let before = camera.viewport_to_world(size, anchor);
        camera.zoom_at(size, anchor, 2.0);
        let after = camera.viewport_to_world(size, anchor);
        assert!((before.x - after.x).abs() < 1e-4);
        assert!((before.y - after.y).abs() < 1e-4);
        assert_eq!(camera.zoom, 2.0);
    }

    #[test]
    fn zoom_clamps_to_the_supported_range() {
        let mut camera = Camera::new(Point2::ZERO, 1.0);
        let size = Vec2::new(100.0, 100.0);
        camera.zoom_at(size, Point2::new(50.0, 50.0), 100.0);
        assert_eq!(camera.zoom, MAX_ZOOM);
        camera.zoom_at(size, Point2::new(50.0, 50.0), 0.0);
        assert_eq!(camera.zoom, MAX_ZOOM);
    }
}
