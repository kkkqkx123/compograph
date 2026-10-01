//! Grid layout placing nodes row by row into a bounding rectangle.
//!
//! The row/column derivation follows the reference grid algorithm: the counts
//! come from the box aspect ratio, explicit overrides win, and rounding drift
//! is repaired by shrinking the short side first or growing the long side
//! first. Per-node manual cells from the reference are absent; every node is
//! placed automatically in index order.

use std::collections::HashMap;

use cg_graph::{FixedNodes, GraphView, Positions};
use cg_types::Point2;

use crate::engine::{CommonOptions, LayoutEngine};

/// Tuning parameters of the grid arrangement.
#[derive(Clone, Debug)]
pub struct GridOptions {
    /// Center of the bounding rectangle in model units.
    pub center: Point2,
    /// Width of the bounding rectangle; non-positive collapses every node.
    pub width: f32,
    /// Height of the bounding rectangle; non-positive collapses every node.
    pub height: f32,
    /// Forced row count; derived from the aspect ratio when absent.
    pub rows: Option<usize>,
    /// Forced column count; derived from the aspect ratio when absent.
    pub cols: Option<usize>,
    /// When true, cells have zero pitch so nodes pack tightly from the corner.
    pub condense: bool,
    /// When true, cell pitch is at least the uniform node extent.
    pub avoid_overlap: bool,
    /// Multiplier applied to the cell pitch after overlap handling.
    pub spacing: f32,
    /// Uniform node extent used for overlap tests.
    pub node_size: f32,
}

impl Default for GridOptions {
    fn default() -> Self {
        Self {
            center: Point2::ZERO,
            width: 640.0,
            height: 480.0,
            rows: None,
            cols: None,
            condense: false,
            avoid_overlap: true,
            spacing: 1.0,
            node_size: 24.0,
        }
    }
}

/// Grid engine implementing the shared layout contract.
pub struct GridLayout {
    options: GridOptions,
    common: CommonOptions,
}

impl GridLayout {
    pub fn new(center: Point2, width: f32, height: f32) -> Self {
        Self {
            options: GridOptions {
                center,
                width,
                height,
                ..GridOptions::default()
            },
            common: CommonOptions::default(),
        }
    }

    pub fn with_options(options: GridOptions) -> Self {
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

    pub fn options(&self) -> &GridOptions {
        &self.options
    }
}

impl LayoutEngine for GridLayout {
    fn layout(&self, graph: &dyn GraphView, previous: &Positions, fixed: &FixedNodes) -> Positions {
        let mut ids = graph.node_ids();
        self.common.sort_ids(&mut ids, graph);
        let mut result: Positions = HashMap::new();
        if ids.is_empty() {
            return result;
        }
        if self.options.width <= 0.0 || self.options.height <= 0.0 {
            for node in ids {
                if fixed.contains(&node)
                    && let Some(held) = previous.get(&node)
                {
                    result.insert(node, *held);
                    continue;
                }
                result.insert(node, self.options.center);
            }
            return result;
        }
        let (rows, cols) = grid_shape(
            ids.len(),
            self.options.width,
            self.options.height,
            self.options.rows,
            self.options.cols,
        );
        let mut pitch_w = self.options.width / cols as f32;
        let mut pitch_h = self.options.height / rows as f32;
        if self.options.condense {
            pitch_w = 0.0;
            pitch_h = 0.0;
        }
        if self.options.avoid_overlap {
            pitch_w = pitch_w.max(self.options.node_size);
            pitch_h = pitch_h.max(self.options.node_size);
        }
        let spacing = (self.options.spacing * self.common.spacing()).max(f32::EPSILON);
        pitch_w *= spacing;
        pitch_h *= spacing;
        let corner = Point2::new(
            self.options.center.x - self.options.width / 2.0,
            self.options.center.y - self.options.height / 2.0,
        );
        let mut slot = 0usize;
        for node in ids {
            if fixed.contains(&node)
                && let Some(held) = previous.get(&node)
            {
                result.insert(node, *held);
                continue;
            }
            let row = slot / cols;
            let col = slot % cols;
            result.insert(
                node,
                Point2::new(
                    corner.x + col as f32 * pitch_w + pitch_w / 2.0,
                    corner.y + row as f32 * pitch_h + pitch_h / 2.0,
                ),
            );
            slot += 1;
        }
        result
    }

