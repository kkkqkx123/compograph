//! Force-directed layout following the CoSE physical model.
//!
//! Each iteration applies node repulsion, edge springs and gravity, then moves
//! nodes by their accumulated offsets clamped to the current temperature. The
//! temperature cools geometrically, so motion settles instead of oscillating.
//! Compound nodes, nesting propagation and component separation from the
//! reference algorithm are intentionally absent: this engine refines flat
//! graphs only.

use std::collections::HashMap;

use cg_graph::{FixedNodes, GraphView, NodeIndex, Positions};
use cg_types::{Point2, Vec2};

use crate::engine::LayoutEngine;

/// Tuning parameters of the physical simulation.
///
/// Defaults stay close to the reference implementation the model is ported
/// from; adjust them when the feel of the layout needs to change rather than
/// altering the simulation itself. Spacing comes from these physical
/// parameters; the shared spacing factor never applies to force-directed
/// runs.
#[derive(Clone, Debug)]
pub struct ForceOptions {
    /// Repulsion strength between non-overlapping nodes.
    pub repulsion: f32,
    /// Repulsion multiplier applied to the overlap amount.
    pub overlap: f32,
    /// Rest length of an edge spring.
    pub ideal_length: f32,
    /// Divisor damping edge spring forces.
    pub elasticity: f32,
    /// Pull towards the graph centroid.
    pub gravity: f32,
    /// Upper bound on synchronous iterations.
    pub iterations: usize,
    /// First-iteration displacement cap.
    pub initial_temp: f32,
    /// Per-iteration temperature decay.
    pub cooling: f32,
    /// Temperature below which the simulation stops early.
    pub min_temp: f32,
    /// Uniform node extent used for overlap tests.
    pub node_size: f32,
}

impl Default for ForceOptions {
    fn default() -> Self {
        Self {
            repulsion: 2048.0,
            overlap: 4.0,
            ideal_length: 48.0,
            elasticity: 32.0,
            gravity: 1.0,
            iterations: 500,
            initial_temp: 128.0,
            cooling: 0.99,
            min_temp: 1.0,
            node_size: 24.0,
        }
    }
}

/// Force-directed engine implementing the shared layout contract.
pub struct ForceLayout {
    options: ForceOptions,
}

impl ForceLayout {
    pub fn new() -> Self {
        Self {
            options: ForceOptions::default(),
        }
    }

    pub fn with_options(options: ForceOptions) -> Self {
        Self { options }
    }

    /// Options driving this engine, reused for background refinement.
    pub fn options(&self) -> &ForceOptions {
        &self.options
    }
}

impl Default for ForceLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine for ForceLayout {
    fn layout(&self, graph: &dyn GraphView, previous: &Positions, fixed: &FixedNodes) -> Positions {
        let mut positions = seed_positions(graph, previous, self.options.ideal_length);
        let mut simulation = ForceSimulation::new(self.options.clone());
        let snapshot = snapshot_of(graph);
        simulation.advance(&snapshot, &mut positions, fixed, self.options.iterations);
        positions
    }

    fn name(&self) -> &'static str {
        "force"
    }

    fn force_options(&self) -> Option<ForceOptions> {
        Some(self.options.clone())
    }
}

/// Owned structural input a background task can refine without borrowing live state.
#[derive(Clone, Debug)]
pub struct ForceSnapshot {
    /// Nodes in deterministic index order.
    pub nodes: Vec<NodeIndex>,
    /// Directed edges as (source, target) pairs.
    pub edges: Vec<(NodeIndex, NodeIndex)>,
}

/// Captures the node and edge lists of a graph view in deterministic order.
pub fn snapshot_of(graph: &dyn GraphView) -> ForceSnapshot {
    let mut nodes = graph.node_ids();
    nodes.sort_unstable_by_key(|node| node.index());
    let mut edges = graph.edges();
    edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    ForceSnapshot { nodes, edges }
}

