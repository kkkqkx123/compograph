# Petgraph 架构设计分析

> 分析对象：`https://github.com/petgraph/petgraph`（本地路径 `/workspace/src/petgraph`）
> 结构：Cargo workspace（`crates/core` + `crates/petgraph` + `serialization-tests`），见 `Cargo.toml:1-4`
> 定位：Rust 图数据结构与算法库，**纯计算、无渲染、无交互**，可在 `no_std` 环境使用（`src/petgraph/src/lib.rs:467`）

---

## 0. 一句话定位

Petgraph 是一个**以 trait 抽象、以泛型算法为核心**的 Rust 图论计算库。它把「图数据结构的多种内部表示」与「作用在 trait 上的算法」彻底解耦——算法不绑定具体图类型，只要满足相应访问器 trait 即可复用。

---

## 1. Workspace 分层

`Cargo.toml:1-4` 定义 workspace 成员：

```toml
[workspace]
members  = ["crates/core", "crates/petgraph", "serialization-tests"]
```

| Crate | 职责 | 关键源码 |
| --- | --- | --- |
| `petgraph-core`（`crates/core`） | 底层图数据结构与索引类型 | `src/graph/`（directed/undirected/disjoint/mod/adjacent）、`src/{node,edge,id}.rs` |
| `petgraph`（`crates/petgraph`） | 高层 API：算法、访问器 trait、算子、图 I/O | `src/algo/`、`src/visit/`、`src/operator.rs`、`src/dot/`、`src/graphmap.rs` 等 |
| `serialization-tests` | 序列化兼容性测试 | — |

这种「core（数据结构）/ high-level（算法与 trait）」分层，使算法 crate 与具体存储解耦。

---

## 2. 图类型（Graph Types）

`lib.rs:527` 明确：`Graph<N, E, Ty, Ix>` 是**邻接表（adjacency list）**表示。全库共 6 种图类型：

| 类型 | 文件 | 特点 |
| --- | --- | --- |
| `Graph` | `src/graph_impl/mod.rs`（再导出 `graph_impl`） | 邻接表，功能最全，移除节点会复用索引 |
| `StableGraph` | `src/graph_impl/stable_graph/` | 移除节点后索引保持稳定（`lib.rs:537-538`），代价是额外内存 |
| `GraphMap` | `src/graphmap.rs`（feature `graphmap`） | 以节点权重本身作哈希键，无需独立节点索引 |
| `MatrixGraph` | `src/matrix_graph.rs`（feature `matrix_graph`） | 邻接矩阵表示 |
| `Csr` | `src/csr.rs` | 压缩稀疏行（CSR）邻接矩阵 |
| `List` | `src/adj.rs:163`（`pub struct List<E, Ix>`） | 轻量邻接表变体 |

通用参数 `N`（节点权）、`E`（边权）、`Ty`（`Directed`/`Undirected` 标记类型）、`Ix`（索引位宽 `u8/u16/u32/usize`，默认 `u32`）——见 `lib.rs:135-157`。`EdgeType` trait（`lib.rs:592-608`）以零成本抽象区分有向/无向。

---

## 3. 访问器 Trait 体系（visit 模块）

`src/visit/mod.rs` 是整个库「算法通用化」的基石。核心设计：**`Into*` 系列 trait 用共享引用产出迭代器**，类似 `IntoIterator`（`visit/mod.rs:3-8`）。

### 3.1 关键 trait（`visit/mod.rs:77-93` 及宏定义）

- `GraphBase`（`visit/mod.rs:80-90`）：定义关联类型 `NodeId` / `EdgeId`。
- `GraphRef`（`visit/mod.rs:96-98`）：可拷贝的图引用。
- `IntoNeighbors`（`visit/mod.rs:107-113`）：按节点取邻居迭代器。
- `IntoNeighborsDirected` / `IntoEdges` / `IntoEdgeReferences` / `IntoNodeIdentifiers` / `NodeIndexable` / `NodeCompactIndexable` / `Visitable` / `GetAdjacencyMatrix` 等。

