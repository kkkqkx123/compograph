//! Radial layout nesting nodes in rings by descending score.
//!
//! The ring pipeline mirrors the concentric arrangement: scores sort
//! descending, neighbours within one level width share a ring, and each ring
//! grows until its members stop overlapping. Radial differs in one rule:
//! isolated nodes always settle on the outermost ring, whatever their score,
//! so detached nodes never hide at the center. Callers may supply a score
//! table such as PageRank output; without one, live degree drives the rings.

use std::collections::HashMap;

use cg_graph::{FixedNodes, GraphView, NodeIndex, Positions};
use cg_types::Point2;

use crate::engine::LayoutEngine;

/// Tuning parameters of the radial arrangement.
#[derive(Clone, Debug)]
pub struct RadialOptions {
    /// Center of the rings in model units.
    pub center: Point2,
    /// Angle of the first node of each ring in radians.
    pub start_angle: f32,
    /// Angular span of a ring; a full circle minus one step when absent.
    pub sweep: Option<f32>,
    /// When false, rings run counterclockwise instead of clockwise.
    pub clockwise: bool,
    /// When true, ring gaps widen to the largest gap instead of accumulating.
    pub equidistant: bool,
    /// Clearance added to the node extent for ring spacing.
    pub min_node_spacing: f32,
    /// Score source for ring assignment.
    pub scoring: RadialScoring,
    /// Value spread sharing one ring; defaults to a quarter of the largest
    /// value.
    pub level_width: Option<f32>,
    /// When true, ring radii grow until members stop overlapping.
    pub avoid_overlap: bool,
    /// Uniform node extent used for spacing.
    pub node_size: f32,
}

impl Default for RadialOptions {
    fn default() -> Self {
        Self {
            center: Point2::ZERO,
            start_angle: 3.0 / 2.0 * std::f32::consts::PI,
            sweep: None,
            clockwise: true,
            equidistant: false,
            min_node_spacing: 10.0,
            scoring: RadialScoring::Degree,
            level_width: None,
            avoid_overlap: true,
            node_size: 24.0,
        }
    }
}

/// Score source for ring assignment.
///
/// Closures cannot live in these cloneable options, so the two shapes are
/// expressed as data: live degree, or a caller-supplied table such as
/// PageRank output with a degree fallback for missing entries.
#[derive(Clone, Debug, Default)]
pub enum RadialScoring {
    /// Score every node by its degree.
    #[default]
    Degree,
    /// Score nodes from a caller-supplied table; missing entries fall back
    /// to degree.
    Scores(HashMap<NodeIndex, f32>),
}

/// Radial engine implementing the shared layout contract.
pub struct RadialLayout {
    options: RadialOptions,
}

impl RadialLayout {
    pub fn new(center: Point2) -> Self {
        Self {
            options: RadialOptions {
                center,
                ..RadialOptions::default()
            },
        }
    }

    pub fn with_options(options: RadialOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &RadialOptions {
        &self.options
    }
}

impl LayoutEngine for RadialLayout {
    fn layout(&self, graph: &dyn GraphView, previous: &Positions, fixed: &FixedNodes) -> Positions {
        let mut ids = graph.node_ids();
        ids.sort_unstable_by_key(|node| node.index());
        let mut result: Positions = HashMap::new();
        if ids.is_empty() {
            return result;
        }
        let values = node_values(graph, &ids, &self.options.scoring);
        let mut levels = to_levels(
            &ids,
            &values,
            level_width(&ids, &values, self.options.level_width),
        );
        push_isolated_outermost(graph, &mut levels);
        let radii = ring_radii(&levels, &self.options);
        let direction = if self.options.clockwise { 1.0 } else { -1.0 };
        for (level, radius) in levels.iter().zip(radii.iter()) {
            let step = ring_step(self.options.sweep, level.len());
            for (ordinal, node) in level.iter().enumerate() {
                if fixed.contains(node)
                    && let Some(held) = previous.get(node)
                {
                    result.insert(*node, *held);
                    continue;
                }
                let theta = self.options.start_angle + ordinal as f32 * step * direction;
                result.insert(
                    *node,
                    Point2::new(
                        self.options.center.x + radius * theta.cos(),
                        self.options.center.y + radius * theta.sin(),
                    ),
                );
            }
        }
        result
    }

