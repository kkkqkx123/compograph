//! Shared ring geometry for concentric and radial layouts.
//!
//! Both engines partition scored nodes into rings, then grow each radius
//! until its members stop overlapping. The scoring policy stays with each
//! engine; only the mechanical ring math lives here.

use std::collections::HashMap;

use cg_graph::{GraphView, NodeIndex};

/// Score per node from an optional caller table with a degree fallback.
///
/// A missing table means every node scores by degree; entries present in the
/// table win and the rest still fall back to degree.
pub(crate) fn scores_with_table(
    graph: &dyn GraphView,
    ids: &[NodeIndex],
    table: Option<&HashMap<NodeIndex, f32>>,
) -> HashMap<NodeIndex, f32> {
    let mut values = HashMap::new();
    for node in ids {
        let score = match table {
            None => graph.degree(*node) as f32,
            Some(entries) => entries
                .get(node)
                .copied()
                .unwrap_or_else(|| graph.degree(*node) as f32),
        };
        values.insert(*node, score);
    }
    values
}

/// Width of one ring: explicit value wins, else a quarter of the largest.
pub(crate) fn level_width(
    ids: &[NodeIndex],
    values: &HashMap<NodeIndex, f32>,
    explicit: Option<f32>,
) -> f32 {
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
pub(crate) fn to_levels(
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

/// Angular step between adjacent members of a ring.
pub(crate) fn ring_step(sweep: Option<f32>, members: usize) -> f32 {
    let sweep = sweep
        .unwrap_or(2.0 * std::f32::consts::PI - 2.0 * std::f32::consts::PI / members.max(1) as f32);
    sweep / members.saturating_sub(1).max(1) as f32
}

/// Radius per ring, accumulating the minimum gap outward.
pub(crate) fn ring_radii(
    levels: &[Vec<NodeIndex>],
    sweep: Option<f32>,
    gap: f32,
    avoid_overlap: bool,
    equidistant: bool,
) -> Vec<f32> {
    let mut radii = Vec::with_capacity(levels.len());
    let mut ring = 0.0f32;
    for level in levels {
        let step = ring_step(sweep, level.len());
        if level.len() > 1 && avoid_overlap {
            let chord = ((step.cos() - 1.0).powi(2) + step.sin().powi(2)).sqrt();
            if chord > f32::EPSILON {
                ring = ring.max(gap / chord);
            }
        }
        radii.push(ring);
        ring += gap;
    }
    if equidistant && radii.len() > 1 {
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
    use super::*;

    #[test]
    fn explicit_width_wins_over_largest_value() {
        let ids = vec![cg_graph::NodeIndex::new(0), cg_graph::NodeIndex::new(1)];
        let mut values = HashMap::new();
        values.insert(ids[0], 100.0);
        values.insert(ids[1], 0.0);
        assert_eq!(level_width(&ids, &values, Some(7.0)), 7.0);
        assert_eq!(level_width(&ids, &values, None), 25.0);
    }

    #[test]
    fn single_member_step_collapses_to_zero() {
        let step = ring_step(None, 1);
        assert!(step.abs() < 1e-6);
    }

    #[test]
    fn equidistant_radii_share_one_gap() {
        let levels = vec![
            vec![cg_graph::NodeIndex::new(0)],
            vec![cg_graph::NodeIndex::new(1), cg_graph::NodeIndex::new(2)],
            vec![cg_graph::NodeIndex::new(3)],
        ];
        let radii = ring_radii(&levels, None, 34.0, false, true);
        assert_eq!(radii.len(), 3);
        assert!((radii[1] - radii[0] - (radii[2] - radii[1])).abs() < 1e-3);
    }
}
