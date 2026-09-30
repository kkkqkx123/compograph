//! Position interpolation between layout results.
//!
//! Transitions blend the current coordinates towards a freshly computed
//! target over a fixed number of frames. Easing stays limited to linear and
//! cubic curves, and pinned nodes hold their anchors for the whole run, so
//! the driver can share its generation guard with background refinement.

use cg_graph::{FixedNodes, Positions};
#[cfg(test)]
use cg_graph::NodeIndex;
use cg_types::Point2;

/// Easing applied to interpolation progress.
///
/// Only two curves exist: linear for test baselines and even motion, cubic
/// for gentle starts and landings. No easing library is involved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Easing {
    #[default]
    Linear,
    CubicInOut,
}

/// Eased progress for `t` in the unit interval.
///
/// Non-finite inputs rest at the start; out-of-range inputs clamp to the ends.
pub fn apply_easing(easing: Easing, t: f32) -> f32 {
    let clamped = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    match easing {
        Easing::Linear => clamped,
        Easing::CubicInOut => {
            if clamped < 0.5 {
                4.0 * clamped * clamped * clamped
            } else {
                1.0 - (-2.0 * clamped + 2.0).powi(3) / 2.0
            }
        }
    }
}

/// Interpolated point between `from` and `to` at progress `t`.
pub fn lerp_point(from: Point2, to: Point2, t: f32, easing: Easing) -> Point2 {
    let ratio = apply_easing(easing, t);
    Point2::new(
        from.x + (to.x - from.x) * ratio,
        from.y + (to.y - from.y) * ratio,
    )
}

/// Blended position table at progress `t`.
///
/// Pinned nodes keep their start coordinates for the whole run. Nodes missing
/// from the start appear at their target at once, and nodes missing from the
/// target are dropped, so the output always matches the target membership.
pub fn blend_positions(
    from: &Positions,
    to: &Positions,
    t: f32,
    easing: Easing,
    fixed: &FixedNodes,
) -> Positions {
    let mut out = Positions::new();
    for (node, target) in to {
        if fixed.contains(node) {
            if let Some(anchor) = from.get(node) {
                out.insert(*node, *anchor);
            } else {
                out.insert(*node, *target);
            }
            continue;
        }
        match from.get(node) {
            Some(start) => {
                out.insert(*node, lerp_point(*start, *target, t, easing));
            }
            None => {
                out.insert(*node, *target);
            }
        }
    }
    out
}

/// Scalar tween reusing the position easing.
///
/// Style feedback stages bypass values with the same two curves instead of a
/// separate easing library. Non-finite progress rests at the start.
pub fn tween_scalar(from: f32, to: f32, t: f32, easing: Easing) -> f32 {
    let ratio = apply_easing(easing, t);
    from + (to - from) * ratio
}

/// Fixed-step transition from one position table to another.
///
/// Frames are pulled with [`PositionTransition::next_frame`]; the driver
/// writes each frame back through its usual version and notify path.
pub struct PositionTransition {
    from: Positions,
    to: Positions,
    total: usize,
    current: usize,
    easing: Easing,
    fixed: FixedNodes,
}

