//! Owns layout state and keeps it current as the graph mutates.

use cg_graph::{GraphChangeEvent, GraphStore, Positions, subscribe_graph};
use gpui::{App, Context, Entity, Subscription};

use crate::engine::LayoutEngine;
use crate::reaction::{LAYOUT_FILTER, LayoutWork, work_for};

/// Maximum number of incremental placements before a full re-run is cheaper.
///
/// Position-preserving layouts ring each new node around the existing
/// arrangement, so after enough single additions the result stops reflecting
/// the graph's shape. Re-running the engine periodically bounds that drift.
const INCREMENTAL_PLACEMENT_BUDGET: u32 = 64;

/// Keeps [`Positions`] in step with a live [`GraphStore`].
///
/// The driver holds the subscription that drives it and the running engine, so
/// a caller only hands over an entity and reads [`LayoutDriver::positions`]
/// when painting.
pub struct LayoutDriver {
    engine: Box<dyn LayoutEngine>,
    positions: Positions,
    placements_since_full_run: u32,
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
        let positions = {
            let snapshot = store.read(cx);
            engine.layout(snapshot, &Positions::new())
        };
        let subscription = subscribe_graph(cx, store, LAYOUT_FILTER, {
            let store = store.clone();
            move |this, event, cx| this.react(&store, event, cx)
        });
        Self {
            engine,
            positions,
            placements_since_full_run: 0,
            _subscription: subscription,
        }
    }

    /// Current model-space placement of every node.
    pub fn positions(&self) -> &Positions {
        &self.positions
    }

    /// The engine driving this layout, for identification and pickers.
    pub fn engine_name(&self) -> &'static str {
        self.engine.name()
    }

    /// Replaces the active engine and recomputes every position.
    ///
    /// The previous positions seed the new engine so switching layouts stays
    /// visually continuous where the two engines agree.
    pub fn set_engine(
        &mut self,
        store: &Entity<GraphStore>,
        engine: Box<dyn LayoutEngine>,
        cx: &mut App,
    ) {
        self.positions = engine.layout(store.read(cx), &self.positions);
        self.engine = engine;
        self.placements_since_full_run = 0;
    }

    /// Applies one graph change to the stored positions.
    ///
    /// Incremental placement reuses existing positions so untouched nodes do
    /// not jump, and falls back to a full run once the budget is exhausted.
    pub fn react(&mut self, store: &Entity<GraphStore>, event: &GraphChangeEvent, cx: &mut App) {
        match work_for(event) {
            LayoutWork::None => {}
            LayoutWork::Full => {
                self.positions = self.engine.layout(store.read(cx), &Positions::new());
                self.placements_since_full_run = 0;
            }
            LayoutWork::PlaceNew | LayoutWork::RefreshEdgeGeometry => {
                if self.placements_since_full_run >= INCREMENTAL_PLACEMENT_BUDGET {
                    self.positions = self.engine.layout(store.read(cx), &Positions::new());
                    self.placements_since_full_run = 0;
                } else {
                    let previous = std::mem::take(&mut self.positions);
                    self.positions = self.engine.layout(store.read(cx), &previous);
                    self.placements_since_full_run += 1;
                }
            }
        }
    }
}
