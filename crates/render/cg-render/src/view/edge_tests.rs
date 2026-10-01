//! Edge paint plan tests covering bundling, routing and rebuild agreement.

use cg_geometry::OrthoDirection;
use cg_graph::{GraphView, MockGraph, NodeIndex, Positions};
use cg_types::{Point2, Rect, Vec2};

use super::edges::{
    EdgeOrdinal, edge_ordinals_for, paint_edges, paint_edges_for, paint_edges_for_with_waypoints,
    paint_edges_with_options, paint_single_edge, paint_single_edge_with_waypoints,
    painted_edge_hits,
};
use crate::camera::Camera;
use crate::lod::DetailLevel;
use crate::style::EdgeStyle;
use crate::view::{EdgePaintOptions, PaintedEdge};

fn viewport() -> Vec2 {
    Vec2::new(1024.0, 768.0)
}

fn camera() -> Camera {
    Camera::new(Point2::ZERO, 1.0)
}

fn edge_style(_source: NodeIndex, _target: NodeIndex) -> EdgeStyle {
    EdgeStyle::default()
}

#[test]
fn edges_skip_endpoints_without_positions() {
    let graph = MockGraph::chain(3);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-900.0, -900.0));
    positions.insert(NodeIndex::new(1), Point2::new(-880.0, -900.0));
    let edges = paint_edges(&graph, &positions, &camera(), viewport(), edge_style);
    assert!(edges.is_empty());
}

#[test]
fn lone_edge_stays_straight() {
    let graph = MockGraph::chain(2);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
    let edges = paint_edges(&graph, &positions, &camera(), viewport(), edge_style);
    assert_eq!(edges.len(), 1);
    assert!(edges[0].ctrl.is_none());
    assert!(edges[0].loop_ctrls.is_none());
}

#[test]
fn per_edge_curve_overrides_the_bundle_default() {
    use crate::style::EdgeCurve;

    let graph = MockGraph::chain(2);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    // Diagonal endpoints: taxi on an axis-aligned edge needs no corner,
    // so the bend assertion below requires a genuine turn.
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
    let forced = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
        EdgeStyle {
            curve: EdgeCurve::Bezier,
            ..EdgeStyle::default()
        }
    });
    assert_eq!(forced.len(), 1);
    assert!(forced[0].ctrl.is_some());
    let straight = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
        EdgeStyle {
            curve: EdgeCurve::Straight,
            ..EdgeStyle::default()
        }
    });
    assert!(straight[0].ctrl.is_none());
    let taxi = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
        EdgeStyle {
            curve: EdgeCurve::Taxi,
            ..EdgeStyle::default()
        }
    });
    assert!(!taxi[0].bends.is_empty());
    let single = paint_single_edge(
        EdgeOrdinal {
            pairs: &[(NodeIndex::new(0), NodeIndex::new(1))],
            ordinal: 0,
        },
        &positions,
        &camera(),
        viewport(),
        EdgePaintOptions::default(),
        |_, _| EdgeStyle {
            curve: EdgeCurve::Bezier,
            ..EdgeStyle::default()
        },
    )
    .expect("single edge plans the forced curve");
    assert!(single.ctrl.is_some());
}

#[test]
fn opposite_edges_curve_to_opposite_sides() {
    let mut graph = MockGraph::chain(2);
    graph.push_edge(1, 0);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
    let edges = paint_edges(&graph, &positions, &camera(), viewport(), edge_style);
    assert_eq!(edges.len(), 2);
    let ctrls: Vec<Option<Point2>> = edges.iter().map(|edge| edge.ctrl).collect();
    assert!(ctrls.iter().all(|ctrl| ctrl.is_some()));
    let first = ctrls[0].unwrap_or(Point2::ZERO);
    let second = ctrls[1].unwrap_or(Point2::ZERO);
    assert!((first.y - 384.0).abs() > 0.5);
    assert!((first.y + second.y - 2.0 * 384.0).abs() < 1e-3);
}

