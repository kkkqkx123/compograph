//! Scale baseline recording plan and index cost at three graph sizes.
//!
//! Run with `cargo bench -p cg-render --bench scale`. Synthetic graphs are
//! deterministic, so repeated runs compare identical structure.

use std::time::Instant;

use cg_graph::MockGraph;
use cg_render::{
    Camera, EdgeStyle, NODE_SIDE, NodeStyle, SpatialIndex, chain_with_cross_edges,
    grid_with_random_edges, measure_ms, paint_edges, paint_nodes,
};
use cg_types::{Point2, Vec2};

fn measure(
    nodes: usize,
    edges: &[(cg_graph::NodeIndex, cg_graph::NodeIndex)],
    positions: &cg_graph::Positions,
) -> (f64, f64, usize, usize) {
    let mut graph = MockGraph::isolated(nodes);
    for (source, target) in edges {
        graph.push_edge(source.index(), target.index());
    }
    let camera = Camera::new(Point2::ZERO, 1.0);
    let viewport = Vec2::new(1024.0, 768.0);
    let started = Instant::now();
    let painted_nodes = paint_nodes(&graph, positions, &camera, viewport, |_| {
        NodeStyle::default()
    });
    let painted_edges = paint_edges(&graph, positions, &camera, viewport, |_, _| {
        EdgeStyle::default()
    });
    let plan_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut index = SpatialIndex::with_adaptive_cell(NODE_SIDE);
    let index_ms = measure_ms(|| index.rebuild(positions));
    (plan_ms, index_ms, painted_nodes.len(), painted_edges.len())
}

fn main() {
    println!("scale,graph,nodes,edges,plan_ms,index_ms,painted_nodes,painted_edges");
    for nodes in [1_000usize, 5_000, 10_000] {
        let grid = grid_with_random_edges(nodes, 0.3, 42);
        let (plan_ms, index_ms, painted_nodes, painted_edges) =
            measure(grid.node_count, &grid.edges, &grid.positions);
        println!(
            "scale,grid,{nodes},{},{plan_ms:.2},{index_ms:.2},{painted_nodes},{painted_edges}",
            grid.edges.len()
        );
        let chain = chain_with_cross_edges(nodes, 8, 11);
        let (plan_ms, index_ms, painted_nodes, painted_edges) =
            measure(chain.node_count, &chain.edges, &chain.positions);
        println!(
            "scale,chain,{nodes},{},{plan_ms:.2},{index_ms:.2},{painted_nodes},{painted_edges}",
            chain.edges.len()
        );
    }
}