    fn name(&self) -> &'static str {
        "grid"
    }

    fn set_common(&mut self, common: CommonOptions) {
        self.common = common;
    }
}

/// Row and column counts for `cells` nodes inside a `width` by `height` box.
///
/// Explicit overrides win; otherwise the counts follow the box aspect ratio
/// and rounding drift is repaired by shrinking the short side first when the
/// grid is too large, or growing the long side first when it is too small.
fn grid_shape(
    cells: usize,
    width: f32,
    height: f32,
    rows: Option<usize>,
    cols: Option<usize>,
) -> (usize, usize) {
    if let (Some(rows), Some(cols)) = (rows, cols) {
        return (rows.max(1), cols.max(1));
    }
    if let Some(rows) = rows {
        let rows = rows.max(1);
        return (rows, cells.div_ceil(rows).max(1));
    }
    if let Some(cols) = cols {
        let cols = cols.max(1);
        return (cells.div_ceil(cols).max(1), cols);
    }
    aspect_shape(cells, width, height)
}

/// Derives row/column counts from the bounding box aspect ratio.
fn aspect_shape(cells: usize, width: f32, height: f32) -> (usize, usize) {
    let splits = (cells as f32 * height / width).sqrt();
    let mut rows = splits.round() as usize;
    let mut cols = (width / height * splits).round() as usize;
    rows = rows.max(1);
    cols = cols.max(1);
    if rows * cols > cells {
        let (short_is_rows, short) = if rows <= cols {
            (true, rows)
        } else {
            (false, cols)
        };
        let long = if short_is_rows { cols } else { rows };
        if short > 1 && (short - 1) * long >= cells {
            if short_is_rows {
                rows = short - 1;
            } else {
                cols = short - 1;
            }
        } else if long > 1 && (long - 1) * short >= cells {
            if short_is_rows {
                cols = long - 1;
            } else {
                rows = long - 1;
            }
        }
    } else {
        while rows * cols < cells {
            if rows <= cols {
                rows += 1;
            } else {
                cols += 1;
            }
        }
    }
    (rows, cols)
}

#[cfg(test)]
mod tests {
    use cg_graph::{FixedNodes, MockGraph, NodeIndex};

    use super::*;

    fn layout_of(cells: usize, width: f32, height: f32) -> Positions {
        let graph = MockGraph::isolated(cells);
        let engine = GridLayout::with_options(GridOptions {
            center: Point2::ZERO,
            width,
            height,
            avoid_overlap: false,
            ..GridOptions::default()
        });
        engine.layout(&graph, &Positions::new(), &FixedNodes::default())
    }

    #[test]
    fn empty_graph_places_nothing() {
        let graph = MockGraph::empty();
        let engine = GridLayout::new(Point2::ZERO, 200.0, 200.0);
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert!(positions.is_empty());
    }

    #[test]
    fn four_nodes_fill_a_two_by_two_grid() {
        let positions = layout_of(4, 200.0, 200.0);
        assert_eq!(positions.len(), 4);
        assert_eq!(
            positions.get(&NodeIndex::new(0)),
            Some(&Point2::new(-50.0, -50.0))
        );
        assert_eq!(
            positions.get(&NodeIndex::new(3)),
            Some(&Point2::new(50.0, 50.0))
        );
    }

    #[test]
    fn aspect_derivation_matches_the_reference_rule() {
        assert_eq!(grid_shape(6, 300.0, 200.0, None, None), (2, 3));
        assert_eq!(grid_shape(4, 200.0, 200.0, None, None), (2, 2));
        assert_eq!(grid_shape(5, 640.0, 480.0, Some(2), None), (2, 3));
        assert_eq!(grid_shape(5, 640.0, 480.0, None, Some(2)), (3, 2));
    }

