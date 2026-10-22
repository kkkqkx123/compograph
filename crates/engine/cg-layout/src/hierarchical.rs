//! Hierarchical layout stacking nodes in directed layers.
//!
//! Layering is self-contained over the read-only graph view: strongly
//! connected groups are found with an iterative Tarjan pass, each group takes
//! a single rank, and the acyclic group graph is peeled by longest path. One
//! downward and one upward barycenter sweep then order each layer to reduce
//! edge crossings. Coordinates reuse the same directional mapping as the
//! breadth-first layout.

use std::collections::{HashMap, HashSet};

use compograph_graph::{AdjacencySnapshot, FixedNodes, NodeIndex, Positions};
use compograph_types::Point2;

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
    fn layout(
        &self,
        graph: &AdjacencySnapshot,
        previous: &Positions,
        fixed: &FixedNodes,
    ) -> Positions {
        let layers = compute_layers(graph);
        let mut result: Positions = HashMap::new();
        if layers.is_empty() {
            return result;
        }
        for rank in 0..layers.len() {
            for (index, node) in layers.run(rank).iter().enumerate() {
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

/// Node identifiers grouped into contiguous runs.
///
/// Rankings and strongly connected groups both produce one run per layer or
/// group. A flat run array keeps a chain-shaped graph from allocating one
/// vector per node.
struct NodeRuns {
    /// Start offset of every run; one extra entry closes the last run.
    start: Vec<usize>,
    /// Members grouped by run.
    members: Vec<NodeIndex>,
}

impl NodeRuns {
    fn empty() -> Self {
        Self {
            start: vec![0],
            members: Vec::new(),
        }
    }

    /// Members of one run.
    fn run(&self, index: usize) -> &[NodeIndex] {
        &self.members[self.start[index]..self.start[index + 1]]
    }

    /// Mutable members of one run.
    fn run_mut(&mut self, index: usize) -> &mut [NodeIndex] {
        let from = self.start[index];
        let to = self.start[index + 1];
        &mut self.members[from..to]
    }

    fn len(&self) -> usize {
        self.start.len() - 1
    }

    fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Groups nodes into one run per rank.
    fn from_ranks(ids: &[NodeIndex], ranks: &[usize]) -> Self {
        let rank_count = ranks.iter().copied().max().map(|max| max + 1).unwrap_or(0);
        let mut start = vec![0usize; rank_count + 1];
        for &rank in ranks {
            start[rank + 1] += 1;
        }
        for index in 1..start.len() {
            start[index] += start[index - 1];
        }
        let mut members = vec![NodeIndex::new(0); ids.len()];
        let mut cursor = start.clone();
        for (index, node) in ids.iter().enumerate() {
            let rank = ranks[index];
            let slot = cursor[rank];
            members[slot] = *node;
            cursor[rank] += 1;
        }
        Self { start, members }
    }
}

/// Rank layers from the outermost (sourceless) inward, ordered to reduce crossings.
fn compute_layers(graph: &AdjacencySnapshot) -> NodeRuns {
    let mut ids = graph.nodes().to_vec();
    ids.sort_unstable_by_key(|node| node.index());
    if ids.is_empty() {
        return NodeRuns::empty();
    }
    let mut position = HashMap::with_capacity(ids.len());
    for (index, node) in ids.iter().enumerate() {
        position.insert(*node, index);
    }
    let ranks = component_ranks(graph, &ids, &position);
    let mut layers = NodeRuns::from_ranks(&ids, &ranks);
    order_by_barycenter(graph, &position, &mut layers);
    layers
}

/// Rank per node from strongly connected groups ranked by longest path.
///
/// Sourceless groups rank first; every member of one group shares its rank, so
/// cyclic members land on a single layer while acyclic graphs still receive
/// exact longest-path ranks. The rank relaxes along the group graph in one
/// pass over its edges, so the whole ranking costs time linear in the graph
/// size instead of one sweep per group.
fn component_ranks(
    graph: &AdjacencySnapshot,
    ids: &[NodeIndex],
    position: &HashMap<NodeIndex, usize>,
) -> Vec<usize> {
    let groups = strongly_connected_groups(graph, ids);
    let mut member_of = vec![0usize; ids.len()];
    for group in 0..groups.len() {
        for node in groups.run(group) {
            member_of[position[node]] = group;
        }
    }
    // Group graph edges, deduplicated, then stored in one flat run per group.
    let mut links: Vec<(usize, usize)> = Vec::new();
    for (index, node) in ids.iter().enumerate() {
        let group = member_of[index];
        for &target in graph.successors(*node) {
            let other = member_of[position[&target]];
            if other != group {
                links.push((group, other));
            }
        }
    }
    links.sort_unstable();
    links.dedup();
    let mut starts = vec![0usize; groups.len() + 1];
    for &(group, _) in &links {
        starts[group + 1] += 1;
    }
    for index in 1..starts.len() {
        starts[index] += starts[index - 1];
    }
    let mut targets = vec![0usize; links.len()];
    let mut incoming = vec![0usize; groups.len()];
    let mut cursor = starts.clone();
    for &(group, other) in &links {
        let slot = cursor[group];
        targets[slot] = other;
        cursor[group] += 1;
        incoming[other] += 1;
    }
    // Longest path over the group graph: a group ranks one step behind its
    // deepest predecessor.
    let mut group_rank = vec![0usize; groups.len()];
    let mut ready: Vec<usize> = (0..groups.len())
        .filter(|group| incoming[*group] == 0)
        .collect();
    while let Some(group) = ready.pop() {
        for &other in &targets[starts[group]..starts[group + 1]] {
            group_rank[other] = group_rank[other].max(group_rank[group] + 1);
            incoming[other] -= 1;
            if incoming[other] == 0 {
                ready.push(other);
            }
        }
    }
    let mut rank = vec![0usize; ids.len()];
    for index in 0..ids.len() {
        rank[index] = group_rank[member_of[index]];
    }
    rank
}

/// Strongly connected groups in deterministic order via iterative Tarjan.
///
/// Each group is sorted by node index, and groups are ordered by their
/// smallest member, so repeated runs agree.
fn strongly_connected_groups(graph: &AdjacencySnapshot, ids: &[NodeIndex]) -> NodeRuns {
    let mut marker: HashMap<NodeIndex, usize> = HashMap::new();
    let mut low: HashMap<NodeIndex, usize> = HashMap::new();
    let mut stacked: HashSet<NodeIndex> = HashSet::new();
    let mut trail: Vec<NodeIndex> = Vec::new();
    let mut groups = NodeRuns::empty();
    let mut group: Vec<NodeIndex> = Vec::new();
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
            let outgoing = graph.successors(node);
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
                group.clear();
                while let Some(member) = trail.pop() {
                    stacked.remove(&member);
                    group.push(member);
                    if member == node {
                        break;
                    }
                }
                group.sort_unstable_by_key(|member| member.index());
                groups.members.extend_from_slice(&group);
                groups.start.push(groups.members.len());
            }
        }
    }
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_unstable_by_key(|group| {
        groups
            .run(*group)
            .first()
            .map(|node| node.index())
            .unwrap_or(usize::MAX)
    });
    let mut sorted = NodeRuns::empty();
    let mut members = Vec::with_capacity(groups.members.len());
    for group in order {
        members.extend_from_slice(groups.run(group));
        sorted.start.push(members.len());
    }
    sorted.members = members;
    sorted
}

