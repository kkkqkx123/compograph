//! Per-frame interpolation between resolved element styles.
//!
//! Transitions blend one [`NodeStyle`] or [`EdgeStyle`] into another over a
//! fixed number of frames. Scalars and colors interpolate with the shared
//! layout easing; discrete fields such as shape or arrow switch only at the
//! final frame. Callers pull frames and write them back through the bypass
//! store on their own repaint cadence; nothing here owns a frame loop.

use cg_layout::{Easing, apply_easing, tween_scalar};

use crate::style::{EdgeStyle, LabelStyle, NodeFill, NodeStyle, lerp_rgb};

/// Blended node style at progress `t` in the unit interval.
///
/// Out-of-range progress clamps to the ends through the easing, so callers
/// never need to sanitize frame counters before blending.
pub fn blend_node_style(from: &NodeStyle, to: &NodeStyle, t: f32, easing: Easing) -> NodeStyle {
    let ratio = apply_easing(easing, t);
    let done = ratio >= 1.0;
    NodeStyle {
        fill: NodeFill::gradient(
            lerp_rgb(from.fill.start, to.fill.start, ratio),
            lerp_rgb(from.fill.end, to.fill.end, ratio),
            tween_scalar(from.fill.angle_deg, to.fill.angle_deg, t, easing),
        ),
        stroke: lerp_rgb(from.stroke, to.stroke, ratio),
        stroke_width: tween_scalar(from.stroke_width, to.stroke_width, t, easing),
        opacity: tween_scalar(from.opacity, to.opacity, t, easing),
        label_size: tween_scalar(from.label_size, to.label_size, t, easing),
        scale: tween_scalar(from.scale, to.scale, t, easing),
        shape: if done { to.shape } else { from.shape },
        image: if done {
            to.image.clone()
        } else {
            from.image.clone()
        },
        label: blend_label_style(&from.label, &to.label, t, easing),
    }
}

/// Blended label style at progress `t` in the unit interval.
///
/// Alignment and background switch at the final frame like other discrete
/// fields, while color, radius and padding interpolate.
pub fn blend_label_style(from: &LabelStyle, to: &LabelStyle, t: f32, easing: Easing) -> LabelStyle {
    let ratio = apply_easing(easing, t);
    let done = ratio >= 1.0;
    LabelStyle {
        align: if done { to.align } else { from.align },
        background: if done {
            to.background
        } else {
            from.background
        },
        background_color: lerp_rgb(from.background_color, to.background_color, ratio),
        corner_radius: tween_scalar(from.corner_radius, to.corner_radius, t, easing),
        padding: tween_scalar(from.padding, to.padding, t, easing),
    }
}

/// Blended edge style at progress `t` in the unit interval.
pub fn blend_edge_style(from: &EdgeStyle, to: &EdgeStyle, t: f32, easing: Easing) -> EdgeStyle {
    let ratio = apply_easing(easing, t);
    EdgeStyle {
        tint: lerp_rgb(from.tint, to.tint, ratio),
        width: tween_scalar(from.width, to.width, t, easing),
        opacity: tween_scalar(from.opacity, to.opacity, t, easing),
        arrow_scale: tween_scalar(from.arrow_scale, to.arrow_scale, t, easing),
        label_size: tween_scalar(from.label_size, to.label_size, t, easing),
        arrow: if ratio >= 1.0 { to.arrow } else { from.arrow },
        curve: if ratio >= 1.0 { to.curve } else { from.curve },
        label: blend_label_style(&from.label, &to.label, t, easing),
    }
}

/// Fixed-step transition from one node style to another.
///
/// Frames are pulled with [`NodeStyleTransition::next_frame`]; the caller
/// writes each frame into the bypass store and requests a repaint.
pub struct NodeStyleTransition {
    from: NodeStyle,
    to: NodeStyle,
    total: usize,
    current: usize,
    easing: Easing,
}

impl NodeStyleTransition {
    /// New transition over `steps` frames; zero steps mean a single jump.
    pub fn new(from: NodeStyle, to: NodeStyle, steps: usize, easing: Easing) -> Self {
        Self {
            from,
            to,
            total: steps.max(1),
            current: 0,
            easing,
        }
    }

    /// Total frames of the run.
    pub fn steps_total(&self) -> usize {
        self.total
    }

    /// Frames already produced.
    pub fn steps_done(&self) -> usize {
        self.current
    }

    /// True once every frame has been produced.
    pub fn is_done(&self) -> bool {
        self.current >= self.total
    }

    /// Target style the run converges to.
    pub fn target(&self) -> &NodeStyle {
        &self.to
    }

    /// Next frame, or none once the run is exhausted.
    pub fn next_frame(&mut self) -> Option<NodeStyle> {
        if self.current >= self.total {
            return None;
        }
        self.current += 1;
        let progress = self.current as f32 / self.total as f32;
        Some(blend_node_style(
            &self.from,
            &self.to,
            progress,
            self.easing,
        ))
    }
}

/// Fixed-step transition from one edge style to another.
pub struct EdgeStyleTransition {
    from: EdgeStyle,
    to: EdgeStyle,
    total: usize,
    current: usize,
    easing: Easing,
}

