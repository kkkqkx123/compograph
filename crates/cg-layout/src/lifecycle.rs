//! Layout lifecycle record: start, stop and finish in order.
//!
//! The driver appends one entry when a run begins, one when an explicit stop
//! cancels it, and one when a run settles. Callers drain the record to
//! observe the sequence without subscribing to gpui events; the driver never
//! emits layout events through the application bus.

/// Lifecycle entry recorded by the layout driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutEvent {
    /// A run started for the named engine.
    Started { engine: &'static str },
    /// An explicit stop cancelled the run.
    Stopped,
    /// A run settled after `chunks` write-backs.
    Finished { chunks: u64 },
}
