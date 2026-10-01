//! Table-driven checks over store event sequences, batching and undo.
//!
//! Event delivery is deferred: gpui queues emitted events and dispatches them
//! when the outermost update finishes. Mutations and assertions therefore
//! live in separate updates so the queue drains in between.

use cg_graph::{
    ChangeFilter, GraphDocument, GraphStore, Positions, subscribe_graph, sync_document,
};
use gpui::{App, AppContext, Context, Entity, Subscription, TestAppContext};

struct SequenceRecorder {
    seen: Vec<String>,
    _subscription: Subscription,
}

fn event_name(event: &cg_graph::GraphChangeEvent) -> String {
    match event {
        cg_graph::GraphChangeEvent::NodeAdded(_) => "node-added".to_string(),
        cg_graph::GraphChangeEvent::NodeRemoved(_) => "node-removed".to_string(),
        cg_graph::GraphChangeEvent::EdgeAdded(_) => "edge-added".to_string(),
        cg_graph::GraphChangeEvent::EdgeRemoved(_) => "edge-removed".to_string(),
        cg_graph::GraphChangeEvent::NodeAttrChanged(_) => "node-attr".to_string(),
        cg_graph::GraphChangeEvent::EdgeAttrChanged(_) => "edge-attr".to_string(),
        cg_graph::GraphChangeEvent::NodeClassChanged(_) => "node-class".to_string(),
        cg_graph::GraphChangeEvent::EdgeClassChanged(_) => "edge-class".to_string(),
        cg_graph::GraphChangeEvent::NodeDataChanged(_) => "node-data".to_string(),
        cg_graph::GraphChangeEvent::EdgeDataChanged(_) => "edge-data".to_string(),
        cg_graph::GraphChangeEvent::EdgeEndpointsChanged(_) => "edge-moved".to_string(),
        cg_graph::GraphChangeEvent::ParentChanged(_) => "parent".to_string(),
        cg_graph::GraphChangeEvent::CollapsedChanged(_) => "collapsed".to_string(),
        cg_graph::GraphChangeEvent::StructureReset => "reset".to_string(),
        cg_graph::GraphChangeEvent::BatchCommitted => "batch".to_string(),
    }
}

impl SequenceRecorder {
    fn new(cx: &mut Context<Self>, store: &Entity<GraphStore>) -> Self {
        let subscription = subscribe_graph(cx, store, ChangeFilter::ALL, |this, event, _cx| {
            this.seen.push(event_name(event));
        });
        Self {
            seen: Vec::new(),
            _subscription: subscription,
        }
    }
}

fn setup(cx: &mut TestAppContext) -> (Entity<GraphStore>, Entity<SequenceRecorder>) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let watched = store.clone();
    let recorder = cx.update(|cx: &mut App| cx.new(|cx| SequenceRecorder::new(cx, &watched)));
    (store, recorder)
}

fn seen(cx: &mut TestAppContext, recorder: &Entity<SequenceRecorder>) -> Vec<String> {
    cx.update(|cx| recorder.read(cx).seen.clone())
}

#[gpui::test]
fn nested_batches_broadcast_once(cx: &mut TestAppContext) {
    let (store, recorder) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.begin_batch();
            graph.add_node(cx, "a");
            graph.begin_batch();
            graph.add_node(cx, "b");
            graph.end_batch(cx);
            graph.add_node(cx, "c");
            assert!(graph.in_batch());
            graph.end_batch(cx);
        });
    });
    assert_eq!(seen(cx, &recorder), vec!["batch".to_string()]);
    assert_eq!(cx.update(|cx| store.read(cx).node_count()), 3);
}

#[gpui::test]
fn empty_batches_stay_silent(cx: &mut TestAppContext) {
    let (store, recorder) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            graph.begin_batch();
            assert!(!graph.end_batch(cx));
        });
    });
    assert!(seen(cx, &recorder).is_empty());
}

#[gpui::test]
fn node_removal_reports_edges_first_and_clears_tables(cx: &mut TestAppContext) {
    let (store, recorder) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let a = graph.add_node(cx, "a");
            let b = graph.add_node(cx, "b");
            let edge = graph.add_edge(cx, a, b, 1.0);
            graph.set_edge_attr(cx, edge, "kind", cg_graph::DataValue::Text("x".into()));
            graph.add_edge_class(cx, edge, "fast");
            graph.add_edge(cx, b, b, 2.0);
            graph.remove_node(cx, a);
        });
    });
    let sequence = seen(cx, &recorder);
    assert_eq!(
        sequence,
        vec![
            "node-added",
            "node-added",
            "edge-added",
            "edge-attr",
            "edge-class",
            "edge-added",
            "edge-removed",
            "node-removed",
        ]
    );
    cx.update(|cx| {
        store.update(cx, |graph, _cx| {
            assert_eq!(graph.node_count(), 1);
            assert_eq!(graph.edge_count(), 1);
            assert!(graph.edge_attrs(cg_graph::EdgeIndex::new(0)).is_empty());
        });
    });
}

