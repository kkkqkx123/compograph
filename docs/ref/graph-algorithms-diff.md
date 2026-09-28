# Petgraph 与 Cytoscape.js 图算法差异分析

> 对比对象：
> - Petgraph（纯计算库）：`/workspace/src/petgraph`，算法见 `crates/petgraph/src/algo/` 与 `crates/petgraph/src/visit/`
> - Cytoscape.js（分析+可视化库）：`/workspace/src/cytoscape.js`，算法见 `src/collection/algorithms/`
>
> 核心结论先行：**两者几乎没有算法「正面竞争」**——Petgraph 偏「经典图论/组合算法」（最短路、最大流、匹配、连通性、同构、传递归约等），Cytoscape.js 偏「网络科学与聚类分析」（中心性、聚类、Euler、最小割、布局）。重叠部分很少（最短路、SCC、MST、PageRank、A*、BFS/DFS）。

---

## 0. 设计哲学差异（决定算法清单的根本原因）

| 维度 | Petgraph | Cytoscape.js |
| --- | --- | --- |
| 算法形态 | 自由函数，作用于**访问器 trait**（如 `IntoNeighbors`、`IntoEdgeReferences`），跨 6 种图类型复用（`algo/mod.rs:9-11`） | 方法挂在 `Collection.prototype` 上，运行在「活的」图模型，结果可写回元素 data/样式（`collection/algorithms/index.mjs:21-44`） |
| 算法目标 | 通用图论计算、正确性、零成本抽象 | 网络分析、可视化前的属性计算 |
| 可视化相关 | 无 | 含 9 种布局算法 + 渲染（非算法但常被一并比较） |

---

## 1. 算法能力总表

> 标记：`✅ 支持` / `❌ 不支持` / `⚠️ 部分/间接`

