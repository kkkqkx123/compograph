//! Owns layout state and keeps it current as the graph mutates.

use std::collections::HashSet;

use cg_graph::{
    FixedNodes, GraphChangeEvent, GraphStore, GraphView, NodeIndex, Positions, subscribe_graph,
};
use gpui::{App, Context, Entity, Subscription, Task};

use crate::engine::LayoutEngine;
use crate::force::{ForceSimulation, snapshot_of};
use crate::reaction::{LAYOUT_FILTER, LayoutWork, work_for};

/// Maximum number of incremental placements before a full re-run is cheaper.
///
/// Position-preserving layouts ring each new node around the existing
/// arrangement, so after enough single additions the result stops reflecting
/// the graph's shape. Re-running the engine periodically bounds that drift.
const INCREMENTAL_PLACEMENT_BUDGET: u32 = 64;

/// Iterations computed per background chunk between write-backs.
const BACKGROUND_CHUNK_ITERATIONS: usize = 25;

/// Keeps [`Positions`] in step with a live [`GraphStore`].
///
/// The driver holds the subscription that drives it and the running engine, so
/// a caller only hands over an entity and reads [`LayoutDriver::positions`]
/// when painting.
pub struct LayoutDriver {
    engine: Box<dyn LayoutEngine>,
    positions: Positions,
    pinned: FixedNodes,
    placements_since_full_run: u32,
    /// Counts background refinements; stale tasks check it before writing back.
    generation: u64,
    /// Held so an in-flight refinement is cancelled when replaced or dropped.
    _task: Option<Task<()>>,
    _subscription: Subscription,
}

impl LayoutDriver {
    /// Builds a driver and runs `engine` once over the current graph.
    ///
    /// The driver owns its own subscription, so it is held as an entity of its
    /// own rather than nested inside the view that reads its positions.
    pub fn new(
        cx: &mut Context<Self>,
        store: &Entity<GraphStore>,
        engine: Box<dyn LayoutEngine>,
    ) -> Self {
        let store_snapshot = store.read(cx);
        let view: &dyn GraphView = store_snapshot;
        let positions = engine.layout(view, &Positions::new(), &FixedNodes::default());
        let subscription = subscribe_graph(cx, store, LAYOUT_FILTER, {
            let store = store.clone();
            move |this, event, cx| this.react(&store, event, cx)
        });
        Self {
            engine,
            positions,
            pinned: FixedNodes::default(),
            placements_since_full_run: 0,
            generation: 0,
            _task: None,
            _subscription: subscription,
        }
    }

    /// Current model-space placement of every node.
    pub fn positions(&self) -> &Positions {
        &self.positions
    }

    /// Nodes held in place by the user; layouts move around them.
    pub fn pinned(&self) -> &FixedNodes {
        &self.pinned
    }

