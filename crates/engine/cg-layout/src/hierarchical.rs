//! Hierarchical layout stacking nodes in directed layers.
//!
//! Layering is self-contained over the read-only graph view: strongly
//! connected groups are found with an iterative Tarjan pass, each group takes
//! a single rank, and the acyclic group graph is peeled by longest path. One
//! downward and one upward barycenter sweep then order each layer to reduce
//! edge crossings. Coordinates reuse the same directional mapping as the
//! breadth-first layout.

use std::collections::{HashMap, HashSet};

use cg_graph::{FixedNodes, GraphView, NodeIndex, Positions};
use cg_types::Point2;

use crate::breadthfirst::BfsDirection;
use crate::engine::LayoutEngine;

/// Tuning parameters of the hierarchical arrangement.
#[derive(Clone, Debug)]
pub struct HierarchicalOptions {
    /// Center of the bounding rectangle in model units.
    pub center: Point2,
    /// Width of the bounding rectangle.
    pub width: f32,
    /// Height of the bounding rectangle.
    pub height: f32,
    /// Padding kept clear inside the box edges.
    pub padding: f32,
    /// Growth direction of the rank stack.
    pub direction: BfsDirection,
    /// When true, nodes inside a layer keep at least the node extent apart.
    pub avoid_overlap: bool,
    /// Uniform node extent used for spacing.
    pub node_size: f32,
}

impl Default for HierarchicalOptions {
    fn default() -> Self {
        Self {
            center: Point2::ZERO,
            width: 640.0,
            height: 480.0,
            padding: 30.0,
            direction: BfsDirection::Downward,
            avoid_overlap: true,
            node_size: 24.0,
        }
    }
}

/// Hierarchical engine implementing the shared layout contract.
pub struct HierarchicalLayout {
    options: HierarchicalOptions,
}

impl HierarchicalLayout {
    pub fn new(center: Point2, width: f32, height: f32) -> Self {
        Self {
            options: HierarchicalOptions {
                center,
                width,
                height,
                ..HierarchicalOptions::default()
            },
        }
    }

    pub fn with_options(options: HierarchicalOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &HierarchicalOptions {
        &self.options
    }
}

impl LayoutEngine for HierarchicalLayout {
    fn layout(&self, graph: &dyn GraphView, previous: &Positions, fixed: &FixedNodes) -> Positions {
        let layers = compute_layers(graph);
        let mut result: Positions = HashMap::new();
        if layers.is_empty() {
            return result;
        }
        for (rank, layer) in layers.iter().enumerate() {
            for (index, node) in layer.iter().enumerate() {
                if fixed.contains(node)
                    && let Some(held) = previous.get(node)
                {
                    result.insert(*node, *held);
                    continue;
                }
                result.insert(*node, rank_position(&self.options, &layers, rank, index));
            }
        }
        result
    }

    fn name(&self) -> &'static str {
        "hierarchical"
    }
}

/// Rank layers from the outermost (sourceless) inward, ordered to reduce crossings.
fn compute_layers(graph: &dyn GraphView) -> Vec<Vec<NodeIndex>> {
    let mut ids = graph.node_ids();
    ids.sort_unstable_by_key(|node| node.index());
    if ids.is_empty() {
        return Vec::new();
    }
    let rank = component_ranks(graph, &ids);
    let max_rank = rank.values().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<NodeIndex>> = vec![Vec::new(); max_rank + 1];
    for node in &ids {
        layers[rank[node]].push(*node);
    }
    order_by_barycenter(graph, &mut layers);
    layers.retain(|layer| !layer.is_empty());
    layers
}

