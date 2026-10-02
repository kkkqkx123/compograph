# petgraph 依赖分析

> 用途：架构分析与依赖选型参考。整理 compograph 对 petgraph 的实际使用点，并评估该依赖的必要性。

## 概况

- 依赖版本：petgraph 0.8（workspace 根 `Cargo.toml` 统一声明）。
- 唯一直接依赖方：`crates/foundation/cg-graph`。其余 crate 均通过 `cg-graph` 再导出的 `NodeIndex`/`EdgeIndex` 与 `GraphView` trait 间接使用，不接触 petgraph 类型。

## 使用点分类

### 1. 数据结构：`StableGraph`（核心依赖）

`StableGraph<NodeData, EdgeData, Directed>` 是 `GraphStore` 的存储底座，覆盖 store、batch、affinity、attrs、classes、selector、collection、io、view 等模块。

关键收益是**节点/边删除后索引保持稳定**（不像 `Graph` 会 compact 重编号），满足图形编辑器"UI 持有的索引在删除操作后仍有效"的语义。

### 2. Visitor trait 体系

- 遍历接口：`EdgeRef`、`IntoEdgeReferences`、`IntoNodeIdentifiers`、`NodeIndexable`、`Direction`
- 遍历器（`algo/traversal.rs`）：`Bfs`、`Dfs`、`DfsPostOrder`、`Topo`

### 3. 成品算法（`algo/` 各模块薄包装）

| 模块 | 调用的 petgraph 算法 |
|---|---|
| paths | `dijkstra`、`astar`、`bellman_ford`、`johnson`、`spfa`、`bidirectional_dijkstra`、`k_shortest_path`、`find_negative_cycle` |
| order | `toposort`、`tarjan_scc`、`kosaraju_scc`、`simple_fast`（支配树） |
| connectivity | `is_cyclic_directed`、`has_path_connecting` |
| spanning | `min_spanning_tree`、`min_spanning_tree_prim` |
| combinatorial | `all_simple_paths`、`dsatur_coloring`、`greedy_feedback_arc_set`、`greedy_matching`、`maximum_matching`、`maximal_cliques`、`dinics`（最大流） |
| centrality | `page_rank` |

### 4. 杂项

- `dot::Dot`：DOT 导出格式化器（`io/dot.rs`），仅用于格式化输出；DOT 导入是手写实现。
- `graph::Graph`（紧凑图）：仅在 `algo/combinatorial.rs` 中做 StableGraph→Graph 转换，适配部分算法接口。

## 必要性结论

**必要，且当前引入方式合理。**

1. **与自研代码不重叠**。项目自研的是领域层（布局引擎、渲染、交互、属性表、类表、折叠层级、快照批量操作、选择器、变更事件系统）；petgraph 承担的是通用图论部分（30+ 经典算法）。重写这些等于维护一个图论库，收益几乎为零。
2. **`StableGraph` 的稳定索引语义不易等价替换**。自研需实现带空闲槽回收的邻接表 + 索引版本校验，复杂度和出错面都大，而这恰是 petgraph 最成熟的部分。
3. **架构隔离已经到位**。petgraph 被严格限制在 cg-graph 内部；下游只接触索引类型与 `GraphView` trait，将来替换存储层的改动面收敛在一个 crate 内。
4. **零传播**。petgraph 不出现在任何 render/app crate 中，不拖入平台代码，编译负担仅限 foundation 层。

## 可选优化

- `Dot` 格式化器只用其格式化能力，若想进一步收窄依赖面，可自行实现 DOT 序列化（为单一功能放弃整个依赖不划算，仅作记录）。

## 后续选型原则

- 下游 crate 继续通过 `cg-graph` 的索引类型与查询方法访问图，禁止直接依赖 petgraph（维持严格 DAG）。
- 新增图论算法优先复用 petgraph 现成实现，经 `algo/` 薄包装暴露；确无现成实现再考虑自研。
