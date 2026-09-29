//! Pointer-driven manipulation state: dragging, panning and selection.

use cg_graph::NodeIndex;
use cg_types::{Point2, Vec2};

/// A node currently held by the pointer.
pub struct DragGesture {
    pub node: NodeIndex,
    /// Offset from the node center to the grab point, in model units.
    pub grab_offset: Vec2,
}

/// Tracks the in-progress node drag, if any.
#[derive(Default)]
pub struct DragState {
    pub active: Option<DragGesture>,
}

impl DragState {
    pub fn begin(&mut self, node: NodeIndex, grab_offset: Vec2) {
        self.active = Some(DragGesture { node, grab_offset });
    }

    pub fn end(&mut self) -> Option<DragGesture> {
        self.active.take()
    }

    pub fn active_node(&self) -> Option<NodeIndex> {
        self.active.as_ref().map(|gesture| gesture.node)
    }
}

/// Canvas pan tracking the last pointer position in viewport pixels.
#[derive(Default)]
pub struct PanState {
    last_viewport: Option<Point2>,
}

impl PanState {
    pub fn begin(&mut self, viewport_point: Point2) {
        self.last_viewport = Some(viewport_point);
    }

    /// Advances the pan anchor, returning the viewport displacement.
    pub fn advance(&mut self, viewport_point: Point2) -> Option<Vec2> {
        let previous = self.last_viewport?;
        self.last_viewport = Some(viewport_point);
        Some(viewport_point - previous)
    }

    pub fn end(&mut self) {
        self.last_viewport = None;
    }

    pub fn is_active(&self) -> bool {
        self.last_viewport.is_some()
    }
}

/// Single-node selection state; empty selection carries none.
#[derive(Default)]
pub struct SelectionState {
    selected: Option<NodeIndex>,
}

impl SelectionState {
    pub fn select(&mut self, node: NodeIndex) {
        self.selected = Some(node);
    }

    pub fn clear(&mut self) {
        self.selected = None;
    }

    pub fn selected(&self) -> Option<NodeIndex> {
        self.selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_types::Vec2;

    #[test]
    fn drag_state_round_trips() {
        let mut state = DragState::default();
        assert!(state.active_node().is_none());
        state.begin(NodeIndex::new(3), Vec2::new(1.0, 2.0));
        assert_eq!(state.active_node(), Some(NodeIndex::new(3)));
        let gesture = state.end();
        assert_eq!(gesture.map(|g| g.node), Some(NodeIndex::new(3)));
        assert!(state.active_node().is_none());
    }

    #[test]
    fn pan_advance_reports_viewport_deltas() {
        let mut pan = PanState::default();
        assert!(!pan.is_active());
        pan.begin(Point2::new(10.0, 10.0));
        assert!(pan.is_active());
        assert_eq!(
            pan.advance(Point2::new(14.0, 12.0)),
            Some(Vec2::new(4.0, 2.0))
        );
        pan.end();
        assert!(!pan.is_active());
        assert_eq!(pan.advance(Point2::new(0.0, 0.0)), None);
    }

    #[test]
    fn selection_holds_a_single_node() {
        let mut selection = SelectionState::default();
        assert!(selection.selected().is_none());
        selection.select(NodeIndex::new(2));
        assert_eq!(selection.selected(), Some(NodeIndex::new(2)));
        selection.clear();
        assert!(selection.selected().is_none());
    }
}
