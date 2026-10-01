//! Compound post-processing applied after each layout pass.
//!
//! Engines keep their flat math untouched; the driver separates top-level
//! groups and snaps containers to their descendant bounds here.

use std::collections::HashMap;

use cg_graph::{GraphStore, NodeIndex, Positions};
use cg_types::{Point2, Rect};

/// Gap kept between separated top-level groups, in model units.
pub const GROUP_GAP: f32 = 80.0;

/// Bounds of one node set from live positions.
fn group_bounds(members: &[NodeIndex], positions: &Positions, half_extent: f32) -> Option<Rect> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut placed = false;
    for node in members {
        if let Some(point) = positions.get(node).copied() {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
            placed = true;
        }
    }
    if !placed {
        return None;
    }
    Some(Rect::from_corners(
        Point2::new(min_x - half_extent, min_y - half_extent),
        Point2::new(max_x + half_extent, max_y + half_extent),
    ))
}

/// Shifts top-level groups apart so their bounds no longer overlap.
pub fn separate_groups(store: &GraphStore, positions: &mut Positions, half_extent: f32) {
    let groups = store.compound_groups();
    if groups.len() < 2 {
        return;
    }
    let mut bounds: Vec<Option<Rect>> = groups
        .iter()
        .map(|members| group_bounds(members, positions, half_extent))
        .collect();
    let mut cursor = 0.0f32;
    for (index, members) in groups.iter().enumerate() {
        let Some(rect) = bounds[index] else {
            continue;
        };
        let width = rect.size.x.max(half_extent * 2.0);
        let shift = cursor - rect.origin.x;
        if shift.abs() > 1e-6 {
            for node in members {
                if let Some(point) = positions.get_mut(node) {
                    point.x += shift;
                }
            }
            let moved = Rect::new(Point2::new(rect.origin.x + shift, rect.origin.y), rect.size);
            bounds[index] = Some(moved);
        }
        cursor += width + GROUP_GAP;
    }
}

/// Snaps every visible container to its descendant bounds center.
pub fn snap_containers(store: &GraphStore, positions: &mut Positions, half_extent: f32) {
    let mut containers: Vec<NodeIndex> = store
        .visible_node_ids()
        .into_iter()
        .filter(|node| store.is_container(*node))
        .collect();
    containers.sort_by_key(|node| std::cmp::Reverse(store.ancestors_of(*node).len()));
    for container in containers {
        let mut leaves: Vec<Point2> = Vec::new();
        for member in store.descendants_of(container) {
            if !store.is_visible(member) || store.is_container(member) {
                continue;
            }
            if let Some(point) = positions.get(&member).copied() {
                leaves.push(point);
            }
        }
        if leaves.is_empty() {
            continue;
        }
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for point in leaves {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }
        let _ = half_extent;
        positions.insert(
            container,
            Point2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0),
        );
    }
}

/// Full compound post pass: separate groups, then snap containers.
pub fn apply_compound_postprocess(store: &GraphStore, positions: &mut Positions, half_extent: f32) {
    if !store.has_compound() {
        return;
    }
    separate_groups(store, positions, half_extent);
    snap_containers(store, positions, half_extent);
}

/// Centers of top-level groups, for tests and status views.
pub fn group_centers(store: &GraphStore, positions: &Positions) -> HashMap<NodeIndex, Point2> {
    let mut centers = HashMap::new();
    for group in store.compound_groups() {
        let root = group
            .first()
            .copied()
            .and_then(|node| store.top_ancestor(node));
        let Some(root) = root else {
            continue;
        };
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut count = 0usize;
        for node in &group {
            if let Some(point) = positions.get(node) {
                x += point.x;
                y += point.y;
                count += 1;
            }
        }
        if count > 0 {
            centers.insert(root, Point2::new(x / count as f32, y / count as f32));
        }
    }
    centers
}

/// Snapshot of the compound hierarchy for background write-backs.
///
/// Background tasks cannot read the live store, so the driver captures the
/// grouping once and reuses it for every chunk of the same generation.
#[derive(Clone, Debug, Default)]
pub struct CompoundSnapshot {
    /// Top-level groups in deterministic order.
    pub groups: Vec<Vec<NodeIndex>>,
    /// Containers with their leaf descendants, deepest first.
    pub containers: Vec<(NodeIndex, Vec<NodeIndex>)>,
}

impl CompoundSnapshot {
    /// Captures grouping from the live store; none when flat.
    pub fn capture(store: &GraphStore) -> Option<Self> {
        if !store.has_compound() {
            return None;
        }
        let groups = store.compound_groups();
        let mut containers: Vec<(NodeIndex, Vec<NodeIndex>)> = Vec::new();
        for node in store.visible_node_ids() {
            if !store.is_container(node) {
                continue;
            }
            let leaves: Vec<NodeIndex> = store
                .descendants_of(node)
                .into_iter()
                .filter(|member| store.is_visible(*member) && !store.is_container(*member))
                .collect();
            containers.push((node, leaves));
        }
        containers.sort_by_key(|(node, _)| std::cmp::Reverse(store.ancestors_of(*node).len()));
        Some(Self { groups, containers })
    }

    /// Applies separation and container snapping without the live store.
    pub fn polish(&self, positions: &mut Positions, half_extent: f32) {
        separate_groups_with(&self.groups, positions, half_extent);
        snap_containers_with(&self.containers, positions);
    }
}

