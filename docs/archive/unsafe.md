# Recorded unsafe code

The project keeps `unsafe` out of the product code. The cases below are the
only ones allowed to use it, and each one records why the safe alternative
does not work.

## Counting allocator in the layout baseline

- File: `crates/engine/cg-layout/benches/layout_cost.rs`
- What: a `GlobalAlloc` implementation that counts allocations and bytes.
- Why: a benchmark is the only place where the allocations of the standard
  library are observable. Every other way of counting allocations either
  needs a profiler dependency or misses the allocations the library makes on
  the caller's behalf.
- Safety: the implementation forwards to `System` unchanged and only reads
  the layout size, which the caller of the allocator already guarantees.
  The counters are plain relaxed atomics, so a lost update under contention
  only affects the reported count.
