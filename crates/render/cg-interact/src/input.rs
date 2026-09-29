//! Pointer-driven manipulation state: dragging, panning and selection.

use std::collections::BTreeSet;

use cg_graph::NodeIndex;
use cg_types::{Point2, Rect, Vec2};

use super::handlers::normalize_drag;

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

/// In-progress rubber-band box selection in viewport pixels.
#[derive(Default)]
pub struct BoxSelectState {
    start: Option<Point2>,
    current: Option<Point2>,
}

impl BoxSelectState {
    pub fn begin(&mut self, viewport_point: Point2) {
        self.start = Some(viewport_point);
        self.current = Some(viewport_point);
    }

    pub fn update(&mut self, viewport_point: Point2) {
        if self.start.is_some() {
            self.current = Some(viewport_point);
        }
    }

    pub fn end(&mut self) -> Option<(Point2, Point2)> {
        let span = self.start.zip(self.current);
        self.start = None;
        self.current = None;
        span
    }

    pub fn cancel(&mut self) {
        self.start = None;
        self.current = None;
    }

    pub fn is_active(&self) -> bool {
        self.start.is_some()
    }

    /// Normalized viewport rectangle of the current drag, if any.
    pub fn rect(&self) -> Option<Rect> {
        self.start
            .zip(self.current)
            .map(|(start, current)| normalize_drag(start, current))
    }
}

/// Multi-node selection state; empty selection carries no nodes.
///
/// The set stays ordered by node index so iteration is reproducible. The
/// single-node accessors mirror the previous single-selection API for
/// existing callers: `selected` returns the smallest member.
#[derive(Default)]
pub struct SelectionState {
    members: BTreeSet<usize>,
}

impl SelectionState {
    /// Replaces the selection with one node.
    pub fn select(&mut self, node: NodeIndex) {
        self.members.clear();
        self.members.insert(node.index());
    }

    /// Replaces the selection with the given nodes.
    pub fn select_many(&mut self, nodes: impl IntoIterator<Item = NodeIndex>) {
        self.members.clear();
        self.members
            .extend(nodes.into_iter().map(|node| node.index()));
    }

    /// Adds one node to the selection.
    pub fn add(&mut self, node: NodeIndex) {
        self.members.insert(node.index());
    }

    /// Adds several nodes to the selection.
    pub fn add_many(&mut self, nodes: impl IntoIterator<Item = NodeIndex>) {
        self.members
            .extend(nodes.into_iter().map(|node| node.index()));
    }

    /// Removes one node from the selection.
    pub fn remove(&mut self, node: NodeIndex) {
        self.members.remove(&node.index());
    }

    /// Toggles one node in the selection.
    pub fn toggle(&mut self, node: NodeIndex) {
        if !self.members.remove(&node.index()) {
            self.members.insert(node.index());
        }
    }

    pub fn clear(&mut self) {
        self.members.clear();
    }

    pub fn contains(&self, node: NodeIndex) -> bool {
        self.members.contains(&node.index())
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Smallest selected node, preserving the previous single-select API.
    pub fn selected(&self) -> Option<NodeIndex> {
        self.members
            .iter()
            .next()
            .map(|index| NodeIndex::new(*index))
    }

    /// Every selected node in index order.
    pub fn iter(&self) -> impl Iterator<Item = NodeIndex> + '_ {
        self.members.iter().map(|index| NodeIndex::new(*index))
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

    #[test]
    fn selection_accumulates_and_toggles_members() {
        let mut selection = SelectionState::default();
        selection.select(NodeIndex::new(2));
        selection.add(NodeIndex::new(5));
        assert_eq!(selection.len(), 2);
        assert!(selection.contains(NodeIndex::new(5)));
        selection.toggle(NodeIndex::new(2));
        assert!(!selection.contains(NodeIndex::new(2)));
        assert_eq!(selection.selected(), Some(NodeIndex::new(5)));
        selection.select_many([NodeIndex::new(7), NodeIndex::new(3)]);
        assert_eq!(
            selection.iter().collect::<Vec<_>>(),
            vec![NodeIndex::new(3), NodeIndex::new(7)]
        );
        selection.remove(NodeIndex::new(3));
        assert!(selection.contains(NodeIndex::new(7)));
        assert!(!selection.is_empty());
    }

    #[test]
    fn box_select_tracks_a_normalized_rect() {
        let mut rubber = BoxSelectState::default();
        assert!(!rubber.is_active());
        rubber.begin(Point2::new(30.0, 10.0));
        rubber.update(Point2::new(10.0, 40.0));
        let rect = rubber.rect().expect("active drag has a rect");
        assert_eq!(rect.origin, Point2::new(10.0, 10.0));
        assert_eq!(rect.size, Vec2::new(20.0, 30.0));
        let span = rubber.end().expect("span on release");
        assert_eq!(span, (Point2::new(30.0, 10.0), Point2::new(10.0, 40.0)));
        assert!(!rubber.is_active());
    }
}
