//! Layered element styling with per-element bypass overrides.
//!
//! The main styles live in [`StyleSheet`] as plain defaults. [`StyleMapper`]
//! derives patches from typed predicates over labels, degrees, or algorithm
//! result sets, without any string selector language. [`BypassStore`] holds
//! transient per-element overrides for selection, hover, and algorithm
//! highlights, so highlights never mutate the main styles.
//!
//! Concrete vocabularies live in flat sibling modules; this module only
//! re-exports them so existing `style::` paths keep working.

pub use crate::appearance::{
    DEFAULT_EDGE_TINT, DEFAULT_NODE_STROKE, EdgeCurve, EdgeStyle, EdgeStylePatch,
    HIGHLIGHT_EDGE_TINT, NodeStyle, NodeStylePatch, StyleSheet,
};
pub use crate::bypass::BypassStore;
pub use crate::edge_rules::{EdgeMapper, EdgeNumberMap, EdgePredicate, EdgeRule};
pub use crate::fill::{DEFAULT_NODE_FILL, HOVER_NODE_FILL, NodeFill, SELECTED_NODE_FILL};
pub use crate::mapping::{EdgeColorMap, NodeColorMap, color_map, linear_map};
pub use crate::node_rules::{NodeDataTables, NodeNumberMap, NodePredicate, NodeRule, StyleMapper};
pub use crate::palette::{
    MAX_RANK_SCALE, MIN_RANK_SCALE, SCC_GROUP_FILLS, SCC_OVERFLOW_FILL, scale_for_rank, scc_fill,
};
pub use crate::text::{LabelAlign, LabelStyle};

pub(crate) use crate::fill::lerp_rgb;
