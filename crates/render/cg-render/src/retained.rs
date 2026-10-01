//! Retained paint cache refreshing on version differences.
//!
//! The cache owns the last node, edge and arrow plans plus the full reuse
//! key: three versions (structure, positions, style), the paint options and
//! the camera snapshot. Viewport motion only reprojects; only structural,
//! position or style changes rebuild geometry. Painting walks the cache
//! grouped by tint, keeping batch-friendly order.

use std::collections::HashSet;

use cg_graph::{NodeIndex, Positions};
use cg_types::{Point2, Vec2};

use crate::camera::Camera;
use crate::style::{EdgeStyle, NodeStyle};
use crate::view::{
    EdgePaintOptions, PaintedArrow, PaintedEdge, PaintedNode, paint_single_arrow,
    paint_single_edge_with_waypoints, paint_single_node,
};
use crate::waypoints::WaypointStore;

/// Three versions identifying what changed since the last refresh.
///
/// The triplet stays fixed: paint options and the camera snapshot are key
/// dimensions beside the versions, never extra version counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheVersions {
    pub structure: u64,
    pub positions: u64,
    pub style: u64,
}

/// Camera snapshot distinguishing reprojection from geometry rebuilds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraSnapshot {
    pub center: Point2,
    pub zoom: f32,
    pub viewport: Vec2,
}

impl CameraSnapshot {
    pub fn captures(camera: &Camera, viewport: Vec2) -> Self {
        Self {
            center: camera.center,
            zoom: camera.zoom,
            viewport,
        }
    }
}

/// Cached paint plans with dirty tracking.
///
/// Plans stay culled to the viewport they were built for. `node_order` holds
/// the candidate identifiers in plan order and `edge_ordinals` holds the
/// pair-list ordinal behind each cached edge and arrow, so partial refreshes
/// can locate entries without rebuilding the rest.
#[derive(Debug, Default)]
pub struct RetainedCache {
    nodes: Vec<PaintedNode>,
    node_order: Vec<NodeIndex>,
    edges: Vec<PaintedEdge>,
    edge_ordinals: Vec<usize>,
    arrows: Vec<PaintedArrow>,
    versions: CacheVersions,
    options: EdgePaintOptions,
    camera: CameraSnapshot,
    /// Nodes whose cached plans are stale.
    dirty_nodes: HashSet<NodeIndex>,
    /// True when the whole cache must be rebuilt.
    structure_dirty: bool,
    enabled: bool,
    populated: bool,
}

impl RetainedCache {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.structure_dirty = true;
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn versions(&self) -> CacheVersions {
        self.versions
    }

    pub fn is_populated(&self) -> bool {
        self.populated
    }

