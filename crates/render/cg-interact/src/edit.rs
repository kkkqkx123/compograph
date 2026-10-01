//! Edit tools and actions for graph authoring gestures.
//!
//! Gestures only produce [`EditAction`] values; the graph store is never
//! touched here. Consumers validate each action against their own rules and
//! apply it to the store, which keeps undo and business checks on their side.

use cg_graph::NodeIndex;
use cg_types::Point2;

/// Active authoring tool of the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditTool {
    /// Pointer selects, drags and box-selects existing content.
    #[default]
    Select,
    /// Pointer pans the viewport.
    Pan,
    /// Tapping blank canvas requests a node at the tap point.
    AddNode,
    /// Dragging from one node to another requests an edge between them.
    Connect,
}

/// Structural change requested by a gesture.
///
/// Values carry everything the store needs to apply them; existence checks
/// stay with the caller because only it knows the current store contents.
#[derive(Clone, Debug, PartialEq)]
pub enum EditAction {
    /// Add a node at a model-space point.
    AddNode { at: Point2 },
    /// Add an edge from `source` to `target`.
    Connect {
        source: NodeIndex,
        target: NodeIndex,
    },
    /// Remove nodes in index order; incident edges go with them.
    RemoveNodes(Vec<NodeIndex>),
}

/// In-progress edge request from a press on a source node.
///
/// The draft completes when the pointer releases over a target node, which
/// keeps half-finished gestures representable without touching the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectDraft {
    /// Node where the gesture started.
    pub source: NodeIndex,
}

impl ConnectDraft {
    /// Completes the request with the release target.
    pub fn finish(self, target: NodeIndex) -> EditAction {
        EditAction::Connect {
            source: self.source,
            target,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_defaults_to_select() {
        assert_eq!(EditTool::default(), EditTool::Select);
    }

    #[test]
    fn draft_finishes_into_a_connect_action() {
        let draft = ConnectDraft {
            source: NodeIndex::new(2),
        };
        assert_eq!(
            draft.finish(NodeIndex::new(5)),
            EditAction::Connect {
                source: NodeIndex::new(2),
                target: NodeIndex::new(5),
            }
        );
    }

    #[test]
    fn actions_carry_their_store_payload() {
        let point = Point2::new(3.0, 4.0);
        let add = EditAction::AddNode { at: point };
        assert_eq!(add, EditAction::AddNode { at: point });
        let remove = EditAction::RemoveNodes(vec![NodeIndex::new(1), NodeIndex::new(7)]);
        let EditAction::RemoveNodes(members) = remove else {
            unreachable!("constructed a node removal");
        };
        assert_eq!(members, vec![NodeIndex::new(1), NodeIndex::new(7)]);
    }
}