/// Resumable simulation carrying the cooling temperature across chunks.
///
/// Background refinement keeps one simulation and calls [`ForceSimulation::advance`]
/// per chunk, so cooling continues smoothly across write-backs.
pub struct ForceSimulation {
    options: ForceOptions,
    temperature: f32,
}

impl ForceSimulation {
    pub fn new(options: ForceOptions) -> Self {
        let temperature = options.initial_temp;
        Self {
            options,
            temperature,
        }
    }

    /// True once cooling has decayed below the minimum temperature.
    pub fn is_settled(&self) -> bool {
        self.temperature < self.options.min_temp
    }

    /// Runs up to `iterations` steps, stopping early below the minimum temperature.
    pub fn advance(
        &mut self,
        snapshot: &ForceSnapshot,
        positions: &mut Positions,
        fixed: &FixedNodes,
        iterations: usize,
    ) {
        for _ in 0..iterations {
            if self.temperature < self.options.min_temp {
                break;
            }
            simulation_step(snapshot, positions, fixed, &self.options, self.temperature);
            self.temperature *= self.options.cooling;
        }
    }
}

/// Places unseen nodes on a ring around the previous centroid.
///
/// Deterministic angles from the node index keep repeated runs identical.
fn seed_positions(graph: &dyn GraphView, previous: &Positions, ideal_length: f32) -> Positions {
    let mut seeded: Positions = HashMap::new();
    let mut known: Vec<Point2> = Vec::new();
    for node in graph.node_ids() {
        if let Some(position) = previous.get(&node) {
            seeded.insert(node, *position);
            known.push(*position);
        }
    }
    let centroid = centroid_of(&known);
    let mut missing: Vec<NodeIndex> = graph
        .node_ids()
        .into_iter()
        .filter(|node| !seeded.contains_key(node))
        .collect();
    missing.sort_unstable_by_key(|node| node.index());
    for (ordinal, node) in missing.into_iter().enumerate() {
        let angle = deterministic_angle(node, ordinal);
        let radius = ideal_length * 2.0;
        seeded.insert(
            node,
            Point2::new(
                centroid.x + radius * angle.cos(),
                centroid.y + radius * angle.sin(),
            ),
        );
    }
    seeded
}

fn centroid_of(points: &[Point2]) -> Point2 {
    if points.is_empty() {
        return Point2::ZERO;
    }
    let mut x = 0.0f32;
    let mut y = 0.0f32;
    for point in points {
        x += point.x;
        y += point.y;
    }
    Point2::new(x / points.len() as f32, y / points.len() as f32)
}