/// Shifts the given groups apart without reading the store.
pub fn separate_groups_with(
    groups: &[Vec<NodeIndex>],
    positions: &mut Positions,
    half_extent: f32,
) {
    if groups.len() < 2 {
        return;
    }
    let mut bounds: Vec<Option<Rect>> = groups
        .iter()
        .map(|members| group_bounds(members, positions, half_extent))
        .collect();
    let mut cursor = 0.0f32;
    for (index, members) in groups.iter().enumerate() {
        let Some(rect) = bounds[index] else {
            continue;
        };
        let width = rect.size.x.max(half_extent * 2.0);
        let shift = cursor - rect.origin.x;
        if shift.abs() > 1e-6 {
            for node in members {
                if let Some(point) = positions.get_mut(node) {
                    point.x += shift;
                }
            }
            bounds[index] = Some(Rect::new(
                Point2::new(rect.origin.x + shift, rect.origin.y),
                rect.size,
            ));
        }
        cursor += width + GROUP_GAP;
    }
}

/// Snaps containers to their captured leaf descendants.
pub fn snap_containers_with(containers: &[(NodeIndex, Vec<NodeIndex>)], positions: &mut Positions) {
    for (container, leaves) in containers {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        let mut placed = false;
        for leaf in leaves {
            if let Some(point) = positions.get(leaf).copied() {
                min_x = min_x.min(point.x);
                min_y = min_y.min(point.y);
                max_x = max_x.max(point.x);
                max_y = max_y.max(point.y);
                placed = true;
            }
        }
        if placed {
            positions.insert(
                *container,
                Point2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_graphs_skip_postprocessing() {
        let store = GraphStore::new();
        let mut positions = Positions::new();
        apply_compound_postprocess(&store, &mut positions, 12.0);
        assert!(positions.is_empty());
        assert!(group_centers(&store, &positions).is_empty());
    }

    #[test]
    fn separated_groups_no_longer_overlap() {
        let first = vec![NodeIndex::new(0), NodeIndex::new(1)];
        let second = vec![NodeIndex::new(2), NodeIndex::new(3)];
        let groups = vec![first, second];
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(10.0, 0.0));
        positions.insert(NodeIndex::new(2), Point2::new(5.0, 0.0));
        positions.insert(NodeIndex::new(3), Point2::new(15.0, 0.0));
        separate_groups_with(&groups, &mut positions, 12.0);
        let left = group_bounds(&groups[0], &positions, 12.0).expect("left bounds exist");
        let right = group_bounds(&groups[1], &positions, 12.0).expect("right bounds exist");
        assert!(left.origin.x + left.size.x + GROUP_GAP - 1.0 <= right.origin.x);
        assert!(!left.intersects(right));
    }

    #[test]
    fn separated_groups_stay_disjoint_with_vertical_overlap() {
        let first = vec![NodeIndex::new(0), NodeIndex::new(1)];
        let second = vec![NodeIndex::new(2), NodeIndex::new(3)];
        let groups = vec![first, second];
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(0.0, 200.0));
        positions.insert(NodeIndex::new(2), Point2::new(0.0, 100.0));
        positions.insert(NodeIndex::new(3), Point2::new(0.0, 110.0));
        separate_groups_with(&groups, &mut positions, 12.0);
        let left = group_bounds(&groups[0], &positions, 12.0).expect("left bounds exist");
        let right = group_bounds(&groups[1], &positions, 12.0).expect("right bounds exist");
        assert!(!left.intersects(right));
    }

    #[test]
    fn containers_snap_to_their_leaf_centers() {
        let container = NodeIndex::new(9);
        let leaves = vec![NodeIndex::new(10), NodeIndex::new(11)];
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(10), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(11), Point2::new(20.0, 10.0));
        positions.insert(container, Point2::new(-100.0, -100.0));
        snap_containers_with(&[(container, leaves)], &mut positions);
        assert_eq!(positions.get(&container), Some(&Point2::new(10.0, 5.0)));
        let mut empty = Positions::new();
        empty.insert(container, Point2::new(7.0, 7.0));
        snap_containers_with(&[(container, Vec::new())], &mut empty);
        assert_eq!(empty.get(&container), Some(&Point2::new(7.0, 7.0)));
    }

    #[test]
    fn container_center_matches_visible_descendant_bounds() {
        let container = NodeIndex::new(20);
        let leaves = vec![NodeIndex::new(21), NodeIndex::new(22), NodeIndex::new(23)];
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(21), Point2::new(-30.0, 10.0));
        positions.insert(NodeIndex::new(22), Point2::new(10.0, -20.0));
        positions.insert(NodeIndex::new(23), Point2::new(50.0, 40.0));
        positions.insert(container, Point2::new(0.0, 0.0));
        snap_containers_with(&[(container, leaves)], &mut positions);
        let placed = positions
            .get(&container)
            .copied()
            .expect("container placed");
        assert!((placed.x - 10.0).abs() < 1e-4);
        assert!((placed.y - 10.0).abs() < 1e-4);
    }

    #[test]
    fn folding_keeps_position_and_unfolding_restores_center() {
        let container = NodeIndex::new(30);
        let leaves = vec![NodeIndex::new(31), NodeIndex::new(32)];
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(31), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(32), Point2::new(20.0, 10.0));
        positions.insert(container, Point2::new(-100.0, -100.0));
        snap_containers_with(&[(container, leaves.clone())], &mut positions);
        let expanded = positions.get(&container).copied().expect("snapped");
        assert_eq!(expanded, Point2::new(10.0, 5.0));
        snap_containers_with(&[(container, Vec::new())], &mut positions);
        assert_eq!(positions.get(&container), Some(&Point2::new(10.0, 5.0)));
        snap_containers_with(&[(container, leaves)], &mut positions);
        assert_eq!(positions.get(&container), Some(&Point2::new(10.0, 5.0)));
    }
}
