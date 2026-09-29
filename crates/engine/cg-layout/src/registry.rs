//! Name-based registry that maps layout identifiers to engine instances.

use std::collections::HashMap;

use crate::breadthfirst::{BreadthFirstLayout, BreadthFirstOptions};
use crate::circle::{CircleLayout, CircleOptions};
use crate::concentric::{ConcentricLayout, ConcentricOptions};
use crate::engine::LayoutEngine;
use crate::force::ForceLayout;
use crate::grid::{GridLayout, GridOptions};
use crate::hierarchical::{HierarchicalLayout, HierarchicalOptions};
use crate::preset::PresetLayout;
use crate::random::RandomLayout;

/// Radius of the scatter fallback layouts in the default registry.
pub const DEFAULT_DEMO_RADIUS: f32 = 220.0;

/// Holds one engine per registered layout name.
///
/// Dynamic dispatch is used because layouts are chosen at runtime from user
/// input, and the set of engines is open to future additions.
pub struct LayoutRegistry {
    engines: HashMap<&'static str, Box<dyn LayoutEngine>>,
}

impl LayoutRegistry {
    pub fn new() -> Self {
        Self {
            engines: HashMap::new(),
        }
    }

    pub fn register(&mut self, engine: Box<dyn LayoutEngine>) {
        let name = engine.name();
        self.engines.insert(name, engine);
    }

    /// Registry holding every built-in layout engine.
    ///
    /// Discrete engines use their default bounding boxes and ignore previous
    /// coordinates on placement; the force engine refines from them. Absent
    /// layouts stay absent instead of using placeholders.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(ForceLayout::new()));
        registry.register(Box::new(RandomLayout::new(DEFAULT_DEMO_RADIUS)));
        registry.register(Box::new(PresetLayout::new(DEFAULT_DEMO_RADIUS)));
        registry.register(Box::new(GridLayout::with_options(GridOptions::default())));
        registry.register(Box::new(CircleLayout::with_options(
            CircleOptions::default(),
        )));
        registry.register(Box::new(BreadthFirstLayout::with_options(
            BreadthFirstOptions::default(),
        )));
        registry.register(Box::new(ConcentricLayout::with_options(
            ConcentricOptions::default(),
        )));
        registry.register(Box::new(HierarchicalLayout::with_options(
            HierarchicalOptions::default(),
        )));
        registry
    }

    /// Builds one engine by name with default options, if registered.
    pub fn engine_for(name: &str) -> Option<Box<dyn LayoutEngine>> {
        match name {
            "force" => Some(Box::new(ForceLayout::new())),
            "random" => Some(Box::new(RandomLayout::new(DEFAULT_DEMO_RADIUS))),
            "preset" => Some(Box::new(PresetLayout::new(DEFAULT_DEMO_RADIUS))),
            "grid" => Some(Box::new(GridLayout::with_options(GridOptions::default()))),
            "circle" => Some(Box::new(CircleLayout::with_options(
                CircleOptions::default(),
            ))),
            "breadthfirst" => Some(Box::new(BreadthFirstLayout::with_options(
                BreadthFirstOptions::default(),
            ))),
            "concentric" => Some(Box::new(ConcentricLayout::with_options(
                ConcentricOptions::default(),
            ))),
            "hierarchical" => Some(Box::new(HierarchicalLayout::with_options(
                HierarchicalOptions::default(),
            ))),
            _ => None,
        }
    }

    pub fn get(&self, name: &str) -> Option<&dyn LayoutEngine> {
        self.engines.get(name).map(|engine| engine.as_ref())
    }

    /// Sorted identifiers of all registered layouts.
    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.engines.keys().copied().collect();
        names.sort_unstable();
        names
    }
}

impl Default for LayoutRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use cg_graph::Positions;

    use super::*;
    use crate::engine::LayoutEngine;
    use crate::preset::PresetLayout;

    struct Fixed;

    impl LayoutEngine for Fixed {
        fn layout(
            &self,
            _graph: &dyn cg_graph::GraphView,
            _previous: &Positions,
            _fixed: &cg_graph::FixedNodes,
        ) -> Positions {
            HashMap::new()
        }

        fn name(&self) -> &'static str {
            "fixed"
        }
    }

    #[test]
    fn registry_resolves_engines_by_name() {
        let mut registry = LayoutRegistry::new();
        registry.register(Box::new(PresetLayout::new(100.0)));
        registry.register(Box::new(Fixed));
        assert_eq!(registry.names(), vec!["fixed", "preset"]);
        assert!(registry.get("preset").is_some());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn default_registry_lists_all_builtin_layouts() {
        let registry = LayoutRegistry::with_defaults();
        assert_eq!(
            registry.names(),
            vec![
                "breadthfirst",
                "circle",
                "concentric",
                "force",
                "grid",
                "hierarchical",
                "preset",
                "random",
            ]
        );
        for name in registry.names() {
            assert!(registry.get(name).is_some());
            assert!(LayoutRegistry::engine_for(name).is_some());
        }
        assert!(LayoutRegistry::engine_for("missing").is_none());
    }
}