#[test]
fn self_loop_becomes_an_upward_cubic() {
    let mut graph = MockGraph::isolated(1);
    graph.push_edge(0, 0);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
    let edges = paint_edges(&graph, &positions, &camera(), viewport(), edge_style);
    assert_eq!(edges.len(), 1);
    let [ctrl_a, ctrl_b] = edges[0].loop_ctrls.unwrap_or([Point2::ZERO; 2]);
    assert_eq!(edges[0].start, edges[0].end);
    assert!(ctrl_a.y < edges[0].start.y && ctrl_b.y < edges[0].start.y);
}

#[test]
fn edge_opacity_rides_on_the_resolved_style() {
    let graph = MockGraph::chain(2);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
    let edges = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
        EdgeStyle {
            opacity: 0.25,
            ..EdgeStyle::default()
        }
    });
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].opacity, 0.25);
}

#[test]
fn painted_edge_hit_covers_straight_curved_and_loop() {
    use crate::arrows::ArrowKind;

    let straight = PaintedEdge {
        source: NodeIndex::new(0),
        target: NodeIndex::new(1),
        start: Point2::new(0.0, 0.0),
        end: Point2::new(10.0, 0.0),
        ctrl: None,
        loop_ctrls: None,
        bends: Vec::new(),
        aggregated: false,
        tint: 0,
        width: 1.0,
        opacity: 1.0,
        arrow: ArrowKind::Triangle,
        arrow_scale: 1.0,
    };
    let rect = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
    assert!(painted_edge_hits(&straight, rect));
    let far = Rect::new(Point2::new(4.0, 50.0), Vec2::new(2.0, 2.0));
    assert!(!painted_edge_hits(&straight, far));
    let curved = PaintedEdge {
        ctrl: Some(Point2::new(5.0, 10.0)),
        ..straight
    };
    let bulge = Rect::new(Point2::new(3.0, 3.0), Vec2::new(4.0, 4.0));
    assert!(painted_edge_hits(&curved, bulge));
}

#[test]
fn simplified_level_drops_bezier_controls() {
    use super::heads::paint_arrows_for_level;

    let mut graph = MockGraph::chain(2);
    graph.push_edge(1, 0);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
    let options = EdgePaintOptions {
        level: DetailLevel::Simplified,
        ..EdgePaintOptions::default()
    };
    let edges = paint_edges_with_options(
        &graph,
        &positions,
        &camera(),
        viewport(),
        options,
        edge_style,
    );
    assert_eq!(edges.len(), 2);
    assert!(edges.iter().all(|edge| edge.ctrl.is_none()));
    assert!(paint_arrows_for_level(&edges, DetailLevel::Minimal).is_empty());
    assert_eq!(paint_arrows_for_level(&edges, DetailLevel::Full).len(), 2);
}

#[test]
fn dense_bundle_renders_as_haystack_without_controls() {
    let mut graph = MockGraph::isolated(2);
    for _ in 0..6 {
        graph.push_edge(0, 1);
    }
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
    let options = EdgePaintOptions {
        force_haystack: true,
        ..EdgePaintOptions::default()
    };
    let edges = paint_edges_with_options(
        &graph,
        &positions,
        &camera(),
        viewport(),
        options,
        edge_style,
    );
    assert_eq!(edges.len(), 6);
    assert!(edges.iter().all(|edge| edge.ctrl.is_none()));
    assert!(edges.iter().all(|edge| edge.aggregated));
    let repeated = paint_edges_with_options(
        &graph,
        &positions,
        &camera(),
        viewport(),
        options,
        edge_style,
    );
    assert_eq!(
        edges.iter().map(|edge| edge.start).collect::<Vec<_>>(),
        repeated.iter().map(|edge| edge.start).collect::<Vec<_>>()
    );
}

