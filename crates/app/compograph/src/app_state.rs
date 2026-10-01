//! Application state: constants, root view data, and construction.
//!
//! This module owns the `GraphWindow` fields, the panel constants, and the
//! small helpers every other view module builds on. Painting, algorithm
//! dispatch, and file dialogs live in the sibling modules; nothing here
//! paints or runs algorithms.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use cg_graph::{
    ChangeFilter, ClusterMetric, GraphChangeEvent, GraphStore, NodeIndex, subscribe_graph,
};
use cg_interact::{BoxSelectState, DragState, InteractLocks, SelectMode, SelectionState};
use cg_layout::{LayoutDriver, LayoutRegistry};
use cg_render::{
    BypassStore, Camera, DetailLevel, EdgeMapper, EdgeStylePatch, FrameMetrics, ImageCache,
    LodParams, NodeStylePatch, RetainedCache, SpatialIndex, StyleMapper, StyleSheet,
    subscribe_repaint,
};
use cg_types::{Point2, Vec2};
use gpui::{AppContext, Context, Entity, FocusHandle, Subscription, Task};

use crate::algo_panel::EdgePair;

/// Node count of the built-in smoke scene.
pub(crate) const DEMO_NODE_COUNT: usize = 12;

/// Clicks shorter than this viewport distance count as taps, not box selects.
pub(crate) const TAP_THRESHOLD: f32 = 4.0;

/// Key dismissing the current selection and any in-progress box select.
pub(crate) const DISMISS_KEY: &str = "escape";

/// PageRank refinement rounds per panel run.
pub(crate) const PAGERANK_ITERATIONS: usize = 20;

/// Markov inflation per panel run; larger values yield finer groups.
pub(crate) const MARKOV_INFLATION: f32 = 2.0;

/// Markov iteration budget per panel run.
pub(crate) const MARKOV_ITERATIONS: usize = 20;

/// K-means iteration budget per panel run.
pub(crate) const KMEANS_ITERATIONS: usize = 20;

/// Affinity propagation damping per panel run.
pub(crate) const AFFINITY_DAMPING: f32 = 0.5;

/// Affinity propagation iteration budget per panel run.
pub(crate) const AFFINITY_ITERATIONS: usize = 100;

/// Default cluster count for k-means panel runs.
pub(crate) const CLUSTER_K_DEFAULT: usize = 2;

/// Largest cluster count selectable from the panel.
pub(crate) const CLUSTER_K_MAX: usize = 10;

/// Default distance threshold for hierarchical and metric panel runs.
pub(crate) const CLUSTER_THRESHOLD_DEFAULT: f32 = 120.0;

/// Step of the panel threshold controls, in model units.
pub(crate) const CLUSTER_THRESHOLD_STEP: f32 = 20.0;

/// Largest distance threshold selectable from the panel.
pub(crate) const CLUSTER_THRESHOLD_MAX: f32 = 1000.0;

/// Damping step of the panel controls, clamped to the unit interval.
pub(crate) const DAMPING_STEP: f32 = 0.05;

/// Magnification applied to exported images.
pub(crate) const EXPORT_SCALE: f32 = 2.0;

/// True when the pressed key dismisses the selection.
pub(crate) fn is_dismiss_key(key: &str) -> bool {
    key == DISMISS_KEY
}

/// Ordinal of `node` within the sorted store order, for endpoint slots.
pub(crate) fn endpoint_slot(ids: &[NodeIndex], node: NodeIndex) -> Option<usize> {
    ids.iter().position(|candidate| *candidate == node)
}

/// Directory the file dialogs open in, falling back to scratch space.
pub(crate) fn working_directory() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Root view that owns the graph state and draws the canvas.
pub(crate) struct GraphWindow {
    pub(crate) store: Entity<GraphStore>,
    pub(crate) layout: Entity<LayoutDriver>,
    /// Kept alive so structural edits keep repainting the window.
    pub(crate) _refresh: Subscription,
    /// Kept alive so background layout passes repaint the window.
    pub(crate) _layout_observer: Subscription,
    /// Kept alive so structural edits invalidate the retained paint cache.
    pub(crate) _structure_mark: Subscription,
    pub(crate) camera: Camera,
    pub(crate) viewport: Vec2,
    pub(crate) drag: DragState,
    pub(crate) rubber: BoxSelectState,
    pub(crate) selection: SelectionState,
    pub(crate) selected_edges: Vec<(NodeIndex, NodeIndex)>,
    pub(crate) select_mode: SelectMode,
    pub(crate) locks: InteractLocks,
    pub(crate) neighbor_hops: usize,
    pub(crate) neighbor_nodes: BTreeSet<NodeIndex>,
    pub(crate) neighbor_edges: Vec<(NodeIndex, NodeIndex)>,
    pub(crate) spatial: SpatialIndex,
    pub(crate) focus: FocusHandle,
    pub(crate) layouts: Vec<&'static str>,
    pub(crate) menu_open: bool,
    pub(crate) sheet: StyleSheet,
    pub(crate) mapper: StyleMapper,
    pub(crate) edge_mapper: EdgeMapper,
    pub(crate) bypass: BypassStore,
    pub(crate) hovered: Option<NodeIndex>,
    pub(crate) hover_anchor: Point2,
    pub(crate) algo_nodes: HashMap<NodeIndex, NodeStylePatch>,
    pub(crate) algo_edges: HashMap<EdgePair, EdgeStylePatch>,
    pub(crate) algo_summary: String,
    pub(crate) algo_busy: bool,
    pub(crate) algo_generation: u64,
    pub(crate) algo_task: Option<Task<()>>,
    pub(crate) io_task: Option<Task<()>>,
    pub(crate) algo_start: usize,
    pub(crate) algo_goal: usize,
    pub(crate) damping: f32,
    pub(crate) cluster_k: usize,
    pub(crate) cluster_metric: ClusterMetric,
    pub(crate) cluster_threshold: f32,
    pub(crate) io_message: String,
    pub(crate) lod: DetailLevel,
    pub(crate) lod_params: LodParams,
    pub(crate) aggregate: bool,
    pub(crate) retained: RetainedCache,
    pub(crate) structure_version: u64,
    pub(crate) style_version: u64,
    pub(crate) spatial_version: Option<u64>,
    pub(crate) metrics: FrameMetrics,
    pub(crate) export_message: String,
    pub(crate) waypoints: cg_render::WaypointStore,
    pub(crate) images: ImageCache,
}

