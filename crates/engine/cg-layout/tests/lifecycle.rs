//! Table-driven checks over the driver lifecycle record and shared options.
//!
//! Layout events are recorded in the driver, not broadcast, so assertions
//! read the drained record directly. Graph mutations still travel through
//! gpui subscriptions, keeping the deferred-delivery rule: mutate and assert
//! in separate updates.

use cg_graph::{GraphStore, NodeIndex};
use cg_layout::driver::LayoutDriver;
use cg_layout::engine::{CommonOptions, SortKey};
use cg_layout::grid::{GridLayout, GridOptions};
use cg_layout::lifecycle::LayoutEvent;
use cg_layout::{Easing, LayoutEngine};
use cg_types::Point2;
use gpui::{App, AppContext, Entity, TestAppContext};

fn grid_engine() -> Box<dyn LayoutEngine> {
    Box::new(GridLayout::with_options(GridOptions {
        center: Point2::ZERO,
        width: 300.0,
        height: 100.0,
        avoid_overlap: false,
        ..GridOptions::default()
    }))
}

fn setup(cx: &mut TestAppContext) -> (Entity<GraphStore>, Entity<LayoutDriver>) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let watched = store.clone();
    let layout = cx.update(|cx: &mut App| cx.new(|cx| LayoutDriver::new(cx, &watched, grid_engine())));
    cx.update(|cx| {
        layout.update(cx, |driver, _cx| {
            driver.drain_events();
        });
    });
    (store, layout)
}

fn drain(cx: &mut TestAppContext, layout: &Entity<LayoutDriver>) -> Vec<LayoutEvent> {
    cx.update(|cx| layout.update(cx, |driver, _cx| driver.drain_events()))
}

fn kinds(events: &[LayoutEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|event| match event {
            LayoutEvent::Started { .. } => "started",
            LayoutEvent::Stopped => "stopped",
            LayoutEvent::Finished { .. } => "finished",
        })
        .collect()
}

#[gpui::test]
fn full_runs_record_start_and_finish(cx: &mut TestAppContext) {
    let (store, layout) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "a");
        });
    });
    assert_eq!(kinds(&drain(cx, &layout)), vec!["started", "finished"]);
    cx.update(|cx| {
        store.update(cx, |graph, cx| graph.clear(cx));
    });
    assert_eq!(kinds(&drain(cx, &layout)), vec!["started", "finished"]);
}

#[gpui::test]
fn animated_swaps_finish_after_pumping_frames(cx: &mut TestAppContext) {
    let (store, layout) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "a");
            graph.add_node(cx, "b");
        });
    });
    drain(cx, &layout);
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            driver.set_engine_animated(&store, grid_engine(), 3, Easing::Linear, cx);
        });
    });
    assert_eq!(kinds(&drain(cx, &layout)), vec!["started"]);
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            assert!(driver.has_transition());
            assert!(driver.step_transition(&store, cx));
            assert!(driver.step_transition(&store, cx));
            assert!(!driver.step_transition(&store, cx));
            assert!(!driver.has_transition());
        });
    });
    assert_eq!(kinds(&drain(cx, &layout)), vec!["finished"]);
}

#[gpui::test]
fn stop_cancels_the_transition_and_freezes_positions(cx: &mut TestAppContext) {
    let (store, layout) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "a");
            graph.add_node(cx, "b");
        });
    });
    drain(cx, &layout);
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            driver.set_engine_animated(&store, grid_engine(), 8, Easing::Linear, cx);
            assert!(driver.step_transition(&store, cx));
        });
    });
    let frozen = cx.update(|cx| layout.read(cx).positions().clone());
    cx.update(|cx| {
        layout.update(cx, |driver, _cx| driver.stop());
    });
    assert_eq!(kinds(&drain(cx, &layout)), vec!["started", "stopped"]);
    cx.update(|cx| {
        layout.update(cx, |driver, cx| {
            assert!(!driver.has_transition());
            assert!(!driver.step_transition(&store, cx));
            assert_eq!(driver.positions(), &frozen);
        });
    });
    assert!(drain(cx, &layout).is_empty());
}

#[gpui::test]
fn settled_runs_leave_a_single_fit_request(cx: &mut TestAppContext) {
    let (store, layout) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "a");
        });
    });
    cx.update(|cx| {
        layout.update(cx, |driver, _cx| {
            assert!(driver.take_fit_request());
            assert!(!driver.take_fit_request());
        });
    });
    cx.update(|cx| {
        layout.update(cx, |driver, _cx| {
            driver.set_common_options(CommonOptions {
                fit_view: false,
                ..CommonOptions::default()
            });
        });
    });
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "b");
        });
    });
    cx.update(|cx| {
        layout.update(cx, |driver, _cx| {
            assert!(!driver.take_fit_request());
        });
    });
}

#[gpui::test]
fn shared_sort_inputs_reach_the_engine(cx: &mut TestAppContext) {
    let (store, layout) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let a = graph.add_node(cx, "a");
            let b = graph.add_node(cx, "b");
            let c = graph.add_node(cx, "c");
            graph.add_edge(cx, a, b, 1.0);
            graph.add_edge(cx, b, c, 1.0);
        });
    });
    let base = cx.update(|cx| layout.read(cx).positions().clone());
    cx.update(|cx| {
        layout.update(cx, |driver, _cx| {
            driver.set_common_options(CommonOptions {
                sort: SortKey::DegreeDesc,
                ..CommonOptions::default()
            });
        });
        store.update(cx, |graph, cx| {
            graph.add_node(cx, "d");
        });
    });
    let sorted = cx.update(|cx| layout.read(cx).positions().clone());
    assert_ne!(
        sorted.get(&NodeIndex::new(1)),
        base.get(&NodeIndex::new(1))
    );
}