| 算法类别 | 具体算法 | Petgraph | Cytoscape.js | 备注 |
| --- | --- | :---: | :---: | --- |
| **单源最短路** | Dijkstra | ✅ `dijkstra`（`algo/dijkstra.rs`，`algo/mod.rs:45`） | ✅ `dijkstra.mjs` | 重叠 |
| | 双向 Dijkstra | ✅ `bidirectional_dijkstra`（`algo/mod.rs:45`） | ❌ | — |
| | A* | ✅ `astar`（`algo/astar.rs`，`algo/mod.rs:41`） | ✅ `a-star.mjs` | 重叠 |
| | Bellman-Ford | ✅ `bellman_ford` + `find_negative_cycle`（`algo/mod.rs:42`） | ✅ `bellman-ford.mjs` | 重叠 |
| | SPFA | ✅ `spfa`（`algo/spfa.rs`，`algo/mod.rs:68`） | ❌ | — |
| **全源最短路** | Floyd-Warshall | ✅ `floyd_warshall`（`algo/mod.rs:47`） | ✅ `floyd-warshall.mjs` | 重叠 |
| | Johnson | ✅ `johnson` + `parallel_johnson`（`algo/mod.rs:52-54`） | ❌ | — |
| **K 短路** | Yen's KSP | ✅ `k_shortest_path`（`algo/k_shortest_path.rs`，`algo/mod.rs:55`） | ❌ | — |
| **最小生成树** | Kruskal / Prim | ✅ `min_spanning_tree` + `min_spanning_tree_prim`（`algo/mod.rs:59`） | ✅ `kruskal.mjs`（仅 Kruskal） | 重叠（Cytoscape 无 Prim） |
| **最大流** | Ford-Fulkerson / Dinic | ✅ `ford_fulkerson` + `dinics`（`algo/maximum_flow/`，`algo/mod.rs:58`） | ❌ | **Petgraph 独有** |
| **匹配** | 最大匹配 / 贪心匹配 | ✅ `maximum_matching` + `greedy_matching`（`algo/matching.rs`，`algo/mod.rs:56`） | ❌ | **Petgraph 独有** |
| **强连通分量** | Tarjan / Kosaraju | ✅ `tarjan_scc` + `kosaraju_scc`（`algo/mod.rs:63-66`） | ✅ `tarjan-strongly-connected.mjs` | 重叠（Petgraph 多 Kosaraju） |
| | 凝聚（SCC→单节点） | ✅ `condensation`（`algo/mod.rs:481-518`） | ❌ | — |
| **连通性** | 连通分量计数 | ✅ `connected_components`（`algo/mod.rs:133`） | ⚠️ 经 BFS/DFS 可间接得，无直接 API | — |
| | 关节点（割点） | ✅ `articulation_points`（`algo/articulation_points.rs`，`algo/mod.rs:14`） | ✅ 经 `hopcroft-tarjan-biconnected.mjs`（双连通分量含割点） | 路径不同 |
| | 桥（割边） | ✅ `bridges`（`algo/bridges.rs`，`algo/mod.rs:17`） | ❌ | — |
| | 二分图判定 | ✅ `is_bipartite_undirected`（`algo/mod.rs:561`） | ❌ | — |
| | 可达性 | ✅ `has_path_connecting`（`algo/mod.rs:366`） | ✅ 经 `bfs-dfs.mjs` | — |
| **环检测** | 有向/无向环 | ✅ `is_cyclic_directed` / `is_cyclic_undirected`（`algo/mod.rs:167,281`） | ❌ 无独立 API | — |
| **拓扑排序** | 拓扑序 | ✅ `toposort`（`algo/mod.rs:208`） | ❌ | **Petgraph 独有** |
| **支配树** | 支配者 | ✅ `dominators`（`algo/dominators.rs`，`algo/mod.rs:20`） | ❌ | **Petgraph 独有** |
| **传递归约** | transitive reduction | ✅ `tred`（`algo/tred.rs`，`algo/mod.rs:37`） | ❌ | **Petgraph 独有** |
| **反馈弧集** | feedback arc set | ✅ `greedy_feedback_arc_set`（`algo/mod.rs:46`） | ❌ | **Petgraph 独有** |
| **Steiner 树** | steiner tree | ✅ `steiner_tree`（需 `stable_graph`，`algo/mod.rs:36,70`） | ❌ | **Petgraph 独有** |
| **图着色** | DSatur 着色 | ✅ `dsatur_coloring`（`algo/coloring.rs`，`algo/mod.rs:44`） | ❌ | **Petgraph 独有** |
| **极大团** | maximal cliques | ✅ `maximal_cliques`（`algo/maximal_cliques.rs`，`algo/mod.rs:57`） | ❌ | **Petgraph 独有** |
| **图同构** | 图/子图同构 | ✅ `is_isomorphic` / `is_isomorphic_matching` / `is_isomorphic_subgraph` / `subgraph_isomorphisms_iter`（`algo/mod.rs:48-51`） | ❌ | **Petgraph 独有** |
| **简单路径枚举** | all simple paths | ✅ `all_simple_paths` / `all_simple_paths_multi`（`algo/mod.rs:67`） | ❌ | — |
| **中心性** | PageRank | ✅ `page_rank`（`algo/mod.rs:60`） | ✅ `page-rank.mjs` | 重叠 |
| | 度中心性 | ❌ 无独立 API（可自算） | ✅ `degree-centrality.mjs` | **Cytoscape 独有** |
| | 接近中心性 | ❌ | ✅ `closeness-centrality.mjs` | **Cytoscape 独有** |
| | 介数中心性 | ❌ | ✅ `betweenness-centrality.mjs` | **Cytoscape 独有** |
| **聚类** | Markov 聚类 (MCL) | ❌ | ✅ `markov-clustering.mjs` | **Cytoscape 独有** |
| | k-means / k-medoids / 模糊 C | ❌ | ✅ `k-clustering.mjs`（含三种） | **Cytoscape 独有** |
| | 层次聚类 (HCA) | ❌ | ✅ `hierarchical-clustering.mjs` | **Cytoscape 独有** |
| | 亲和传播 | ❌ | ✅ `affinity-propagation.mjs` | **Cytoscape 独有** |
| **Euler 路径** | Hierholzer | ❌ | ✅ `hierholzer.mjs` | **Cytoscape 独有** |
| **最小割** | Karger-Stein | ❌ | ✅ `karger-stein.mjs`（随机化） | **Cytoscape 独有** |
| **遍历** | BFS / DFS | ✅ `Bfs` / `Dfs` / `DfsPostOrder` / `depth_first_search`（`visit/mod.rs:12-24`） | ✅ `bfs-dfs.mjs` | 重叠（Petgraph 更丰富：含 DfsPostOrder、Topo） |
| **布局（几何定位）** | 力导向/圆形/同心等 | ❌（纯计算无布局） | ✅ 9 种（`extensions/layout/`：breadthfirst/circle/concentric/cose/grid/null/preset/random） | **Cytoscape 独有** |

---

## 2. 关键差异解读

