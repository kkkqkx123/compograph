//! Layout cost baseline: wall time and heap allocations per engine.
//!
//! Run with `cargo bench -p compograph-layout --bench layout_cost`. The
//! fixture is a production-shaped graph store, so the numbers include the
//! real query costs; a mock graph answers every query with a linear scan and
//! would measure the mock instead of the layout.
//!
//! The run has two passes. The first pass measures wall time, the second one
//! measures the allocations of the same cases. A counting allocator reports
//! every allocation of the process, so both numbers come from one run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use compograph_graph::{
    AdjacencySnapshot, FixedNodes, GraphStore, GraphView, MockGraph, Positions,
};
use compograph_layout::engine::LayoutEngine;
use compograph_layout::{
    BreadthFirstLayout, BreadthFirstOptions, CircleLayout, CircleOptions, ConcentricLayout,
    ConcentricOptions, ForceLayout, ForceOptions, GridLayout, GridOptions, HierarchicalLayout,
    HierarchicalOptions, PresetLayout, RadialLayout, RadialOptions, RandomLayout,
};
use gpui::{App, AppContext, Entity, TestAppContext, TestDispatcher};

/// Counting allocator that reports every allocation of the process.
///
/// A global allocator is the only place where the allocations of the
/// standard library show up as well, so the baseline counts them instead of
/// guessing from the call sites. The counters are read around a measured
/// region, so a report covers that region alone.
struct CountingAllocator;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Allocations and bytes since the counters were last read.
fn allocation_counters() -> (u64, u64) {
    (
        ALLOCATIONS.load(Ordering::Relaxed),
        ALLOCATED_BYTES.load(Ordering::Relaxed),
    )
}

/// Node counts the baseline walks through.
const SIZES: [usize; 4] = [1_000, 5_000, 10_000, 50_000];

/// Node counts used for the scaling sweep.
const SCALING_SIZES: [usize; 5] = [1_000, 2_000, 4_000, 8_000, 16_000];

/// Edge span of the chain, so every node has a few neighbours.
const CROSS_SPAN: usize = 8;

/// Iterations of the force simulation; one background chunk.
const FORCE_ITERATIONS: usize = 25;

/// Queries the baseline runs once per node.
const QUERIES: [(&str, u8); 4] = [
    ("node_ids", 0),
    ("degree", 1),
    ("neighbors", 2),
    ("successors", 3),
];

/// Engine under test with the node count it stays feasible at.
struct Entry {
    engine: Box<dyn LayoutEngine>,
    max_nodes: usize,
}

fn main() {
    let mut cx = TestAppContext::build(TestDispatcher::new(7), Some("layout_cost"));

    println!("time,kind,nodes,edges,ms");
    run_pass(&mut cx, false);

    println!();
    println!("alloc,kind,nodes,edges,allocs,bytes");
    run_pass(&mut cx, true);
}

/// Runs every case once, reporting either timings or allocation counts.
fn run_pass(cx: &mut TestAppContext, counting: bool) {
    for nodes in SIZES {
        let (store, edges) = build_store(cx, nodes);
        for entry in engines() {
            if nodes > entry.max_nodes {
                continue;
            }
            let (ms, allocs, bytes) = cx.update(|cx: &mut App| {
                let view: &dyn GraphView = store.read(cx);
                let snapshot = AdjacencySnapshot::capture(view);
                measure(entry.engine.as_ref(), &snapshot, counting)
            });
            report(
                counting,
                entry.engine.name(),
                nodes,
                edges,
                ms,
                allocs,
                bytes,
            );
        }
    }

    for nodes in SCALING_SIZES {
        let (store, edges) = build_store(cx, nodes);
        for entry in [
            Entry {
                engine: Box::new(HierarchicalLayout::with_options(
                    HierarchicalOptions::default(),
                )),
                max_nodes: usize::MAX,
            },
            Entry {
                engine: Box::new(BreadthFirstLayout::with_options(
                    BreadthFirstOptions::default(),
                )),
                max_nodes: usize::MAX,
            },
        ] {
            let (ms, allocs, bytes) = cx.update(|cx: &mut App| {
                let view: &dyn GraphView = store.read(cx);
                let snapshot = AdjacencySnapshot::capture(view);
                measure(entry.engine.as_ref(), &snapshot, counting)
            });
            report(
                counting,
                entry.engine.name(),
                nodes,
                edges,
                ms,
                allocs,
                bytes,
            );
        }
    }

    for nodes in [1_000usize, 10_000] {
        let (store, edges) = build_store(cx, nodes);
        for (label, probe) in QUERIES {
            let (ms, allocs, bytes) = cx.update(|cx: &mut App| {
                let view: &dyn GraphView = store.read(cx);
                measure_query(view, probe, counting)
            });
            report(counting, label, nodes, edges, ms, allocs, bytes);
        }
    }

    for nodes in [1_000usize, 5_000] {
        let (store, edges) = build_compound_store(cx, nodes);
        for (label, probe) in QUERIES {
            let (ms, allocs, bytes) = cx.update(|cx: &mut App| {
                let view: &dyn GraphView = store.read(cx);
                measure_query(view, probe, counting)
            });
            report(counting, label, nodes, edges, ms, allocs, bytes);
        }
    }

    for nodes in [1_000usize, 5_000, 10_000] {
        let mock = mock_graph(nodes);
        let engine = Box::new(BreadthFirstLayout::with_options(
            BreadthFirstOptions::default(),
        ));
        let snapshot = AdjacencySnapshot::capture(&mock);
        let (ms, allocs, bytes) = measure(engine.as_ref(), &snapshot, counting);
        report(
            counting,
            "breadthfirst",
            nodes,
            mock.edge_count(),
            ms,
            allocs,
            bytes,
        );
    }
}

