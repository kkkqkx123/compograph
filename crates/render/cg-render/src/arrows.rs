//! Arrowhead shapes sharing one orientation rule.
//!
//! Every kind points along the edge end tangent; the dot needs only a center.
//! Vertex tables are screen pixel polygons starting with the tip, so drawing
//! and export consume the same points the plan stores.

use cg_types::Point2;

/// Head shape of a directed edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ArrowKind {
    /// Filled triangle, the historical default.
    #[default]
    Triangle,
    /// Swallowtail with a notched back edge.
    Dovetail,
    /// Perpendicular bar.
    Tee,
    /// Filled dot needing no orientation.
    Dot,
    /// Four point diamond.
    Diamond,
}

/// Screen pixel polygon of `kind` with the tip first.
///
/// `angle` points along the edge; the dot ignores it and centers on `tip`.
/// Length runs along the edge and half width across it. The tee returns its
/// four bar corners; the dot samples its outline.
pub fn arrow_polygon(
    kind: ArrowKind,
    tip: Point2,
    angle: f32,
    length: f32,
    half_width: f32,
) -> Vec<Point2> {
    let length = length.max(2.0);
    let half_width = half_width.max(1.0);
    let axis = Point2::new(angle.cos(), angle.sin());
    let normal = Point2::new(-axis.y, axis.x);
    let base = Point2::new(tip.x - axis.x * length, tip.y - axis.y * length);
    let left = Point2::new(
        base.x + normal.x * half_width,
        base.y + normal.y * half_width,
    );
    let right = Point2::new(
        base.x - normal.x * half_width,
        base.y - normal.y * half_width,
    );
    match kind {
        ArrowKind::Triangle => vec![tip, left, right],
        ArrowKind::Dovetail => {
            let notch = Point2::new(
                base.x + axis.x * length * 0.4,
                base.y + axis.y * length * 0.4,
            );
            vec![tip, left, notch, right]
        }
        ArrowKind::Tee => {
            let thickness = 2.0f32.max(length * 0.18);
            let front = Point2::new(tip.x - axis.x * thickness, tip.y - axis.y * thickness);
            let wide = half_width * 1.4;
            vec![
                Point2::new(tip.x + normal.x * wide, tip.y + normal.y * wide),
                Point2::new(tip.x - normal.x * wide, tip.y - normal.y * wide),
                Point2::new(front.x - normal.x * wide, front.y - normal.y * wide),
                Point2::new(front.x + normal.x * wide, front.y + normal.y * wide),
            ]
        }
        ArrowKind::Dot => (0..12)
            .map(|ordinal| {
                let step = ordinal as f32 * 2.0 * std::f32::consts::PI / 12.0;
                Point2::new(
                    tip.x + step.cos() * half_width,
                    tip.y + step.sin() * half_width,
                )
            })
            .collect(),
        ArrowKind::Diamond => {
            let mid = Point2::new(tip.x - axis.x * length * 0.5, tip.y - axis.y * length * 0.5);
            vec![
                tip,
                Point2::new(mid.x + normal.x * half_width, mid.y + normal.y * half_width),
                base,
                Point2::new(mid.x - normal.x * half_width, mid.y - normal.y * half_width),
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_reports_its_vertex_count() {
        let tip = Point2::ZERO;
        assert_eq!(
            arrow_polygon(ArrowKind::Triangle, tip, 0.0, 10.0, 4.0).len(),
            3
        );
        assert_eq!(
            arrow_polygon(ArrowKind::Dovetail, tip, 0.0, 10.0, 4.0).len(),
            4
        );
        assert_eq!(arrow_polygon(ArrowKind::Tee, tip, 0.0, 10.0, 4.0).len(), 4);
        assert_eq!(arrow_polygon(ArrowKind::Dot, tip, 0.0, 10.0, 4.0).len(), 12);
        assert_eq!(
            arrow_polygon(ArrowKind::Diamond, tip, 0.0, 10.0, 4.0).len(),
            4
        );
        assert_eq!(ArrowKind::default(), ArrowKind::Triangle);
    }

    #[test]
    fn tip_leads_and_heads_rotate_with_the_edge() {
        let tip = Point2::new(10.0, 5.0);
        for kind in [ArrowKind::Triangle, ArrowKind::Dovetail, ArrowKind::Diamond] {
            let east = arrow_polygon(kind, tip, 0.0, 10.0, 4.0);
            let north = arrow_polygon(kind, tip, std::f32::consts::FRAC_PI_2, 10.0, 4.0);
            assert_eq!(east[0], tip);
            assert_eq!(north[0], tip);
            assert!(east[1].x < tip.x);
            assert!(north[1].y < tip.y);
        }
        let dot_turned = arrow_polygon(ArrowKind::Dot, tip, 1.2, 10.0, 4.0);
        let dot_straight = arrow_polygon(ArrowKind::Dot, tip, 0.0, 10.0, 4.0);
        assert_eq!(dot_turned, dot_straight);
    }

    #[test]
    fn bar_rotates_with_the_edge() {
        let tip = Point2::new(10.0, 5.0);
        let east = arrow_polygon(ArrowKind::Tee, tip, 0.0, 10.0, 4.0);
        let north = arrow_polygon(ArrowKind::Tee, tip, std::f32::consts::FRAC_PI_2, 10.0, 4.0);
        assert_eq!(east.len(), 4);
        assert_eq!(north.len(), 4);
        assert_ne!(east, north);
        assert!((east[0].x - tip.x).abs() < 1e-4);
        assert!((north[0].y - tip.y).abs() < 1e-4);
    }
}
