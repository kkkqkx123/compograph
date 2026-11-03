//! Label paint planning for the graph canvas.
//!
//! Labels are planned separately from node and edge geometry because they are
//! shaped as text rather than stroked as paths. Planning stays free of gpui
//! types so it can be exercised headlessly; [`crate::view::graph_view`] turns
//! the resulting plan into shaped lines at paint time.
//!
//! Labels anchor just below the node body and are centered horizontally by the
//! renderer. Nodes without text are skipped, and the coarsest detail level
//! drops labels entirely to keep dense views readable.
//!
//! Concrete helpers live in flat sibling modules; this module only re-exports
//! them so existing `text::` paths keep working.

pub use crate::label_envelope::{
    EDGE_LABEL_GAP, LABEL_GAP, edge_label_anchor, edge_label_angle, estimate_label_block,
    label_envelope, label_envelope_styled, node_label_origin,
};
pub use crate::label_plan::{
    PaintedEdgeLabel, PaintedLabel, draws_labels, paint_edge_labels_for,
    paint_edge_labels_for_with_style, paint_labels_for, paint_labels_for_with_style,
};
pub use crate::label_style::{
    DEFAULT_EDGE_LABEL_COLOR, DEFAULT_EDGE_LABEL_SIZE, DEFAULT_LABEL_COLOR, DEFAULT_LABEL_SIZE,
    LABEL_BACKGROUND_FILL, LABEL_BACKGROUND_PAD, LABEL_CORNER_RADIUS, LabelAlign, LabelBackground,
    LabelStyle,
};
pub use crate::label_wrap::{
    LABEL_LINE_HEIGHT_SCALE, MAX_LABEL_CHARS_PER_LINE, line_height, split_label_lines,
};
