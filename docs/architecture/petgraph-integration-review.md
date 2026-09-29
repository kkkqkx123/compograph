# compograph 中 petgraph 的引入方式与设计评审

> 日期：2026-09-29 · 基线：petgraph `0.8.3`（本地克隆 `/workspace/repos/petgraph` @ `a4d94bd`）、compograph HEAD `397e70f` + 本次阶段 0 收尾改动
> 关联：[架构设计](./architecture-design.md) · [借鉴设计说明](./borrowing-design.md) · [分阶段实施方案](../plan/compograph-implementation-plan.md)

---

## 1. 结论先行

> **当前对 petgraph 的引入方式是合理的，且是 Rust 生态中处理"通用库 + 应用特化需求"的标准做法：workspace 统一版本 + 各层按需直接依赖 + 在 `cg-graph` 内用薄封装收敛算法入口。** 存在两处可优化点（`petgraph` 直接依赖面偏宽、算法桥可加 feature gate），但都不构成当前阶段的阻塞问题。

---

## 2. 引入方式的事实描述

### 2.1 版本声明的单一来源

petgraph 是**外部 crates.io 依赖**，不是源码移植、也不是 path 依赖。版本在根 `Cargo.toml` 的 `[workspace.dependencies]` 中集中声明，子 crate 一律 `petgraph.workspace = true`：

```toml
# 根 Cargo.toml
[workspace.dependencies]
petgraph = "0.8"
```

`Cargo.lock` 锁定解析结果为 `0.8.3`。这符合 AGENTS.md §「重要备注 1」的规定（依赖统一集中在 workspace，子 crate 用 `xxx.workspace = true`）。

### 2.2 直接依赖 petgraph 的 crate（改造后仅 1 个）

| crate | 层的角色 | 与 petgraph 的关系 |
|---|---|---|
| `cg-graph` | foundation / 图模型 | **唯一直接依赖**：拥有 `StableGraph` 实例、定义 `NodeData`/`EdgeData` 权重、发出 `NodeIndex`/`EdgeIndex`、算法桥 |
| `cg-layout` | engine / 布局 | 经 `cg-graph` 的 `node_ids()` 遍历，标识用 `cg_graph::NodeIndex` |
| `cg-render` | render / 渲染 | 同上 |
| `cg-interact` | render / 交互 | 携带 `cg_graph::NodeIndex` 标识拖拽目标 |
| `compograph` | app / 应用 | 经 `cg-graph` 的 `node_ids()` 遍历 |

> 改造前这 5 个 crate 均直接 `use petgraph::...`（共 7 处引用）；改造后仅 `cg-graph` 依赖 petgraph，其余全部经 `cg-graph` 门面访问。

### 2.3 引用点分布（改造后，自建代码）

```
cg-graph:     全部 petgraph 引用集中于此（store.rs / algo.rs / events.rs / positions.rs / binding.rs）
cg-layout:     0 处
cg-render:     0 处
cg-interact:   0 处
compograph:    0 处
```

改造前共 68 处类型/函数引用散落在 5 个 crate，现将 `petgraph` 名称空间收敛到 `cg-graph` 内部。

### 2.4 关键设计选择：`StableGraph` 封装进单一源

`cg-graph/src/store.rs` 是唯一持有图结构的类型。它把 `StableGraph` 私有化在 `GraphStore` 内，只通过 `graph()` 暴露只读引用：

```rust
pub struct GraphStore {
    graph: StableGraph<NodeData, EdgeData, Directed>,   // 私有
}

impl GraphStore {
    pub fn graph(&self) -> &StableGraph<NodeData, EdgeData, Directed> { &self.graph }
}
```

下游（layout/render）拿到的是 `&StableGraph`，通过 petgraph 的 `visit` trait 遍历，而不是自己持有图。

### 2.5 算法桥：新增的收敛层

本次阶段 0 收尾新增 `cg-graph/src/algo.rs`，把 petgraph 算法包成"不泄漏泛型"的普通函数：

```rust
pub fn shortest_paths(graph: &Graph, start: NodeIndex) -> HashMap<NodeIndex, f32>   // dijkstra
pub fn strongly_connected_components(graph: &Graph) -> Vec<Vec<NodeIndex>>           // tarjan_scc
pub fn rank_nodes(graph: &Graph, damping: f32, iterations: usize) -> Vec<f32>        // page_rank
pub fn successors(graph: &Graph, node: NodeIndex) -> Vec<(NodeIndex, EdgeIndex)>
pub fn predecessors(graph: &Graph, node: NodeIndex) -> Vec<(NodeIndex, EdgeIndex)>
```

一个实测细节：petgraph 0.8.3 的 `dijkstra` 返回 `hashbrown::HashMap`（petgraph 内部 re-export 的 hashbrown），封装层需 `.into_iter().collect()` 转为 `std::HashMap`，否则调用方会看到意外的类型 —— 这正是"桥接层"要吸收的库细节。

---

## 3. 设计合理性评审

### 3.1 合理之处