    /// The engine driving this layout, for identification and pickers.
    pub fn engine_name(&self) -> &'static str {
        self.engine.name()
    }

    /// Replaces the active engine and recomputes every position.
    ///
    /// The previous positions seed the new engine so switching layouts stays
    /// visually continuous where the two engines agree. Any in-flight
    /// background refinement is abandoned so its late write-back cannot
    /// overwrite the fresh result.
    pub fn set_engine(
        &mut self,
        store: &Entity<GraphStore>,
        engine: Box<dyn LayoutEngine>,
        cx: &mut App,
    ) {
        self.generation += 1;
        self._task = None;
        let store_snapshot = store.read(cx);
        let view: &dyn GraphView = store_snapshot;
        self.positions = engine.layout(view, &self.positions, &self.pinned);
        self.engine = engine;
        self.placements_since_full_run = 0;
    }

    /// Holds `node` at its current coordinates during future layouts.
    pub fn pin(&mut self, node: NodeIndex) {
        self.pinned.insert(node);
    }

    /// Releases a node held by [`LayoutDriver::pin`].
    pub fn unpin(&mut self, node: NodeIndex) {
        self.pinned.remove(&node);
    }

    /// Moves a pinned node directly, for example while dragging.
    ///
    /// Returns false when the node has no known position and nothing moved.
    pub fn move_pinned(
        &mut self,
        node: NodeIndex,
        position: cg_types::Point2,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(slot) = self.positions.get_mut(&node) else {
            return false;
        };
        *slot = position;
        self.pinned.insert(node);
        cx.notify();
        true
    }

    /// Refines the current positions in the background when the engine is
    /// force-directed, and synchronously otherwise.
    ///
    /// Background work proceeds in chunks with a write-back after each chunk,
    /// so the canvas animates towards convergence. A newer refinement or
    /// engine swap bumps the generation, and stale chunks stop writing.
    pub fn request_refine(&mut self, store: &Entity<GraphStore>, cx: &mut Context<Self>) {
        let Some(options) = self.engine.force_options() else {
            let store_snapshot = store.read(cx);
            let view: &dyn GraphView = store_snapshot;
            let previous = std::mem::take(&mut self.positions);
            self.positions = self.engine.layout(view, &previous, &self.pinned);
            self.placements_since_full_run = 0;
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        let store_snapshot = store.read(cx);
        let snapshot = snapshot_of(store_snapshot as &dyn GraphView);
        let live: HashSet<NodeIndex> = snapshot.nodes.iter().copied().collect();
        let mut working = self.positions.clone();
        working.retain(|node, _| live.contains(node));
        for node in snapshot.nodes.iter() {
            if !working.contains_key(node) {
                working.insert(*node, cg_types::Point2::ZERO);
            }
        }
        let pinned = self.pinned.clone();
        let task = cx.spawn(
            async move |weak: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let mut simulation = ForceSimulation::new(options);
                let mut working = working;
                loop {
                    let pinned_chunk = pinned.clone();
                    let snapshot_chunk = snapshot.clone();
                    let outcome = cx
                        .background_executor()
                        .spawn(async move {
                            simulation.advance(
                                &snapshot_chunk,
                                &mut working,
                                &pinned_chunk,
                                BACKGROUND_CHUNK_ITERATIONS,
                            );
                            let settled = simulation.is_settled();
                            (simulation, working, settled)
                        })
                        .await;
                    let (simulation_back, working_back, settled) = outcome;
                    simulation = simulation_back;
                    working = working_back;
                    let stale = weak
                        .update(&mut *cx, |this: &mut Self, cx| {
                            if this.generation != generation {
                                return true;
                            }
                            this.positions = working.clone();
                            cx.notify();
                            false
                        })
                        .unwrap_or(true);
                    if stale || settled {
                        break;
                    }
                }
            },
        );
        self._task = Some(task);
    }

    /// Applies one graph change to the stored positions.
    ///
    /// Incremental placement reuses existing positions so untouched nodes do
    /// not jump, and falls back to a full run once the budget is exhausted.
    pub fn react(&mut self, store: &Entity<GraphStore>, event: &GraphChangeEvent, cx: &mut App) {
        match work_for(event) {
            LayoutWork::None => {}
            LayoutWork::Full => {
                self.run_full(store, cx);
            }
            LayoutWork::PlaceNew | LayoutWork::RefreshEdgeGeometry => {
                if self.placements_since_full_run >= INCREMENTAL_PLACEMENT_BUDGET {
                    self.run_full(store, cx);
                } else {
                    let previous = std::mem::take(&mut self.positions);
                    let store_snapshot = store.read(cx);
                    let view: &dyn GraphView = store_snapshot;
                    self.positions = self.engine.layout(view, &previous, &self.pinned);
                    self.placements_since_full_run += 1;
                }
            }
        }
    }

    fn run_full(&mut self, store: &Entity<GraphStore>, cx: &mut App) {
        self.generation += 1;
        self._task = None;
        let store_snapshot = store.read(cx);
        let view: &dyn GraphView = store_snapshot;
        self.positions = self.engine.layout(view, &Positions::new(), &self.pinned);
        self.placements_since_full_run = 0;
    }
}
