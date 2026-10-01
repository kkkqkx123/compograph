//! Label geometry: anchors, block estimates and selection envelopes.

use cg_geometry::{cubic_bezier, quadratic_bezier};
use cg_types::{Point2, Rect, Vec2};

use crate::label_style::{
    DEFAULT_LABEL_SIZE, LabelAlign, LabelBackground, LabelStyle,
};
use crate::label_wrap::{line_height, split_label_lines};
use crate::view::PaintedEdge;

/// Vertical gap, in pixels, between a node body and its label baseline.
pub const LABEL_GAP: f32 = 4.0;

/// Vertical gap, in pixels, between an edge anchor and its label top edge.
pub const EDGE_LABEL_GAP: f32 = 3.0;

/// Anchor of an edge label along the painted path.
///
/// Straight edges anchor at the segment midpoint and curved edges at the
/// curve midpoint, so the label tracks the visible geometry. Polylines anchor
/// at the middle of the bend list and self loops at the cubic midpoint.
pub fn edge_label_anchor(edge: &PaintedEdge) -> Point2 {
    if let Some([ctrl_a, ctrl_b]) = edge.loop_ctrls {
        return cubic_bezier(edge.start, ctrl_a, ctrl_b, edge.end, 0.5);
    }
    let bends = edge.bends();
    if !bends.is_empty() {
        let mut line = Vec::with_capacity(bends.len() + 2);
        line.push(edge.start);
        line.extend_from_slice(&bends);
        line.push(edge.end);
        let mid = line.len() / 2;
        if line.len() % 2 == 1 {
            return line[mid];
        }
        let (a, b) = (line[mid - 1], line[mid]);
        return Point2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    }
    if let Some(ctrl) = edge.ctrl {
        return quadratic_bezier(edge.start, ctrl, edge.end, 0.5);
    }
    Point2::new(
        (edge.start.x + edge.end.x) / 2.0,
        (edge.start.y + edge.end.y) / 2.0,
    )
}

/// Rotation of an edge label along the edge end tangent, in radians.
///
/// The tangent matches the arrow orientation rule: self loops use the return
/// tangent, curves use their control point, polylines use their last bend.
/// Angles fold into the readable half circle so text never renders upside
/// down. Horizontal edges report zero. The paint plan keeps zero until the
/// canvas can draw rotated text; this helper is reserved for that path.
pub fn edge_label_angle(edge: &PaintedEdge) -> f32 {
    let reference = edge
        .loop_ctrls
        .map(|ctrls| ctrls[1])
        .or(edge.ctrl)
        .or_else(|| edge.bends.last().copied())
        .unwrap_or(edge.start);
    let dx = edge.end.x - reference.x;
    let dy = edge.end.y - reference.y;
    if dx.abs() <= f32::EPSILON && dy.abs() <= f32::EPSILON {
        return 0.0;
    }
    let mut angle = dy.atan2(dx);
    while angle > std::f32::consts::FRAC_PI_2 {
        angle -= std::f32::consts::PI;
    }
    while angle < -std::f32::consts::FRAC_PI_2 {
        angle += std::f32::consts::PI;
    }
    angle
}

/// Estimated text block for `text` at `size`, in pixels.
///
/// Width scales with the longest wrapped line and height stacks every line by
/// the line height. The estimate stays headless; the canvas replaces it with
/// shaped widths when painting backgrounds.
pub fn estimate_label_block(text: &str, size: f32) -> Vec2 {
    let size = if size.is_finite() && size > 0.0 {
        size
    } else {
        DEFAULT_LABEL_SIZE
    };
    let lines = split_label_lines(text);
    let longest = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as f32;
    Vec2::new(longest * size * 0.6, lines.len() as f32 * line_height(size))
}

/// Outer envelope of a label around `origin` with rotation and background.
///
/// The unrotated block centers horizontally on the origin and extends downward
/// from its top edge; its corners rotate around the origin before the
/// background padding expands the box. Unrotated bare labels return the tight
/// block. Callers pass zero rotation to match the horizontal canvas paint;
/// the rotated branch is reserved for a future rotated paint path.
pub fn label_envelope(
    origin: Point2,
    text: &str,
    size: f32,
    rotation: f32,
    background: LabelBackground,
) -> Rect {
    label_envelope_styled(
        origin,
        text,
        size,
        rotation,
        &LabelStyle {
            background,
            ..LabelStyle::default()
        },
    )
}

/// Outer envelope of a label honoring alignment, padding and background.
///
/// Alignment moves the horizontal anchor: center keeps the historical
/// centered block, left starts the block at the anchor, right ends it there.
/// Padding expands the box only when a background is drawn.
pub fn label_envelope_styled(
    origin: Point2,
    text: &str,
    size: f32,
    rotation: f32,
    style: &LabelStyle,
) -> Rect {
    let block = estimate_label_block(text, size);
    let left = match style.align {
        LabelAlign::Center => origin.x - block.x / 2.0,
        LabelAlign::Left => origin.x,
        LabelAlign::Right => origin.x - block.x,
    };
    let corners = [
        Point2::new(left, origin.y),
        Point2::new(left + block.x, origin.y),
        Point2::new(left + block.x, origin.y + block.y),
        Point2::new(left, origin.y + block.y),
    ];
    let (sin, cos) = rotation.sin_cos();
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for corner in corners {
        let local_x = corner.x - origin.x;
        let local_y = corner.y - origin.y;
        let rotated_x = origin.x + local_x * cos - local_y * sin;
        let rotated_y = origin.y + local_x * sin + local_y * cos;
        min_x = min_x.min(rotated_x);
        min_y = min_y.min(rotated_y);
        max_x = max_x.max(rotated_x);
        max_y = max_y.max(rotated_y);
    }
    let pad = match style.background {
        LabelBackground::None => 0.0,
        LabelBackground::Rect | LabelBackground::RoundRect => style.padding(),
    };
    Rect::new(
        Point2::new(min_x - pad, min_y - pad),
        Vec2::new(
            (max_x - min_x + pad * 2.0).max(1.0),
            (max_y - min_y + pad * 2.0).max(1.0),
        ),
    )
}