#[test]
fn ortho_option_routes_with_capped_bends() {
    let graph = MockGraph::chain(2);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
    let options = EdgePaintOptions {
        ortho: Some(OrthoDirection::Auto),
        ..EdgePaintOptions::default()
    };
    let edges = paint_edges_with_options(
        &graph,
        &positions,
        &camera(),
        viewport(),
        options,
        edge_style,
    );
    assert_eq!(edges.len(), 1);
    assert!(edges[0].bends().len() <= 2);
    assert!(!edges[0].aggregated);
    let rect = Rect::from_corners(edges[0].start, edges[0].end);
    assert!(painted_edge_hits(&edges[0], rect));
}

#[test]
fn taxi_option_routes_through_a_single_corner() {
    let graph = MockGraph::chain(2);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
    let options = EdgePaintOptions {
        taxi: Some(OrthoDirection::HorizontalFirst),
        ..EdgePaintOptions::default()
    };
    let edges = paint_edges_with_options(
        &graph,
        &positions,
        &camera(),
        viewport(),
        options,
        edge_style,
    );
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].bends().len(), 1);
    assert!(!edges[0].aggregated);
    let corner = edges[0].bends()[0];
    assert_eq!(corner, Point2::new(edges[0].end.x, edges[0].start.y));
    assert!(painted_edge_hits(
        &edges[0],
        Rect::from_corners(edges[0].start, corner)
    ));
}

#[test]
fn single_edge_rebuild_matches_the_bulk_plan() {
    let mut graph = MockGraph::chain(3);
    graph.push_edge(1, 0);
    graph.push_edge(2, 2);
    let mut pairs = graph.edges();
    pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
    positions.insert(NodeIndex::new(2), Point2::new(-200.0, 40.0));
    let options = EdgePaintOptions::default();
    let bulk = paint_edges_for(
        &pairs,
        &positions,
        &camera(),
        viewport(),
        options,
        edge_style,
    );
    assert_eq!(bulk.len(), pairs.len());
    for (ordinal, pair) in pairs.iter().enumerate() {
        let single = paint_single_edge(
            EdgeOrdinal {
                pairs: &pairs,
                ordinal,
            },
            &positions,
            &camera(),
            viewport(),
            options,
            edge_style,
        )
        .unwrap_or_else(|| panic!("pair {pair:?} stays visible"));
        let from_bulk = bulk
            .iter()
            .find(|edge| edge.source == pair.0 && edge.target == pair.1)
            .cloned()
            .unwrap_or_else(|| panic!("bulk keeps {pair:?}"));
        assert_eq!(single.start, from_bulk.start);
        assert_eq!(single.end, from_bulk.end);
        assert_eq!(single.ctrl, from_bulk.ctrl);
        assert_eq!(single.loop_ctrls, from_bulk.loop_ctrls);
    }
    assert!(
        paint_single_edge(
            EdgeOrdinal {
                pairs: &pairs,
                ordinal: 99,
            },
            &positions,
            &camera(),
            viewport(),
            options,
            edge_style
        )
        .is_none()
    );
}

#[test]
fn edge_ordinals_track_parallel_edges_and_loops() {
    let mut graph = MockGraph::isolated(2);
    graph.push_edge(0, 1);
    graph.push_edge(0, 1);
    graph.push_edge(0, 0);
    let mut pairs = graph.edges();
    pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    assert_eq!(
        pairs,
        vec![
            (NodeIndex::new(0), NodeIndex::new(0)),
            (NodeIndex::new(0), NodeIndex::new(1)),
            (NodeIndex::new(0), NodeIndex::new(1)),
        ]
    );
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
    let edges = paint_edges_for(
        &pairs,
        &positions,
        &camera(),
        viewport(),
        EdgePaintOptions::default(),
        edge_style,
    );
    assert_eq!(edges.len(), 3);
    assert_eq!(edge_ordinals_for(&edges, &pairs), vec![0, 1, 2]);
}

