# compograph

## Project Overview

compograph is a pure Rust desktop graph visualization application: the graph model and algorithm core are based on petgraph, the UI foundation is gpui from zed-gpui (Zed's GPU-accelerated GUI framework), and layout, rendering, and interaction are developed in-house by this project.

**No-backward-compatible**: The project is in the development phase; backward compatibility does not need to be considered, only a reasonable architecture needs to be maintained.

**Document Reference Rule**: Code comments must not contain any document structure identifiers (such as P1, P2-3, §4.1, phase3, G2, etc.). Comments should only describe code intent and must not reference external document locations.

**Language**: Code, comments, logs, and error messages must all be in English; documentation is in Chinese. **No Chinese text may appear in code files.**

## Code Architecture

The top level contains `crates/`, `docs/`, `Cargo.toml`, `rust-toolchain.toml`, `.gitmodules`, and `AGENTS.md`.

`crates/` is divided into two parts: self-built layers and upstream mount points:

- **Upstream source (gpui and its dependencies)**: Mounted as a **git submodule** at `crates/vendor/zed-gpui`, containing a snapshot of the `lean` branch of the zed-gpui repository, preserving the upstream original layout (`crates/gpui`, `crates/refineable/derive_refineable`, `tooling/perf`, and other paths consistent with upstream). This project consumes the crates within it through `path` dependencies, for example `gpui = { path = "crates/vendor/zed-gpui/crates/gpui" }`.
- **foundation layer**: `cg-types` (geometry primitives, leaf crate), `cg-geometry` (hit testing and curve geometry, framework-agnostic), `cg-graph` (`GraphStore` graph storage + petgraph `StableGraph` adaptation + change event broadcasting and subscription filtering)
- **engine layer**: `cg-layout` (`LayoutEngine` trait, layout registry, concrete layout implementations, `LayoutDriver` change-driven position maintenance)
- **render layer**: `cg-render` (GraphView canvas, `Camera` viewport mapping, draw planning), `cg-interact` (pointer drag/pan/zoom/selection state)
- **app layer**: `compograph` (desktop application binary, gpui bootstrapping and assembly)

Self-built crates are layered as `foundation/`, `engine/`, `render/`, and `app/`; upstream is not among them, so this project's own code is visible at a glance and is not drowned out by upstream files.

### Rust Crate Dependency DAG

```
foundation: cg-types <- cg-geometry
cg-types <- cg-graph (also depends on petgraph and gpui's EventEmitter marker)
engine: cg-graph + cg-types <- cg-layout (subscription-driven requires gpui's Context, so it depends on gpui at the same level)
render: cg-graph + cg-layout + cg-geometry + cg-types <- cg-render
cg-graph + cg-render + cg-types <- cg-interact
app: all of the above <- compograph (also depends on gpui_platform)
upstream: 27 upstream crates within crates/vendor/zed-gpui interdepend,
 and never depend in reverse on any cg-* crate
```

`petgraph` is directly depended on only by `cg-graph`; the other self-built crates access the graph through the identifier types (`NodeIndex`/`EdgeIndex`) and query methods exposed by `cg-graph`, and do not directly reference petgraph.

Strict DAG; circular dependencies of any form are prohibited. `cg-graph` depends on gpui because the `EventEmitter` marker trait must be implemented on the type definition side (orphan rule); `cg-layout` depends on it in order to establish change subscriptions using `Context`/`App`. Neither depends on any gpui platform backend, so they can still be unit tested headlessly.

## Upstream Source Specifications

- Upstream is a submodule snapshot of the zed-gpui `lean` branch, preserving the upstream layout as-is.
- **Modifying** upstream code is **prohibited**; do not format it or add business dependencies. When changes to upstream are needed, complete them within the zed-gpui repository and go through its synchronization/release process, then update the submodule pointer in this project.
- **Upstream synchronization may only be performed within the zed-gpui repository**. This project never separately fetches/merges upstream: the `.git` of `crates/vendor/zed-gpui` does not participate in this project's workflow and serves only as a read-only mount point.
- The `lean` snapshot has already expanded workspace inheritance (`dep.workspace = true`, etc.) at release time, so this project does not need to restate gpui's workspace table; if you encounter `error inheriting ... from workspace root manifest`, it means the submodule points to unexpanded content or has not been initialized.
- The gpui API line number baseline is `212afa4`; after upgrading upstream, all code referencing gpui APIs must have signatures rechecked.

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

Prerequisites: `rust-toolchain.toml` pins the toolchain (currently 1.98.1). After cloning, first initialize the submodule: `git submodule update --init --depth 1`. In restricted network environments, cargo needs to be configured with a crates.io mirror (such as rsproxy) and a GitHub proxy (such as `git config --global url."https://gh-proxy.com/https://github.com/".insteadOf "https://github.com/"`).

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

