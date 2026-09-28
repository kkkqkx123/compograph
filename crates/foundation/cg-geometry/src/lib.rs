//! Framework-agnostic geometry for hit testing and edge shapes.

pub mod curves;
pub mod picking;

pub use curves::quadratic_bezier;
pub use picking::distance_to_segment;
