//! Algorithm panel outcome mapping.
//!
//! These helpers translate raw algorithm results into bypass patches and
//! summary text. They are pure functions over owned data so the panel stays
//! testable without a window: background tasks compute an [`AlgoOutcome`] off
//! the UI thread, then the view commits it through the generation guard, which
//! drops write-backs from runs the user has already superseded.

use cg_graph::{MinCut, NodeIndex};
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

/// Colors one group per cluster, merging overflow groups.
pub fn groups_outcome(groups: &[Vec<NodeIndex>], label: &str, elapsed_ms: f64) -> AlgoOutcome {
    let mut nodes = Vec::new();
    for (group, members) in groups.iter().enumerate() {
        let patch = NodeStylePatch::tinted(scc_fill(group));
        for node in members {
            nodes.push((*node, patch.clone()));
        }
    }
    let largest = groups.iter().map(Vec::len).max().unwrap_or(0);
    AlgoOutcome {
        nodes,
        edges: Vec::new(),
        summary: format!(
            "{label}: {} groups, largest {largest} ({elapsed_ms:.1}ms)",
            groups.len()
        ),
    }
}

/// Colors one connected group per component, merging overflow groups.
pub fn scc_outcome(components: &[Vec<NodeIndex>], elapsed_ms: f64) -> AlgoOutcome {
    groups_outcome(components, "components", elapsed_ms)
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
///
/// Biconnected groups only join the summary count: cut vertices belong to
/// every block they join, so highlighting whole blocks would repaint the
/// point and bridge signal this outcome is built around.
pub fn cut_outcome(
    points: &[NodeIndex],
    bridges: &[(NodeIndex, NodeIndex)],
    groups: &[Vec<NodeIndex>],
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
            "cuts: {} points, {} bridges, {} blocks ({elapsed_ms:.1}ms)",
            points.len(),
            bridges.len(),
            groups.len()
        ),
    }
}

/// Highlights an Eulerian trail, or reports the first blocking node.
pub fn euler_outcome(
    trail: &Result<Vec<NodeIndex>, NodeIndex>,
    label: &str,
    elapsed_ms: f64,
) -> AlgoOutcome {
    match trail {
        Ok(path) => {
            if path.is_empty() {
                return AlgoOutcome {
                    summary: format!("{label}: no trail ({elapsed_ms:.1}ms)"),
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
                    "{label}: {} nodes, {} edges ({elapsed_ms:.1}ms)",
                    path.len(),
                    path.len().saturating_sub(1)
                ),
            }
        }
        Err(member) => AlgoOutcome {
            summary: format!(
                "{label}: no trail at node {} ({elapsed_ms:.1}ms)",
                member.index()
            ),
            ..AlgoOutcome::default()
        },
    }
}

