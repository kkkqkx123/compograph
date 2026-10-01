//! Breadth-first layout arranging nodes in layers by graph distance.
//!
//! The layering follows the reference breadth-first algorithm: roots seed
//! depth zero, traversal assigns depths, unreachable nodes form their own
//! leading layer, and each layer is reordered so connected nodes sit closer
//! together. The directed maximal adjustment is available but off by default;
//! when enabled it shifts nodes below their deepest in-neighbour once, bailing
//! out entirely on the first repeated shift so cyclic graphs cannot loop.
//! Compound nodes from the reference are absent: every node is placed.

use std::collections::{HashMap, HashSet, VecDeque};

use cg_graph::{FixedNodes, GraphView, NodeIndex, Positions};
use cg_types::Point2;

use crate::engine::{CommonOptions, LayoutEngine};

/// Growth direction of the layer stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BfsDirection {
    /// Roots on top, deeper layers below.
    #[default]
    Downward,
    /// Roots at the bottom, deeper layers above.
    Upward,
    /// Roots on the left, deeper layers to the right.
    Rightward,
    /// Roots on the right, deeper layers to the left.
    Leftward,
}

/// Tuning parameters of the breadth-first arrangement.
#[derive(Clone, Debug)]
pub struct BreadthFirstOptions {
    /// Center of the bounding rectangle in model units.
    pub center: Point2,
    /// Width of the bounding rectangle.
    pub width: f32,
    /// Height of the bounding rectangle.
    pub height: f32,
    /// Padding kept clear inside the box edges.
    pub padding: f32,
    /// When true, traversal follows out-edges only and roots default to
    /// sourceless nodes.
    pub directed: bool,
    /// Explicit roots; derived from the structure when absent.
    pub roots: Option<Vec<NodeIndex>>,
    /// Growth direction of the layer stack.
    pub direction: BfsDirection,
    /// When true, layers become concentric rings instead of rows.
    pub circle: bool,
    /// When true, every layer spreads across the widest layer width.
    pub grid: bool,
    /// When true, pushes nodes below their deepest in-neighbour (directed
    /// graphs only, single shift per node, bails on cycles).
    pub maximal: bool,
    /// When true, nodes inside a layer keep at least the node extent apart.
    pub avoid_overlap: bool,
    /// Uniform node extent used for spacing.
    pub node_size: f32,
}

impl Default for BreadthFirstOptions {
    fn default() -> Self {
        Self {
            center: Point2::ZERO,
            width: 640.0,
            height: 480.0,
            padding: 30.0,
            directed: false,
            roots: None,
            direction: BfsDirection::Downward,
            circle: false,
            grid: false,
            maximal: false,
            avoid_overlap: true,
            node_size: 24.0,
        }
    }
}

/// Breadth-first engine implementing the shared layout contract.
pub struct BreadthFirstLayout {
    options: BreadthFirstOptions,
    common: CommonOptions,
}

impl BreadthFirstLayout {
    pub fn new(center: Point2, width: f32, height: f32) -> Self {
        Self {
            options: BreadthFirstOptions {
                center,
                width,
                height,
                ..BreadthFirstOptions::default()
            },
            common: CommonOptions::default(),
        }
    }

    pub fn with_options(options: BreadthFirstOptions) -> Self {
        Self {
            options,
            common: CommonOptions::default(),
        }
    }

    /// Overrides the shared sort, fit and spacing inputs.
    pub fn with_common(mut self, common: CommonOptions) -> Self {
        self.common = common;
        self
    }

    pub fn options(&self) -> &BreadthFirstOptions {
        &self.options
    }
}