### 2.1 Petgraph 独有能力（Cytoscape.js 缺失）
- **网络流与匹配**：最大流（Dinic/Ford-Fulkerson）、最大/贪心匹配——这些是组合优化经典问题，Cytoscape.js 完全不涉及。
- **更深的图结构分析**：支配树、传递归约、反馈弧集、Steiner 树、图/子图同构、极大团、DSatur 着色、桥、二分图判定、拓扑排序、K 短路（Yen）、Johnson 全源最短路、SPFA、双向 Dijkstra。
- **更完整的连通性工具**：`articulation_points`、`bridges`、`condensation`、`connected_components`、`is_cyclic_*` 均为一等公民。
- **通用遍历**：除 BFS/DFS 外还有 `DfsPostOrder`、`Topo`、`Reversed` 适配器。

> 原因：Petgraph 面向「图算法工具箱」，强调算法广度与正确性；Cytoscape.js 面向「网络数据分析 + 可视化」，不需要流/匹配/同构这类偏理论的组合算法。

### 2.2 Cytoscape.js 独有能力（Petgraph 缺失）
- **网络科学中心性家族**：度/接近/介数中心性（Petgraph 仅 PageRank）。
- **完整聚类套件**：MCL、k-means/k-medoids/模糊 C、层次聚类、亲和传播——Petgraph 无任何聚类算法。
- **Euler 路径（Hierholzer）** 与 **最小割（Karger-Stein 随机化）**。
- **布局算法**：9 种几何定位布局（力导向 cose、圆形、同心、网格、随机等）。Petgraph 无布局概念（纯计算、不画图）。

> 原因：Cytoscape.js 的典型用户是生物网络/社交网络分析者，需要聚类、中心性、可视化布局；Petgraph 用户是需要在 Rust 中跑图算法的系统/算法工程师。

### 2.3 双方重叠（都支持）
Dijkstra、A*、Bellman-Ford、Floyd-Warshall、Kruskal（MST）、Tarjan SCC、PageRank、BFS/DFS、可达性（has_path_connecting / bfs-dfs）。

即使在重叠区也有差异：
- **MST**：Petgraph 同时提供 Kruskal 与 Prim；Cytoscape.js 仅 Kruskal。
- **SCC**：Petgraph 提供 Tarjan 与 Kosaraju 两种 + 凝聚；Cytoscape.js 仅 Tarjan。
- **最短路**：Petgraph 额外提供双向 Dijkstra、Johnson、SPFA、K 短路；Cytoscape.js 仅 Dijkstra/A*/Bellman-Ford/Floyd。

---

## 3. 实现机制差异

| 机制 | Petgraph | Cytoscape.js |
| --- | --- | --- |
| 算法入口 | 自由函数 `algo::dijkstra(&g, ...)`，泛型约束 trait | `cy.elements().dijkstra(...)` 等集合方法 |
| 权重来源 | 由调用方传入 cost 闭包（`|e| e.weight`），或 `N/E` 泛型权 | 由元素 data 字段经 `weight` 选项读取 |
| 结果形态 | 返回 `HashMap<NodeId, Cost>` / `Vec` / 新图等纯数据 | 多返回路径数组或写回元素 data/样式 |
| 并行 | `rayon` feature 提供 `parallel_johnson` 等（`algo/mod.rs:53`） | 单线程（前端库，靠渲染循环异步） |
| 内存模型 | 零成本抽象、可 `no_std`、索引位宽可配 `Ix` | 活对象 + 事件系统，强依赖 DOM/Canvas |

---

## 4. 选型建议

- 需要在 **Rust 服务端/嵌入式**做**通用图算法（流、匹配、同构、连通性、拓扑、归约）** → 选 **Petgraph**。
- 需要在 **浏览器/Node** 做**网络分析 + 可视化**（中心性、聚类、布局、交互） → 选 **Cytoscape.js**。
- 若前端需要 Petgraph 独有算法：可在服务端用 Petgraph 计算后把结果（如流值、匹配、拓扑序）序列化喂给 Cytoscape.js 渲染；反之 Cytoscape.js 的聚类/中心性结果也可导出到 Rust 做进一步计算。

---

## 5. 事实依据索引

- Petgraph 算法清单：`crates/petgraph/src/algo/mod.rs:14-70`（子模块声明与 `pub use`）
- Petgraph 直接定义算法：`crates/petgraph/src/algo/mod.rs:133-609`（连通分量、环检测、拓扑、可达、凝聚、二分图）
- Petgraph 访问器/遍历：`crates/petgraph/src/visit/mod.rs:12-57`
- Petgraph 图类型：`crates/petgraph/src/lib.rs:123-133`、`src/adj.rs:163`（List）
- Cytoscape 算法清单：`src/collection/algorithms/index.mjs:1-46`
- Cytoscape 布局：`src/extensions/layout/`（9 个文件）
- Cytoscape 渲染：`src/extensions/renderer/`（base/canvas/null + webgl）
