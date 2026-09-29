//! Layout that scatters nodes on a sunflower spiral inside a circle.

use cg_graph::{GraphStore, Positions};
use cg_types::Point2;

use crate::engine::LayoutEngine;

/// Golden angle between successive placement rays.
const GOLDEN_ANGLE: f32 = 2.399_963_2;

/// Deterministic pseudo-random scatter used as a starting arrangement before
/// force-directed refinement. Determinism keeps visual tests reproducible.
pub struct RandomLayout {
    radius: f32,
}

impl RandomLayout {
    pub fn new(radius: f32) -> Self {
        Self { radius }
    }
}

impl LayoutEngine for RandomLayout {
    fn layout(&self, store: &GraphStore, _previous: &Positions) -> Positions {
        let node_count = store.node_count().max(1) as f32;
        let mut result: Positions = Positions::new();
        for (ordinal, node) in store.node_ids().enumerate() {
            let ordinal = ordinal as f32;
            let angle = ordinal * GOLDEN_ANGLE;
            let radius = self.radius * (ordinal / node_count).sqrt();
            result.insert(
                node,
                Point2::new(radius * angle.cos(), radius * angle.sin()),
            );
        }
        result
    }

    fn name(&self) -> &'static str {
        "random"
    }
}
