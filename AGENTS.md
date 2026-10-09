# compograph

## Project Overview

compograph is a pure Rust desktop graph visualization application: the graph model and algorithm core are based on petgraph, the UI foundation is gpui from the `gpui-pre` snapshot of Zed's GPU-accelerated GUI framework, and layout, rendering, and interaction are developed in-house by this project.

**No-backward-compatible**: The project is in the development phase; backward compatibility does not need to be considered, only a reasonable architecture needs to be maintained.

**Document Reference Rule**: Code comments must not contain any document structure identifiers (such as P1, P2-3, §4.1, phase3, G2, etc.). Comments should only describe code intent and must not reference external document locations.

**Language**: Code, comments, logs, and error messages must all be in English; documentation is in Chinese. **No Chinese text may appear in code files.**

## Code Architecture

The top level contains `crates/`, `docs/`, `Cargo.toml`, `rust-toolchain.toml`, and `AGENTS.md`.

`crates/` holds the self-built layers; gpui itself is a registry dependency, not source in the tree:

- **gpui and its platform backend**: Consumed as the published `gpui-pre` / `gpui-pre-platform` snapshot crates from crates.io (the same snapshot line gpui-kit pins, currently `=0.3.8`), declared once in `[workspace.dependencies]`. There is no vendored gpui source and no submodule mount point.
- **foundation layer**: `cg-types` (geometry primitives, leaf crate), `cg-geometry` (hit testing and curve geometry, framework-agnostic), `cg-graph` (`GraphStore` graph storage + petgraph `StableGraph` adaptation + change event broadcasting and subscription filtering)
- **engine layer**: `cg-layout` (`LayoutEngine` trait, layout registry, concrete layout implementations, `LayoutDriver` change-driven position maintenance)
- **render layer**: `cg-render` (GraphView canvas, `Camera` viewport mapping, draw planning), `cg-interact` (pointer drag/pan/zoom/selection state)
- **app layer**: `compograph` (desktop application binary, gpui bootstrapping and assembly)

Self-built crates are layered as `foundation/`, `engine/`, `render/`, and `app/`; no upstream source lives in the tree, so this project's own code is visible at a glance and is not drowned out by vendor files.

### Rust Crate Dependency DAG

```
foundation: cg-types <- cg-geometry
cg-types <- cg-graph (also depends on petgraph and gpui's EventEmitter marker)
engine: cg-graph + cg-types <- cg-layout (subscription-driven requires gpui's Context, so it depends on gpui at the same level)
render: cg-graph + cg-layout + cg-geometry + cg-types <- cg-render
cg-graph + cg-render + cg-types <- cg-interact
app: all of the above <- compograph (also depends on gpui_platform)
upstream: the gpui-pre snapshot crates from crates.io interdepend,
 and never depend in reverse on any cg-* crate
```

`petgraph` is directly depended on only by `cg-graph`; the other self-built crates access the graph through the identifier types (`NodeIndex`/`EdgeIndex`) and query methods exposed by `cg-graph`, and do not directly reference petgraph.

Strict DAG; circular dependencies of any form are prohibited. `cg-graph` depends on gpui because the `EventEmitter` marker trait must be implemented on the type definition side (orphan rule); `cg-layout` depends on it in order to establish change subscriptions using `Context`/`App`. Neither depends on any gpui platform backend, so they can still be unit tested headlessly.

## GPUI Dependency Specifications

- gpui arrives from crates.io as the `gpui-pre` / `gpui-pre-platform` snapshot crates, pinned to an exact version in `[workspace.dependencies]`. The pin must stay identical to gpui-kit's workspace pin, because both must build against one GPUI API baseline; bump them together.
- The pin is exact on purpose: a caret requirement would let a newer snapshot move the build onto an API this project was never compiled against.
- The snapshot carries published, workspace-inheritance-free manifests, so this project never restates a gpui workspace table.
- Do not vendor gpui source into this tree or reintroduce a submodule mount point.
- After upgrading the pin, recheck the signatures of all code that references gpui APIs.

## Rust Development Specifications

### Module Structure

Each crate's `lib.rs` directly declares `pub mod` and `pub use` exports. Subfiles use flat naming—nested module directories are prohibited, and `mod.rs` is prohibited.

### File Layout

```
crates/<name>/src/
├── lib.rs              <- all pub mod declarations and pub use exports
├── <module_name>.rs    <- submodule implementation
└── ...
```

## Build and Run

Prerequisites: `rust-toolchain.toml` pins the toolchain (currently 1.98.1). In restricted network environments, cargo needs to be configured with a crates.io mirror (such as rsproxy).

```shell
cargo clippy --all-targets --all-features   # full compilation check
cargo test --workspace                      # full test suite
cargo run -p compograph                     # launch desktop application
```

## Testing

Unit tests are placed in the same file under `#[cfg(test)]`; large files are split into a separate `test.rs`; integration tests are placed in `tests/`; benchmarks are placed in `benches/`.

## Coding Specifications

- **Safety**: `unwrap` is prohibited (in tests, `expect` may be used). Except for low-level operations, `unsafe` is prohibited; if it must be used, it must be recorded in `docs/archive/unsafe.md`.
- **Types**: Minimize the use of `dyn`; prefer concrete types; all dynamic dispatch must have comments explaining the reason.
- **Dependencies**: All sub-crates form a strict DAG; dependency versions are centralized in the workspace section of the root `Cargo.toml`.

## Important Notes

1. **Rust dependencies**: Uniformly centralized in the root `Cargo.toml` under `[workspace.dependencies]`; sub-crates always use `xxx.workspace = true`.
2. **Planning/design documents**: Avoid large code snippets; focus on concise natural language descriptions.
3. **Documentation synchronization**: `docs/architecture/` and `docs/plan/` should be continuously updated as code lands to replace outdated content (update rather than merely mark).

