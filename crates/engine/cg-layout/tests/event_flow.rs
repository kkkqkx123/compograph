//! End-to-end checks that a graph edit reaches the layout through the event
//! subscription, exercising the same wiring the application uses.
//!
//! Event delivery is deferred: gpui queues emitted events and dispatches them
//! when the outermost update finishes. Each step below therefore performs its
//! mutation and its assertion in separate updates, so the queue drains in
//! between and the assertions observe a settled state.

use cg_graph::{ChangeFilter, GraphStore, Positions, subscribe_graph};
use cg_layout::anim::Easing;
use cg_layout::driver::LayoutDriver;
use cg_layout::random::RandomLayout;
use cg_layout::reaction::{LayoutWork, work_for};
use cg_types::Point2;
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
fn cancel_and_engine_switch_bump_the_generation(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(50.0))))
    });

    assert_eq!(cx.update(|cx| layout.read(cx).progress().generation), 0);
    cx.update(|cx| {
        layout.update(cx, |driver, _| driver.cancel());
    });
    assert_eq!(cx.update(|cx| layout.read(cx).progress().generation), 1);
    assert!(!cx.update(|cx| layout.read(cx).is_running()));
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            driver.set_engine(&store, Box::new(RandomLayout::new(50.0)), cx);
        });
    });
    cx.update(|cx| {
        let driver = layout.read(cx);
        assert_eq!(driver.progress().generation, 2);
        assert_eq!(driver.positions_version(), 1);
        assert!(!driver.is_running());
    });
}

#[gpui::test]
fn drag_pin_holds_the_node_and_versions_the_write(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "solo");
        });
    });
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(50.0))))
    });

    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            let node = store.read(cx).node_ids().next().expect("node exists");
            driver.pin(node);
            assert!(driver.pinned().contains(&node));
            assert!(driver.move_pinned(node, Point2::new(7.0, 8.0), cx));
            assert_eq!(driver.positions().get(&node), Some(&Point2::new(7.0, 8.0)));
            assert!(!driver.move_pinned(cg_graph::NodeIndex::new(99), Point2::ZERO, cx));
        });
    });
    assert_eq!(cx.update(|cx| layout.read(cx).positions_version()), 1);
}

#[gpui::test]
fn static_refine_on_small_graphs_lands_synchronously(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            for index in 0..3 {
                graph.add_node(cx, format!("n{index}"));
            }
        });
    });
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(50.0))))
    });

    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            driver.request_refine(&store, cx);
        });
    });
    cx.update(|cx| {
        let driver = layout.read(cx);
        assert!(!driver.is_running());
        assert_eq!(driver.positions_version(), 1);
        assert_eq!(driver.positions().len(), 3);
    });
}

#[gpui::test]
fn incremental_reaction_supersedes_the_generation(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(50.0))))
    });

    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "a");
        });
    });
    cx.update(|cx| {
        let driver = layout.read(cx);
        assert_eq!(driver.progress().generation, 1);
        assert_eq!(driver.positions_version(), 1);
        assert_eq!(driver.positions().len(), 1);
    });
}

#[gpui::test]
fn transition_blends_to_the_target_and_clears(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "a");
            graph.add_node(cx, "b");
        });
    });
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(50.0))))
    });

    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            let mut target = Positions::new();
            for node in store.read(cx).node_ids() {
                target.insert(node, Point2::new(100.0, 100.0));
            }
            driver.begin_transition(target.clone(), 2, Easing::Linear);
            assert!(driver.has_transition());
            assert_eq!(driver.transition_progress(), Some((0, 2)));
        });
    });
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            assert!(driver.step_transition(&store, cx));
            assert!(driver.has_transition());
            assert_eq!(driver.transition_progress(), Some((1, 2)));
        });
    });
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            assert!(!driver.step_transition(&store, cx));
            assert!(!driver.has_transition());
            for point in driver.positions().values() {
                assert_eq!(*point, Point2::new(100.0, 100.0));
            }
            assert!(!driver.step_transition(&store, cx));
        });
    });
}

#[gpui::test]
fn drag_and_engine_switch_cancel_the_transition(cx: &mut TestAppContext) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "solo");
        });
    });
    let layout = cx.update(|cx: &mut App| {
        cx.new(|cx| LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(50.0))))
    });

    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            let node = store.read(cx).node_ids().next().expect("node exists");
            let mut target = Positions::new();
            target.insert(node, Point2::new(90.0, 90.0));
            let before = driver.progress().generation;
            driver.begin_transition(target, 4, Easing::CubicInOut);
            assert!(driver.has_transition());
            assert_eq!(driver.progress().generation, before + 1);
        });
    });
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            let node = store.read(cx).node_ids().next().expect("node exists");
            assert!(driver.move_pinned(node, Point2::new(7.0, 8.0), cx));
            assert!(!driver.has_transition());
            assert_eq!(driver.positions().get(&node), Some(&Point2::new(7.0, 8.0)));
        });
    });
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            let node = store.read(cx).node_ids().next().expect("node exists");
            let mut target = Positions::new();
            target.insert(node, Point2::new(40.0, 40.0));
            driver.begin_transition(target, 4, Easing::Linear);
            assert!(driver.has_transition());
            driver.set_engine(&store, Box::new(RandomLayout::new(50.0)), cx);
            assert!(!driver.has_transition());
        });
    });
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
