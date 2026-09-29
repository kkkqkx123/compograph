//! Frame-level render metrics with a fixed counting rule.
//!
//! Every plan stage reports through the same counters so baseline and
//! optimized paths stay comparable. Memory stays estimated from vector
//! lengths and index cells without process-level profiling.

use std::collections::VecDeque;
use std::time::Instant;

/// Counts of one paint plan build.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanCounts {
    pub nodes: usize,
    pub edges: usize,
    pub arrows: usize,
}

impl PlanCounts {
    pub fn total(&self) -> usize {
        self.nodes + self.edges + self.arrows
    }

    /// Rough memory estimate of the plan vectors in bytes.
    pub fn estimated_bytes(&self, index_cells: usize) -> usize {
        self.nodes * size_of::<u64>()
            + self.edges * size_of::<u64>() * 2
            + self.arrows * size_of::<u64>()
            + index_cells * size_of::<u64>()
    }
}

/// Timings of one frame's plan and index work, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameSample {
    pub counts: PlanCounts,
    pub plan_ms: f64,
    pub index_ms: f64,
    pub visible_nodes: usize,
}

/// Rolling summary of recent frames for the status bar.
#[derive(Debug, Default)]
pub struct FrameMetrics {
    samples: VecDeque<FrameSample>,
    capacity: usize,
}

impl FrameMetrics {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, sample: FrameSample) {
        if self.samples.len() >= self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn average_plan_ms(&self) -> f64 {
        average(self.samples.iter().map(|sample| sample.plan_ms))
    }

    pub fn average_index_ms(&self) -> f64 {
        average(self.samples.iter().map(|sample| sample.index_ms))
    }

    pub fn latest_visible(&self) -> usize {
        self.samples
            .back()
            .map(|sample| sample.visible_nodes)
            .unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }
}

fn average(values: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut count = 0usize;
    for value in values {
        sum += value;
        count += 1;
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

/// Measures the wall time of `build` in milliseconds.
pub fn measure_ms(build: impl FnOnce()) -> f64 {
    let started = Instant::now();
    build();
    started.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_total_and_memory_follow_vector_lengths() {
        let counts = PlanCounts {
            nodes: 10,
            edges: 20,
            arrows: 20,
        };
        assert_eq!(counts.total(), 50);
        let lean = counts.estimated_bytes(4);
        let dense = counts.estimated_bytes(40);
        assert!(dense > lean);
    }

    #[test]
    fn rolling_average_covers_recent_frames() {
        let mut metrics = FrameMetrics::new(3);
        for plan_ms in [1.0, 2.0, 3.0, 4.0] {
            metrics.push(FrameSample {
                counts: PlanCounts {
                    nodes: 1,
                    edges: 0,
                    arrows: 0,
                },
                plan_ms,
                index_ms: 0.5,
                visible_nodes: 1,
            });
        }
        assert_eq!(metrics.len(), 3);
        assert!((metrics.average_plan_ms() - 3.0).abs() < 1e-9);
        assert_eq!(metrics.latest_visible(), 1);
    }
}