impl GraphWindow {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let store = cx.new(|_| GraphStore::new());
        // Seed the demo scene. Each mutation broadcasts through the store, and
        // the layout driver reacts on its own subscription.
        store.update(cx, |graph, cx| {
            for ordinal in 0..DEMO_NODE_COUNT {
                graph.add_node(cx, format!("n{ordinal}"));
            }
            let node_ids: Vec<_> = graph.node_ids().collect();
            for pair in node_ids.windows(2) {
                graph.add_edge(cx, pair[0], pair[1], 1.0);
            }
        });
        let layouts = LayoutRegistry::with_defaults().names();
        let first = layouts.first().copied().unwrap_or("force");
        let engine = LayoutRegistry::engine_for(first).expect("default layout is registered");
        let layout = cx.new(|cx| LayoutDriver::new(cx, &store, engine));
        // Structural edits repaint the window directly, independently of the
        // position changes the layout driver reports.
        let refresh = subscribe_repaint(cx, &store);
        // Structural edits also invalidate the retained paint cache, so a
        // later topology change can never reuse plans from before the edit.
        // The counter is the structure generation: every accepted mutation
        // moves it exactly once through this subscription.
        let structure_mark =
            subscribe_graph(
                cx,
                &store,
                ChangeFilter::ALL,
                |this, event, _cx| match event {
                    GraphChangeEvent::NodeAttrChanged(_)
                    | GraphChangeEvent::EdgeAttrChanged(_)
                    | GraphChangeEvent::NodeClassChanged(_)
                    | GraphChangeEvent::EdgeClassChanged(_) => {
                        this.style_version += 1;
                        this.retained.mark_structure();
                    }
                    _ => {
                        this.structure_version += 1;
                        this.retained.mark_structure();
                    }
                },
            );
        let layout_observer = cx.observe(&layout, |_this, _entity, cx| {
            cx.notify();
        });
        let view = Self {
            store,
            layout,
            _refresh: refresh,
            _structure_mark: structure_mark,
            _layout_observer: layout_observer,
            camera: Camera::new(Point2::ZERO, 1.0),
            viewport: Vec2::new(1024.0, 768.0),
            drag: DragState::default(),
            rubber: BoxSelectState::default(),
            selection: SelectionState::default(),
            selected_edges: Vec::new(),
            select_mode: SelectMode::Single,
            locks: InteractLocks::default(),
            neighbor_hops: 1,
            neighbor_nodes: BTreeSet::new(),
            neighbor_edges: Vec::new(),
            spatial: SpatialIndex::new(48.0),
            focus: cx.focus_handle(),
            layouts,
            menu_open: false,
            sheet: StyleSheet::default(),
            mapper: StyleMapper::default(),
            edge_mapper: EdgeMapper::new(),
            bypass: BypassStore::new(),
            hovered: None,
            hover_anchor: Point2::ZERO,
            algo_nodes: HashMap::new(),
            algo_edges: HashMap::new(),
            algo_summary: "no algorithm run yet".to_string(),
            algo_busy: false,
            algo_generation: 0,
            algo_task: None,
            io_task: None,
            algo_start: 0,
            algo_goal: 1,
            damping: 0.85,
            cluster_k: CLUSTER_K_DEFAULT,
            cluster_metric: ClusterMetric::Euclidean,
            cluster_threshold: CLUSTER_THRESHOLD_DEFAULT,
            io_message: String::new(),
            lod: DetailLevel::Full,
            lod_params: LodParams::default(),
            aggregate: false,
            retained: RetainedCache::new(false),
            structure_version: 0,
            style_version: 0,
            spatial_version: None,
            metrics: FrameMetrics::new(30),
            export_message: String::new(),
            waypoints: cg_render::WaypointStore::new(),
            images: cg_render::ImageCache::new(),
        };
        view.layout.update(cx, |driver, cx| {
            driver.request_refine(&view.store, cx);
        });
        view
    }

    pub(crate) fn viewport_point(position: gpui::Point<gpui::Pixels>) -> Point2 {
        Point2::new(f32::from(position.x), f32::from(position.y))
    }

    pub(crate) fn hit_radius(&self) -> f32 {
        (24.0 / self.camera.zoom).max(4.0)
    }

    pub(crate) fn switch_layout(&mut self, name: &'static str, cx: &mut Context<Self>) {
        let Some(engine) = LayoutRegistry::engine_for(name) else {
            return;
        };
        let store = self.store.clone();
        self.layout.update(cx, |driver, cx| {
            driver.set_engine_animated(&store, engine, 24, cg_layout::Easing::CubicInOut, cx);
        });
        self.menu_open = false;
        cx.notify();
    }
}
