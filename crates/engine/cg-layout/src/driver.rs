//! Owns layout state and keeps it current as the graph mutates.

use std::collections::HashSet;
use std::time::Instant;

use cg_graph::{
    FixedNodes, GraphChangeEvent, GraphStore, GraphView, NodeIndex, Positions, subscribe_graph,
};
use gpui::{App, Context, Entity, Subscription, Task};

use crate::compound::{CompoundSnapshot, apply_compound_postprocess};
use crate::engine::LayoutEngine;
use crate::force::{ForceSimulation, snapshot_of};
use crate::reaction::{LAYOUT_FILTER, LayoutWork, work_for};
use crate::static_view::StaticView;

/// Half extent of a node body used for compound group separation.
const COMPOUND_HALF_EXTENT: f32 = 12.0;

fn polish_compound(store: &GraphStore, positions: &mut Positions) {
    apply_compound_postprocess(store, positions, COMPOUND_HALF_EXTENT);
}

/// Maximum number of incremental placements before a full re-run is cheaper.
///
/// Position-preserving layouts ring each new node around the existing
/// arrangement, so after enough single additions the result stops reflecting
/// the graph's shape. Re-running the engine periodically bounds that drift.
const INCREMENTAL_PLACEMENT_BUDGET: u32 = 64;

/// Iterations computed per background chunk between write-backs.
const BACKGROUND_CHUNK_ITERATIONS: usize = 25;

/// Node count below which non-force engines run synchronously.
pub const SYNC_LAYOUT_NODE_LIMIT: usize = 2_000;

