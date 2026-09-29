//! Pointer-driven manipulation state: dragging, panning and selection.

use cg_graph::NodeIndex;
use cg_types::Vec2;

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
}
