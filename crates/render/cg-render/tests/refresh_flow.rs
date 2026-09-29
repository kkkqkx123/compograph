//! Checks that a graph edit reaches a repaint through the render refresh hook.
//!
//! Event delivery is deferred: gpui queues emitted events and dispatches them
//! when the outermost update finishes. Each step therefore performs its
//! mutation and its assertion in separate updates, so the queue drains in
//! between and the assertion observes a settled state.

use std::cell::Cell;
use std::rc::Rc;

use cg_graph::GraphStore;
use cg_render::subscribe_repaint;
use gpui::{App, AppContext, Context, Entity, Subscription, TestAppContext};

/// Stand-in for a painter view. It subscribes to the store the same way the
/// real graph view does; an observer counts the notifications that would
/// repaint it.
struct Painter {
    _subscription: Subscription,
}

impl Painter {
    fn new(cx: &mut Context<Self>, store: &Entity<GraphStore>) -> Self {
        Self {
            _subscription: subscribe_repaint(cx, store),
        }
    }
}

/// Store, painter and the observer that counts repaints. Keeping every handle
/// in one struct keeps them alive for the whole test.
struct Harness {
    store: Entity<GraphStore>,
    _painter: Entity<Painter>,
    repaints: Rc<Cell<usize>>,
    _observer: Subscription,
}

fn setup(cx: &mut TestAppContext) -> Harness {
    cx.update(|cx: &mut App| {
        let store = cx.new(|_| GraphStore::new());
        let painter = cx.new(|cx| Painter::new(cx, &store));
        let repaints = Rc::new(Cell::new(0));
        let counter = repaints.clone();
        // Observing the painter fires on every notify, standing in for a frame
        // that would repaint the canvas.
        let observer = cx.observe(&painter, move |_painter, _cx| {
            counter.set(counter.get() + 1);
        });
        Harness {
            store,
            _painter: painter,
            repaints,
            _observer: observer,
        }
    })
}

#[gpui::test]
fn structural_edit_marks_the_painter_dirty(cx: &mut TestAppContext) {
    let harness = setup(cx);
    assert_eq!(harness.repaints.get(), 0);

    cx.update(|cx| {
        harness.store.update(cx, |graph, cx| {
            graph.add_node(cx, "a");
        });
    });

    assert_eq!(
        harness.repaints.get(),
        1,
        "a node addition repaints the graph view"
    );
}

#[gpui::test]
fn reset_also_marks_the_painter_dirty(cx: &mut TestAppContext) {
    let harness = setup(cx);
    assert_eq!(harness.repaints.get(), 0);

    cx.update(|cx| {
        harness.store.update(cx, |graph, cx| graph.clear(cx));
    });

    assert_eq!(
        harness.repaints.get(),
        1,
        "a structure reset repaints the graph view"
    );
}
