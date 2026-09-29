//! End-to-end checks that a graph edit reaches the layout through the event
//! subscription, exercising the same wiring the application uses.
//!
//! Event delivery is deferred: gpui queues emitted events and dispatches them
//! when the outermost update finishes. Each step below therefore performs its
//! mutation and its assertion in separate updates, so the queue drains in
//! between and the assertions observe a settled state.

use cg_graph::{ChangeFilter, GraphStore, Positions, subscribe_graph};
use cg_layout::driver::LayoutDriver;
use cg_layout::random::RandomLayout;
use cg_layout::reaction::{LayoutWork, work_for};
use gpui::{App, AppContext, Context, Entity, Subscription, TestAppContext};

/// Records every event a subscriber observes, so tests can assert on the exact
/// sequence a consumer would see.
struct EventRecorder {
    seen: Vec<LayoutWork>,
    _subscription: Subscription,
}

impl EventRecorder {
    fn new(cx: &mut Context<Self>, store: &Entity<GraphStore>) -> Self {
        let subscription = subscribe_graph(cx, store, ChangeFilter::ALL, |this, event, _cx| {
            this.seen.push(work_for(event));
        });
        Self {
            seen: Vec::new(),
            _subscription: subscription,
        }
    }
}

#[gpui::test]
fn node_addition_reaches_the_layout_driver(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(100.0))))
    });

    assert_eq!(
        cx.update(|cx| layout.read(cx).positions().len()),
        0,
        "an empty graph has no positions"
    );

    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "a");
        });
    });

    assert_eq!(
        cx.update(|cx| layout.read(cx).positions().len()),
        1,
        "the driver places a node added after construction"
    );
}

#[gpui::test]
fn reset_rebuilds_every_position(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(100.0))))
    });

    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            for index in 0..5 {
                graph.add_node(cx, format!("n{index}"));
            }
        });
    });
    assert_eq!(cx.update(|cx| layout.read(cx).positions().len()), 5);

    cx.update(|cx| {
        store.update(cx, |graph, cx| graph.clear(cx));
    });

    assert_eq!(
        cx.update(|cx| layout.read(cx).positions().len()),
        0,
        "a structure reset drops stale positions"
    );
}

#[gpui::test]
fn subscriber_sees_the_mutations_in_order(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let recorder = cx.update(|cx: &mut App| cx.new(|cx| EventRecorder::new(cx, &store)));

    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let a = graph.add_node(cx, "a");
            let b = graph.add_node(cx, "b");
            graph.add_edge(cx, a, b, 1.0);
        });
    });

    assert_eq!(
        cx.update(|cx| recorder.read(cx).seen.clone()),
        vec![
            LayoutWork::PlaceNew,
            LayoutWork::PlaceNew,
            LayoutWork::RefreshEdgeGeometry,
        ],
        "each mutation maps to the work its consumers must do"
    );
}

#[gpui::test]
fn node_removal_keeps_the_remaining_node_count_consistent(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(100.0))))
    });

    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let a = graph.add_node(cx, "a");
            let b = graph.add_node(cx, "b");
            graph.add_edge(cx, a, b, 1.0);
        });
    });
    assert_eq!(cx.update(|cx| layout.read(cx).positions().len()), 2);

    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let a = graph.node_ids().next().expect("node exists");
            graph.remove_node(cx, a);
        });
    });

    assert_eq!(
        cx.update(|cx| layout.read(cx).positions().len()),
        1,
        "positions track the nodes that remain after removal"
    );
}

#[gpui::test]
fn engine_substitution_preserves_positions_it_receives(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "solo");
        });
    });
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(50.0))))
    });

    let before: Positions = cx.update(|cx| layout.read(cx).positions().clone());
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            driver.set_engine(&store, Box::new(RandomLayout::new(50.0)), cx);
        });
    });
    let after: Positions = cx.update(|cx| layout.read(cx).positions().clone());

    assert_eq!(before.len(), after.len());
    assert_eq!(before, after, "a deterministic engine reproduces itself");
}