    #[test]
    fn explicit_rows_derive_columns() {
        let graph = MockGraph::isolated(5);
        let engine = GridLayout::with_options(GridOptions {
            rows: Some(2),
            avoid_overlap: false,
            ..GridOptions::default()
        });
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_eq!(positions.len(), 5);
    }

    #[test]
    fn degenerate_boxes_collapse_to_the_center() {
        let graph = MockGraph::isolated(3);
        let engine = GridLayout::new(Point2::new(7.0, 9.0), 0.0, 100.0);
        let positions = engine.layout(&graph, &Positions::new(), &FixedNodes::default());
        assert!(
            positions
                .values()
                .all(|point| *point == Point2::new(7.0, 9.0))
        );
    }

    #[test]
    fn common_sort_reorders_slots_by_degree() {
        use crate::engine::{CommonOptions, SortKey};

        let graph = MockGraph::chain(3);
        let plain = GridLayout::with_options(GridOptions {
            center: Point2::ZERO,
            width: 300.0,
            height: 100.0,
            avoid_overlap: false,
            ..GridOptions::default()
        });
        let base = plain.layout(&graph, &Positions::new(), &FixedNodes::default());
        let sorted = GridLayout::with_options(GridOptions {
            center: Point2::ZERO,
            width: 300.0,
            height: 100.0,
            avoid_overlap: false,
            ..GridOptions::default()
        })
        .with_common(CommonOptions {
            sort: SortKey::DegreeDesc,
            ..CommonOptions::default()
        })
        .layout(&graph, &Positions::new(), &FixedNodes::default());
        assert_ne!(
            base.get(&NodeIndex::new(1)),
            sorted.get(&NodeIndex::new(1))
        );
        assert_eq!(sorted.get(&NodeIndex::new(1)), base.get(&NodeIndex::new(0)));
    }

    #[test]
    fn common_spacing_scales_the_pitch() {
        use crate::engine::CommonOptions;

        let graph = MockGraph::isolated(2);
        let plain = GridLayout::with_options(GridOptions {
            center: Point2::ZERO,
            width: 200.0,
            height: 100.0,
            avoid_overlap: false,
            ..GridOptions::default()
        })
        .layout(&graph, &Positions::new(), &FixedNodes::default());
        let wide = GridLayout::with_options(GridOptions {
            center: Point2::ZERO,
            width: 200.0,
            height: 100.0,
            avoid_overlap: false,
            ..GridOptions::default()
        })
        .with_common(CommonOptions {
            spacing_factor: 2.0,
            ..CommonOptions::default()
        })
        .layout(&graph, &Positions::new(), &FixedNodes::default());
        let plain_gap =
            (plain.get(&NodeIndex::new(1)).unwrap().x - plain.get(&NodeIndex::new(0)).unwrap().x)
                .abs();
        let wide_gap =
            (wide.get(&NodeIndex::new(1)).unwrap().x - wide.get(&NodeIndex::new(0)).unwrap().x)
                .abs();
        assert!((wide_gap - plain_gap * 2.0).abs() < 1e-3);
    }

    #[test]
    fn fixed_nodes_keep_their_previous_positions() {
        let graph = MockGraph::isolated(3);
        let mut previous = Positions::new();
        previous.insert(NodeIndex::new(1), Point2::new(500.0, 500.0));
        let mut fixed = FixedNodes::default();
        fixed.insert(NodeIndex::new(1));
        let engine = GridLayout::new(Point2::ZERO, 200.0, 200.0);
        let positions = engine.layout(&graph, &previous, &fixed);
        assert_eq!(
            positions.get(&NodeIndex::new(1)),
            Some(&Point2::new(500.0, 500.0))
        );
        assert_eq!(positions.len(), 3);
    }
}