/// Rank per node from strongly connected groups peeled by longest path.
///
/// Sourceless groups peel first; every member of one group shares its rank, so
/// cyclic members land on a single layer while acyclic graphs still receive
/// exact longest-path ranks.
fn component_ranks(graph: &dyn GraphView, ids: &[NodeIndex]) -> HashMap<NodeIndex, usize> {
    let present: HashSet<NodeIndex> = ids.iter().copied().collect();
    let groups = strongly_connected_groups(graph, ids, &present);
    let mut member_of: HashMap<NodeIndex, usize> = HashMap::new();
    for (group, members) in groups.iter().enumerate() {
        for node in members {
            member_of.insert(*node, group);
        }
    }
    let mut pending: Vec<HashSet<usize>> = vec![HashSet::new(); groups.len()];
    for node in ids {
        let group = member_of[node];
        for source in graph.predecessors(*node) {
            if !present.contains(&source) {
                continue;
            }
            let other = member_of[&source];
            if other != group {
                pending[group].insert(other);
            }
        }
    }
    let mut group_rank: HashMap<usize, usize> = HashMap::new();
    let mut placed: HashSet<usize> = HashSet::new();
    let mut current = 0usize;
    while placed.len() < groups.len() {
        let mut ready: Vec<usize> = (0..groups.len())
            .filter(|group| {
                !placed.contains(group)
                    && pending[*group].iter().all(|other| placed.contains(other))
            })
            .collect();
        ready.sort_unstable_by_key(|group| {
            groups[*group]
                .iter()
                .map(|node| node.index())
                .min()
                .unwrap_or(usize::MAX)
        });
        for group in ready {
            placed.insert(group);
            group_rank.insert(group, current);
        }
        current += 1;
    }
    let mut rank = HashMap::new();
    for node in ids {
        rank.insert(*node, group_rank[&member_of[node]]);
    }
    rank
}

/// Strongly connected groups in deterministic order via iterative Tarjan.
///
/// Successor edges leaving the given node set are ignored. Each group is
/// sorted by node index, and groups are ordered by their smallest member, so
/// repeated runs agree.
fn strongly_connected_groups(
    graph: &dyn GraphView,
    ids: &[NodeIndex],
    present: &HashSet<NodeIndex>,
) -> Vec<Vec<NodeIndex>> {
    let mut followers: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
    for node in ids {
        let mut outgoing: Vec<NodeIndex> = graph
            .successors(*node)
            .into_iter()
            .filter(|target| present.contains(target))
            .collect();
        outgoing.sort_unstable_by_key(|target| target.index());
        followers.insert(*node, outgoing);
    }
    let mut marker: HashMap<NodeIndex, usize> = HashMap::new();
    let mut low: HashMap<NodeIndex, usize> = HashMap::new();
    let mut stacked: HashSet<NodeIndex> = HashSet::new();
    let mut trail: Vec<NodeIndex> = Vec::new();
    let mut groups: Vec<Vec<NodeIndex>> = Vec::new();
    let mut order = 0usize;
    for root in ids {
        if marker.contains_key(root) {
            continue;
        }
        let mut frames: Vec<(NodeIndex, Option<NodeIndex>, usize)> = vec![(*root, None, 0)];
        while let Some((node, parent, next)) = frames.pop() {
            if next == 0 && !marker.contains_key(&node) {
                marker.insert(node, order);
                low.insert(node, order);
                order += 1;
                trail.push(node);
                stacked.insert(node);
            }
            let outgoing = followers.get(&node).cloned().unwrap_or_default();
            if next < outgoing.len() {
                let target = outgoing[next];
                frames.push((node, parent, next + 1));
                if !marker.contains_key(&target) {
                    frames.push((target, Some(node), 0));
                } else if stacked.contains(&target) {
                    let considering = low[&node].min(marker[&target]);
                    low.insert(node, considering);
                }
                continue;
            }
            if let Some(source) = parent {
                let considering = low[&source].min(low[&node]);
                low.insert(source, considering);
            }
            if low[&node] == marker[&node] {
                let mut group = Vec::new();
                while let Some(member) = trail.pop() {
                    stacked.remove(&member);
                    group.push(member);
                    if member == node {
                        break;
                    }
                }
                group.sort_unstable_by_key(|member| member.index());
                groups.push(group);
            }
        }
    }
    groups.sort_unstable_by_key(|group| {
        group
            .iter()
            .map(|node| node.index())
            .min()
            .unwrap_or(usize::MAX)
    });
    groups
}

