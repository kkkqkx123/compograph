# compograph 开发者指南（AI Agent 必读）

## 项目概述

compograph 是纯 Rust 桌面图可视化应用：图模型与算法内核基于 petgraph，UI 底座为 zed-gpui 的 gpui（Zed GPU 加速 GUI 框架），布局、渲染与交互由本项目自研。

**No-backward-compatible**：项目处于开发期，无需考虑向后兼容，保持合理架构即可。

**Document Reference Rule**：代码注释中禁止出现任何文档结构标识（如 P1、P2-3、§4.1、phase3、G2 等）。注释只描述代码意图，不得引用外部文档位置。

**Language**：代码、注释、日志、错误信息一律使用英文；文档使用中文。**代码文件中禁止出现任何中文。**

## 代码架构

顶层包含 `crates/`、`docs/`、`Cargo.toml`、`rust-toolchain.toml`、`.gitmodules`、`AGENTS.md`。

`crates/` 分为自建分层与上游挂载点两部分：

- **上游源码（gpui 及其依赖）**：以 **git submodule** 挂在 `crates/vendor/zed-gpui`，内容为 zed-gpui 仓库 `lean` 分支的快照，保持上游原始布局（`crates/gpui`、`crates/refineable/derive_refineable`、`tooling/perf` 等路径与上游一致）。本项目通过 `path` 依赖消费其中的 crate，例如 `gpui = { path = "crates/vendor/zed-gpui/crates/gpui" }`。
- **foundation 层**：`cg-types`（几何原语，叶子 crate）、`cg-geometry`（命中测试与曲线几何，框架无关）、`cg-graph`（`GraphStore` 图存储 + petgraph `StableGraph` 适配 + 变更事件广播与订阅过滤）
- **engine 层**：`cg-layout`（`LayoutEngine` trait、布局注册表、具体布局实现、`LayoutDriver` 变更驱动的位置维护）
- **render 层**：`cg-render`（GraphView 画布、`Camera` 视口映射、绘制计划）、`cg-interact`（指针拖拽/平移/缩放/选择状态）
- **app 层**：`compograph`（桌面应用二进制，gpui 引导与装配）

自建 crate 按 `foundation/`、`engine/`、`render/`、`app/` 分层；上游不在其中，因此本项目自身的代码一眼可见，不被上游文件淹没。

### Rust Crate 依赖 DAG

```
foundation:  cg-types ← cg-geometry
             cg-types ← cg-graph（另依赖 petgraph 与 gpui 的 EventEmitter 标记）
engine:      cg-graph + cg-types ← cg-layout（订阅驱动需要 gpui 的 Context，故同级依赖 gpui）
render:      cg-graph + cg-layout + cg-geometry + cg-types ← cg-render ← cg-interact
app:         上述全部 ← compograph（另依赖 gpui_platform）
upstream:    crates/vendor/zed-gpui 内 27 个上游 crate 互依，
             绝不反向依赖任何 cg-* crate
```

严格 DAG，禁止任何形式的循环依赖。`cg-graph` 依赖 gpui 是因为 `EventEmitter` 标记 trait 必须在类型定义侧实现（孤儿规则）；`cg-layout` 依赖它是为了用 `Context`/`App` 建立变更订阅。两者都不依赖任何 gpui 平台后端，因此仍可 headless 单测。

## 上游源码规范

- 上游是 zed-gpui `lean` 分支的 submodule 快照，原样保留上游布局。
- **禁止修改**上游代码、不格式化、不加业务依赖。需要改动上游时，在 zed-gpui 仓库内完成并走它的同步/发布流程，再在本项目更新 submodule 指向。
- **上游同步只能在 zed-gpui 仓库内进行**。本项目绝不单独 fetch/merge 上游：`crates/vendor/zed-gpui` 的 `.git` 不参与本项目的工作流，只作为只读挂载点。
- `lean` 快照在发布时已展开工作区继承（`dep.workspace = true` 等），因此本项目无需复述 gpui 的 workspace 表；若遇到 `error inheriting ... from workspace root manifest`，说明 submodule 指向了未展开的内容或未初始化。
- gpui API 行号基线为 `212afa4`；升级上游后所有引用 gpui API 的代码需复核签名。

## Rust 开发规范

### 模块结构

每个 crate 的 `lib.rs` 直接声明 `pub mod` 与 `pub use` 导出。子文件使用扁平命名——禁止嵌套模块目录，禁止 `mod.rs`。

### 文件布局

```
crates/<name>/src/
├── lib.rs              ← 全部 pub mod 声明与 pub use 导出
├── <module_name>.rs    ← 子模块实现
└── ...
```

## 构建与运行

前置条件：`rust-toolchain.toml` 钉定工具链（当前 1.98.1）。克隆后先初始化 submodule：`git submodule update --init --depth 1`。受限网络环境下需为 cargo 配置 crates.io 镜像（如 rsproxy）与 GitHub 代理（如 `git config --global url."https://gh-proxy.com/https://github.com/".insteadOf "https://github.com/"`）。

```shell
cargo clippy --all-targets --all-features   # 全量编译检查
cargo test --workspace                      # 全量测试
cargo run -p compograph                     # 启动桌面应用
```

## 测试

单元测试置于同文件 `#[cfg(test)]`；大文件拆分独立 `test.rs`；集成测试置于 `tests/`；基准置于 `benches/`。

## 编码规范

- **安全**：禁用 `unwrap`（测试中可用 `expect`）。除底层操作外禁用 `unsafe`，如需使用须在 `docs/archive/unsafe.md` 记录。
- **类型**：尽量少用 `dyn`，优先具体类型；所有动态派发须注释说明理由。
- **依赖**：所有子 crate 构成严格 DAG；依赖版本集中在根 `Cargo.toml` 的 workspace 段。

## 重要备注

1. **Rust 依赖**：统一集中在根 `Cargo.toml` 的 `[workspace.dependencies]`，子 crate 一律 `xxx.workspace = true`。
2. **计划/设计文档**：避免大段代码贴片，以简明自然语言描述为主。
3. **文档同步**：`docs/architecture/` 与 `docs/plan/` 随代码落地持续更新过期内容（更新而非仅标记）。