/// Progress of the active layout task for status display.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutProgress {
    pub generation: u64,
    pub chunks_written: u64,
    pub running: bool,
    pub last_ms: f64,
}

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
    /// Counts every committed position write; views rebuild indexes only when
    /// this moves, so viewport-only frames reuse the cached cells.
    positions_version: u64,
    /// Milliseconds of the latest refinement, updated on every write-back.
    last_refine_ms: f64,
    /// Completed background write-backs of the current generation.
    chunks_written: u64,
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
        let started = Instant::now();
        let mut positions = engine.layout(view, &Positions::new(), &FixedNodes::default());
        polish_compound(store_snapshot, &mut positions);
        let last_refine_ms = started.elapsed().as_secs_f64() * 1000.0;
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
            positions_version: 0,
            last_refine_ms,
            chunks_written: 0,
            _task: None,
            _subscription: subscription,
        }
    }

    /// Current model-space placement of every node.
    pub fn positions(&self) -> &Positions {
        &self.positions
    }

    /// Monotonic counter of committed position writes.
    ///
    /// Viewport motion never moves it; every layout write-back, drag move and
    /// import does. Rendering uses it to skip index rebuilds on camera-only
    /// frames.
    pub fn positions_version(&self) -> u64 {
        self.positions_version
    }

    /// Milliseconds of the latest refinement, for status display.
    ///
    /// Synchronous engines report the dispatch cost; force-directed engines
    /// update this on every background write-back, so it settles with the
    /// layout.
    pub fn last_refine_ms(&self) -> f64 {
        self.last_refine_ms
    }

    /// Progress snapshot of the active layout task.
    pub fn progress(&self) -> LayoutProgress {
        LayoutProgress {
            generation: self.generation,
            chunks_written: self.chunks_written,
            running: self._task.is_some(),
            last_ms: self.last_refine_ms,
        }
    }

    /// Cooperatively cancels the in-flight task.
    ///
    /// The task checks the generation at chunk boundaries, so cancellation
    /// takes effect on the next write-back without preemption.
    pub fn cancel(&mut self) {
        self.generation += 1;
        self._task = None;
    }

    /// True while a background task may still write back.
    pub fn is_running(&self) -> bool {
        self._task.is_some()
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
        self.chunks_written = 0;
        self.positions_version += 1;
        let store_snapshot = store.read(cx);
        let view: &dyn GraphView = store_snapshot;
        let mut positions = engine.layout(view, &self.positions, &self.pinned);
        polish_compound(store_snapshot, &mut positions);
        self.positions = positions;
        self.engine = engine;
        self.placements_since_full_run = 0;
    }

    /// Replaces every position at once, for example after a file import.
    ///
    /// Any in-flight background refinement is abandoned so its late
    /// write-back cannot overwrite the restored coordinates.
    pub fn replace_positions(&mut self, positions: Positions, cx: &mut Context<Self>) {
        self.generation += 1;
        self._task = None;
        self.chunks_written = 0;
        self.positions_version += 1;
        self.positions = positions;
        self.placements_since_full_run = 0;
        cx.notify();
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
        self.positions_version += 1;
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
            let node_count = store.read(cx).node_count();
            if node_count < SYNC_LAYOUT_NODE_LIMIT {
                let store_snapshot = store.read(cx);
                let view: &dyn GraphView = store_snapshot;
                let previous = std::mem::take(&mut self.positions);
                let started = Instant::now();
                let mut positions = self.engine.layout(view, &previous, &self.pinned);
                polish_compound(store_snapshot, &mut positions);
                self.positions = positions;
                self.last_refine_ms = started.elapsed().as_secs_f64() * 1000.0;
                self.placements_since_full_run = 0;
                self.chunks_written = 0;
                self.positions_version += 1;
                return;
            }
            self.run_static_in_background(store, cx);
            return;
        };
        self.generation += 1;
        self.chunks_written = 0;
        let generation = self.generation;
        let store_snapshot = store.read(cx);
        let compound = CompoundSnapshot::capture(store_snapshot);
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
        let started = Instant::now();
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
                            let mut polished = working.clone();
                            if let Some(snapshot) = compound.as_ref() {
                                snapshot.polish(&mut polished, COMPOUND_HALF_EXTENT);
                            }
                            this.positions = polished;
                            this.positions_version += 1;
                            this.last_refine_ms = started.elapsed().as_secs_f64() * 1000.0;
                            this.chunks_written += 1;
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
                    // The synchronous write supersedes any in-flight background
                    // task, so its late chunks must not overwrite this result.
                    self.generation += 1;
                    self._task = None;
                    self.chunks_written = 0;
                    let previous = std::mem::take(&mut self.positions);
                    let store_snapshot = store.read(cx);
                    let view: &dyn GraphView = store_snapshot;
                    let mut positions = self.engine.layout(view, &previous, &self.pinned);
                    polish_compound(store_snapshot, &mut positions);
                    self.positions = positions;
                    self.positions_version += 1;
                    self.placements_since_full_run += 1;
                }
            }
        }
    }

    fn run_full(&mut self, store: &Entity<GraphStore>, cx: &mut App) {
        self.generation += 1;
        self._task = None;
        self.chunks_written = 0;
        let store_snapshot = store.read(cx);
        let view: &dyn GraphView = store_snapshot;
        let mut positions = self.engine.layout(view, &Positions::new(), &self.pinned);
        polish_compound(store_snapshot, &mut positions);
        self.positions = positions;
        self.positions_version += 1;
        self.placements_since_full_run = 0;
    }

    /// Runs a static engine off the UI thread with a single write-back.
    ///
    /// Large static layouts still block for hundreds of milliseconds, so the
    /// computation moves to the background executor while the generation
    /// guard discards late results after cancellation or engine switches.
    /// Dragged nodes stay pinned because the snapshot carries them through.
    fn run_static_in_background(&mut self, store: &Entity<GraphStore>, cx: &mut Context<Self>) {
        self.generation += 1;
        self.chunks_written = 0;
        let generation = self.generation;
        let live = store.read(cx);
        let compound = CompoundSnapshot::capture(live);
        let snapshot = snapshot_of(live as &dyn GraphView);
        let previous = self.positions.clone();
        let pinned = self.pinned.clone();
        let engine_name = self.engine.name();
        let started = Instant::now();
        let task = cx.spawn(
            async move |weak: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let outcome = cx
                    .background_executor()
                    .spawn(async move {
                        let engine = crate::registry::LayoutRegistry::engine_for(engine_name)?;
                        let mut seeded = previous;
                        seeded.retain(|node, _| snapshot.nodes.contains(node));
                        Some(engine.layout(&StaticView::new(snapshot.clone()), &seeded, &pinned))
                    })
                    .await;
                weak.update(&mut *cx, |this: &mut Self, cx| {
                    if this.generation != generation {
                        return;
                    }
                    if let Some(mut positions) = outcome {
                        if let Some(snapshot) = compound.as_ref() {
                            snapshot.polish(&mut positions, COMPOUND_HALF_EXTENT);
                        }
                        this.positions = positions;
                        this.positions_version += 1;
                        this.last_refine_ms = started.elapsed().as_secs_f64() * 1000.0;
                        this.chunks_written += 1;
                        this.placements_since_full_run = 0;
                        cx.notify();
                    }
                    this._task = None;
                })
                .ok();
            },
        );
        self._task = Some(task);
    }
}