#[gpui::test]
fn reconnect_moves_endpoints_without_add_or_remove(cx: &mut TestAppContext) {
    let (store, recorder) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let a = graph.add_node(cx, "a");
            let b = graph.add_node(cx, "b");
            let c = graph.add_node(cx, "c");
            let edge = graph.add_edge(cx, a, b, 3.0);
            graph.set_edge_attr(cx, edge, "kind", cg_graph::DataValue::Text("x".into()));
            let moved = graph.reconnect_edge(cx, edge, a, c).expect("reconnect works");
            assert_eq!(graph.edge_endpoints(moved), Some((a, c)));
            assert_eq!(
                graph.edge_attr(moved, "kind"),
                Some(cg_graph::DataValue::Text("x".into()))
            );
            assert!(graph.reconnect_edge(cx, moved, a, c).is_none());
            assert!(
                graph
                    .reconnect_edge(cx, moved, a, cg_graph::NodeIndex::new(99))
                    .is_none()
            );
        });
    });
    let sequence = seen(cx, &recorder);
    assert_eq!(
        sequence,
        vec![
            "node-added",
            "node-added",
            "node-added",
            "edge-added",
            "edge-attr",
            "edge-moved",
        ]
    );
}

#[gpui::test]
fn payload_edits_announce_data_events(cx: &mut TestAppContext) {
    let (store, recorder) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let a = graph.add_node(cx, "a");
            let b = graph.add_node(cx, "b");
            let edge = graph.add_edge(cx, a, b, 1.0);
            assert!(graph.set_node_label(cx, a, "alpha"));
            assert!(!graph.set_node_label(cx, a, "alpha"));
            assert!(!graph.set_node_label(cx, cg_graph::NodeIndex::new(99), "x"));
            assert!(graph.set_edge_weight(cx, edge, 2.5));
            assert!(!graph.set_edge_weight(cx, edge, f32::NAN));
            assert!(!graph.set_edge_weight(cx, cg_graph::EdgeIndex::new(77), 1.0));
        });
    });
    assert_eq!(
        seen(cx, &recorder),
        vec![
            "node-added",
            "node-added",
            "edge-added",
            "node-data",
            "edge-data",
        ]
    );
}

#[gpui::test]
fn undo_groups_batches_and_redo_reapplies(cx: &mut TestAppContext) {
    let (store, recorder) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            assert!(!graph.can_undo());
            assert!(!graph.undo(cx));
            let a = graph.add_node(cx, "a");
            graph.batch(cx, |graph, cx| {
                graph.add_node(cx, "b");
                graph.add_edge(cx, a, cg_graph::NodeIndex::new(1), 1.0);
                assert!(!graph.undo(cx));
            });
            assert_eq!(graph.history_len(), 2);
            assert!(graph.undo(cx));
            assert_eq!(graph.node_count(), 1);
            assert!(graph.redo(cx));
            assert_eq!(graph.node_count(), 2);
            assert_eq!(graph.edge_count(), 1);
            assert!(graph.undo(cx));
            assert_eq!(graph.node_count(), 1);
            assert!(graph.undo(cx));
            assert_eq!(graph.node_count(), 0);
            assert!(!graph.can_undo());
            assert!(graph.redo(cx));
            assert_eq!(graph.node_count(), 1);
        });
    });
    assert!(seen(cx, &recorder).contains(&"batch".to_string()));
}

#[gpui::test]
fn sync_updates_in_place_and_replaces_on_reshape(cx: &mut TestAppContext) {
    let (store, _) = setup(cx);
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let a = graph.add_node(cx, "a");
            let b = graph.add_node(cx, "b");
            graph.add_edge(cx, a, b, 1.0);
        });
    });
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let mut positions = Positions::new();
            let mut document =
                GraphDocument::collect_from_store(graph, &Positions::new());
            document.nodes[0].label = "alpha".to_string();
            document.nodes[0].position = Some([7.0, 8.0]);
            let report = sync_document(graph, cx, &document, &mut positions)
                .expect("in-place sync works");
            assert!(!report.replaced);
            assert_eq!(report.updated, 2);
            assert_eq!(report.added, 0);
            assert_eq!(graph.node_data(cg_graph::NodeIndex::new(0)).unwrap().label, "alpha");
        });
    });
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let mut positions = Positions::new();
            let document = GraphDocument {
                nodes: vec![
                    cg_graph::NodeEntry {
                        id: 10,
                        label: "x".to_string(),
                        ..Default::default()
                    },
                    cg_graph::NodeEntry {
                        id: 11,
                        label: "y".to_string(),
                        position: Some([1.0, 1.0]),
                        ..Default::default()
                    },
                ],
                edges: vec![cg_graph::EdgeEntry {
                    source: 10,
                    target: 11,
                    weight: 2.0,
                    ..Default::default()
                }],
            };
            let report =
                sync_document(graph, cx, &document, &mut positions).expect("replace works");
            assert!(report.replaced);
            assert_eq!(report.added, 2);
            assert_eq!(report.removed, 2);
            assert_eq!(graph.node_count(), 2);
            assert_eq!(graph.edge_count(), 1);
        });
    });
    cx.update(|cx| {
        store.update(cx, |graph, cx| {
            let mut positions = Positions::new();
            let bad = GraphDocument {
                nodes: vec![cg_graph::NodeEntry {
                    id: 0,
                    label: "a".to_string(),
                    ..Default::default()
                }],
                edges: vec![cg_graph::EdgeEntry {
                    source: 0,
                    target: 9,
                    weight: 1.0,
                    ..Default::default()
                }],
            };
            assert!(sync_document(graph, cx, &bad, &mut positions).is_err());
            assert_eq!(graph.node_count(), 2);
        });
    });
}