impl EdgeStyleTransition {
    /// New transition over `steps` frames; zero steps mean a single jump.
    pub fn new(from: EdgeStyle, to: EdgeStyle, steps: usize, easing: Easing) -> Self {
        Self {
            from,
            to,
            total: steps.max(1),
            current: 0,
            easing,
        }
    }

    /// Total frames of the run.
    pub fn steps_total(&self) -> usize {
        self.total
    }

    /// Frames already produced.
    pub fn steps_done(&self) -> usize {
        self.current
    }

    /// True once every frame has been produced.
    pub fn is_done(&self) -> bool {
        self.current >= self.total
    }

    /// Target style the run converges to.
    pub fn target(&self) -> &EdgeStyle {
        &self.to
    }

    /// Next frame, or none once the run is exhausted.
    pub fn next_frame(&mut self) -> Option<EdgeStyle> {
        if self.current >= self.total {
            return None;
        }
        self.current += 1;
        let progress = self.current as f32 / self.total as f32;
        Some(blend_edge_style(
            &self.from,
            &self.to,
            progress,
            self.easing,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::NodeShape;

    #[test]
    fn node_blend_pins_endpoints_and_midpoint() {
        let from = NodeStyle {
            fill: NodeFill::solid(0x000000),
            stroke_width: 1.0,
            opacity: 0.0,
            ..NodeStyle::default()
        };
        let to = NodeStyle {
            fill: NodeFill::solid(0xFFFFFF),
            stroke_width: 5.0,
            opacity: 1.0,
            shape: NodeShape::Ellipse,
            ..NodeStyle::default()
        };
        let start = blend_node_style(&from, &to, 0.0, Easing::Linear);
        assert_eq!(start.stroke_width, 1.0);
        assert_eq!(start.opacity, 0.0);
        assert_eq!(start.fill.start, 0x000000);
        assert_eq!(start.shape, NodeShape::Square);
        let mid = blend_node_style(&from, &to, 0.5, Easing::Linear);
        assert_eq!(mid.stroke_width, 3.0);
        assert_eq!(mid.opacity, 0.5);
        assert_eq!(mid.fill.start, 0x808080);
        assert_eq!(mid.shape, NodeShape::Square);
        let end = blend_node_style(&from, &to, 1.0, Easing::Linear);
        assert_eq!(end.stroke_width, 5.0);
        assert_eq!(end.opacity, 1.0);
        assert_eq!(end.shape, NodeShape::Ellipse);
        assert_eq!(end.fill, to.fill);
    }

    #[test]
    fn edge_blend_switches_arrow_only_at_the_end() {
        use crate::style::EdgeCurve;

        let from = EdgeStyle::default();
        let to = EdgeStyle {
            width: 4.0,
            curve: EdgeCurve::Bezier,
            ..EdgeStyle::default()
        };
        let mid = blend_edge_style(&from, &to, 0.5, Easing::CubicInOut);
        assert!((mid.width - 2.75).abs() < 1e-4);
        assert_eq!(mid.arrow, from.arrow);
        assert_eq!(mid.curve, EdgeCurve::Auto);
        let end = blend_edge_style(&from, &to, 1.0, Easing::CubicInOut);
        assert_eq!(end.width, 4.0);
        assert_eq!(end.arrow, to.arrow);
        assert_eq!(end.curve, EdgeCurve::Bezier);
    }

    #[test]
    fn label_blend_interpolates_color_and_switches_shape_at_the_end() {
        use crate::text::{LabelAlign, LabelBackground, LabelStyle};

        let from = LabelStyle::default();
        let to = LabelStyle {
            align: LabelAlign::Right,
            background: LabelBackground::RoundRect,
            background_color: 0xFFFFFF,
            corner_radius: 8.0,
            padding: 6.0,
        };
        let mid = blend_label_style(&from, &to, 0.5, Easing::Linear);
        assert_eq!(mid.align, LabelAlign::Center);
        assert_eq!(mid.background, LabelBackground::None);
        let end = blend_label_style(&from, &to, 1.0, Easing::Linear);
        assert_eq!(end, to);
    }

    #[test]
    fn node_transition_steps_to_the_target() {
        let from = NodeStyle::default();
        let to = NodeStyle {
            opacity: 0.25,
            ..NodeStyle::default()
        };
        let mut run = NodeStyleTransition::new(from, to.clone(), 2, Easing::Linear);
        assert_eq!(run.steps_total(), 2);
        assert!(!run.is_done());
        let first = run.next_frame().expect("first frame");
        assert_eq!(first.opacity, 0.625);
        let last = run.next_frame().expect("last frame");
        assert_eq!(last, to);
        assert!(run.next_frame().is_none());
        assert!(run.is_done());
        assert_eq!(run.steps_done(), 2);
        assert_eq!(run.target(), &to);
    }

    #[test]
    fn edge_transition_with_zero_steps_jumps_once() {
        let from = EdgeStyle::default();
        let to = EdgeStyle {
            tint: 0xFF0000,
            ..EdgeStyle::default()
        };
        let mut run = EdgeStyleTransition::new(from, to, 0, Easing::Linear);
        assert_eq!(run.steps_total(), 1);
        assert_eq!(run.next_frame(), Some(to));
        assert!(run.next_frame().is_none());
        assert_eq!(run.target(), &to);
    }
}