impl LayoutEngine for BreadthFirstLayout {
    fn layout(&self, graph: &dyn GraphView, previous: &Positions, fixed: &FixedNodes) -> Positions {
        let layers = compute_layers(graph, &self.options, &self.common);
        let mut result: Positions = HashMap::new();
        if layers.is_empty() {
            return result;
        }
        let mut spaced = self.options.clone();
        spaced.width = self.common.scaled(self.options.width);
        spaced.height = self.common.scaled(self.options.height);
        spaced.padding = self.common.scaled(self.options.padding);
        spaced.node_size = self.common.scaled(self.options.node_size);
        for (depth, layer) in layers.iter().enumerate() {
            for (index, node) in layer.iter().enumerate() {
                if fixed.contains(node)
                    && let Some(held) = previous.get(node)
                {
                    result.insert(*node, *held);
                    continue;
                }
                result.insert(*node, layer_position(&spaced, &layers, depth, index));
            }
        }
        result
    }

    fn name(&self) -> &'static str {
        "breadthfirst"
    }

    fn set_common(&mut self, common: CommonOptions) {
        self.common = common;
    }
}

/// Layers of node identifiers from the outermost (roots) inward.
fn compute_layers(
    graph: &dyn GraphView,
    options: &BreadthFirstOptions,
    common: &CommonOptions,
) -> Vec<Vec<NodeIndex>> {
    let mut ids = graph.node_ids();
    common.sort_ids(&mut ids, graph);
    if ids.is_empty() {
        return Vec::new();
    }
    let present: HashSet<NodeIndex> = ids.iter().copied().collect();
    let roots = pick_roots(graph, &ids, options, &present);
    let mut depth: HashMap<NodeIndex, usize> = HashMap::new();
    let mut queue: VecDeque<NodeIndex> = VecDeque::new();
    for root in roots {
        if depth.contains_key(&root) {
            continue;
        }
        depth.insert(root, 0);
        queue.push_back(root);
    }
    while let Some(node) = queue.pop_front() {
        let next = depth[&node] + 1;
        let mut outgoers = if options.directed {
            graph.successors(node)
        } else {
            graph.neighbors(node)
        };
        outgoers.sort_unstable_by_key(|node| node.index());
        for next_node in outgoers {
            if !present.contains(&next_node) || depth.contains_key(&next_node) {
                continue;
            }
            depth.insert(next_node, next);
            queue.push_back(next_node);
        }
    }
    if options.directed && options.maximal {
        maximal_adjustment(graph, &ids, &mut depth);
    }
    // Nodes still unassigned after traversal (and maximal adjustment, which
    // can claim previously unreachable nodes on cyclic graphs) form their own
    // leading layer.
    let mut orphans: Vec<NodeIndex> = ids
        .iter()
        .copied()
        .filter(|node| !depth.contains_key(node))
        .collect();
    orphans.sort_unstable_by_key(|node| node.index());
    let max_depth = depth.values().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<NodeIndex>> = vec![Vec::new(); max_depth + 1];
    let mut by_index: Vec<NodeIndex> = depth.keys().copied().collect();
    by_index.sort_unstable_by_key(|node| node.index());
    for node in by_index {
        layers[depth[&node]].push(node);
    }
    layers.retain(|layer| !layer.is_empty());
    // Slot records each node as (depth, layer length, index in layer); layers
    // sort from the outside inward so upper positions settle before the layers
    // below read them, mirroring the reference ordering pass.
    let mut slot: HashMap<NodeIndex, (usize, usize, usize)> = HashMap::new();
    for (depth_index, layer) in layers.iter().enumerate() {
        for (index, node) in layer.iter().enumerate() {
            slot.insert(*node, (depth_index, layer.len(), index));
        }
    }
    for (depth_index, layer) in layers.iter_mut().enumerate() {
        sort_layer_by_connectivity(graph, layer, &slot);
        let length = layer.len();
        for (index, node) in layer.iter().enumerate() {
            slot.insert(*node, (depth_index, length, index));
        }
    }
    if !orphans.is_empty() {
        layers.insert(0, orphans);
    }
    layers
}