/// Reorders layers by barycenter sweeps: one pass downward using predecessor
/// positions, one pass upward using successor positions.
fn order_by_barycenter(graph: &dyn GraphView, layers: &mut [Vec<NodeIndex>]) {
    downward_pass(graph, layers);
    upward_pass(graph, layers);
}

/// Sorts each layer by the mean order position of its predecessors.
fn downward_pass(graph: &dyn GraphView, layers: &mut [Vec<NodeIndex>]) {
    let mut order: HashMap<NodeIndex, f32> = HashMap::new();
    for (rank, layer) in layers.iter_mut().enumerate() {
        if rank == 0 {
            for (index, node) in layer.iter().enumerate() {
                order.insert(*node, index as f32);
            }
            continue;
        }
        let current = std::mem::take(layer);
        let mut scored: Vec<(NodeIndex, f32)> = Vec::with_capacity(current.len());
        for node in &current {
            let mut total = 0.0f32;
            let mut samples = 0usize;
            for source in graph.predecessors(*node) {
                if let Some(position) = order.get(&source) {
                    total += *position;
                    samples += 1;
                }
            }
            scored.push((
                *node,
                if samples == 0 {
                    order.len() as f32
                } else {
                    total / samples as f32
                },
            ));
        }
        scored.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.index().cmp(&b.0.index()))
        });
        let reordered: Vec<NodeIndex> = scored.into_iter().map(|(node, _)| node).collect();
        for (index, node) in reordered.iter().enumerate() {
            order.insert(*node, index as f32);
        }
        *layer = reordered;
    }
}

/// Sorts each layer by the mean order position of its successors.
fn upward_pass(graph: &dyn GraphView, layers: &mut [Vec<NodeIndex>]) {
    if layers.is_empty() {
        return;
    }
    let mut order: HashMap<NodeIndex, f32> = HashMap::new();
    let last = layers.len() - 1;
    for (rank, layer) in layers.iter_mut().enumerate().rev() {
        if rank == last {
            for (index, node) in layer.iter().enumerate() {
                order.insert(*node, index as f32);
            }
            continue;
        }
        let current = std::mem::take(layer);
        let mut scored: Vec<(NodeIndex, f32)> = Vec::with_capacity(current.len());
        for node in &current {
            let mut total = 0.0f32;
            let mut samples = 0usize;
            for outgoing in graph.successors(*node) {
                if let Some(position) = order.get(&outgoing) {
                    total += *position;
                    samples += 1;
                }
            }
            scored.push((
                *node,
                if samples == 0 {
                    order.len() as f32
                } else {
                    total / samples as f32
                },
            ));
        }
        scored.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.index().cmp(&b.0.index()))
        });
        let reordered: Vec<NodeIndex> = scored.into_iter().map(|(node, _)| node).collect();
        for (index, node) in reordered.iter().enumerate() {
            order.insert(*node, index as f32);
        }
        *layer = reordered;
    }
}

/// Coordinates of one node from its rank, index and the layout direction.
fn rank_position(
    options: &HierarchicalOptions,
    layers: &[Vec<NodeIndex>],
    rank: usize,
    index: usize,
) -> Point2 {
    let layer_size = layers[rank].len().max(1);
    let rank_count = layers.len().max(1);
    let min_gap = if options.avoid_overlap {
        options.node_size
    } else {
        0.0
    };
    let vertical = if rank_count <= 1 {
        0.0
    } else {
        ((options.height - 2.0 * options.padding - options.node_size) / (rank_count - 1) as f32)
            .max(min_gap)
    };
    let horizontal = if layer_size <= 1 {
        0.0
    } else {
        ((options.width - 2.0 * options.padding - options.node_size) / (layer_size - 1) as f32)
            .max(min_gap)
    };
    let across = (index + 1) as f32 - (layer_size + 1) as f32 / 2.0;
    let along = (rank + 1) as f32 - (rank_count + 1) as f32 / 2.0;
    let (dx, dy) = match options.direction {
        BfsDirection::Downward => (across * horizontal, along * vertical),
        BfsDirection::Upward => (across * horizontal, -along * vertical),
        BfsDirection::Rightward => (along * vertical, across * horizontal),
        BfsDirection::Leftward => (-along * vertical, across * horizontal),
    };
    Point2::new(options.center.x + dx, options.center.y + dy)
}

