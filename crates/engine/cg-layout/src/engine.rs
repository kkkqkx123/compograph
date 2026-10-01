//! The layout engine contract: turning graph structure into node positions.

use std::cmp::Ordering;

use cg_graph::{FixedNodes, GraphView, NodeIndex, Positions};
use cg_types::Point2;

use crate::force::ForceOptions;

/// Ordering applied to node placement in order-driven layouts.
///
/// Score-driven layouts keep their scoring as the primary order; the key only
/// documents that intent. Force-directed layouts ignore ordering entirely and
/// let the simulation settle positions instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    /// Keep the caller or engine order untouched.
    #[default]
    Stable,
    /// Ascending stable index order.
    IndexAsc,
    /// Ascending visible degree, ties broken by index.
    DegreeAsc,
    /// Descending visible degree, ties broken by index.
    DegreeDesc,
}

/// Options shared by every layout engine.
///
/// Defaults preserve the historical behavior: stable order, a view-fit
/// request on settled runs and a neutral spacing factor of one. Fitting stays
/// with the caller: the flag only records a request the viewport assembly may
/// consume or ignore.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommonOptions {
    pub sort: SortKey,
    pub fit_view: bool,
    pub spacing_factor: f32,
}

impl Default for CommonOptions {
    fn default() -> Self {
        Self {
            sort: SortKey::Stable,
            fit_view: true,
            spacing_factor: 1.0,
        }
    }
}

impl CommonOptions {
    /// Spacing multiplier guarded to positive finite values.
    ///
    /// Non-positive or non-finite factors fall back to one, so a bad option
    /// can never collapse or invert a layout.
    pub fn spacing(&self) -> f32 {
        if self.spacing_factor.is_finite() && self.spacing_factor > 0.0 {
            self.spacing_factor
        } else {
            1.0
        }
    }

    /// Base length scaled by the spacing factor.
    pub fn scaled(&self, base: f32) -> f32 {
        base * self.spacing()
    }

    /// Orders `ids` for placement without touching engine scoring.
    pub fn sort_ids(&self, ids: &mut [NodeIndex], graph: &dyn GraphView) {
        match self.sort {
            SortKey::Stable => {}
            SortKey::IndexAsc => {
                ids.sort_unstable_by_key(|node| node.index());
            }
            SortKey::DegreeAsc => {
                ids.sort_by(|left, right| {
                    graph
                        .degree(*left)
                        .cmp(&graph.degree(*right))
                        .then_with(|| left.index().cmp(&right.index()))
                });
            }
            SortKey::DegreeDesc => {
                ids.sort_by(|left, right| {
                    graph
                        .degree(*right)
                        .cmp(&graph.degree(*left))
                        .then_with(|| left.index().cmp(&right.index()))
                });
            }
        }
    }

    /// Scales full-run positions about their centroid by the spacing factor.
    ///
    /// Full runs recompute every coordinate, so uniform scaling stays
    /// consistent there. Incremental placements keep existing coordinates and
    /// never call this, so untouched nodes cannot drift.
    pub fn scale_full_run(&self, positions: &mut Positions) {
        let factor = self.spacing();
        if factor == 1.0 || positions.is_empty() {
            return;
        }
        let mut sum = Point2::ZERO;
        let mut count = 0u32;
        for point in positions.values() {
            sum = Point2::new(sum.x + point.x, sum.y + point.y);
            count += 1;
        }
        if count == 0 {
            return;
        }
        let center = Point2::new(sum.x / count as f32, sum.y / count as f32);
        for point in positions.values_mut() {
            *point = Point2::new(
                center.x + (point.x - center.x) * factor,
                center.y + (point.y - center.y) * factor,
            );
        }
    }
}

/// Produces model-space coordinates for every node of the graph.
///
/// Implementations may use `previous` as a starting point so that incremental
/// edits keep the rest of the graph stable instead of reshuffling everything.
/// Nodes in `fixed` keep their previous coordinates; engines still let them
/// attract their neighbours so the surroundings settle around them.
///
/// Dynamic dispatch is used because layouts are chosen at runtime from user
/// input, and the set of engines is open to future additions.
pub trait LayoutEngine {
    fn layout(&self, graph: &dyn GraphView, previous: &Positions, fixed: &FixedNodes) -> Positions;

    /// Identifier used by the layout registry and user-facing pickers.
    fn name(&self) -> &'static str;

    /// Force-directed options when this engine refines in the background.
    ///
    /// Only force-directed engines override this; the driver runs any other
    /// engine synchronously and ignores the background path.
    fn force_options(&self) -> Option<ForceOptions> {
        None
    }

    /// Receives shared options; engines without shared inputs keep the default.
    ///
    /// Order-driven layouts apply the sort key and spacing factor, while
    /// score-driven and force-directed layouts only honor the subset their
    /// contract supports.
    fn set_common(&mut self, _common: CommonOptions) {}

    /// Compares two nodes for placement order; index order by default.
    ///
    /// Engines call this when sibling nodes need a deterministic tiebreak.
    /// The default keeps plain index order so engines without shared inputs
    /// behave exactly as before.
    fn compare_nodes(&self, left: NodeIndex, right: NodeIndex) -> Ordering {
        left.index().cmp(&right.index())
    }
}

#[cfg(test)]
mod tests {
    use cg_graph::{MockGraph, Positions};

    use super::*;

    #[test]
    fn spacing_guards_reject_bad_factors() {
        let unit = CommonOptions {
            spacing_factor: 2.0,
            ..CommonOptions::default()
        };
        assert_eq!(unit.spacing(), 2.0);
        assert_eq!(unit.scaled(10.0), 20.0);
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let guarded = CommonOptions {
                spacing_factor: bad,
                ..CommonOptions::default()
            };
            assert_eq!(guarded.spacing(), 1.0);
        }
    }

    #[test]
    fn sort_keys_order_by_index_and_degree() {
        let graph = MockGraph::chain(3);
        let mut ids = graph.node_ids();
        CommonOptions::default().sort_ids(&mut ids, &graph);
        assert_eq!(ids.len(), 3);
        let mut by_degree = graph.node_ids();
        CommonOptions {
            sort: SortKey::DegreeDesc,
            ..CommonOptions::default()
        }
        .sort_ids(&mut by_degree, &graph);
        assert_eq!(by_degree[0].index(), 1);
        let mut ascending = graph.node_ids();
        CommonOptions {
            sort: SortKey::DegreeAsc,
            ..CommonOptions::default()
        }
        .sort_ids(&mut ascending, &graph);
        assert_eq!(ascending.last().map(|node| node.index()), Some(1));
    }

    #[test]
    fn full_run_scaling_centers_on_the_centroid() {
        let graph = MockGraph::isolated(2);
        let mut positions: Positions = graph
            .node_ids()
            .into_iter()
            .enumerate()
            .map(|(ordinal, node)| {
                (
                    node,
                    Point2::new(ordinal as f32 * 10.0, ordinal as f32 * 10.0),
                )
            })
            .collect();
        CommonOptions {
            spacing_factor: 2.0,
            ..CommonOptions::default()
        }
        .scale_full_run(&mut positions);
        let mut points: Vec<Point2> = positions.values().copied().collect();
        points.sort_by(|left, right| {
            left.x
                .partial_cmp(&right.x)
                .unwrap_or(Ordering::Equal)
        });
        assert_eq!(
            points,
            vec![Point2::new(-5.0, -5.0), Point2::new(15.0, 15.0)]
        );
        let mut unit = positions.clone();
        CommonOptions::default().scale_full_run(&mut unit);
        assert_eq!(unit, positions);
    }
}