/// Default roots: sourceless nodes for directed traversal, otherwise the
/// highest-degree node of each connected component.
fn pick_roots(
    graph: &dyn GraphView,
    ids: &[NodeIndex],
    options: &BreadthFirstOptions,
    present: &HashSet<NodeIndex>,
) -> Vec<NodeIndex> {
    if let Some(roots) = options.roots.as_ref() {
        let mut filtered: Vec<NodeIndex> = roots
            .iter()
            .copied()
            .filter(|node| present.contains(node))
            .collect();
        filtered.sort_unstable_by_key(|node| node.index());
        filtered.dedup_by_key(|node| node.index());
        return filtered;
    }
    if options.directed {
        return ids
            .iter()
            .copied()
            .filter(|node| graph.predecessors(*node).is_empty())
            .collect();
    }
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut roots = Vec::new();
    for seed in ids {
        if visited.contains(seed) {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = VecDeque::from([*seed]);
        visited.insert(*seed);
        while let Some(node) = queue.pop_front() {
            component.push(node);
            for neighbour in graph.neighbors(node) {
                if present.contains(&neighbour) && visited.insert(neighbour) {
                    queue.push_back(neighbour);
                }
            }
        }
        let best = component
            .iter()
            .map(|node| graph.degree(*node))
            .max()
            .unwrap_or(0);
        let mut heads: Vec<NodeIndex> = component
            .into_iter()
            .filter(|node| graph.degree(*node) == best)
            .collect();
        heads.sort_unstable_by_key(|node| node.index());
        roots.extend(heads);
    }
    roots.sort_unstable_by_key(|node| node.index());
    roots.dedup_by_key(|node| node.index());
    roots
}

/// Pushes nodes below their deepest in-neighbour, at most once per node.
///
/// A repeated shift proves the graph is cyclic, in which case the adjustment
/// stops and keeps the breadth-first depths computed so far.
fn maximal_adjustment(
    graph: &dyn GraphView,
    ids: &[NodeIndex],
    depth: &mut HashMap<NodeIndex, usize>,
) {
    let mut shifted: HashSet<NodeIndex> = HashSet::new();
    let mut queue: VecDeque<NodeIndex> = ids.iter().copied().collect();
    while let Some(node) = queue.pop_front() {
        let deepest = graph
            .predecessors(node)
            .into_iter()
            .filter_map(|incoming| depth.get(&incoming).copied())
            .max()
            .unwrap_or(0);
        let current = depth.get(&node).copied().unwrap_or(0);
        let needs_shift = if graph.predecessors(node).is_empty() {
            false
        } else {
            current <= deepest
        };
        if needs_shift {
            if !shifted.insert(node) {
                return;
            }
            depth.insert(node, deepest + 1);
            for outgoing in graph.successors(node) {
                queue.push_back(outgoing);
            }
        }
    }
}

/// Reorders one layer so nodes linked to similarly positioned upper layers
/// sit together; ties break by node index for determinism.
///
/// Each node scores the mean of its upper neighbours' relative positions
/// (index within their layer divided by that layer's length); nodes without
/// upper neighbours score zero and settle at the front.
fn sort_layer_by_connectivity(
    graph: &dyn GraphView,
    layer: &mut [NodeIndex],
    slot: &HashMap<NodeIndex, (usize, usize, usize)>,
) {
    let mut percent: HashMap<NodeIndex, f32> = HashMap::new();
    for node in layer.iter() {
        let node_depth = slot.get(node).map(|entry| entry.0).unwrap_or(0);
        let mut total = 0.0f32;
        let mut samples = 0usize;
        for neighbour in graph.neighbors(*node) {
            let Some((near_depth, near_len, near_index)) = slot.get(&neighbour).copied() else {
                continue;
            };
            if near_depth >= node_depth {
                continue;
            }
            total += near_index as f32 / near_len.max(1) as f32;
            samples += 1;
        }
        percent.insert(
            *node,
            if samples == 0 {
                0.0
            } else {
                total / samples as f32
            },
        );
    }
    layer.sort_by(|a, b| {
        percent[a]
            .partial_cmp(&percent[b])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.index().cmp(&b.index()))
    });
}