/// Highlights every edge crossing the global minimum cut.
pub fn mincut_outcome(cut: &MinCut, elapsed_ms: f64) -> AlgoOutcome {
    AlgoOutcome {
        nodes: Vec::new(),
        edges: cut
            .edges
            .iter()
            .map(|(source, target)| ((*source, *target), EdgeStylePatch::highlighted()))
            .collect(),
        summary: format!(
            "min cut: weight {:.2}, {} edges ({elapsed_ms:.1}ms)",
            cut.weight,
            cut.edges.len()
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

/// Thickens every edge carrying flow and totals the flow value.
pub fn flow_outcome(
    detailed: &[(NodeIndex, NodeIndex, f32)],
    value: f32,
    elapsed_ms: f64,
) -> AlgoOutcome {
    AlgoOutcome {
        nodes: Vec::new(),
        edges: detailed
            .iter()
            .map(|(source, target, _)| ((*source, *target), EdgeStylePatch::highlighted()))
            .collect(),
        summary: format!(
            "max flow: value {value:.2}, {} edges ({elapsed_ms:.1}ms)",
            detailed.len()
        ),
    }
}

/// Highlights every matched pair as one edge.
pub fn matching_outcome(
    pairs: &[(NodeIndex, NodeIndex)],
    label: &str,
    elapsed_ms: f64,
) -> AlgoOutcome {
    AlgoOutcome {
        nodes: Vec::new(),
        edges: pairs
            .iter()
            .map(|(source, target)| ((*source, *target), EdgeStylePatch::highlighted()))
            .collect(),
        summary: format!("{label}: {} pairs ({elapsed_ms:.1}ms)", pairs.len()),
    }
}

/// Highlights every feedback edge whose removal breaks all directed cycles.
pub fn feedback_outcome(edges: &[(NodeIndex, NodeIndex)], elapsed_ms: f64) -> AlgoOutcome {
    AlgoOutcome {
        nodes: Vec::new(),
        edges: edges
            .iter()
            .map(|(source, target)| ((*source, *target), EdgeStylePatch::highlighted()))
            .collect(),
        summary: format!("feedback arcs: {} edges ({elapsed_ms:.1}ms)", edges.len()),
    }
}

/// Highlights the union of up to `limit` simple paths.
pub fn simple_paths_outcome(paths: &[Vec<NodeIndex>], elapsed_ms: f64) -> AlgoOutcome {
    if paths.is_empty() {
        return AlgoOutcome {
            summary: format!("simple paths: no path ({elapsed_ms:.1}ms)"),
            ..AlgoOutcome::default()
        };
    }
    use std::collections::BTreeSet;
    let mut node_set: BTreeSet<NodeIndex> = BTreeSet::new();
    let mut edge_set: BTreeSet<EdgePair> = BTreeSet::new();
    for path in paths {
        for node in path {
            node_set.insert(*node);
        }
        for pair in path.windows(2) {
            edge_set.insert((pair[0], pair[1]));
        }
    }
    AlgoOutcome {
        nodes: node_set
            .into_iter()
            .map(|node| (node, NodeStylePatch::selected()))
            .collect(),
        edges: edge_set
            .into_iter()
            .map(|pair| (pair, EdgeStylePatch::highlighted()))
            .collect(),
        summary: format!(
            "simple paths: {} paths, longest {} ({elapsed_ms:.1}ms)",
            paths.len(),
            paths.iter().map(Vec::len).max().unwrap_or(0)
        ),
    }
}

/// Colors undirected components and summarizes directed reachability and cycles.
pub fn connectivity_outcome(
    groups: &[Vec<NodeIndex>],
    reachable: bool,
    directed_cyclic: bool,
    undirected_cyclic: bool,
    bipartite: bool,
    elapsed_ms: f64,
) -> AlgoOutcome {
    let mut outcome = groups_outcome(groups, "connectivity", elapsed_ms);
    outcome.summary = format!(
        "connectivity: {} groups, reachable {}, directed cyclic {}, undirected cyclic {}, bipartite {} ({:.1}ms)",
        groups.len(),
        reachable,
        directed_cyclic,
        undirected_cyclic,
        bipartite,
        elapsed_ms
    );
    outcome
}

/// Colors condensation groups and highlights edges crossing groups.
pub fn condensation_outcome(
    groups: &[Vec<NodeIndex>],
    crossing: &[(NodeIndex, NodeIndex)],
    elapsed_ms: f64,
) -> AlgoOutcome {
    let mut outcome = groups_outcome(groups, "condensation", elapsed_ms);
    for (source, target) in crossing {
        outcome
            .edges
            .push(((*source, *target), EdgeStylePatch::highlighted()));
    }
    outcome.summary = format!(
        "condensation: {} groups, {} crossing edges ({elapsed_ms:.1}ms)",
        groups.len(),
        crossing.len()
    );
    outcome
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
        let groups = vec![vec![NodeIndex::new(0), NodeIndex::new(1)]];
        let outcome = cut_outcome(&points, &bridges, &groups, 0.5);
        assert_eq!(outcome.nodes.len(), 1);
        assert_eq!(outcome.edges.len(), 1);
        assert!(outcome.summary.contains("1 points"));
        assert!(outcome.summary.contains("1 blocks"));
    }

    #[test]
    fn euler_outcome_covers_trail_empty_and_blocked() {
        let trail = vec![NodeIndex::new(0), NodeIndex::new(1), NodeIndex::new(2)];
        let outcome = euler_outcome(&Ok(trail), "euler directed", 0.5);
        assert_eq!(outcome.nodes.len(), 3);
        assert_eq!(outcome.edges.len(), 2);
        assert!(outcome.summary.contains("3 nodes"));
        let empty = euler_outcome(&Ok(Vec::new()), "euler directed", 0.5);
        assert!(empty.nodes.is_empty() && empty.summary.contains("no trail"));
        let blocked = euler_outcome(&Err(NodeIndex::new(4)), "euler directed", 0.5);
        assert!(blocked.nodes.is_empty() && blocked.summary.contains("node 4"));
    }

    #[test]
    fn mincut_outcome_highlights_crossing_edges() {
        let cut = MinCut {
            weight: 2.5,
            edges: vec![(NodeIndex::new(0), NodeIndex::new(1))],
        };
        let outcome = mincut_outcome(&cut, 0.5);
        assert!(outcome.nodes.is_empty());
        assert_eq!(outcome.edges.len(), 1);
        assert!(outcome.summary.contains("2.50"));
    }

    #[test]
    fn groups_outcome_labels_cluster_runs() {
        let groups = vec![vec![NodeIndex::new(0)], vec![NodeIndex::new(1)]];
        let outcome = groups_outcome(&groups, "markov", 0.5);
        assert_eq!(outcome.nodes.len(), 2);
        assert!(outcome.summary.contains("markov"));
        assert!(outcome.summary.contains("2 groups"));
    }

    #[test]
    fn flow_matching_feedback_and_simple_paths_highlight_edges() {
        let flow = flow_outcome(&[(NodeIndex::new(0), NodeIndex::new(1), 2.0)], 2.0, 0.5);
        assert_eq!(flow.edges.len(), 1);
        assert!(flow.summary.contains("2.00"));
        let matched = matching_outcome(&[(NodeIndex::new(0), NodeIndex::new(1))], "matching", 0.5);
        assert_eq!(matched.edges.len(), 1);
        assert!(matched.summary.contains("1 pairs"));
        let feedback = feedback_outcome(&[(NodeIndex::new(1), NodeIndex::new(0))], 0.5);
        assert_eq!(feedback.edges.len(), 1);
        assert!(feedback.summary.contains("feedback"));
        let paths = vec![vec![NodeIndex::new(0), NodeIndex::new(1)]];
        let simple = simple_paths_outcome(&paths, 0.5);
        assert_eq!(simple.nodes.len(), 2);
        assert_eq!(simple.edges.len(), 1);
        let empty = simple_paths_outcome(&[], 0.5);
        assert!(empty.nodes.is_empty() && empty.summary.contains("no path"));
    }

    #[test]
    fn connectivity_and_condensation_summarize_structure() {
        let groups = vec![vec![NodeIndex::new(0)], vec![NodeIndex::new(1)]];
        let connected = connectivity_outcome(&groups, true, false, false, true, 0.5);
        assert_eq!(connected.nodes.len(), 2);
        assert!(connected.summary.contains("2 groups"));
        let condensed =
            condensation_outcome(&groups, &[(NodeIndex::new(0), NodeIndex::new(1))], 0.5);
        assert_eq!(condensed.nodes.len(), 2);
        assert_eq!(condensed.edges.len(), 1);
        assert!(condensed.summary.contains("crossing"));
    }
}