| 维度 | 评价 | 说明 |
|---|---|---|
| **版本集中** | ✅ 合理 | workspace 单点声明 + 子 crate `workspace = true`，符合项目自定规范，避免各 crate 漂移 |
| **复用而非移植** | ✅ 正确 | petgraph 是成熟算法库，`borrowing-design.md` 明确"Cargo 依赖直接复用，不复制源码"，避免了重写 Dijkstra/Tarjan/PageRank 的巨大风险 |
| **`StableGraph` 而非 `Graph`** | ✅ 关键正确 | `Graph` 删除节点后会复用索引，导致缓存的坐标/选中态错位；`StableGraph` 保持索引稳定，与 `Positions = HashMap<NodeIndex, Point2>` 外挂坐标的设计天然契合 |
| **单一持有者** | ✅ 合理 | 图结构只在 `GraphStore` 一处，下游只读，避免多处可变引用与事件广播不一致 |
| **算法桥收敛** | ✅ 合理 | 新增的 `algo.rs` 让算法入口集中、可 headless 测试，且不把 petgraph 泛型泄给上层 |
| **类型直用而非自造** | ✅ 合理 | 直接使用 `NodeIndex`/`EdgeIndex` 作为跨层标识，而非自造 u32 包装。省去转换层，且 `StableGraph` 的索引语义天然正确 |

### 3.2 已完成的改进

评审提出的两项改进已在本次收尾中落地，`petgraph` 的直接依赖面从 5 个 crate 收敛到 **1 个**（仅 `cg-graph`）。

#### 改进 1：`cg-graph` 作为唯一 petgraph 门面 ✅ 已实施

- `cg-graph` 现 `pub use petgraph::stable_graph::{NodeIndex, EdgeIndex};`，下游改从 `cg-graph` 导入标识类型。
- `GraphStore` 新增用途明确查询方法：`node_ids()`、`node_count()`、`edge_count()`、`node_data()`、`edge_endpoints()`、`successors()`、`predecessors()`。
- 下游 `cg-layout` / `cg-render` / `cg-interact` / `app` 的**全部 petgraph 直接引用已移除**（原 4+1+1+1=7 处 → 0 处），对应 `Cargo.toml` 中的 `petgraph` 依赖一并删除。
- `cg-interact` 因需 `NodeIndex` 而改依赖 `cg-graph`（此前依赖 petgraph）。

#### 改进 2：只读查询方法替代具体类型遍历 ✅ 已实施

- 下游原先通过 `store.graph().node_identifiers()` 遍历（依赖 petgraph 的 `IntoNodeIdentifiers` trait），现统一改为 `store.node_ids()`。
- `store.graph()` 仍保留，供算法桥等确需完整 petgraph 表面的内部使用，但文档已注明"优先使用用途明确的方法"。
- 邻接查询（`successors`/`predecessors`）从 `algo.rs` 迁入 `GraphStore` 方法，语义更贴近"图自身的查询"而非"算法"。

#### 尚未实施（保留为后续可选）

规划中的**只读视图 trait**（`borrowing-design.md` B2，feature-list §1.5，P1）未落地：当前是通过"`GraphStore` 暴露语义化方法"来解耦，而非抽 trait。二者效果相近，且方法方案更轻、更少泛型。若将来出现"布局/渲染需作用于非 `GraphStore` 的图"（如分析子图、mock 图），再抽 trait 更合适。此项不阻塞阶段 1。

---

## 4. 评审结论

| 项 | 结论 |
|---|---|
| 引入方式（workspace 版本 + 直接依赖） | ✅ 合理，符合项目规范与 Rust 生态惯例 |
| `StableGraph` 选型 | ✅ 关键正确，与坐标外挂设计一致 |
| 单一持有者 + 只读暴露 | ✅ 合理 |
| 算法桥（本次新增） | ✅ 合理，收敛了泛型与库细节 |
| 具体类型泄漏到下游 | ✅ 已改进：下游改走 `GraphStore` 语义化查询方法，petgraph 直接依赖收敛到 `cg-graph` 一个 crate |
| petgraph 直接依赖面 | ✅ 已收敛：`cg-graph` 作为唯一门面，re-export `NodeIndex`/`EdgeIndex` |
| 只读视图 trait（feature-list §1.5） | ⏸ 未做，以方法替代；待出现非 `GraphStore` 图需求时再抽 |

**总评：设计合理，无返工。** 评审提出的两项改进已落地，依赖面从 5 个 crate 收敛到 1 个。剩余的可选项（只读视图 trait）按需再议，不阻塞阶段 1。

---

## 5. 附：petgraph 在本项目的角色边界

沿用 `borrowing-design.md` 的定位——petgraph 是 **L1 数据/算法内核**，角色边界清晰：

- **它负责**：图是什么（`StableGraph` 结构）、图怎么算（`algo/*` 算法库）、图怎么遍历（`visit/*` trait 体系）。
- **它不负责**（故本项目自研）：坐标（`Positions` 外挂）、布局（`cg-layout`）、渲染（`cg-render`）、交互（`cg-interact`）。
- 这一边界在 petgraph 上游得到印证：`petgraph/src/lib.rs` 全文无布局/坐标/渲染 API。
