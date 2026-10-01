//! Circle layout spacing nodes evenly around a ring.
//!
//! The radius selection follows the reference circle algorithm: an explicit
//! radius wins, a lone node sits at the center, otherwise the ring fits the
//! bounding box and grows until neighbouring nodes stop overlapping.

use std::collections::HashMap;

use cg_graph::{FixedNodes, GraphView, Positions};
use cg_types::Point2;

use crate::engine::{CommonOptions, LayoutEngine};

/// Spacing applied between neighbours when avoiding overlap.
const OVERLAP_SPACING: f32 = 1.75;

/// Tuning parameters of the circular arrangement.
#[derive(Clone, Debug)]
pub struct CircleOptions {
    /// Center of the ring in model units.
    pub center: Point2,
    /// Width of the bounding rectangle used for the default radius.
    pub width: f32,
    /// Height of the bounding rectangle used for the default radius.
    pub height: f32,
    /// Fixed ring radius; derived from the box when absent.
    pub radius: Option<f32>,
    /// Angle of the first node in radians.
    pub start_angle: f32,
    /// Angular span between the first and last node; a full ring minus one
    /// step when absent.
    pub sweep: Option<f32>,
    /// When false, nodes run counterclockwise instead of clockwise.
    pub clockwise: bool,
    /// When true, the radius grows until neighbours stop overlapping.
    pub avoid_overlap: bool,
    /// Uniform node extent used for overlap tests.
    pub node_size: f32,
}

impl Default for CircleOptions {
    fn default() -> Self {
        Self {
            center: Point2::ZERO,
            width: 640.0,
            height: 480.0,
            radius: None,
            start_angle: 3.0 / 2.0 * std::f32::consts::PI,
            sweep: None,
            clockwise: true,
            avoid_overlap: true,
            node_size: 24.0,
        }
    }
}

/// Circle engine implementing the shared layout contract.
pub struct CircleLayout {
    options: CircleOptions,
    common: CommonOptions,
}

impl CircleLayout {
    pub fn new(center: Point2, width: f32, height: f32) -> Self {
        Self {
            options: CircleOptions {
                center,
                width,
                height,
                ..CircleOptions::default()
            },
            common: CommonOptions::default(),
        }
    }

    pub fn with_options(options: CircleOptions) -> Self {
        Self {
            options,
            common: CommonOptions::default(),
        }
    }

    /// Overrides the shared sort, fit and spacing inputs.
    pub fn with_common(mut self, common: CommonOptions) -> Self {
        self.common = common;
        self
    }

    pub fn options(&self) -> &CircleOptions {
        &self.options
    }
}

impl LayoutEngine for CircleLayout {
    fn layout(&self, graph: &dyn GraphView, previous: &Positions, fixed: &FixedNodes) -> Positions {
        let mut ids = graph.node_ids();
        self.common.sort_ids(&mut ids, graph);
        let mut result: Positions = HashMap::new();
        if ids.is_empty() {
            return result;
        }
        let count = ids.len();
        let sweep = self
            .options
            .sweep
            .unwrap_or(2.0 * std::f32::consts::PI - 2.0 * std::f32::consts::PI / count as f32);
        let step = sweep / count.saturating_sub(1).max(1) as f32;
        let radius = circle_radius(
            count,
            self.options.width,
            self.options.height,
            self.options.radius,
            step,
            self.options.avoid_overlap,
            self.options.node_size,
        ) * self.common.spacing();
        let direction = if self.options.clockwise { 1.0 } else { -1.0 };
        for (ordinal, node) in ids.into_iter().enumerate() {
            if fixed.contains(&node)
                && let Some(held) = previous.get(&node)
            {
                result.insert(node, *held);
                continue;
            }
            let theta = self.options.start_angle + ordinal as f32 * step * direction;
            result.insert(
                node,
                Point2::new(
                    self.options.center.x + radius * theta.cos(),
                    self.options.center.y + radius * theta.sin(),
                ),
            );
        }
        result
    }