/// Coordinates of one node from its layer, index and the layout direction.
fn layer_position(
    options: &BreadthFirstOptions,
    layers: &[Vec<NodeIndex>],
    depth: usize,
    index: usize,
) -> Point2 {
    let layer_size = layers[depth].len().max(1);
    let depth_count = layers.len().max(1);
    let min_gap = if options.avoid_overlap {
        options.node_size
    } else {
        0.0
    };
    let vertical = if depth_count <= 1 {
        0.0
    } else {
        ((options.height - 2.0 * options.padding - options.node_size) / (depth_count - 1) as f32)
            .max(min_gap)
    };
    let span = if options.grid {
        layers.iter().map(Vec::len).max().unwrap_or(1).max(1)
    } else {
        layer_size
    };
    let horizontal = if span <= 1 {
        0.0
    } else {
        ((options.width - 2.0 * options.padding - options.node_size) / (span - 1) as f32)
            .max(min_gap)
    };
    if options.circle {
        let step = (options.width.min(options.height) / 2.0 / depth_count as f32).max(min_gap);
        let mut radius = step * depth as f32 + step;
        if depth == 0 && layers[0].len() == 1 {
            radius = 0.0;
        }
        let theta = 2.0 * std::f32::consts::PI / layer_size as f32 * index as f32;
        return Point2::new(
            options.center.x + radius * theta.cos(),
            options.center.y + radius * theta.sin(),
        );
    }
    let across = (index + 1) as f32 - (layer_size + 1) as f32 / 2.0;
    let along = (depth + 1) as f32 - (depth_count + 1) as f32 / 2.0;
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

    fn chain_options() -> BreadthFirstOptions {
        BreadthFirstOptions {
            avoid_overlap: false,
            padding: 0.0,
            node_size: 0.0,
            ..BreadthFirstOptions::default()
        }
    }

    #[test]
    fn empty_graph_has_no_layers() {
        let graph = MockGraph::empty();
        let engine = BreadthFirstLayout::with_options(chain_options());
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert!(positions.is_empty());
    }

    #[test]
    fn chain_forms_one_node_per_layer() {
        let graph = MockGraph::chain(4);
        let engine = BreadthFirstLayout::with_options(BreadthFirstOptions {
            directed: true,
            ..chain_options()
        });
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 4);
        let depths: Vec<f32> = (0..4)
            .map(|index| positions[&NodeIndex::new(index)].y)
            .collect();
        assert!(depths[0] < depths[1] && depths[1] < depths[2] && depths[2] < depths[3]);
    }

    #[test]
    fn layers_match_breadth_first_depths() {
        let mut graph = MockGraph::chain(3);
        graph.push_edge(0, 2);
        let options = BreadthFirstOptions {
            directed: true,
            ..chain_options()
        };
        let layers = compute_layers(&graph, &options, &CommonOptions::default());
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0], vec![NodeIndex::new(0)]);
        assert_eq!(layers[1].len(), 2);
    }

    #[test]
    fn orphans_open_their_own_layer() {
        let mut graph = MockGraph::chain(2);
        graph.push_edge(2, 2);
        let options = BreadthFirstOptions {
            directed: true,
            ..chain_options()
        };
        let layers = compute_layers(&graph, &options, &CommonOptions::default());
        assert_eq!(layers[0], vec![NodeIndex::new(2)]);
        assert!(layers.iter().flatten().count() == 3);
    }

    #[test]
    fn cyclic_graphs_keep_breadth_first_depths() {
        let mut graph = MockGraph::chain(2);
        graph.push_edge(1, 0);
        let options = BreadthFirstOptions {
            directed: true,
            maximal: true,
            ..chain_options()
        };
        let layers = compute_layers(&graph, &options, &CommonOptions::default());
        assert!(!layers.is_empty());
        assert!(layers.iter().flatten().count() == 2);
    }

    #[test]
    fn upward_mirrors_downward() {
        let graph = MockGraph::chain(3);
        let down = BreadthFirstLayout::with_options(chain_options());
        let up = BreadthFirstLayout::with_options(BreadthFirstOptions {
            direction: BfsDirection::Upward,
            ..chain_options()
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