    pub fn nodes(&self) -> &[PaintedNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[PaintedEdge] {
        &self.edges
    }

    pub fn arrows(&self) -> &[PaintedArrow] {
        &self.arrows
    }

    /// Marks structural changes; the next refresh rebuilds affected bundles.
    pub fn mark_structure(&mut self) {
        self.structure_dirty = true;
    }

    /// Marks moved nodes; only their incident edges refresh.
    pub fn mark_moved(&mut self, nodes: impl IntoIterator<Item = NodeIndex>) {
        self.dirty_nodes.extend(nodes);
    }

    /// Marks style changes for the given nodes.
    pub fn mark_styled(&mut self, nodes: impl IntoIterator<Item = NodeIndex>) {
        self.dirty_nodes.extend(nodes);
    }

    /// Nodes currently dirty, for tests and targeted refresh.
    pub fn dirty_nodes(&self) -> Vec<NodeIndex> {
        let mut nodes: Vec<NodeIndex> = self.dirty_nodes.iter().copied().collect();
        nodes.sort_unstable_by_key(|node| node.index());
        nodes
    }

    /// True when only reprojection is needed.
    pub fn needs_reproject(&self, camera: &Camera, viewport: Vec2) -> bool {
        self.camera != CameraSnapshot::captures(camera, viewport)
    }

    /// Stores freshly built plans and clears dirty state.
    ///
    /// `node_order` carries the candidate identifiers in plan order and
    /// `edge_ordinals` carries the pair-list ordinal behind each cached edge
    /// and arrow, so later partial refreshes can locate entries. `options`
    /// joins the reuse key beside the versions and the camera snapshot.
    pub fn store(
        &mut self,
        plans: StoredPlans,
        versions: CacheVersions,
        options: EdgePaintOptions,
        camera: &Camera,
        viewport: Vec2,
    ) {
        self.nodes = plans.nodes;
        self.node_order = plans.node_order;
        self.edges = plans.edges;
        self.edge_ordinals = plans.edge_ordinals;
        self.arrows = plans.arrows;
        self.versions = versions;
        self.options = options;
        self.camera = CameraSnapshot::captures(camera, viewport);
        self.dirty_nodes.clear();
        self.structure_dirty = false;
        self.populated = true;
    }

    /// Refreshes moved nodes and their incident edges in place.
    ///
    /// Structure and style versions plus paint options must already match the
    /// cache; only the dirty set and the position generation may differ. Entries that leave
    /// the viewport are dropped and ones that enter are inserted, so a drag
    /// across the cull boundary still converges. Returns false when the cache
    /// cannot be patched, in which case the caller rebuilds fully.
    pub fn refresh_moved(
        &mut self,
        input: RefreshInput<'_>,
        node_style: impl Fn(NodeIndex) -> NodeStyle,
        edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        if !self.populated || self.structure_dirty || self.dirty_nodes.is_empty() {
            return false;
        }
        if self.options != input.options {
            return false;
        }
        if self.versions.structure != input.versions.structure
            || self.versions.style != input.versions.style
        {
            return false;
        }
        if self.camera != CameraSnapshot::captures(input.camera, input.viewport) {
            return false;
        }
        let moved: Vec<NodeIndex> = self.dirty_nodes();
        for node in &moved {
            match paint_single_node(
                *node,
                input.positions,
                input.camera,
                input.viewport,
                &node_style,
            ) {
                Some(entry) => match self.node_order.iter().position(|member| *member == *node) {
                    Some(slot) => self.nodes[slot] = entry,
                    None => {
                        self.node_order.push(*node);
                        self.nodes.push(entry);
                    }
                },
                None => {
                    if let Some(slot) = self.node_order.iter().position(|member| *member == *node) {
                        self.node_order.remove(slot);
                        self.nodes.remove(slot);
                    }
                }
            }
        }
        let moved_set: HashSet<NodeIndex> = moved.into_iter().collect();
        let draws_arrows = input.options.level.draws_arrows();
        for (ordinal, (source, target)) in input.pairs.iter().enumerate() {
            if !moved_set.contains(source) && !moved_set.contains(target) {
                continue;
            }
            let slot = self
                .edge_ordinals
                .iter()
                .position(|member| *member == ordinal);
            match paint_single_edge_with_waypoints(
                input.pairs,
                ordinal,
                input.positions,
                input.camera,
                input.viewport,
                input.options,
                input.waypoints,
                &edge_style,
            ) {
                Some(entry) => {
                    let arrow = draws_arrows.then(|| paint_single_arrow(&entry));
                    match slot {
                        Some(at) => {
                            self.edges[at] = entry;
                            if let Some(head) = arrow
                                && at < self.arrows.len()
                            {
                                self.arrows[at] = head;
                            }
                        }
                        None => {
                            let at = self
                                .edge_ordinals
                                .iter()
                                .position(|member| *member > ordinal)
                                .unwrap_or(self.edge_ordinals.len());
                            self.edge_ordinals.insert(at, ordinal);
                            self.edges.insert(at, entry);
                            if let Some(head) = arrow
                                && at <= self.arrows.len()
                            {
                                self.arrows.insert(at, head);
                            }
                        }
                    }
                }
                None => {
                    if let Some(at) = slot {
                        self.edge_ordinals.remove(at);
                        self.edges.remove(at);
                        if at < self.arrows.len() {
                            self.arrows.remove(at);
                        }
                    }
                }
            }
        }
        self.versions = input.versions;
        self.camera = CameraSnapshot::captures(input.camera, input.viewport);
        self.dirty_nodes.clear();
        true
    }

    /// True when cached plans can be reused for these versions.
    pub fn is_fresh(&self, versions: CacheVersions) -> bool {
        !self.structure_dirty && self.dirty_nodes.is_empty() && self.versions == versions
    }

    /// True when the cached plans can be reused as built for these inputs.
    ///
    /// Versions alone identify graph changes; paint options and the camera
    /// snapshot are key dimensions beside them, so every dimension must
    /// match. This keeps [`CacheVersions`] exactly three-dimensional: new
    /// inputs arrive as key dimensions here instead of extra versions.
    pub fn is_reusable(
        &self,
        versions: CacheVersions,
        options: EdgePaintOptions,
        camera: &Camera,
        viewport: Vec2,
    ) -> bool {
        self.enabled
            && self.populated
            && self.is_fresh(versions)
            && self.options == options
            && !self.needs_reproject(camera, viewport)
    }
}

/// One full plan set stored into the retained cache.
pub struct StoredPlans {
    pub nodes: Vec<PaintedNode>,
    pub node_order: Vec<NodeIndex>,
    pub edges: Vec<PaintedEdge>,
    pub edge_ordinals: Vec<usize>,
    pub arrows: Vec<PaintedArrow>,
}

/// Inputs for one partial refresh of moved nodes.
pub struct RefreshInput<'a> {
    pub pairs: &'a [(NodeIndex, NodeIndex)],
    pub positions: &'a Positions,
    pub camera: &'a Camera,
    pub viewport: Vec2,
    pub options: EdgePaintOptions,
    pub versions: CacheVersions,
    pub waypoints: &'a WaypointStore,
}

