//! Name-based registry that maps layout identifiers to engine instances.

use std::collections::HashMap;

use crate::engine::LayoutEngine;

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
        fn layout(&self, _store: &cg_graph::GraphStore, _previous: &Positions) -> Positions {
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
}
