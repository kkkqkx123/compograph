//! Per-frame paint planning without any widgets.
//!
//! This module turns the current store, positions, and cache versions into
//! owned paint plans. It holds no callbacks; the view modules consume the
//! returned parts.

use std::collections::HashMap;
use std::time::Instant;

use cg_graph::{GraphView, NodeIndex, Positions};
use cg_interact::NODE_HALF_EXTENT;
use cg_render::{
    CacheVersions, DetailLevel, EdgePaintOptions, FrameSample, NodeShape,
    PaintedArrow, PaintedContainer, PaintedEdge, PaintedEdgeLabel, PaintedLabel, PaintedNode,
    PaintedRubberBand, PlanCounts, RefreshInput, StoredPlans, all_compound_bounds,
    clip_painted_edge, edge_ordinals_for, paint_arrows_for_level, paint_edge_labels_for,
    paint_edges_for_with_waypoints, paint_labels_for, paint_nodes_for_level, visible_node_ids,
    world_viewport_rect,
};
use cg_types::{Point2, Rect, Vec2};
use gpui::{Context, Window};

use crate::app_state::GraphWindow;

/// Owned paint plans one frame hands to the canvas.
pub(crate) struct FrameParts {
    pub(crate) containers: Vec<PaintedContainer>,
    pub(crate) nodes: Vec<PaintedNode>,
    pub(crate) edges: Vec<PaintedEdge>,
    pub(crate) arrows: Vec<PaintedArrow>,
    pub(crate) labels: Vec<PaintedLabel>,
    pub(crate) edge_labels: Vec<PaintedEdgeLabel>,
    pub(crate) rubber_band: Option<PaintedRubberBand>,
}

impl GraphWindow {
    /// Plans currently held by the retained cache.
    pub(crate) fn cached_plans(&self) -> (Vec<PaintedNode>, Vec<PaintedEdge>, Vec<PaintedArrow>) {
        (
            self.retained.nodes().to_vec(),
            self.retained.edges().to_vec(),
            self.retained.arrows().to_vec(),
        )
    }