/// Incident-edge filter for targeted refresh.
///
/// Returns the subset of `edges` touching any node in `moved`.
pub fn incident_edges(edges: &[PaintedEdge], moved: &HashSet<NodeIndex>) -> Vec<usize> {
    edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| moved.contains(&edge.source) || moved.contains(&edge.target))
        .map(|(ordinal, _)| ordinal)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_graph::NodeIndex;

    fn edge_between(a: usize, b: usize) -> PaintedEdge {
        PaintedEdge {
            source: NodeIndex::new(a),
            target: NodeIndex::new(b),
            start: Point2::ZERO,
            end: Point2::ZERO,
            ctrl: None,
            loop_ctrls: None,
            bends: Vec::new(),
            aggregated: false,
            tint: 0,
            width: 1.0,
            opacity: 1.0,
            arrow: crate::arrows::ArrowKind::Triangle,
            arrow_scale: 1.0,
        }
    }

    #[test]
    fn moving_one_node_dirties_only_incident_edges() {
        let edges = vec![edge_between(0, 1), edge_between(1, 2), edge_between(2, 3)];
        let mut moved = HashSet::new();
        moved.insert(NodeIndex::new(1));
        assert_eq!(incident_edges(&edges, &moved), vec![0, 1]);
    }

    #[test]
    fn style_change_leaves_other_versions_untouched() {
        let mut cache = RetainedCache::new(true);
        let versions = CacheVersions {
            structure: 3,
            positions: 5,
            style: 1,
        };
        cache.store(
            StoredPlans {
                nodes: vec![],
                node_order: vec![],
                edges: vec![],
                edge_ordinals: vec![],
                arrows: vec![],
            },
            versions,
            EdgePaintOptions::default(),
            &Camera::new(Point2::ZERO, 1.0),
            Vec2::new(10.0, 10.0),
        );
        cache.mark_styled([NodeIndex::new(7)]);
        assert!(!cache.is_fresh(versions));
        assert_eq!(cache.dirty_nodes(), vec![NodeIndex::new(7)]);
    }

    #[test]
    fn partial_refresh_matches_a_full_rebuild() {
        use crate::style::{EdgeStyle, NodeStyle};
        use crate::view::{EdgePaintOptions, paint_arrows, paint_edges_for, paint_nodes_for};
        use cg_graph::Positions;

        let pairs = vec![
            (NodeIndex::new(0), NodeIndex::new(1)),
            (NodeIndex::new(2), NodeIndex::new(1)),
        ];
        let node_order = vec![NodeIndex::new(0), NodeIndex::new(1), NodeIndex::new(2)];
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-100.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(2), Point2::new(100.0, 0.0));
        let camera = Camera::new(Point2::ZERO, 1.0);
        let viewport = Vec2::new(1024.0, 768.0);
        let options = EdgePaintOptions::default();
        let node_style = |_: NodeIndex| NodeStyle::default();
        let edge_style = |_: NodeIndex, _: NodeIndex| EdgeStyle::default();
        let versions = CacheVersions {
            structure: 1,
            positions: 1,
            style: 1,
        };
        let mut cache = RetainedCache::new(true);
        let nodes = paint_nodes_for(&node_order, &positions, &camera, viewport, node_style);
        let edges = paint_edges_for(&pairs, &positions, &camera, viewport, options, edge_style);
        let arrows = paint_arrows(&edges);
        assert_eq!(edges.len(), 2);
        cache.store(
            StoredPlans {
                nodes,
                node_order: node_order.clone(),
                edges,
                edge_ordinals: vec![0, 1],
                arrows,
            },
            versions,
            EdgePaintOptions::default(),
            &camera,
            viewport,
        );

        positions.insert(NodeIndex::new(1), Point2::new(20.0, 10.0));
        cache.mark_moved([NodeIndex::new(1)]);
        let refreshed = CacheVersions {
            positions: 2,
            ..versions
        };
        assert!(cache.refresh_moved(
            RefreshInput {
                pairs: &pairs,
                positions: &positions,
                camera: &camera,
                viewport,
                options,
                versions: refreshed,
                waypoints: &WaypointStore::new(),
            },
            node_style,
            edge_style,
        ));
        let expect_nodes = paint_nodes_for(&node_order, &positions, &camera, viewport, node_style);
        let expect_edges =
            paint_edges_for(&pairs, &positions, &camera, viewport, options, edge_style);
        let expect_arrows = paint_arrows(&expect_edges);
        assert_eq!(format!("{:?}", cache.nodes()), format!("{expect_nodes:?}"));
        assert_eq!(format!("{:?}", cache.edges()), format!("{expect_edges:?}"));
        assert_eq!(
            format!("{:?}", cache.arrows()),
            format!("{expect_arrows:?}")
        );
        assert!(cache.is_fresh(refreshed));

        positions.insert(NodeIndex::new(2), Point2::new(5000.0, 0.0));
        cache.mark_moved([NodeIndex::new(2)]);
        let culled = CacheVersions {
            positions: 3,
            ..versions
        };
        assert!(cache.refresh_moved(
            RefreshInput {
                pairs: &pairs,
                positions: &positions,
                camera: &camera,
                viewport,
                options,
                versions: culled,
                waypoints: &WaypointStore::new(),
            },
            node_style,
            edge_style,
        ));
        let expect_nodes = paint_nodes_for(&node_order, &positions, &camera, viewport, node_style);
        assert_eq!(cache.nodes().len(), expect_nodes.len());
        assert_eq!(cache.nodes().len(), 2);
    }

    #[test]
    fn reuse_key_covers_options_beside_versions() {
        use crate::style::{EdgeStyle, NodeStyle};
        use cg_graph::Positions;

        let versions = CacheVersions {
            structure: 1,
            positions: 1,
            style: 1,
        };
        let camera = Camera::new(Point2::ZERO, 1.0);
        let viewport = Vec2::new(100.0, 100.0);
        let plans = || StoredPlans {
            nodes: vec![],
            node_order: vec![],
            edges: vec![],
            edge_ordinals: vec![],
            arrows: vec![],
        };
        let mut cache = RetainedCache::new(true);
        cache.store(
            plans(),
            versions,
            EdgePaintOptions::default(),
            &camera,
            viewport,
        );
        assert!(cache.is_reusable(versions, EdgePaintOptions::default(), &camera, viewport));
        let haystack = EdgePaintOptions {
            force_haystack: true,
            ..EdgePaintOptions::default()
        };
        assert!(!cache.is_reusable(versions, haystack, &camera, viewport));
        cache.mark_moved([NodeIndex::new(9)]);
        let positions = Positions::new();
        let pairs: Vec<(NodeIndex, NodeIndex)> = Vec::new();
        let node_style = |_: NodeIndex| NodeStyle::default();
        let edge_style = |_: NodeIndex, _: NodeIndex| EdgeStyle::default();
        assert!(!cache.refresh_moved(
            RefreshInput {
                pairs: &pairs,
                positions: &positions,
                camera: &camera,
                viewport,
                options: haystack,
                versions,
                waypoints: &WaypointStore::new(),
            },
            node_style,
            edge_style,
        ));
        let mut disabled = RetainedCache::new(false);
        disabled.store(
            plans(),
            versions,
            EdgePaintOptions::default(),
            &camera,
            viewport,
        );
        disabled.mark_moved([NodeIndex::new(9)]);
        assert!(!disabled.is_reusable(versions, EdgePaintOptions::default(), &camera, viewport));
        assert!(!disabled.refresh_moved(
            RefreshInput {
                pairs: &pairs,
                positions: &positions,
                camera: &camera,
                viewport,
                options: EdgePaintOptions::default(),
                versions,
                waypoints: &WaypointStore::new(),
            },
            node_style,
            edge_style,
        ));
    }

    #[test]
    fn camera_only_change_keeps_geometry_fresh() {
        let mut cache = RetainedCache::new(true);
        let versions = CacheVersions {
            structure: 1,
            positions: 1,
            style: 1,
        };
        let camera = Camera::new(Point2::ZERO, 1.0);
        cache.store(
            StoredPlans {
                nodes: vec![],
                node_order: vec![],
                edges: vec![],
                edge_ordinals: vec![],
                arrows: vec![],
            },
            versions,
            EdgePaintOptions::default(),
            &camera,
            Vec2::new(100.0, 100.0),
        );
        assert!(cache.is_fresh(versions));
        assert!(cache.needs_reproject(
            &Camera::new(Point2::new(5.0, 0.0), 1.0),
            Vec2::new(100.0, 100.0)
        ));
    }
}
