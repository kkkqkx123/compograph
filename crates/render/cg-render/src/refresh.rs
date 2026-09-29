//! Keeps a view repainting when the graph structure changes.
//!
//! Painting reads node positions, so a structural edit usually reaches the
//! screen through the layout layer's own notification. This module closes the
//! remaining gap: an edit that leaves positions untouched still has to repaint,
//! because the set of nodes and edges the canvas iterates over has changed.

use cg_graph::{ChangeFilter, GraphStore, subscribe_graph};
use gpui::{App, Context, Entity, EntityId, Subscription};

/// Subscribes the calling view to structural changes of `store`.
///
/// The returned handle must be retained by the view: dropping it cancels the
/// repaint hook. Each accepted mutation marks the calling view dirty, so the
/// next frame re-runs painting against the updated structure. Call this from
/// inside the view that paints the graph, so the notification lands on that
/// view rather than on an intermediate entity.
pub fn subscribe_repaint<T>(cx: &mut Context<T>, store: &Entity<GraphStore>) -> Subscription
where
    T: 'static,
{
    let view: EntityId = cx.entity_id();
    subscribe_graph(
        cx,
        store,
        ChangeFilter::ALL,
        move |_this, _event, cx: &mut App| {
            cx.notify(view);
        },
    )
}