    fn name(&self) -> &'static str {
        "radial"
    }
}

/// Score per node: caller-supplied table entries win, the rest use degree.
fn node_values(
    graph: &dyn GraphView,
    ids: &[NodeIndex],
    scoring: &RadialScoring,
) -> HashMap<NodeIndex, f32> {
    let mut values = HashMap::new();
    for node in ids {
        let score = match scoring {
            RadialScoring::Degree => graph.degree(*node) as f32,
            RadialScoring::Scores(table) => table
                .get(node)
                .copied()
                .unwrap_or_else(|| graph.degree(*node) as f32),
        };
        values.insert(*node, score);
    }
    values
}

/// Width of one ring: explicit value wins, else a quarter of the largest.
fn level_width(ids: &[NodeIndex], values: &HashMap<NodeIndex, f32>, explicit: Option<f32>) -> f32 {
    if let Some(width) = explicit {
        return width.max(0.0);
    }
    let largest = ids
        .iter()
        .map(|node| values.get(node).copied().unwrap_or(0.0))
        .fold(0.0f32, f32::max);
    largest / 4.0
}

/// Rings of nodes from the highest values inward, in index order within a ring.
fn to_levels(
    ids: &[NodeIndex],
    values: &HashMap<NodeIndex, f32>,
    level_width: f32,
) -> Vec<Vec<NodeIndex>> {
    let mut ordered: Vec<NodeIndex> = ids.to_vec();
    ordered.sort_by(|a, b| {
        values[b]
            .partial_cmp(&values[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.index().cmp(&b.index()))
    });
    let mut levels: Vec<Vec<NodeIndex>> = vec![Vec::new()];
    for node in ordered {
        let starts_new_ring = levels
            .last()
            .and_then(|level| level.first())
            .map(|first| (values[first] - values[&node]).abs() >= level_width)
            .unwrap_or(false);
        if starts_new_ring {
            levels.push(Vec::new());
        }
        if let Some(level) = levels.last_mut() {
            level.push(node);
        }
    }
    for level in levels.iter_mut() {
        level.sort_unstable_by_key(|node| node.index());
    }
    levels
}

/// Moves isolated nodes into their own outermost ring.
///
/// Degree-zero nodes carry no structural pull, so they leave whatever score
/// tier they landed in and share one ring past every connected node. Rings
/// left empty by the move are dropped; when every node is isolated the group
/// stays on the innermost ring so a lone node rests at the center.
fn push_isolated_outermost(graph: &dyn GraphView, levels: &mut Vec<Vec<NodeIndex>>) {
    let mut isolated = Vec::new();
    for level in levels.iter_mut() {
        level.retain(|node| {
            if graph.degree(*node) == 0 {
                isolated.push(*node);
                false
            } else {
                true
            }
        });
    }
    levels.retain(|level| !level.is_empty());
    if isolated.is_empty() {
        return;
    }
    isolated.sort_unstable_by_key(|node| node.index());
    levels.push(isolated);
}

/// Angular step between adjacent members of a ring.
fn ring_step(sweep: Option<f32>, members: usize) -> f32 {
    let sweep = sweep
        .unwrap_or(2.0 * std::f32::consts::PI - 2.0 * std::f32::consts::PI / members.max(1) as f32);
    sweep / members.saturating_sub(1).max(1) as f32
}

/// Radius per ring, accumulating the minimum gap outward.
fn ring_radii(levels: &[Vec<NodeIndex>], options: &RadialOptions) -> Vec<f32> {
    let gap = options.node_size + options.min_node_spacing;
    let mut radii = Vec::with_capacity(levels.len());
    let mut ring = 0.0f32;
    for level in levels {
        let step = ring_step(options.sweep, level.len());
        if level.len() > 1 && options.avoid_overlap {
            let chord = ((step.cos() - 1.0).powi(2) + step.sin().powi(2)).sqrt();
            if chord > f32::EPSILON {
                ring = ring.max(gap / chord);
            }
        }
        radii.push(ring);
        ring += gap;
    }
    if options.equidistant && radii.len() > 1 {
        let widest = radii
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .fold(0.0f32, f32::max);
        let mut even = Vec::with_capacity(radii.len());
        even.push(radii[0]);
        for _ in 1..radii.len() {
            let last = even.last().copied().unwrap_or(0.0);
            even.push(last + widest);
        }
        return even;
    }
    radii
}

#[cfg(test)]
mod tests {
    use cg_graph::MockGraph;

    use super::*;

    #[test]
    fn empty_graph_places_nothing() {
        let graph = MockGraph::empty();
        let engine = RadialLayout::new(Point2::ZERO);
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert!(positions.is_empty());
    }

    #[test]
    fn lone_node_sits_at_the_center() {
        let graph = MockGraph::isolated(1);
        let engine = RadialLayout::new(Point2::new(3.0, 4.0));
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(
            positions.get(&NodeIndex::new(0)),
            Some(&Point2::new(3.0, 4.0))
        );
    }

    #[test]
    fn hub_settles_inside_its_leaves() {
        let mut graph = MockGraph::isolated(5);
        for leaf in 1..5 {
            graph.push_edge(0, leaf);
            graph.push_edge(leaf, 0);
        }
        let engine = RadialLayout::new(Point2::ZERO);
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        let hub = positions[&NodeIndex::new(0)];
        let hub_gap = (hub.x.powi(2) + hub.y.powi(2)).sqrt();
        for leaf in 1..5 {
            let point = positions[&NodeIndex::new(leaf)];
            let gap = (point.x.powi(2) + point.y.powi(2)).sqrt();
            assert!(hub_gap < gap);
        }
    }

    #[test]
    fn isolated_nodes_settle_on_the_outermost_ring() {
        let mut graph = MockGraph::isolated(4);
        graph.push_edge(0, 1);
        graph.push_edge(1, 0);
        graph.push_edge(1, 2);
        graph.push_edge(2, 1);
        let engine = RadialLayout::new(Point2::ZERO);
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        let gap = |node: usize| {
            let point = positions[&NodeIndex::new(node)];
            (point.x.powi(2) + point.y.powi(2)).sqrt()
        };
        let lonely = gap(3);
        for node in 0..3 {
            assert!(lonely > gap(node));
        }
    }

    #[test]
    fn supplied_values_override_degree() {
        let graph = MockGraph::chain(3);
        let mut values = HashMap::new();
        values.insert(NodeIndex::new(2), 100.0);
        let engine = RadialLayout::with_options(RadialOptions {
            scoring: RadialScoring::Scores(values),
            ..RadialOptions::default()
        });
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        let inner = positions[&NodeIndex::new(2)];
        let outer = positions[&NodeIndex::new(0)];
        let inner_gap = (inner.x.powi(2) + inner.y.powi(2)).sqrt();
        let outer_gap = (outer.x.powi(2) + outer.y.powi(2)).sqrt();
        assert!(inner_gap < outer_gap);
    }

    #[test]
    fn fixed_nodes_hold_their_positions() {
        let graph = MockGraph::chain(3);
        let mut previous = Positions::new();
        previous.insert(NodeIndex::new(1), Point2::new(50.0, 60.0));
        let mut fixed = FixedNodes::default();
        fixed.insert(NodeIndex::new(1));
        let engine = RadialLayout::new(Point2::ZERO);
        let positions = engine.layout(&graph, &previous, &fixed);
        assert_eq!(
            positions.get(&NodeIndex::new(1)),
            Some(&Point2::new(50.0, 60.0))
        );
        assert_eq!(positions.len(), 3);
    }
}
