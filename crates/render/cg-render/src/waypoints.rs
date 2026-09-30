//! User waypoint storage for custom edge routes.
//!
//! Waypoints live in world coordinates keyed by directed endpoints plus the
//! occurrence among parallel edges of one direction. Planning converts them
//! to screen pixels through the camera; cleaning reuses the shared polyline
//! builder so drawing and hit testing share one sampling.

use std::collections::HashMap;

use cg_graph::NodeIndex;
use cg_types::Point2;

/// Ordered user waypoints in world coordinates.
#[derive(Clone, Debug, Default)]
pub struct WaypointStore {
    entries: HashMap<(usize, usize, usize), Vec<Point2>>,
}

impl WaypointStore {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Stores the waypoint sequence for one directed occurrence.
    ///
    /// Empty sequences clear the entry. Occurrence counts parallel edges of
    /// one direction from zero in pair order.
    pub fn set(
        &mut self,
        source: NodeIndex,
        target: NodeIndex,
        occurrence: usize,
        points: Vec<Point2>,
    ) {
        let key = (source.index(), target.index(), occurrence);
        if points.is_empty() {
            self.entries.remove(&key);
        } else {
            self.entries.insert(key, points);
        }
    }

    /// Stores the sequence for the first edge of one directed pair.
    pub fn set_single(&mut self, source: NodeIndex, target: NodeIndex, points: Vec<Point2>) {
        self.set(source, target, 0, points);
    }

    /// Waypoints for one directed occurrence, if any.
    pub fn get(&self, source: NodeIndex, target: NodeIndex, occurrence: usize) -> Vec<Point2> {
        self.entries
            .get(&(source.index(), target.index(), occurrence))
            .cloned()
            .unwrap_or_default()
    }

    /// Clears one directed occurrence.
    pub fn clear(&mut self, source: NodeIndex, target: NodeIndex, occurrence: usize) {
        self.entries
            .remove(&(source.index(), target.index(), occurrence));
    }

    pub fn clear_all(&mut self) {
        self.entries.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directed_occurrences_stay_independent() {
        let mut store = WaypointStore::new();
        let a = NodeIndex::new(0);
        let b = NodeIndex::new(1);
        assert!(store.is_empty());
        store.set_single(a, b, vec![Point2::new(1.0, 2.0)]);
        store.set(a, b, 1, vec![Point2::new(3.0, 4.0)]);
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(a, b, 0), vec![Point2::new(1.0, 2.0)]);
        assert_eq!(store.get(a, b, 1), vec![Point2::new(3.0, 4.0)]);
        assert!(store.get(b, a, 0).is_empty());
        store.clear(a, b, 0);
        assert_eq!(store.len(), 1);
        store.clear_all();
        assert!(store.is_empty());
    }

    #[test]
    fn empty_sequences_clear_entries() {
        let mut store = WaypointStore::new();
        let a = NodeIndex::new(0);
        let b = NodeIndex::new(1);
        store.set_single(a, b, vec![Point2::new(1.0, 1.0)]);
        store.set_single(a, b, Vec::new());
        assert!(store.is_empty());
    }
}
