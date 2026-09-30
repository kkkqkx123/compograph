//! File import and export for the graph structure with node positions.
//!
//! The JSON schema stores a node table alongside an edge table, so one file
//! carries both structure and layout. Import and export stay pure functions
//! over owned data. Each format lives in its own file under `io/`; this
//! module only declares the pieces and re-exports them so existing `io::`
//! paths keep working.
//!
//! DOT export reuses petgraph's own formatter. DOT import uses a small
//! hand-written parser covering the subset this application writes plus the
//! common shapes emitted by external tools, so a round trip needs no optional
//! petgraph feature.

pub mod document;
pub mod dot;
pub mod json;

pub use document::{EdgeEntry, GraphDocument, IoError, NodeEntry, remap_positions};
pub use dot::{export_dot, import_dot};
#[cfg(feature = "json-io")]
pub use json::{export_json, import_json};