impl PositionTransition {
    /// New transition over `steps` frames; zero steps mean a single jump.
    pub fn new(
        from: Positions,
        to: Positions,
        steps: usize,
        easing: Easing,
        fixed: FixedNodes,
    ) -> Self {
        Self {
            from,
            to,
            total: steps.max(1),
            current: 0,
            easing,
            fixed,
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

    /// Easing of the run.
    pub fn easing(&self) -> Easing {
        self.easing
    }

    /// Target membership the run converges to.
    pub fn target(&self) -> &Positions {
        &self.to
    }

    /// Next frame, or none once the run is exhausted.
    pub fn next_frame(&mut self) -> Option<Positions> {
        if self.current >= self.total {
            return None;
        }
        self.current += 1;
        let progress = self.current as f32 / self.total as f32;
        Some(blend_positions(
            &self.from,
            &self.to,
            progress,
            self.easing,
            &self.fixed,
        ))
    }
}

/// Nodes of a transition target in deterministic order, for tests.
#[cfg(test)]
fn target_order(transition: &PositionTransition) -> Vec<NodeIndex> {
    let mut order: Vec<NodeIndex> = transition.to.keys().copied().collect();
    order.sort_unstable_by_key(|node| node.index());
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> Point2 {
        Point2::new(x, y)
    }

    #[test]
    fn easing_pins_endpoints_and_clamps() {
        assert_eq!(apply_easing(Easing::Linear, 0.0), 0.0);
        assert_eq!(apply_easing(Easing::Linear, 1.0), 1.0);
        assert_eq!(apply_easing(Easing::Linear, 0.25), 0.25);
        assert_eq!(apply_easing(Easing::CubicInOut, 0.0), 0.0);
        assert_eq!(apply_easing(Easing::CubicInOut, 1.0), 1.0);
        assert_eq!(apply_easing(Easing::CubicInOut, 0.5), 0.5);
        assert_eq!(apply_easing(Easing::Linear, -2.0), 0.0);
        assert_eq!(apply_easing(Easing::Linear, 3.0), 1.0);
        assert_eq!(apply_easing(Easing::CubicInOut, f32::NAN), 0.0);
        assert_eq!(apply_easing(Easing::Linear, f32::INFINITY), 0.0);
    }

    #[test]
    fn lerp_covers_endpoints_and_midpoint() {
        let from = point(0.0, 10.0);
        let to = point(10.0, 20.0);
        assert_eq!(lerp_point(from, to, 0.0, Easing::Linear), from);
        assert_eq!(lerp_point(from, to, 1.0, Easing::Linear), to);
        assert_eq!(
            lerp_point(from, to, 0.5, Easing::Linear),
            point(5.0, 15.0)
        );
        assert_eq!(
            lerp_point(from, to, 0.5, Easing::CubicInOut),
            point(5.0, 15.0)
        );
    }

    #[test]
    fn blend_keeps_fixed_nodes_and_matches_membership() {
        let mut from = Positions::new();
        from.insert(NodeIndex::new(0), point(0.0, 0.0));
        from.insert(NodeIndex::new(1), point(0.0, 0.0));
        from.insert(NodeIndex::new(9), point(50.0, 50.0));
        let mut to = Positions::new();
        to.insert(NodeIndex::new(0), point(10.0, 0.0));
        to.insert(NodeIndex::new(1), point(0.0, 10.0));
        to.insert(NodeIndex::new(2), point(7.0, 7.0));
        let mut fixed = FixedNodes::new();
        fixed.insert(NodeIndex::new(1));
        let blended = blend_positions(&from, &to, 0.5, Easing::Linear, &fixed);
        assert_eq!(
            blended.get(&NodeIndex::new(0)),
            Some(&point(5.0, 0.0))
        );
        assert_eq!(
            blended.get(&NodeIndex::new(1)),
            Some(&point(0.0, 0.0))
        );
        assert_eq!(
            blended.get(&NodeIndex::new(2)),
            Some(&point(7.0, 7.0))
        );
        assert!(!blended.contains_key(&NodeIndex::new(9)));
    }

    #[test]
    fn transition_steps_monotonically_to_the_target() {
        let mut from = Positions::new();
        from.insert(NodeIndex::new(0), point(0.0, 0.0));
        let mut to = Positions::new();
        to.insert(NodeIndex::new(0), point(8.0, 0.0));
        let mut run =
            PositionTransition::new(from, to.clone(), 4, Easing::Linear, FixedNodes::new());
        assert_eq!(run.steps_total(), 4);
        assert_eq!(target_order(&run), vec![NodeIndex::new(0)]);
        assert!(!run.is_done());
        let mut frames = Vec::new();
        while let Some(frame) = run.next_frame() {
            frames.push(frame);
        }
        assert_eq!(frames.len(), 4);
        assert!(run.is_done());
        assert!(run.next_frame().is_none());
        assert_eq!(frames[0].get(&NodeIndex::new(0)), Some(&point(2.0, 0.0)));
        assert_eq!(frames[3].get(&NodeIndex::new(0)), Some(&point(8.0, 0.0)));
        assert_eq!(run.steps_done(), 4);
        assert_eq!(run.target(), &to);
    }

    #[test]
    fn empty_transition_still_lands_empty() {
        let mut run = PositionTransition::new(
            Positions::new(),
            Positions::new(),
            3,
            Easing::CubicInOut,
            FixedNodes::new(),
        );
        let mut count = 0;
        while let Some(frame) = run.next_frame() {
            assert!(frame.is_empty());
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn scalar_tween_follows_the_same_easing() {
        assert_eq!(tween_scalar(2.0, 6.0, 0.0, Easing::Linear), 2.0);
        assert_eq!(tween_scalar(2.0, 6.0, 1.0, Easing::Linear), 6.0);
        assert_eq!(tween_scalar(2.0, 6.0, 0.5, Easing::Linear), 4.0);
        assert_eq!(tween_scalar(2.0, 6.0, f32::NAN, Easing::Linear), 2.0);
        assert_eq!(tween_scalar(0.0, 10.0, 0.5, Easing::CubicInOut), 5.0);
    }

    #[test]
    fn cubic_easing_stays_within_the_unit_interval() {
        let mut previous = 0.0f32;
        let mut ordinal = 0;
        while ordinal <= 20 {
            let value = apply_easing(Easing::CubicInOut, ordinal as f32 / 20.0);
            assert!(value >= 0.0 && value <= 1.0);
            assert!(value >= previous);
            previous = value;
            ordinal += 1;
        }
    }
}
