//! Uniform-grid index accelerating pointer hit tests.
//!
//! The index only narrows candidates: callers still run exact geometry tests
//! on the returned nodes. It is rebuilt from scratch whenever positions change
//! instead of tracking incremental edits, which keeps the write path trivial.

use std::collections::HashMap;

use cg_graph::{NodeIndex, Positions};
use cg_types::{Point2, Rect};

/// Maps model-space positions to grid cells of fixed edge length.
pub struct SpatialIndex {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<NodeIndex>>,
}

impl SpatialIndex {
    /// Builds an empty index; `cell` is the grid edge length in model units.
    pub fn new(cell: f32) -> Self {
        Self {
            cell: cell.max(1.0),
            cells: HashMap::new(),
        }
    }

    /// Grid edge length in model units.
    pub fn cell(&self) -> f32 {
        self.cell
    }

    /// Rebuilds the index from the current positions.
    pub fn rebuild(&mut self, positions: &Positions) {
        self.cells.clear();
        let mut entries: Vec<(&NodeIndex, &Point2)> = positions.iter().collect();
        entries.sort_unstable_by_key(|(node, _)| node.index());
        for (node, point) in entries {
            self.cells
                .entry(cell_of(*point, self.cell))
                .or_default()
                .push(*node);
        }
    }

    /// Nodes whose cells overlap the disc around `point`.
    ///
    /// Results arrive sorted by node index for reproducible hit testing.
    pub fn query_point(&self, point: Point2, radius: f32) -> Vec<NodeIndex> {
        let min = cell_of(Point2::new(point.x - radius, point.y - radius), self.cell);
        let max = cell_of(Point2::new(point.x + radius, point.y + radius), self.cell);
        let mut found = Vec::new();
        for gx in min.0..=max.0 {
            for gy in min.1..=max.1 {
                if let Some(nodes) = self.cells.get(&(gx, gy)) {
                    found.extend(nodes.iter().copied());
                }
            }
        }
        found.sort_unstable_by_key(|node| node.index());
        found.dedup_by_key(|node| node.index());
        found
    }

    /// Nodes whose cells overlap `rect`.
    pub fn query_rect(&self, rect: Rect) -> Vec<NodeIndex> {
        let far = Point2::new(rect.origin.x + rect.size.x, rect.origin.y + rect.size.y);
        let min = cell_of(rect.origin, self.cell);
        let max = cell_of(far, self.cell);
        let mut found = Vec::new();
        for gx in min.0..=max.0 {
            for gy in min.1..=max.1 {
                if let Some(nodes) = self.cells.get(&(gx, gy)) {
                    found.extend(nodes.iter().copied());
                }
            }
        }
        found.sort_unstable_by_key(|node| node.index());
        found.dedup_by_key(|node| node.index());
        found
    }
}

fn cell_of(point: Point2, cell: f32) -> (i32, i32) {
    (
        (point.x / cell).floor() as i32,
        (point.y / cell).floor() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions_at(points: &[(u32, f32, f32)]) -> Positions {
        let mut positions = Positions::new();
        for (index, x, y) in points {
            positions.insert(NodeIndex::new(*index as usize), Point2::new(*x, *y));
        }
        positions
    }

    #[test]
    fn empty_index_answers_nothing() {
        let index = SpatialIndex::new(32.0);
        assert!(index.query_point(Point2::ZERO, 10.0).is_empty());
        assert!(
            index
                .query_rect(Rect::new(Point2::ZERO, cg_types::Vec2::new(10.0, 10.0)))
                .is_empty()
        );
    }

    #[test]
    fn point_query_spans_cell_borders() {
        let mut index = SpatialIndex::new(10.0);
        index.rebuild(&positions_at(&[(0, 9.0, 5.0), (1, 40.0, 40.0)]));
        let found = index.query_point(Point2::new(11.0, 5.0), 3.0);
        assert_eq!(found, vec![NodeIndex::new(0)]);
    }

    #[test]
    fn rect_query_covers_overlapping_cells() {
        let mut index = SpatialIndex::new(10.0);
        index.rebuild(&positions_at(&[
            (0, 5.0, 5.0),
            (1, 15.0, 5.0),
            (2, 50.0, 50.0),
        ]));
        let rect = Rect::new(Point2::new(0.0, 0.0), cg_types::Vec2::new(20.0, 10.0));
        assert_eq!(
            index.query_rect(rect),
            vec![NodeIndex::new(0), NodeIndex::new(1)]
        );
    }

    #[test]
    fn rebuild_replaces_stale_entries() {
        let mut index = SpatialIndex::new(10.0);
        index.rebuild(&positions_at(&[(0, 5.0, 5.0)]));
        index.rebuild(&positions_at(&[(0, 100.0, 100.0)]));
        assert!(index.query_point(Point2::new(5.0, 5.0), 3.0).is_empty());
        assert_eq!(
            index.query_point(Point2::new(100.0, 100.0), 3.0),
            vec![NodeIndex::new(0)]
        );
    }
}