/// Prints one measured case in the shape of the current pass.
fn report(
    counting: bool,
    kind: &str,
    nodes: usize,
    edges: usize,
    ms: f64,
    allocs: u64,
    bytes: u64,
) {
    if counting {
        println!("alloc,{kind},{nodes},{edges},{allocs},{bytes}");
    } else {
        println!("time,{kind},{nodes},{edges},{ms:.2}");
    }
}

/// Engines in registry order, with the options the default registry uses.
///
/// The node cap keeps the quadratic engines inside the time budget: the
/// hierarchical ranking walks its group table once per rank, and the force
/// simulation compares every node pair.
fn engines() -> Vec<Entry> {
    let linear = |engine: Box<dyn LayoutEngine>| Entry {
        engine,
        max_nodes: 50_000,
    };
    vec![
        linear(Box::new(GridLayout::with_options(GridOptions::default()))),
        linear(Box::new(CircleLayout::with_options(
            CircleOptions::default(),
        ))),
        linear(Box::new(RandomLayout::new(220.0))),
        linear(Box::new(PresetLayout::new(220.0))),
        linear(Box::new(ConcentricLayout::with_options(
            ConcentricOptions::default(),
        ))),
        linear(Box::new(RadialLayout::with_options(
            RadialOptions::default(),
        ))),
        linear(Box::new(BreadthFirstLayout::with_options(
            BreadthFirstOptions::default(),
        ))),
        Entry {
            engine: Box::new(HierarchicalLayout::with_options(
                HierarchicalOptions::default(),
            )),
            max_nodes: 50_000,
        },
        Entry {
            engine: Box::new(ForceLayout::with_options(ForceOptions {
                iterations: FORCE_ITERATIONS,
                ..ForceOptions::default()
            })),
            max_nodes: 10_000,
        },
    ]
}

/// Builds a store shaped like a chain with cross links.
fn build_store(cx: &mut TestAppContext, nodes: usize) -> (Entity<GraphStore>, usize) {
    let store = cx.update(|cx: &mut App| cx.new(|_| GraphStore::new()));
    let mut edges = 0usize;
    cx.update(|cx: &mut App| {
        store.update(cx, |graph, cx| {
            graph.batch(cx, |graph, cx| {
                let mut ids = Vec::with_capacity(nodes);
                for index in 0..nodes {
                    ids.push(graph.add_node(cx, format!("n{index}")));
                }
                for index in 1..nodes {
                    graph.add_edge(cx, ids[index - 1], ids[index], 1.0);
                    edges += 1;
                }
                let mut index = 0;
                while index + CROSS_SPAN < nodes {
                    graph.add_edge(cx, ids[index], ids[index + CROSS_SPAN], 1.0);
                    edges += 1;
                    index += CROSS_SPAN;
                }
            });
        });
    });
    (store, edges)
}

/// Builds a store whose nodes sit inside containers, so the visible-edge
/// path of the store answers every query.
fn build_compound_store(cx: &mut TestAppContext, nodes: usize) -> (Entity<GraphStore>, usize) {
    let (store, edges) = build_store(cx, nodes);
    cx.update(|cx: &mut App| {
        store.update(cx, |graph, cx| {
            graph.batch(cx, |graph, cx| {
                let ids: Vec<_> = graph.node_ids().collect();
                let mut index = 0;
                while index + 2 < ids.len() {
                    let _ = graph.set_parent(cx, ids[index + 1], Some(ids[index]));
                    index += 2;
                }
            });
        });
    });
    (store, edges)
}

/// The same shape as the store fixture, held by the headless mock.
fn mock_graph(nodes: usize) -> MockGraph {
    let mut mock = MockGraph::isolated(nodes);
    for index in 1..nodes {
        mock.push_edge(index - 1, index);
    }
    let mut index = 0;
    while index + CROSS_SPAN < nodes {
        mock.push_edge(index, index + CROSS_SPAN);
        index += CROSS_SPAN;
    }
    mock
}

/// Time and heap usage of one layout run.
fn measure(
    engine: &dyn LayoutEngine,
    snapshot: &AdjacencySnapshot,
    counting: bool,
) -> (f64, u64, u64) {
    let before = counting.then(allocation_counters);
    let started = Instant::now();
    let positions = engine.layout(snapshot, &Positions::new(), &FixedNodes::default());
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let after = counting.then(allocation_counters);
    std::hint::black_box(&positions);
    let (allocs, bytes) = match (before, after) {
        (Some((allocs, bytes)), Some((later_allocs, later_bytes))) => (
            later_allocs.saturating_sub(allocs),
            later_bytes.saturating_sub(bytes),
        ),
        _ => (0, 0),
    };
    (elapsed, allocs, bytes)
}

/// Time and heap usage of one query pass over every node.
fn measure_query(view: &dyn GraphView, probe: u8, counting: bool) -> (f64, u64, u64) {
    let before = counting.then(allocation_counters);
    let started = Instant::now();
    let mut count = 0usize;
    if probe == 0 {
        count = view.node_ids().len();
    } else {
        for node in view.node_ids() {
            match probe {
                1 => count += view.degree(node),
                2 => count += view.neighbors(node).len(),
                _ => count += view.successors(node).len(),
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let after = counting.then(allocation_counters);
    std::hint::black_box(count);
    let (allocs, bytes) = match (before, after) {
        (Some((allocs, bytes)), Some((later_allocs, later_bytes))) => (
            later_allocs.saturating_sub(allocs),
            later_bytes.saturating_sub(bytes),
        ),
        _ => (0, 0),
    };
    (elapsed, allocs, bytes)
}