`trait_template!` 宏（`visit/mod.rs:77` 起）统一生成 trait 定义与委托实现（含 `&mut G` 委托），降低样板。

### 3.2 遍历器（Walker 模式）

`visit/` 提供 `Dfs`、`Bfs`、`DfsPostOrder`、`Topo` 以及回调式 `depth_first_search`（`visit/mod.rs:12-24`）。它们使用「walker」设计：遍历时不持有图的可变借用，仅在 `.next()` 时借用，并可通过 `Walker` trait 转为迭代器（`visit/mod.rs:13-16`）。

### 3.3 图适配器

- `Reversed`（`visit/reversed.rs`）：把图反向，无需复制。
- `filter`（`visit/filter.rs`）：边/节点过滤视图。
- `UndirectedAdaptor`（`visit/undirected_adaptor.rs`）：把有向图当无向处理。

`visit/mod.rs:39-57` 的表格列出了上述 trait 在 6 种图类型上的实现覆盖情况。

---

## 4. 算法模块（algo 模块）

`src/algo/mod.rs` 是算法集合。每个算法一个子模块（`algo/mod.rs:14-37`），核心算法通过 `pub use` 暴露（`algo/mod.rs:41-70`）。设计目标明确：**「逐步把算法迁移到基于图 trait，使其普遍适用」**（`algo/mod.rs:9-11`）。

直接定义在 `algo/mod.rs` 的算法：**连通分量、环检测、拓扑排序、可达性、凝聚、二分图判定**（`algo/mod.rs:133-609`）。

详细算法清单见 `docs/analysis/graph-algorithms-diff.md`。

---

## 5. 数据与算子

- **数据 trait**（`src/data.rs`）：`FromElements`、`FromIndex` 等，支持从边集合构造图（`lib.rs:500`）。
- **算子**（`src/operator.rs`）：从已有图生成新图，如 `complement`（图补，`operator.rs:59-79`）、子图等。
- **并查集**（`src/unionfind.rs`）：`UnionFind`，被 `connected_components`、`is_cyclic_undirected` 复用（`algo/mod.rs:137`）。

---

## 6. 图 I/O 与序列化

| 能力 | 文件 | feature |
| --- | --- | --- |
| DOT 导出 | `src/dot/mod.rs`、`src/dot/dot_parser.rs` | 默认 |
| DOT 导入 | `src/dot/dot_parser.rs` | `dot_parser` |
| graph6 编解码 | `src/graph6/` | 默认 |
| serde 序列化 | `src/serde_utils.rs`、`graph_impl/serialization.rs` | `serde-1` |
| 图生成器 | `src/generate.rs` | `generate`（不稳定） |

---

## 7. 特性开关（Crate Features）

`lib.rs:437-464` 列出：

- 默认开启：`graphmap`、`stable_graph`、`matrix_graph`、`std`（可关闭以进入 `no_std`）。
- 可选：`serde-1`（序列化）、`rayon`（并行迭代/算法，如 `parallel_johnson`，`algo/mod.rs:53-54`）、`dot_parser`、`generate`、`unstable`。

`#![no_std]`（`lib.rs:467`）配合 `extern crate alloc`，保证可在嵌入式/无标准库环境使用。

---

## 8. 架构特征小结

| 特征 | 表现 |
| --- | --- |
| 设计范式 | trait 抽象 + 泛型算法 + workspace 分层 |
| 关注点分离 | core（数据结构）与 petgraph（算法/trait/I/O）分离 |
| 多表示 | 6 种图类型，覆盖邻接表/稳定表/哈希表/矩阵/CSR |
| 算法通用性 | 算法作用于访问器 trait，跨图类型复用 |
| 零成本抽象 | 标记类型 `Directed/Undirected`、`Reversed` 适配器无拷贝 |
| 运行环境 | `no_std` 友好，可并行（rayon），可选序列化 |
| 可视化 | 无（与 Cytoscape.js 根本差异） |

**与 Cytoscape.js 的本质差异**：Petgraph 是纯计算库，强调泛型复用与零成本抽象；Cytoscape.js 是「模型 + 渲染 + 交互」一体化库，算法以方法形式运行在活图模型上。
