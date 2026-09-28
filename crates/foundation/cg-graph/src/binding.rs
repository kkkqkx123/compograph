//! Subscription helpers that keep downstream stages in sync with the graph.
//!
//! Consumers that only care about a subset of mutations express that as a
//! [`ChangeFilter`] instead of matching on every variant at each call site.

use gpui::{Context, Entity, Subscription};

use crate::events::GraphChangeEvent;
use crate::store::GraphStore;

/// Predicate deciding whether a graph change is relevant to one consumer.
///
/// Keeping the decision in a value rather than inline in each callback makes
/// the reaction rules testable without an application instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeFilter {
    /// React when nodes are added or removed.
    pub structure: bool,
    /// React when edges are added or removed.
    pub topology: bool,
    /// React when the whole structure is replaced.
    pub reset: bool,
}

impl ChangeFilter {
    /// Reactive to every mutation, including full resets.
    pub const ALL: Self = Self {
        structure: true,
        topology: true,
        reset: true,
    };

    /// Reactive to node-level edits only.
    pub const NODES: Self = Self {
        structure: true,
        topology: false,
        reset: true,
    };

    /// Reactive to edge-level edits only.
    pub const EDGES: Self = Self {
        structure: false,
        topology: true,
        reset: true,
    };

    /// Whether `event` should reach a consumer holding this filter.
    pub fn accepts(&self, event: &GraphChangeEvent) -> bool {
        match event {
            GraphChangeEvent::NodeAdded(_) | GraphChangeEvent::NodeRemoved(_) => self.structure,
            GraphChangeEvent::EdgeAdded(_) | GraphChangeEvent::EdgeRemoved(_) => self.topology,
            GraphChangeEvent::StructureReset => self.reset,
        }
    }
}

/// Subscribe `T` to graph mutations accepted by `filter`.
///
/// The callback receives the application handle alongside the event, because
/// reacting usually means reading other entities that the event refers to.
/// The returned handle must be retained by the subscriber; dropping it cancels
/// the subscription. Events rejected by the filter never invoke `on_change`.
pub fn subscribe_graph<T>(
    cx: &mut Context<T>,
    store: &Entity<GraphStore>,
    filter: ChangeFilter,
    mut on_change: impl FnMut(&mut T, &GraphChangeEvent, &mut gpui::App) + 'static,
) -> Subscription
where
    T: 'static,
{
    cx.subscribe(store, move |this, _store, event, cx| {
        if filter.accepts(event) {
            on_change(this, event, cx);
        }
    })
}

#[cfg(test)]
mod tests {
    use petgraph::stable_graph::{EdgeIndex, NodeIndex};

    use super::*;

    #[test]
    fn all_filter_accepts_every_variant() {
        let events = [
            GraphChangeEvent::NodeAdded(NodeIndex::new(0)),
            GraphChangeEvent::NodeRemoved(NodeIndex::new(0)),
            GraphChangeEvent::EdgeAdded(EdgeIndex::new(0)),
            GraphChangeEvent::EdgeRemoved(EdgeIndex::new(0)),
            GraphChangeEvent::StructureReset,
        ];
        for event in &events {
            assert!(ChangeFilter::ALL.accepts(event));
        }
    }

    #[test]
    fn node_filter_rejects_edge_edits_but_keeps_resets() {
        let filter = ChangeFilter::NODES;
        assert!(filter.accepts(&GraphChangeEvent::NodeAdded(NodeIndex::new(1))));
        assert!(!filter.accepts(&GraphChangeEvent::EdgeAdded(EdgeIndex::new(1))));
        assert!(filter.accepts(&GraphChangeEvent::StructureReset));
    }

    #[test]
    fn edge_filter_rejects_node_edits_but_keeps_resets() {
        let filter = ChangeFilter::EDGES;
        assert!(!filter.accepts(&GraphChangeEvent::NodeRemoved(NodeIndex::new(1))));
        assert!(filter.accepts(&GraphChangeEvent::EdgeRemoved(EdgeIndex::new(1))));
        assert!(filter.accepts(&GraphChangeEvent::StructureReset));
    }
}
