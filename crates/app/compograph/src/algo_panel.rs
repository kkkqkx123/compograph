//! Algorithm panel outcome mapping.
//!
//! These helpers translate raw algorithm results into bypass patches and
//! summary text. They are pure functions over owned data so the panel stays
//! testable without a window: background tasks compute an [`AlgoOutcome`] off
//! the UI thread, then the view commits it through the generation guard, which
//! drops write-backs from runs the user has already superseded.

use cg_graph::NodeIndex;
use cg_render::{EdgeStylePatch, NodeStylePatch, scale_for_rank, scc_fill};

/// Ordered node pair identifying one highlighted edge.
pub type EdgePair = (NodeIndex, NodeIndex);

/// Patch sets one algorithm run applies on top of the selection bypass.
#[derive(Clone, Debug, Default)]
pub struct AlgoOutcome {
    pub nodes: Vec<(NodeIndex, NodeStylePatch)>,
    pub edges: Vec<(EdgePair, EdgeStylePatch)>,
    pub summary: String,
}

/// True when `incoming` no longer matches the latest requested generation.
///
/// The view bumps its generation on every run and commits only the outcome
/// carrying the current value, so overlapping background runs cannot
/// overwrite each other out of order.
pub fn is_stale(current: u64, incoming: u64) -> bool {
    current != incoming
}

/// Highlights a node sequence and its consecutive edges.
pub fn path_outcome(path: &[NodeIndex], cost: f32, elapsed_ms: f64, label: &str) -> AlgoOutcome {
    if path.is_empty() {
        return AlgoOutcome {
            summary: format!("{label}: no path ({elapsed_ms:.1}ms)"),
            ..AlgoOutcome::default()
        };
    }
    let nodes = path
        .iter()
        .map(|node| (*node, NodeStylePatch::selected()))
        .collect();
    let mut edges = Vec::new();
    for pair in path.windows(2) {
        edges.push(((pair[0], pair[1]), EdgeStylePatch::highlighted()));
    }
    AlgoOutcome {
        nodes,
        edges,
        summary: format!(
            "{label}: cost {cost:.2}, {} nodes ({elapsed_ms:.1}ms)",
            path.len()
        ),
    }
}

/// Colors one connected group per component, merging overflow groups.
pub fn scc_outcome(components: &[Vec<NodeIndex>], elapsed_ms: f64) -> AlgoOutcome {
    let mut nodes = Vec::new();
    for (group, component) in components.iter().enumerate() {
        let patch = NodeStylePatch::tinted(scc_fill(group));
        for node in component {
            nodes.push((*node, patch.clone()));
        }
    }
    let largest = components.iter().map(Vec::len).max().unwrap_or(0);
    AlgoOutcome {
        nodes,
        edges: Vec::new(),
        summary: format!(
            "components: {} groups, largest {largest} ({elapsed_ms:.1}ms)",
            components.len()
        ),
    }
}

/// Maps centrality scores to node sizes over the observed score range.
pub fn centrality_outcome(
    order: &[NodeIndex],
    scores: &[f32],
    label: &str,
    elapsed_ms: f64,
) -> AlgoOutcome {
    let mut finite: Vec<f32> = scores
        .iter()
        .copied()
        .filter(|score| score.is_finite())
        .collect();
    finite.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (min, max) = match (finite.first(), finite.last()) {
        (Some(min), Some(max)) => (*min, *max),
        _ => (0.0, 0.0),
    };
    let nodes = order
        .iter()
        .zip(scores.iter())
        .map(|(node, score)| {
            (
                *node,
                NodeStylePatch::rescaled(scale_for_rank(*score, min, max)),
            )
        })
        .collect();
    let peak = finite.last().copied().unwrap_or(0.0);
    AlgoOutcome {
        nodes,
        edges: Vec::new(),
        summary: format!(
            "{label}: {} nodes, peak {peak:.3} ({elapsed_ms:.1}ms)",
            order.len()
        ),
    }
}

/// Highlights articulation points and bridges together.
pub fn cut_outcome(
    points: &[NodeIndex],
    bridges: &[(NodeIndex, NodeIndex)],
    elapsed_ms: f64,
) -> AlgoOutcome {
    AlgoOutcome {
        nodes: points
            .iter()
            .map(|node| (*node, NodeStylePatch::selected()))
            .collect(),
        edges: bridges
            .iter()
            .map(|(source, target)| ((*source, *target), EdgeStylePatch::highlighted()))
            .collect(),
        summary: format!(
            "cuts: {} points, {} bridges ({elapsed_ms:.1}ms)",
            points.len(),
            bridges.len()
        ),
    }
}