/// Reorders layers by barycenter sweeps: one pass downward using predecessor
/// positions, one pass upward using successor positions.
fn order_by_barycenter(
    graph: &AdjacencySnapshot,
    position: &HashMap<NodeIndex, usize>,
    layers: &mut NodeRuns,
) {
    let mut order = vec![f32::NAN; layers.members.len()];
    let mut scored: Vec<(NodeIndex, f32)> = Vec::new();
    downward_pass(graph, position, layers, &mut order, &mut scored);
    upward_pass(graph, position, layers, &mut order, &mut scored);
}

/// Sorts each layer by the mean order position of its predecessors.
fn downward_pass(
    graph: &AdjacencySnapshot,
    position: &HashMap<NodeIndex, usize>,
    layers: &mut NodeRuns,
    order: &mut [f32],
    scored: &mut Vec<(NodeIndex, f32)>,
) {
    order.fill(f32::NAN);
    let mut placed = 0usize;
    for rank in 0..layers.len() {
        let layer = layers.run_mut(rank);
        if rank == 0 {
            for (index, node) in layer.iter().enumerate() {
                order[position[node]] = index as f32;
            }
            placed += layer.len();
            continue;
        }
        scored.clear();
        for node in layer.iter() {
            let mut total = 0.0f32;
            let mut samples = 0usize;
            for &source in graph.predecessors(*node) {
                let score = order[position[&source]];
                if score.is_finite() {
                    total += score;
                    samples += 1;
                }
            }
            scored.push((
                *node,
                if samples == 0 {
                    placed as f32
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
        for (index, (node, _)) in scored.iter().enumerate() {
            layer[index] = *node;
            order[position[node]] = index as f32;
        }
        placed += layer.len();
    }
}

/// Sorts each layer by the mean order position of its successors.
fn upward_pass(
    graph: &AdjacencySnapshot,
    position: &HashMap<NodeIndex, usize>,
    layers: &mut NodeRuns,
    order: &mut [f32],
    scored: &mut Vec<(NodeIndex, f32)>,
) {
    if layers.is_empty() {
        return;
    }
    order.fill(f32::NAN);
    let mut placed = 0usize;
    let last = layers.len() - 1;
    for rank in (0..layers.len()).rev() {
        let layer = layers.run_mut(rank);
        if rank == last {
            for (index, node) in layer.iter().enumerate() {
                order[position[node]] = index as f32;
            }
            placed += layer.len();
            continue;
        }
        scored.clear();
        for node in layer.iter() {
            let mut total = 0.0f32;
            let mut samples = 0usize;
            for &outgoing in graph.successors(*node) {
                let score = order[position[&outgoing]];
                if score.is_finite() {
                    total += score;
                    samples += 1;
                }
            }
            scored.push((
                *node,
                if samples == 0 {
                    placed as f32
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
        for (index, (node, _)) in scored.iter().enumerate() {
            layer[index] = *node;
            order[position[node]] = index as f32;
        }
        placed += layer.len();
    }
}

/// Coordinates of one node from its rank, index and the layout direction.
fn rank_position(
    options: &HierarchicalOptions,
    layers: &NodeRuns,
    rank: usize,
    index: usize,
) -> Point2 {
    let layer_size = layers.run(rank).len().max(1);
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
    use compograph_graph::MockGraph;

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
        let snapshot = AdjacencySnapshot::capture(&graph);
        let positions = engine.layout(&snapshot, &Positions::new(), &FixedNodes::default());
        assert!(positions.is_empty());
    }

    #[test]
    fn chain_ranks_grow_downward() {
        let graph = MockGraph::chain(4);
        let engine = HierarchicalLayout::with_options(plain_options());
        let snapshot = AdjacencySnapshot::capture(&graph);
        let positions = engine.layout(&snapshot, &Positions::new(), &FixedNodes::default());
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
        let snapshot = AdjacencySnapshot::capture(&graph);
        let positions = engine.layout(&snapshot, &Positions::new(), &FixedNodes::default());
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
        let snapshot = AdjacencySnapshot::capture(&graph);
        let positions = engine.layout(&snapshot, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 2);
    }

    #[test]
    fn cycle_members_share_one_layer() {
        let mut graph = MockGraph::chain(2);
        graph.push_edge(1, 0);
        let engine = HierarchicalLayout::with_options(plain_options());
        let snapshot = AdjacencySnapshot::capture(&graph);
        let positions = engine.layout(&snapshot, &Positions::new(), &FixedNodes::default());
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
        let snapshot = AdjacencySnapshot::capture(&graph);
        let positions = engine.layout(&snapshot, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 3);
        assert!(positions[&NodeIndex::new(0)].y < positions[&NodeIndex::new(1)].y);
        let cyclic = positions[&NodeIndex::new(1)].y - positions[&NodeIndex::new(2)].y;
        assert!(cyclic.abs() < 1e-6);
    }

    #[test]
    fn deep_paths_rank_below_short_edges() {
        let mut graph = MockGraph::empty();
        graph.push_edge(0, 1);
        graph.push_edge(1, 2);
        graph.push_edge(2, 3);
        graph.push_edge(0, 3);
        let engine = HierarchicalLayout::with_options(plain_options());
        let snapshot = AdjacencySnapshot::capture(&graph);
        let positions = engine.layout(&snapshot, &Positions::new(), &FixedNodes::default());
        let depth = |index: usize| positions[&NodeIndex::new(index)].y;
        // The rank follows the longest path, so the short edge from the
        // source does not pull the last node up.
        assert!(depth(0) < depth(1) && depth(1) < depth(2) && depth(2) < depth(3));
    }

    #[test]
    fn upward_mirrors_downward() {
        let graph = MockGraph::chain(3);
        let down = HierarchicalLayout::with_options(plain_options());
        let up = HierarchicalLayout::with_options(HierarchicalOptions {
            direction: BfsDirection::Upward,
            ..plain_options()
        });
        let snapshot = AdjacencySnapshot::capture(&graph);
        let below = down.layout(&snapshot, &Positions::new(), &FixedNodes::default());
        let above = up.layout(&snapshot, &Positions::new(), &FixedNodes::default());
        for index in 0..3 {
            let node = NodeIndex::new(index);
            assert!((below[&node].x - above[&node].x).abs() < 1e-3);
            assert!((below[&node].y + above[&node].y).abs() < 1e-3);
        }
    }
}