#[test]
fn painted_loop_hit_matches_the_upward_geometry() {
    let mut graph = MockGraph::isolated(1);
    graph.push_edge(0, 0);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
    let loops = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
        EdgeStyle::default()
    });
    assert_eq!(loops.len(), 1);
    let above = Rect::new(Point2::new(412.0, 284.0), Vec2::new(200.0, 80.0));
    assert!(painted_edge_hits(&loops[0], above));
    let below = Rect::new(Point2::new(412.0, 500.0), Vec2::new(200.0, 80.0));
    assert!(!painted_edge_hits(&loops[0], below));
}

#[test]
fn waypoint_edge_routes_directly_and_hits_consistently() {
    use crate::waypoints::WaypointStore;

    let graph = MockGraph::chain(2);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
    let mut pairs = graph.edges();
    pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    let plain = paint_edges_for(
        &pairs,
        &positions,
        &camera(),
        viewport(),
        EdgePaintOptions::default(),
        edge_style,
    );
    assert_eq!(plain.len(), 1);
    assert!(plain[0].bends().is_empty());
    let mut store = WaypointStore::new();
    store.set_single(
        NodeIndex::new(0),
        NodeIndex::new(1),
        vec![Point2::new(-370.0, -30.0), Point2::new(-330.0, 60.0)],
    );
    let routed = paint_edges_for_with_waypoints(
        &pairs,
        &positions,
        &camera(),
        viewport(),
        EdgePaintOptions::default(),
        &store,
        edge_style,
    );
    assert_eq!(routed.len(), 1);
    assert_eq!(routed[0].bends().len(), 2);
    assert!(routed[0].ctrl.is_none());
    assert!(painted_edge_hits(
        &routed[0],
        Rect::from_corners(routed[0].start, routed[0].bends()[0])
    ));
    let single = paint_single_edge_with_waypoints(
        EdgeOrdinal {
            pairs: &pairs,
            ordinal: 0,
        },
        &positions,
        &camera(),
        viewport(),
        EdgePaintOptions::default(),
        &store,
        edge_style,
    )
    .expect("waypoint edge stays visible");
    assert_eq!(single.bends(), routed[0].bends());
}

#[test]
fn waypoint_cleaning_degrades_to_straight() {
    use crate::waypoints::WaypointStore;

    let graph = MockGraph::chain(2);
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
    let mut pairs = graph.edges();
    pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    let mut store = WaypointStore::new();
    store.set_single(
        NodeIndex::new(0),
        NodeIndex::new(1),
        vec![Point2::new(f32::NAN, 0.0), Point2::new(-400.0, 0.0)],
    );
    let routed = paint_edges_for_with_waypoints(
        &pairs,
        &positions,
        &camera(),
        viewport(),
        EdgePaintOptions::default(),
        &store,
        edge_style,
    );
    assert_eq!(routed.len(), 1);
    assert!(routed[0].bends().is_empty());
}

#[test]
fn waypoints_skip_haystack_and_manhattan_derivation() {
    use crate::waypoints::WaypointStore;

    let mut graph = MockGraph::isolated(2);
    for _ in 0..6 {
        graph.push_edge(0, 1);
    }
    let mut positions = Positions::new();
    positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
    positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
    let mut pairs = graph.edges();
    pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    let mut store = WaypointStore::new();
    store.set(
        NodeIndex::new(0),
        NodeIndex::new(1),
        0,
        vec![Point2::new(-350.0, -40.0)],
    );
    let options = EdgePaintOptions {
        force_haystack: true,
        ortho: Some(OrthoDirection::Auto),
        ..EdgePaintOptions::default()
    };
    let routed = paint_edges_for_with_waypoints(
        &pairs,
        &positions,
        &camera(),
        viewport(),
        options,
        &store,
        edge_style,
    );
    assert_eq!(routed.len(), 6);
    let first = routed
        .iter()
        .find(|edge| !edge.bends().is_empty())
        .expect("first parallel edge keeps waypoints");
    assert_eq!(first.bends().len(), 1);
    assert!(!first.aggregated);
}