/// Maps scores to node sizes over the observed score range.
pub fn pagerank_outcome(order: &[NodeIndex], scores: &[f32], elapsed_ms: f64) -> AlgoOutcome {
    centrality_outcome(order, scores, "pagerank", elapsed_ms)
}

/// Highlights nodes in traversal order without implying a path.
pub fn traversal_outcome(order: &[NodeIndex], label: &str, elapsed_ms: f64) -> AlgoOutcome {
    AlgoOutcome {
        nodes: order
            .iter()
            .map(|node| (*node, NodeStylePatch::selected()))
            .collect(),
        edges: Vec::new(),
        summary: format!("{label}: {} nodes reached ({elapsed_ms:.1}ms)", order.len()),
    }
}

/// Highlights a topological order, or reports the cycle member.
pub fn topo_outcome(order: &Result<Vec<NodeIndex>, NodeIndex>, elapsed_ms: f64) -> AlgoOutcome {
    match order {
        Ok(sequence) => traversal_outcome(sequence, "topological order", elapsed_ms),
        Err(member) => AlgoOutcome {
            summary: format!(
                "topological order: cycle at node {} ({elapsed_ms:.1}ms)",
                member.index()
            ),
            ..AlgoOutcome::default()
        },
    }
}

/// Thickens every kept edge of a transitive reduction.
pub fn reduction_outcome(
    kept: &Result<Vec<(NodeIndex, NodeIndex)>, NodeIndex>,
    elapsed_ms: f64,
) -> AlgoOutcome {
    match kept {
        Ok(edges) => AlgoOutcome {
            nodes: Vec::new(),
            edges: edges
                .iter()
                .map(|(source, target)| ((*source, *target), EdgeStylePatch::widened()))
                .collect(),
            summary: format!(
                "transitive reduction: {} edges ({elapsed_ms:.1}ms)",
                edges.len()
            ),
        },
        Err(member) => AlgoOutcome {
            summary: format!(
                "transitive reduction: cycle at node {} ({elapsed_ms:.1}ms)",
                member.index()
            ),
            ..AlgoOutcome::default()
        },
    }
}

/// Reports all-pairs reachability without highlighting.
///
/// Reachability is a dense matrix property, so the panel shows counts while
/// the canvas keeps its current highlights.
pub fn pairs_outcome(reachable: usize, possible: usize, elapsed_ms: f64) -> AlgoOutcome {
    AlgoOutcome {
        summary: format!("all pairs: {reachable} of {possible} reachable ({elapsed_ms:.1}ms)"),
        ..AlgoOutcome::default()
    }
}

/// Highlights every node dominated from the root.
pub fn dominators_outcome(
    dominated: &[NodeIndex],
    root: NodeIndex,
    elapsed_ms: f64,
) -> AlgoOutcome {
    AlgoOutcome {
        nodes: dominated
            .iter()
            .map(|node| (*node, NodeStylePatch::selected()))
            .collect(),
        edges: Vec::new(),
        summary: format!(
            "dominators from {}: {} nodes ({elapsed_ms:.1}ms)",
            root.index(),
            dominated.len()
        ),
    }
}