/// Deterministic angle in `[0, TAU)` derived from the node index.
fn deterministic_angle(node: NodeIndex, ordinal: usize) -> f32 {
    let mut hash = node.index() as u64 ^ 0x9E37_79B9_7F4A_7C15;
    hash ^= (ordinal as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(0x1656_67B1_9E37_79F9);
    let unit = ((hash >> 11) % 1_000_000) as f32 / 1_000_000.0;
    unit * std::f32::consts::TAU
}

fn simulation_step(
    snapshot: &ForceSnapshot,
    positions: &mut Positions,
    fixed: &FixedNodes,
    options: &ForceOptions,
    temperature: f32,
) {
    let order = &snapshot.nodes;
    let mut offsets: HashMap<NodeIndex, Vec2> = HashMap::new();
    for node in order {
        offsets.insert(*node, Vec2::ZERO);
    }
    apply_repulsion(order, positions, &mut offsets, options);
    apply_edge_forces(&snapshot.edges, positions, &mut offsets, options);
    apply_gravity(order, positions, &mut offsets, options);
    move_nodes(order, positions, fixed, &offsets, temperature);
}

/// Pairwise repulsion with an overlap fast path for touching boxes.
fn apply_repulsion(
    order: &[NodeIndex],
    positions: &Positions,
    offsets: &mut HashMap<NodeIndex, Vec2>,
    options: &ForceOptions,
) {
    for (left, node_a) in order.iter().enumerate() {
        let Some(a) = positions.get(node_a) else {
            continue;
        };
        for node_b in order.iter().skip(left + 1) {
            let Some(b) = positions.get(node_b) else {
                continue;
            };
            let mut dx = b.x - a.x;
            let mut dy = b.y - a.y;
            if dx == 0.0 && dy == 0.0 {
                let angle = deterministic_angle(*node_b, left);
                dx = angle.cos();
                dy = angle.sin();
            }
            let overlap = overlap_amount(*a, *b, dx, dy, options.node_size);
            let distance = (dx * dx + dy * dy).sqrt().max(0.5);
            let (force_x, force_y) = if overlap > 0.0 {
                let force = options.overlap * overlap;
                (force * dx / distance, force * dy / distance)
            } else {
                let clipped = clipped_distance(*a, *b, dx, dy, options.node_size).max(1.0);
                let force = 2.0 * options.repulsion / (clipped * clipped);
                (force * dx / distance, force * dy / distance)
            };
            add_offset(offsets, *node_a, Vec2::new(-force_x, -force_y));
            add_offset(offsets, *node_b, Vec2::new(force_x, force_y));
        }
    }
}

/// Amount by which two square node boxes overlap along the given direction.
fn overlap_amount(a: Point2, b: Point2, dx: f32, dy: f32, size: f32) -> f32 {
    let half = size / 2.0;
    let overlap_x = if dx > 0.0 {
        (a.x + half) - (b.x - half)
    } else {
        (b.x + half) - (a.x - half)
    };
    let overlap_y = if dy > 0.0 {
        (a.y + half) - (b.y - half)
    } else {
        (b.y + half) - (a.y - half)
    };
    if overlap_x >= 0.0 && overlap_y >= 0.0 {
        (overlap_x * overlap_x + overlap_y * overlap_y).sqrt()
    } else {
        0.0
    }
}

/// Distance between the border clipping points of two square nodes.
fn clipped_distance(a: Point2, b: Point2, dx: f32, dy: f32, size: f32) -> f32 {
    let first = clipping_point(a, dx, dy, size);
    let second = clipping_point(b, -dx, -dy, size);
    let gap_x = second.x - first.x;
    let gap_y = second.y - first.y;
    (gap_x * gap_x + gap_y * gap_y).sqrt()
}

/// Intersection of a ray with a square box boundary.
fn clipping_point(node: Point2, dx: f32, dy: f32, size: f32) -> Point2 {
    let half = size / 2.0;
    if dx == 0.0 {
        return Point2::new(node.x, node.y + half * dy.signum());
    }
    if dy == 0.0 {
        return Point2::new(node.x + half * dx.signum(), node.y);
    }
    let slope = (dy / dx).abs();
    if slope <= 1.0 {
        Point2::new(
            node.x + half * dx.signum(),
            node.y + half * slope * dy.signum(),
        )
    } else {
        Point2::new(
            node.x + half / slope * dx.signum(),
            node.y + half * dy.signum(),
        )
    }
}

/// Spring forces pulling edge endpoints towards the rest length.
fn apply_edge_forces(
    edges: &[(NodeIndex, NodeIndex)],
    positions: &Positions,
    offsets: &mut HashMap<NodeIndex, Vec2>,
    options: &ForceOptions,
) {
    for (source, target) in edges {
        let (Some(a), Some(b)) = (positions.get(source), positions.get(target)) else {
            continue;
        };
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        if dx == 0.0 && dy == 0.0 {
            continue;
        }
        let first = clipping_point(*a, dx, dy, options.node_size);
        let second = clipping_point(*b, -dx, -dy, options.node_size);
        let gap_x = second.x - first.x;
        let gap_y = second.y - first.y;
        let length = (gap_x * gap_x + gap_y * gap_y).sqrt();
        if length == 0.0 {
            continue;
        }
        let stretch = options.ideal_length - length;
        let force = stretch * stretch / options.elasticity;
        let force_x = force * gap_x / length;
        let force_y = force * gap_y / length;
        add_offset(offsets, *source, Vec2::new(force_x, force_y));
        add_offset(offsets, *target, Vec2::new(-force_x, -force_y));
    }
}

/// Weak pull of every free node towards the centroid.
fn apply_gravity(
    order: &[NodeIndex],
    positions: &Positions,
    offsets: &mut HashMap<NodeIndex, Vec2>,
    options: &ForceOptions,
) {
    if options.gravity == 0.0 {
        return;
    }
    let points: Vec<Point2> = order
        .iter()
        .filter_map(|node| positions.get(node).copied())
        .collect();
    let center = centroid_of(&points);
    for node in order {
        let Some(position) = positions.get(node) else {
            continue;
        };
        let dx = center.x - position.x;
        let dy = center.y - position.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance > 1.0 {
            add_offset(
                offsets,
                *node,
                Vec2::new(
                    options.gravity * dx / distance,
                    options.gravity * dy / distance,
                ),
            );
        }
    }
}

fn add_offset(offsets: &mut HashMap<NodeIndex, Vec2>, node: NodeIndex, delta: Vec2) {
    if let Some(offset) = offsets.get_mut(&node) {
        *offset = Vec2::new(offset.x + delta.x, offset.y + delta.y);
    }
}

/// Moves free nodes by their temperature-clamped offsets.
fn move_nodes(
    order: &[NodeIndex],
    positions: &mut Positions,
    fixed: &FixedNodes,
    offsets: &HashMap<NodeIndex, Vec2>,
    temperature: f32,
) {
    for node in order {
        if fixed.contains(node) {
            continue;
        }
        let (Some(position), Some(offset)) = (positions.get(node).copied(), offsets.get(node))
        else {
            continue;
        };
        let clamped = clamp_offset(*offset, temperature);
        positions.insert(*node, position + clamped);
    }
}

/// Caps a displacement vector at the current temperature, preserving direction.
fn clamp_offset(offset: Vec2, temperature: f32) -> Vec2 {
    let length = offset.length();
    if length > temperature && length > 0.0 {
        offset * (temperature / length)
    } else {
        offset
    }
}

/// True when two square node boxes centered at `a` and `b` overlap.
pub fn boxes_overlap(a: Point2, b: Point2, size: f32) -> bool {
    let extent = size.max(1.0);
    (a.x - b.x).abs() < extent && (a.y - b.y).abs() < extent
}

/// True when any pair of placed nodes overlaps at `size`.
pub fn has_overlaps(positions: &Positions, size: f32) -> bool {
    let mut points: Vec<Point2> = positions.values().copied().collect();
    points.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });
    for (index, a) in points.iter().enumerate() {
        for b in points.iter().skip(index + 1) {
            if boxes_overlap(*a, *b, size) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use cg_graph::MockGraph;

    use super::*;

    fn quiet_options() -> ForceOptions {
        ForceOptions {
            iterations: 200,
            initial_temp: 32.0,
            cooling: 0.97,
            ..ForceOptions::default()
        }
    }

    fn spread_start(graph: &dyn GraphView) -> Positions {
        let mut start = Positions::new();
        for (ordinal, node) in graph.node_ids().into_iter().enumerate() {
            let side = if ordinal % 2 == 0 { -160.0 } else { 160.0 };
            start.insert(node, Point2::new(side, ordinal as f32 * 10.0));
        }
        start
    }

    #[test]
    fn connected_pair_settles_without_collapsing() {
        let mut graph = MockGraph::empty();
        graph.push_edge(0, 1);
        let layout = ForceLayout::with_options(quiet_options());
        let result = layout.layout(&graph, &spread_start(&graph), &FixedNodes::default());
        let a = result
            .get(&NodeIndex::new(0))
            .copied()
            .unwrap_or(Point2::ZERO);
        let b = result
            .get(&NodeIndex::new(1))
            .copied()
            .unwrap_or(Point2::ZERO);
        let distance = (b - a).length();
        assert!(distance.is_finite() && distance > 1.0 && distance < 320.0);
    }

    #[test]
    fn disconnected_nodes_drift_apart() {
        let graph = MockGraph::isolated(2);
        let layout = ForceLayout::with_options(quiet_options());
        let mut start = Positions::new();
        start.insert(NodeIndex::new(0), Point2::new(-5.0, 0.0));
        start.insert(NodeIndex::new(1), Point2::new(5.0, 0.0));
        let before = (start
            .get(&NodeIndex::new(1))
            .copied()
            .unwrap_or(Point2::ZERO)
            - start
                .get(&NodeIndex::new(0))
                .copied()
                .unwrap_or(Point2::ZERO))
        .length();
        let result = layout.layout(&graph, &start, &FixedNodes::default());
        let after = (result
            .get(&NodeIndex::new(1))
            .copied()
            .unwrap_or(Point2::ZERO)
            - result
                .get(&NodeIndex::new(0))
                .copied()
                .unwrap_or(Point2::ZERO))
        .length();
        assert_eq!(result.len(), 2);
        assert!(after.is_finite() && after >= before);
    }

    #[test]
    fn pinned_nodes_hold_their_ground() {
        let graph = MockGraph::chain(3);
        let layout = ForceLayout::with_options(quiet_options());
        let start = spread_start(&graph);
        let anchor = start
            .get(&NodeIndex::new(0))
            .copied()
            .unwrap_or(Point2::ZERO);
        let mut fixed = FixedNodes::default();
        fixed.insert(NodeIndex::new(0));
        let result = layout.layout(&graph, &start, &fixed);
        assert_eq!(result.get(&NodeIndex::new(0)), Some(&anchor));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn repeated_runs_agree_exactly() {
        let graph = MockGraph::clique(5);
        let layout = ForceLayout::with_options(quiet_options());
        let start = spread_start(&graph);
        let first = layout.layout(&graph, &start, &FixedNodes::default());
        let second = layout.layout(&graph, &start, &FixedNodes::default());
        assert_eq!(first, second);
    }

    #[test]
    fn empty_graph_yields_no_positions() {
        let graph = MockGraph::empty();
        let layout = ForceLayout::with_options(quiet_options());
        let result = layout.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert!(result.is_empty());
    }

    #[test]
    fn overlap_helper_detects_touching_boxes() {
        assert!(boxes_overlap(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 10.0),
            24.0
        ));
        assert!(!boxes_overlap(
            Point2::new(0.0, 0.0),
            Point2::new(30.0, 0.0),
            24.0
        ));
        let mut crowded = Positions::new();
        crowded.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        crowded.insert(NodeIndex::new(1), Point2::new(5.0, 5.0));
        assert!(has_overlaps(&crowded, 24.0));
        let mut spread = Positions::new();
        spread.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        spread.insert(NodeIndex::new(1), Point2::new(100.0, 0.0));
        assert!(!has_overlaps(&spread, 24.0));
        assert!(!has_overlaps(&Positions::new(), 24.0));
    }

    #[test]
    fn small_graph_converges_without_overlaps() {
        let graph = MockGraph::chain(4);
        let layout = ForceLayout::with_options(ForceOptions {
            iterations: 400,
            initial_temp: 32.0,
            cooling: 0.98,
            ..ForceOptions::default()
        });
        let result = layout.layout(&graph, &spread_start(&graph), &FixedNodes::default());
        assert_eq!(result.len(), 4);
        assert!(!has_overlaps(&result, ForceOptions::default().node_size));
        for point in result.values() {
            assert!(point.x.is_finite() && point.y.is_finite());
        }
    }
}
