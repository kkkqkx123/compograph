//! Layout that keeps caller-provided positions and rings new nodes.

use std::collections::HashMap;

use cg_graph::{FixedNodes, GraphView, Positions};
use cg_types::Point2;

use crate::engine::LayoutEngine;

/// Copies `previous` positions for existing nodes and places unseen nodes on
/// a circle of `fallback_radius`, so incremental edits never reshuffle the
/// whole graph.
pub struct PresetLayout {
    fallback_radius: f32,
}

impl PresetLayout {
    pub fn new(fallback_radius: f32) -> Self {
        Self { fallback_radius }
    }
}

impl LayoutEngine for PresetLayout {
    fn layout(
        &self,
        graph: &dyn GraphView,
        previous: &Positions,
        _fixed: &FixedNodes,
    ) -> Positions {
        let node_count = graph.node_count().max(1) as f32;
        let mut result: Positions = HashMap::new();
        let mut ordinal = 0.0f32;
        for node in graph.node_ids() {
            if let Some(position) = previous.get(&node) {
                result.insert(node, *position);
            } else {
                let angle = ordinal / node_count * std::f32::consts::TAU;
                result.insert(
                    node,
                    Point2::new(
                        self.fallback_radius * angle.cos(),
                        self.fallback_radius * angle.sin(),
                    ),
                );
            }
            ordinal += 1.0;
        }
        result
    }

    fn name(&self) -> &'static str {
        "preset"
    }
}
