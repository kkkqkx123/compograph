//! Layout that scatters nodes on a sunflower spiral inside a circle.

use cg_graph::{FixedNodes, GraphView, Positions};
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
    fn layout(
        &self,
        graph: &dyn GraphView,
        _previous: &Positions,
        _fixed: &FixedNodes,
    ) -> Positions {
        let node_count = graph.node_count().max(1) as f32;
        let mut result: Positions = Positions::new();
        for (ordinal, node) in graph.node_ids().into_iter().enumerate() {
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

#[cfg(test)]
mod tests {
    use cg_graph::MockGraph;

    use super::*;

    #[test]
    fn scatter_runs_against_the_view_without_a_store() {
        let graph = MockGraph::chain(4);
        let layout = RandomLayout::new(100.0);
        let positions = layout.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 4);
    }
}