    /// Steps the layout transition and plans the visible geometry.
    ///
    /// The retained cache is consulted first: a full hit reuses its plans,
    /// a moved-only refresh reuses the rest, otherwise the visible set is
    /// repainted from scratch. Metrics are pushed here so every frame,
    /// cached or not, reports one sample.
    pub(crate) fn prepare_frame(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FrameParts {
        let size = window.viewport_size();
        self.viewport = Vec2::new(f32::from(size.width), f32::from(size.height));
        let store_entity = self.store.clone();
        let still_running = self.layout.update(cx, |driver, cx| {
            if driver.has_transition() {
                driver.step_transition(&store_entity, cx)
            } else {
                false
            }
        });
        if still_running {
            cx.notify();
        }
        let store = self.store.read(cx);
        let view: &dyn GraphView = store;
        let positions: &Positions = self.layout.read(cx).positions();
        let positions_version = self.layout.read(cx).positions_version();
        let index_started = Instant::now();
        self.refresh_spatial(positions, positions_version);
        let index_ms = index_started.elapsed().as_secs_f64() * 1000.0;
        let mut labels: Vec<(NodeIndex, String, usize)> = Vec::new();
        for node in view.node_ids() {
            let label = store
                .node_data(node)
                .map(|data| data.label.clone())
                .unwrap_or_default();
            labels.push((node, label, view.degree(node)));
        }
        let sheet = &self.sheet;
        let mapper = &self.mapper;
        let edge_mapper = &self.edge_mapper;
        let bypass = &self.bypass;
        let style_of = |node: NodeIndex| {
            let entry = labels
                .iter()
                .find(|(id, _, _)| *id == node)
                .map(|(_, label, degree)| (label.as_str(), *degree));
            let (label, degree) = entry.unwrap_or(("", 0));
            let attrs = store.node_attrs(node);
            let classes = store.node_classes(node);
            bypass.resolve_node_with_data(
                sheet,
                mapper,
                node,
                Some(label),
                degree,
                &attrs,
                &classes,
            )
        };
        let edge_style_of = |source: NodeIndex, target: NodeIndex| {
            let (attrs, classes) = Self::edge_attrs_classes(store, source, target);
            bypass.resolve_edge_with_data(sheet, edge_mapper, source, target, &attrs, &classes)
        };
        let world_rect = world_viewport_rect(&self.camera, self.viewport);
        // First pass with squares for a provisional level; the minimal level
        // paints squares, so its visible set is the square set. Other levels
        // paint true shapes and refine the set with per-node shapes.
        let square_ids = visible_node_ids(
            positions,
            &self.spatial,
            world_rect,
            NODE_HALF_EXTENT,
            |_| NodeShape::Square,
        );
        let provisional = self
            .lod_params
            .select(self.camera.zoom, square_ids.len(), self.lod);
        let visible_ids = if provisional == DetailLevel::Minimal {
            square_ids
        } else {
            let shapes: HashMap<NodeIndex, NodeShape> = labels
                .iter()
                .map(|(node, label, degree)| {
                    let attrs = store.node_attrs(*node);
                    let classes = store.node_classes(*node);
                    (
                        *node,
                        bypass
                            .resolve_node_with_data(
                                sheet,
                                mapper,
                                *node,
                                Some(label.as_str()),
                                *degree,
                                &attrs,
                                &classes,
                            )
                            .shape,
                    )
                })
                .collect();
            visible_node_ids(
                positions,
                &self.spatial,
                world_rect,
                NODE_HALF_EXTENT,
                |node| shapes.get(&node).copied().unwrap_or_default(),
            )
        };
        self.lod = self
            .lod_params
            .select(self.camera.zoom, visible_ids.len(), provisional);
        let lod = self.lod;
        let edge_options = EdgePaintOptions {
            level: lod,
            aggregate_threshold: cg_render::EDGE_AGGREGATION_THRESHOLD,
            force_haystack: self.aggregate,
            ortho: None,
            taxi: None,
        };
        let versions = CacheVersions {
            structure: self.structure_version,
            positions: positions_version,
            style: self.style_version,
        };
        let mut pairs = view.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        let camera = self.camera;
        let viewport = self.viewport;
        let containers: Vec<PaintedContainer> = if store.has_compound() {
            all_compound_bounds(store, positions, NODE_HALF_EXTENT)
                .into_iter()
                .map(|(id, world_rect)| {
                    let far = Point2::new(
                        world_rect.origin.x + world_rect.size.x,
                        world_rect.origin.y + world_rect.size.y,
                    );
                    let screen_rect = Rect::from_corners(
                        camera.world_to_viewport(viewport, world_rect.origin),
                        camera.world_to_viewport(viewport, far),
                    );
                    let title = store
                        .node_data(id)
                        .map(|data| data.label.clone())
                        .unwrap_or_default();
                    PaintedContainer {
                        id,
                        rect: screen_rect,
                        title,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let plan_started = Instant::now();
        let cached_hit = self
            .retained
            .is_reusable(versions, edge_options, &camera, viewport);
        let partial_hit = !cached_hit
            && self.retained.refresh_moved(
                RefreshInput {
                    pairs: &pairs,
                    positions,
                    camera: &camera,
                    viewport,
                    options: edge_options,
                    versions,
                    waypoints: &self.waypoints,
                },
                style_of,
                edge_style_of,
            );
        let (nodes, edges, arrows) = if cached_hit || partial_hit {
            self.cached_plans()
        } else {
            let nodes =
                paint_nodes_for_level(&visible_ids, positions, &camera, viewport, lod, style_of);
            let raw_edges = paint_edges_for_with_waypoints(
                &pairs,
                positions,
                &camera,
                viewport,
                edge_options,
                &self.waypoints,
                edge_style_of,
            );
            let edges: Vec<PaintedEdge> = raw_edges
                .into_iter()
                .map(|edge| clip_painted_edge(&edge, &containers))
                .collect();
            let arrows = paint_arrows_for_level(&edges, lod);
            if self.retained.enabled() {
                let ordinals = edge_ordinals_for(&edges, &pairs);
                self.retained.store(
                    StoredPlans {
                        nodes: nodes.clone(),
                        node_order: visible_ids.clone(),
                        edges: edges.clone(),
                        edge_ordinals: ordinals,
                        arrows: arrows.clone(),
                    },
                    versions,
                    edge_options,
                    &camera,
                    viewport,
                );
            }
            (nodes, edges, arrows)
        };
        for node in &nodes {
            if let Some(spec) = node.image.clone()
                && self.images.request(&spec)
                && !std::path::Path::new(&spec.path).exists()
            {
                self.images.fail(&spec.path, "missing file");
            }
        }
        // Labels are planned from the visible node plan every frame rather than
        // cached: their cache key would need a text-shaping dimension the
        // retained geometry cache does not track, and shaping is cheap relative
        // to the geometry rebuilds it would key against.
        let label_of = |node: NodeIndex| {
            labels
                .iter()
                .find(|(id, _, _)| *id == node)
                .map(|(_, text, _)| text.clone())
        };
        let label_size_of = |node: NodeIndex| style_of(node).label_size;
        let painted_labels = paint_labels_for(&nodes, lod, label_of, label_size_of);
        let painted_edge_labels = paint_edge_labels_for(
            &edges,
            lod,
            |source, target| {
                store
                    .edge_weight(source, target)
                    .map(|weight| weight.to_string())
            },
            |source, target| edge_style_of(source, target).label_size,
        );
        let plan_ms = plan_started.elapsed().as_secs_f64() * 1000.0;
        self.metrics.push(FrameSample {
            counts: PlanCounts {
                nodes: nodes.len(),
                edges: edges.len(),
                arrows: arrows.len(),
            },
            plan_ms,
            index_ms,
            visible_nodes: visible_ids.len(),
        });
        let rubber_band: Option<PaintedRubberBand> =
            self.rubber.rect().map(|rect| PaintedRubberBand {
                origin: rect.origin,
                size: rect.size,
            });
        FrameParts {
            containers,
            nodes,
            edges,
            arrows,
            labels: painted_labels,
            edge_labels: painted_edge_labels,
            rubber_band,
        }
    }
}
