//! Deterministic synthetic graphs for scale benchmarks.
//!
//! Graphs are generated from a fixed seed only, so baseline and optimized
//! runs compare identical structure. They live in code rather than on disk.

use cg_graph::{NodeIndex, Positions};
use cg_types::Point2;

/// One synthetic benchmark case.
#[derive(Clone, Debug)]
pub struct SynthGraph {
    pub edges: Vec<(NodeIndex, NodeIndex)>,
    pub positions: Positions,
    pub node_count: usize,
}

fn next_rand(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(0xBF58_476D_1CE4_E5B9)
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    *state ^= *state >> 29;
    *state
}

/// Grid with deterministic random cross edges.
pub fn grid_with_random_edges(nodes: usize, extra_ratio: f64, seed: u64) -> SynthGraph {
    let side = (nodes as f64).sqrt().ceil() as usize;
    let mut positions = Positions::new();
    for index in 0..nodes {
        let node = NodeIndex::new(index);
        let col = (index % side) as f32;
        let row = (index / side) as f32;
        positions.insert(node, Point2::new(col * 40.0, row * 40.0));
    }
    let mut edges = Vec::new();
    for row in 0..side {
        for col in 0..side {
            let index = row * side + col;
            if index >= nodes {
                continue;
            }
            if col + 1 < side && index + 1 < nodes {
                edges.push((NodeIndex::new(index), NodeIndex::new(index + 1)));
            }
            if row + 1 < side && index + side < nodes {
                edges.push((NodeIndex::new(index), NodeIndex::new(index + side)));
            }
        }
    }
    let extra = (nodes as f64 * extra_ratio) as usize;
    let mut state = seed;
    for _ in 0..extra {
        let source = (next_rand(&mut state) % nodes as u64) as usize;
        let target = (next_rand(&mut state) % nodes as u64) as usize;
        if source != target {
            edges.push((NodeIndex::new(source), NodeIndex::new(target)));
        }
    }
    SynthGraph {
        edges,
        positions,
        node_count: nodes,
    }
}

/// Chain with deterministic cross-layer edges.
pub fn chain_with_cross_edges(nodes: usize, span: usize, seed: u64) -> SynthGraph {
    let mut positions = Positions::new();
    for index in 0..nodes {
        positions.insert(
            NodeIndex::new(index),
            Point2::new(index as f32 * 24.0, (index % 16) as f32 * 24.0),
        );
    }
    let mut edges = Vec::new();
    for index in 1..nodes {
        edges.push((NodeIndex::new(index - 1), NodeIndex::new(index)));
    }
    let mut state = seed;
    for index in 0..nodes {
        let jump = 2 + (next_rand(&mut state) % span.max(2) as u64) as usize;
        let target = (index + jump) % nodes.max(1);
        if target != index && nodes > 1 {
            edges.push((NodeIndex::new(index), NodeIndex::new(target)));
        }
    }
    SynthGraph {
        edges,
        positions,
        node_count: nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_graphs_are_deterministic() {
        let first = grid_with_random_edges(256, 0.5, 7);
        let second = grid_with_random_edges(256, 0.5, 7);
        assert_eq!(first.edges, second.edges);
        let chain_a = chain_with_cross_edges(128, 8, 11);
        let chain_b = chain_with_cross_edges(128, 8, 11);
        assert_eq!(chain_a.edges, chain_b.edges);
    }

    #[test]
    fn synth_graphs_cover_three_scales() {
        for nodes in [1_000, 5_000, 10_000] {
            let graph = grid_with_random_edges(nodes, 0.3, 42);
            assert_eq!(graph.node_count, nodes);
            assert_eq!(graph.positions.len(), nodes);
            assert!(!graph.edges.is_empty());
        }
    }
}