/// Anchor of a node label below its body honoring the label alignment.
pub fn node_label_origin(center: Point2, side: f32, style: &LabelStyle) -> Point2 {
    let x = match style.align {
        LabelAlign::Center => center.x,
        LabelAlign::Left => center.x - side / 2.0,
        LabelAlign::Right => center.x + side / 2.0,
    };
    Point2::new(x, center.y + side / 2.0 + LABEL_GAP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::label_plan::paint_edge_labels_for;
    use crate::lod::DetailLevel;
    use cg_graph::NodeIndex;

    #[test]
    fn straight_and_curved_edges_anchor_at_their_midpoints() {
        use crate::arrows::ArrowKind;
        use crate::view::PaintedEdge;
        fn edge(start: Point2, end: Point2, ctrl: Option<Point2>) -> PaintedEdge {
            PaintedEdge {
                source: NodeIndex::new(0),
                target: NodeIndex::new(1),
                start,
                end,
                ctrl,
                loop_ctrls: None,
                bends: Vec::new(),
                aggregated: false,
                tint: 0,
                width: 1.0,
                opacity: 1.0,
                arrow: ArrowKind::Triangle,
                arrow_scale: 1.0,
            }
        }
        let straight = edge(Point2::new(0.0, 0.0), Point2::new(100.0, 0.0), None);
        assert_eq!(edge_label_anchor(&straight), Point2::new(50.0, 0.0));
        let curved = edge(
            Point2::new(0.0, 0.0),
            Point2::new(20.0, 0.0),
            Some(Point2::new(10.0, 10.0)),
        );
        assert_eq!(edge_label_anchor(&curved), Point2::new(10.0, 5.0));
        let labels = paint_edge_labels_for(
            &[straight, curved],
            DetailLevel::Full,
            |_, _| Some("1".to_string()),
            |_, _| 11.0,
        );
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].origin, Point2::new(50.0, EDGE_LABEL_GAP));
        assert_eq!(labels[1].origin, Point2::new(10.0, 5.0 + EDGE_LABEL_GAP));
    }

    #[test]
    fn background_envelope_covers_rotation() {
        use crate::label_style::LABEL_BACKGROUND_PAD;

        let origin = Point2::new(50.0, 10.0);
        let plain = label_envelope(origin, "hello", 12.0, 0.0, LabelBackground::None);
        let backed = label_envelope(origin, "hello", 12.0, 0.0, LabelBackground::Rect);
        assert!(backed.size.x > plain.size.x);
        assert!(backed.size.y > plain.size.y);
        assert!((backed.size.x - plain.size.x - LABEL_BACKGROUND_PAD * 2.0).abs() < 1e-3);
        let block = estimate_label_block("hello", 12.0);
        assert!((plain.size.x - block.x).abs() < 1e-3);
        assert!((plain.size.y - block.y).abs() < 1e-3);
        assert!((plain.origin.x - (origin.x - block.x / 2.0)).abs() < 1e-3);
        assert!((plain.origin.y - origin.y).abs() < 1e-3);
    }

    #[test]
    fn label_alignment_moves_the_anchor_and_the_envelope() {
        let center = Point2::new(100.0, 100.0);
        let middle = node_label_origin(
            center,
            24.0,
            &LabelStyle {
                align: LabelAlign::Center,
                ..LabelStyle::default()
            },
        );
        assert_eq!(middle, Point2::new(100.0, 116.0));
        let left = node_label_origin(
            center,
            24.0,
            &LabelStyle {
                align: LabelAlign::Left,
                ..LabelStyle::default()
            },
        );
        assert_eq!(left, Point2::new(88.0, 116.0));
        let right = node_label_origin(
            center,
            24.0,
            &LabelStyle {
                align: LabelAlign::Right,
                ..LabelStyle::default()
            },
        );
        assert_eq!(right, Point2::new(112.0, 116.0));
        let block = estimate_label_block("hello", 12.0);
        for (align, origin_x) in [
            (LabelAlign::Center, 50.0),
            (LabelAlign::Left, 50.0),
            (LabelAlign::Right, 50.0),
        ] {
            let style = LabelStyle {
                align,
                ..LabelStyle::default()
            };
            let envelope =
                label_envelope_styled(Point2::new(origin_x, 10.0), "hello", 12.0, 0.0, &style);
            assert!((envelope.size.x - block.x).abs() < 1e-3);
            match align {
                LabelAlign::Center => {
                    assert!((envelope.origin.x - (origin_x - block.x / 2.0)).abs() < 1e-3);
                }
                LabelAlign::Left => {
                    assert!((envelope.origin.x - origin_x).abs() < 1e-3);
                }
                LabelAlign::Right => {
                    assert!((envelope.origin.x - (origin_x - block.x)).abs() < 1e-3);
                }
            }
        }
    }
}