/// Thickens every edge of a spanning forest and totals its weight.
pub fn forest_outcome(edges: &[(NodeIndex, NodeIndex, f32)], elapsed_ms: f64) -> AlgoOutcome {
    let total: f32 = edges.iter().map(|(_, _, weight)| weight).sum();
    AlgoOutcome {
        nodes: Vec::new(),
        edges: edges
            .iter()
            .map(|(source, target, _)| ((*source, *target), EdgeStylePatch::widened()))
            .collect(),
        summary: format!(
            "spanning forest: {} edges, weight {total:.2} ({elapsed_ms:.1}ms)",
            edges.len()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_outcome_covers_nodes_edges_and_summaries() {
        let path = vec![NodeIndex::new(0), NodeIndex::new(1), NodeIndex::new(2)];
        let outcome = path_outcome(&path, 3.0, 1.0, "shortest");
        assert_eq!(outcome.nodes.len(), 3);
        assert_eq!(outcome.edges.len(), 2);
        assert!(outcome.summary.contains("3.00"));
        let missing = path_outcome(&[], 0.0, 1.0, "shortest");
        assert!(missing.nodes.is_empty() && missing.edges.is_empty());
        assert!(missing.summary.contains("no path"));
    }

    #[test]
    fn scc_outcome_merges_overflow_into_one_fill() {
        let components: Vec<Vec<NodeIndex>> =
            (0..10).map(|group| vec![NodeIndex::new(group)]).collect();
        let outcome = scc_outcome(&components, 0.5);
        assert_eq!(outcome.nodes.len(), 10);
        let overflow = outcome.nodes[9].1.fill.expect("overflow group is tinted");
        assert_eq!(
            overflow,
            outcome.nodes[8].1.fill.expect("overflow shares one fill")
        );
        assert_ne!(
            outcome.nodes[0].1.fill.expect("first group is tinted"),
            overflow
        );
        assert!(outcome.summary.contains("10 groups"));
    }

    #[test]
    fn pagerank_outcome_scales_with_the_score_range() {
        let order = vec![NodeIndex::new(0), NodeIndex::new(1)];
        let outcome = pagerank_outcome(&order, &[0.1, 0.9], 0.5);
        let small = outcome.nodes[0].1.scale.expect("a scale is mapped");
        let large = outcome.nodes[1].1.scale.expect("a scale is mapped");
        assert!(small < large);
        let flat = pagerank_outcome(&order, &[0.5, 0.5], 0.5);
        assert_eq!(flat.nodes[0].1.scale, Some(1.0));
        assert!(outcome.summary.contains("2 nodes"));
    }

    #[test]
    fn forest_outcome_counts_edges_and_stale_generations_lose() {
        let edges = vec![(NodeIndex::new(0), NodeIndex::new(1), 1.5)];
        let outcome = forest_outcome(&edges, 0.5);
        assert_eq!(outcome.edges.len(), 1);
        assert!(outcome.summary.contains("1 edges"));
        assert!(!is_stale(3, 3));
        assert!(is_stale(4, 3));
    }

    #[test]
    fn centrality_outcome_labels_scores_and_scales() {
        let order = vec![NodeIndex::new(0), NodeIndex::new(1)];
        let outcome = centrality_outcome(&order, &[0.1, 0.9], "degree", 0.5);
        assert_eq!(outcome.nodes.len(), 2);
        assert!(outcome.summary.contains("degree"));
        assert!(outcome.summary.contains("2 nodes"));
    }

    #[test]
    fn traversal_topo_reduction_pairs_and_dominators_cover_remaining_bridges() {
        let order = vec![NodeIndex::new(0), NodeIndex::new(1)];
        let walked = traversal_outcome(&order, "bfs", 0.5);
        assert_eq!(walked.nodes.len(), 2);
        assert!(walked.summary.contains("bfs"));
        let topo = topo_outcome(&Ok(order.clone()), 0.5);
        assert_eq!(topo.nodes.len(), 2);
        let cyclic = topo_outcome(&Err(NodeIndex::new(3)), 0.5);
        assert!(cyclic.nodes.is_empty() && cyclic.summary.contains("cycle"));
        let kept = reduction_outcome(&Ok(vec![(NodeIndex::new(0), NodeIndex::new(1))]), 0.5);
        assert_eq!(kept.edges.len(), 1);
        let pairs = pairs_outcome(3, 4, 0.5);
        assert!(pairs.nodes.is_empty() && pairs.summary.contains("3 of 4"));
        let dominated = dominators_outcome(&order, NodeIndex::new(0), 0.5);
        assert_eq!(dominated.nodes.len(), 2);
        assert!(dominated.summary.contains("from 0"));
    }

    #[test]
    fn cut_outcome_highlights_points_and_bridges() {
        let points = vec![NodeIndex::new(1)];
        let bridges = vec![(NodeIndex::new(0), NodeIndex::new(1))];
        let outcome = cut_outcome(&points, &bridges, 0.5);
        assert_eq!(outcome.nodes.len(), 1);
        assert_eq!(outcome.edges.len(), 1);
        assert!(outcome.summary.contains("1 points"));
    }
}