    fn name(&self) -> &'static str {
        "circle"
    }

    fn set_common(&mut self, common: CommonOptions) {
        self.common = common;
    }
}

/// Ring radius for `count` nodes with angular step `step`.
///
/// An explicit radius always wins and a lone node needs no ring. Otherwise the
/// ring fits the box minus one node extent, growing to the overlap-free lower
/// bound when avoidance is enabled.
fn circle_radius(
    count: usize,
    width: f32,
    height: f32,
    radius: Option<f32>,
    step: f32,
    avoid_overlap: bool,
    node_size: f32,
) -> f32 {
    if let Some(radius) = radius {
        return radius.max(0.0);
    }
    if count <= 1 {
        return 0.0;
    }
    let mut ring = (width.min(height) / 2.0 - node_size).max(0.0);
    if avoid_overlap {
        let spread = node_size * OVERLAP_SPACING;
        let chord = ((step.cos() - 1.0).powi(2) + step.sin().powi(2)).sqrt();
        if chord > f32::EPSILON {
            ring = ring.max(spread / chord);
        }
    }
    ring
}

#[cfg(test)]
mod tests {
    use cg_graph::{FixedNodes, MockGraph, NodeIndex};

    use super::*;

    #[test]
    fn empty_graph_places_nothing() {
        let graph = MockGraph::empty();
        let engine = CircleLayout::new(Point2::ZERO, 400.0, 400.0);
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert!(positions.is_empty());
    }

    #[test]
    fn lone_node_sits_at_the_center() {
        let graph = MockGraph::isolated(1);
        let engine = CircleLayout::new(Point2::new(5.0, 7.0), 400.0, 400.0);
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(
            positions.get(&NodeIndex::new(0)),
            Some(&Point2::new(5.0, 7.0))
        );
    }

    #[test]
    fn four_nodes_share_the_ring_evenly() {
        let graph = MockGraph::isolated(4);
        let engine = CircleLayout::with_options(CircleOptions {
            avoid_overlap: false,
            ..CircleOptions::default()
        });
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 4);
        let expected_radius = 480.0f32.min(640.0) / 2.0 - 24.0;
        for position in positions.values() {
            let distance = (position.x.powi(2) + position.y.powi(2)).sqrt();
            assert!((distance - expected_radius).abs() < 1e-3);
        }
    }

    #[test]
    fn explicit_radius_wins_over_the_box() {
        let graph = MockGraph::isolated(3);
        let engine = CircleLayout::with_options(CircleOptions {
            radius: Some(50.0),
            avoid_overlap: false,
            ..CircleOptions::default()
        });
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        for position in positions.values() {
            let distance = (position.x.powi(2) + position.y.powi(2)).sqrt();
            assert!((distance - 50.0).abs() < 1e-3);
        }
    }

    #[test]
    fn counterclockwise_mirrors_clockwise() {
        let graph = MockGraph::isolated(4);
        let clockwise = CircleLayout::with_options(CircleOptions {
            avoid_overlap: false,
            ..CircleOptions::default()
        });
        let counter = CircleLayout::with_options(CircleOptions {
            clockwise: false,
            avoid_overlap: false,
            ..CircleOptions::default()
        });
        let ahead = clockwise.layout(&graph, &Positions::new(), &FixedNodes::default());
        let behind = counter.layout(&graph, &Positions::new(), &FixedNodes::default());
        for index in 0..4 {
            let node = NodeIndex::new(index);
            assert!((ahead[&node].x + behind[&node].x).abs() < 1e-3);
            assert!((ahead[&node].y - behind[&node].y).abs() < 1e-3);
        }
    }

    #[test]
    fn overlap_avoidance_only_grows_the_ring() {
        let tight = circle_radius(8, 100.0, 100.0, None, 0.8, false, 24.0);
        let spread = circle_radius(8, 100.0, 100.0, None, 0.8, true, 24.0);
        assert!(spread >= tight);
    }
}