#[cfg(test)]
mod tests {
    use cg_graph::MockGraph;

    use super::*;

    fn plain_options() -> HierarchicalOptions {
        HierarchicalOptions {
            avoid_overlap: false,
            padding: 0.0,
            node_size: 0.0,
            ..HierarchicalOptions::default()
        }
    }

    #[test]
    fn empty_graph_places_nothing() {
        let graph = MockGraph::empty();
        let engine = HierarchicalLayout::with_options(plain_options());
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert!(positions.is_empty());
    }

    #[test]
    fn chain_ranks_grow_downward() {
        let graph = MockGraph::chain(4);
        let engine = HierarchicalLayout::with_options(plain_options());
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 4);
        for index in 0..3 {
            let upper = positions[&NodeIndex::new(index)].y;
            let lower = positions[&NodeIndex::new(index + 1)].y;
            assert!(upper < lower);
        }
    }

    #[test]
    fn diamond_edges_point_downward() {
        let mut graph = MockGraph::empty();
        graph.push_edge(0, 1);
        graph.push_edge(0, 2);
        graph.push_edge(1, 3);
        graph.push_edge(2, 3);
        let engine = HierarchicalLayout::with_options(plain_options());
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 4);
        assert!(positions[&NodeIndex::new(0)].y < positions[&NodeIndex::new(1)].y);
        assert!(positions[&NodeIndex::new(0)].y < positions[&NodeIndex::new(2)].y);
        assert!(positions[&NodeIndex::new(1)].y < positions[&NodeIndex::new(3)].y);
        assert!(positions[&NodeIndex::new(2)].y < positions[&NodeIndex::new(3)].y);
    }

    #[test]
    fn cycles_still_place_every_node() {
        let mut graph = MockGraph::chain(2);
        graph.push_edge(1, 0);
        let engine = HierarchicalLayout::with_options(plain_options());
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 2);
    }

    #[test]
    fn cycle_members_share_one_layer() {
        let mut graph = MockGraph::chain(2);
        graph.push_edge(1, 0);
        let engine = HierarchicalLayout::with_options(plain_options());
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        let first = positions[&NodeIndex::new(0)].y;
        let second = positions[&NodeIndex::new(1)].y;
        assert!((first - second).abs() < 1e-6);
    }

    #[test]
    fn acyclic_predecessors_stay_above_a_cycle() {
        let mut graph = MockGraph::empty();
        graph.push_edge(0, 1);
        graph.push_edge(1, 2);
        graph.push_edge(2, 1);
        let engine = HierarchicalLayout::with_options(plain_options());
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 3);
        assert!(positions[&NodeIndex::new(0)].y < positions[&NodeIndex::new(1)].y);
        let cyclic = positions[&NodeIndex::new(1)].y - positions[&NodeIndex::new(2)].y;
        assert!(cyclic.abs() < 1e-6);
    }

    #[test]
    fn upward_mirrors_downward() {
        let graph = MockGraph::chain(3);
        let down = HierarchicalLayout::with_options(plain_options());
        let up = HierarchicalLayout::with_options(HierarchicalOptions {
            direction: BfsDirection::Upward,
            ..plain_options()
        });
        let below = down.layout(&graph, &Positions::new(), &FixedNodes::default());
        let above = up.layout(&graph, &Positions::new(), &FixedNodes::default());
        for index in 0..3 {
            let node = NodeIndex::new(index);
            assert!((below[&node].x - above[&node].x).abs() < 1e-3);
            assert!((below[&node].y + above[&node].y).abs() < 1e-3);
        }
    }
}
