# compograph 功能清单（Feature List）

> 版本：v1.0 · 日期：2026-09-28
> 来源标注约定：
> - `[petgraph]` 直接复用 petgraph 现成 API（不重写）
> - `[cy→移植]` 数学/几何从 cytoscape.js 直译为 Rust（源码位置见 [借鉴设计说明](./borrowing-design.md)）
> - `[cy→模式]` 借鉴 cytoscape.js 的设计模式/契约，实现自写
> - `[自研]` 无现成参照或仅参照思路，纯自研
> - `[gpui]` 直接使用 gpui（zed-gpui fork）平台能力
>
> 阶段：P0 骨架闭环 → P1 可交互 → P2 接近 cytoscape 可用度 → P3 大规模。
> **进度口径（2026-09-29 实测校准）**：P0–P3 绝大多数项已落地，表格"阶段"列保留原计划口径，已落地项在"状态"列标注。实测见 [功能实测分析报告](../plan/feature-analysis-report.md)。
> 事实基线：petgraph `a4d94bd`（0.8.3）、zed-gpui `212afa4`（gpui 0.2.2）、cytoscape.js `7ba6340`，均已在本地克隆核验。

---

## 1. L1 数据/算法层（cg-graph）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 1.1 | 可编辑图容器（增删节点/边） | `[petgraph]` | P0 | `StableGraph`（`graph_impl/stable_graph/mod.rs:67`），删除后索引稳定 |
| 1.2 | 节点/边业务数据（label、weight、样式字段） | `[自研]` | P0 | `NodeData`/`EdgeData` 权重类型 |
| 1.3 | 共享状态容器 + 变更事件 | `[gpui]` | P0 | `Entity<GraphStore>`（`entity_map.rs:435`）+ `EventEmitter` + `subscribe`（`app.rs:1272`） |
| 1.4 | 坐标外挂 `PositionStore` | `[自研]` | P0 | petgraph 不存坐标；`Positions = HashMap<NodeIndex,(f32,f32)>` + `fixed` 集 |
| 1.5 | 只读图视图 trait（解耦 L2/L3） | `[petgraph]` | P1 | 基于 `visit` trait 簇（`IntoNeighbors:107`、`IntoNodeIdentifiers:183`、`IntoEdges:147`） |
| 1.6 | 图 I/O（JSON/GraphML 导入导出） | `[自研]` | P2 | ✅ 已落地：JSON 导入导出 + 校验（`io.rs`）；GraphML 未做 |
| 1.7 | DOT 导出/导入 | `[自研]` | P2 | ✅ 已落地：`export_dot`（petgraph `Dot`）+ 自写轻量 `import_dot`（`io.rs`） |

### 1A. 算法库复用清单（petgraph `a4d94bd` 已核验）

| 类别 | 算法/API | 位置 | UI 桥接阶段 |
|---|---|---|---|
| 单源最短路 | `dijkstra`（`dijkstra.rs:92`）、`astar`（`astar.rs:81`）、`bellman_ford`、SPFA、双向 Dijkstra | `algo/` | P2 |
| 全源最短路 | `floyd_warshall`、`johnson`/`parallel_johnson`（rayon） | `algo/` | P2/P3 |
| K 短路 | `k_shortest_path`（Yen） | `algo/` | P3 |
| MST | `min_spanning_tree`（`min_spanning_tree.rs:86`）、`min_spanning_tree_prim`（`:255`） | `algo/` | P2 |
| 最大流 | `ford_fulkerson`（`maximum_flow/ford_fulkerson.rs:164`）、`dinics` | `algo/maximum_flow/` | P3 |
| 匹配 | `maximum_matching`、`greedy_matching`（`matching.rs`） | `algo/` | P3 |
| SCC/连通 | `tarjan_scc`（`scc/tarjan_scc.rs:269`）、`kosaraju_scc`、`condensation`、`connected_components` | `algo/` | P2 |
| 连通性细节 | `articulation_points`（割点）、`bridges`（桥）、二分图判定、`has_path_connecting` | `algo/` | P3 |
| 结构分析 | 拓扑排序 `toposort`、环检测、`dominators`（`dominators.rs:73`）、`tred` 传递归约 | `algo/` | P2/P3 |
| 组合优化 | `steiner_tree`、`dsatur_coloring`（`coloring.rs:57`）、`maximal_cliques`（`maximal_cliques.rs:119`）、`greedy_feedback_arc_set` | `algo/` | P3 |
| 同构 | `is_isomorphic` / `is_isomorphic_matching` / 子图同构 | `algo/isomorphism.rs` | P3 |
| 中心性 | `page_rank`（`page_rank.rs:64`） | `algo/` | P2 |
| 遍历 | `Bfs`/`Dfs`/`DfsPostOrder`/`Topo`、`depth_first_search` | `visit/` | P1（布局内部即用） |

> petgraph 缺失、cytoscape 独有的部分“网络科学”算法（MCL、k-means、层次聚类、亲和传播、Euler 路径、Karger-Stein 最小割）**不在 v1 范围**，列为远期自研项（见 §6）。度/接近/介数中心性已按 P1 方案自研落地，不在远期表。
>
> **已桥接算法（`cg-graph/src/algo.rs`，共 18 个）**：`shortest_paths`/`shortest_path_cost`（Dijkstra）、`shortest_path`（A*）、`heuristic_shortest_path`（带位置启发式 A*）、`strongly_connected_components`（Tarjan）、`minimum_spanning_forest`（Kruskal）、`minimum_spanning_tree_single`（Prim）、`topological_order`、`immediate_dominators`、`rank_nodes`（PageRank）、`transitive_reduction`，另有 P1 新增 `all_pairs_shortest_paths`（自研 Floyd-Warshall）、`bellman_ford_paths` + `negative_cycle_path`、`articulation_points` + `bridges`（自研无向语义）、`breadth_first_order` + `depth_first_order`。中心性三件套在 `centrality.rs`（度/接近/介数，无权版本）。UI 已暴露 21 个面板入口（最短路/引导搜索/连通分量/PageRank/生成树/度中心性/割点桥/全源/负权/遍历/拓扑与归约/接近/介数/单源生成树/支配集/有向欧拉路/无向欧拉路/最小割/层次聚类/马尔可夫聚类/k 均值）。欧拉路双语义在 `euler.rs`，全局最小割在 `min_cut.rs`，三组聚类在 `hierarchical.rs` + `markov.rs` + `kmeans.rs`。

---

## 2. L2 布局层（cg-layout）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 2.1 | `LayoutEngine` trait + 布局注册表 | `[cy→模式]` | P0 | ✅ 已落地（`engine.rs`/`registry.rs`），注册 9 种 |
| 2.2 | 布局桥接契约（产坐标→写回→触发重绘） | `[cy→模式]` | P0 | ✅ 已落地（`driver.rs` + `reaction.rs`） |
| 2.3 | Preset 布局（使用给定坐标） | `[cy→移植]` | P0 | ✅ 已落地（`preset.rs`） |
| 2.4 | Random 布局 | `[cy→移植]` | P0 | ✅ 已落地（`random.rs`） |
| 2.5 | Force-directed（CoSE/FR 物理：斥力/引力/温度退火） | `[cy→移植]` | P1 | ✅ 已落地（`force.rs`） |
| 2.6 | Grid 布局 | `[cy→移植]` | P1 | ✅ 已落地（`grid.rs`） |
| 2.7 | Circle 布局 | `[cy→移植]` | P1 | ✅ 已落地（`circle.rs`） |
| 2.8 | Breadth-first 布局 | `[cy→移植]` | P1 | ✅ 已落地（`breadthfirst.rs`） |
| 2.9 | Concentric 布局 | `[cy→移植]` | P2 | ✅ 已落地（`concentric.rs`） |
| 2.10 | Radial（按分值定半径，默认度数，可外灌 PageRank 等分值表） | `[自研]` | P2 | ✅ 已落地（`radial.rs`，孤立节点放最外环） |
| 2.11 | Hierarchical（DAG 分层） | `[petgraph` 辅助`+自研]` | P2 | ✅ 已落地（`hierarchical.rs`） |
| 2.12 | 增量布局（固定未变节点、局部更新） | `[自研]` | P1 | ✅ 已落地（`driver.rs` 的 `pin`/`move_pinned`/`request_refine`） |
| 2.13 | 后台线程布局 + 分批回写（动画式收敛） | `[gpui]` | P1 | ✅ 已落地（`LayoutProgress` + `SYNC_LAYOUT_NODE_LIMIT`） |

---

## 3. L3 渲染层（cg-render）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 3.1 | 路线 A：`gpui::canvas` 即时绘制 `GraphView` | `[gpui]` | P0 | `elements/canvas.rs:10`；节点 `paint_quad`（`window.rs:4502`） |
| 3.2 | 边绘制（直线/折线/贝塞尔） | `[cy→移植]+[gpui]` | P1 | 几何借 `edge-control-points.mjs`（`findStraightEdgePoints:251`、`findBezierPoints:257`、`findTaxiPoints:309`、`findLoopPoints:162`），落为 `Path`（`scene.rs:840/847/859`） |
| 3.3 | 箭头形状族（三角/燕尾/丁形/圆点/菱形） | `[cy→移植]+[gpui]` | P1 | ✅ 已落地（`arrows.rs` 的多边形顶点表 + `view.rs` 统一入口） |
| 3.4 | 节点形状族（方/圆/椭圆/圆角矩形/三角/菱形） | `[cy→移植]+[gpui]` | P1/P2 | ✅ 已落地（`shapes.rs` 顶点表，绘制、点选、框选、可见查询共用，默认方形） |
| 3.5 | 自环边 | `[cy→移植]` | P2 | ✅ 已落地（`curves.rs` 的 `self_loop_*`） |
| 3.6 | Haystack 边聚合（大图简化边） | `[cy→移植]` | P3 | ✅ 已落地（`aggregation.rs` + `view.rs` 的 `bundle_slot`） |
| 3.7 | 相机 `Camera{offset,zoom}`（平移/缩放） | `[cy→模式]+[gpui]` | P1 | ✅ 已落地（`camera.rs`，含光标锚点缩放） |
| 3.8 | 空间索引（四叉树/均匀网格）+ 点选命中 | `[cy→移植]` | P1 | ✅ 已落地（`spatial.rs` 均匀网格 + `picking.rs`） |
| 3.9 | 框选命中 | `[cy→移植]` | P2 | ✅ 已落地（`picking.rs` 的 `polygon_intersects_rect`/`polyline_intersects_rect` + `shapes.rs` 的 `shape_hits_rect`，与点选同口径） |
| 3.10 | 节点/边标签 | `[自研]`（gpui 文本系统） | P1/P2 | ✅ 已落地：节点标签（`text.rs` + `graph_view` 内文本整形）与边标签（沿边中点锚定，文本取边权重，`Minimal` 隐藏）；多行换行与中文硬换行已落地 |
| 3.11 | 样式属性层（颜色/描边/宽度/透明度…） | `[cy→模式]` | P2 | ✅ 已落地（`style.rs`，常用子集 + mapper/谓词；描边与透明度已接入绘制与软件光栅化） |
| 3.12 | 选中/高亮覆盖（bypass 式 override） | `[cy→模式]` | P2 | ✅ 已落地（`style.rs` 的 `BypassStore`） |
| 3.13 | 绘制顺序编排（先边后节点、标签最上、z-order） | `[cy→模式]` | P1 | ✅ 已落地（`graph_view`：边→箭头→节点→标签→框选） |
| 3.14 | 路线 B：自定义 `Element` + 保留场景（增量 flush） | `[gpui]` | P3 | ✅ 已落地（`retained.rs` 的 `RetainedCache`，三维版本 + 选项与相机快照组成完整复用键） |
| 3.15 | LOD（缩放阈值后降级为点/线） | `[自研]` | P3 | ✅ 已落地（`lod.rs` 三级 + 滞回） |
| 3.16 | 图片导出（PNG） | `[自研]`（思路参考 cytoscape `export-image.mjs`） | P3 | ✅ 已落地：软件光栅化 + 自研 PNG 编码（`export.rs` 的 `encode_png`，无图片依赖）；视口/全图两档，仅覆盖节点与边几何，标签仅画布绘制 |
| 3.17 | 路线 C：wgpu 实例化（十万级） | `[gpui_wgpu]` | 按需 | ❌ 未实现（评估项） |

---

## 4. L3 交互层（cg-interact）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 4.1 | 画布平移（空白处拖拽） | `[自研]+[gpui]` | P1 | ✅ 已落地（`input.rs` 的 `PanState`） |
| 4.2 | 滚轮/捏合缩放（光标锚点） | `[自研]` | P1 | ✅ 已落地（`handlers.rs` 的 `wheel_zoom_factor`） |
| 4.3 | 节点拖拽（fixed + 局部重布局） | `[自研]` | P1 | ✅ 已落地（`DragState` + `drag_position`） |
| 4.4 | 点选 / 多选 | `[cy→移植]` | P1 | ✅ 已落地（`SelectionState`） |
| 4.5 | 框选 | `[cy→移植]` | P2 | ✅ 已落地（`BoxSelectState` + `nodes_in_rect`/`edges_in_rect`，节点框选按形状判定；可见查询两遍制，最简档用方形集） |
| 4.6 | 高亮/悬停效果 | `[cy→模式]` | P2 | ✅ 已落地（`hover_node_shaped` + `BypassStore`） |
| 4.7 | 悬停 tooltip（popover） | `[gpui]` | P2 | ✅ 已落地（`main.rs` 的 `hover_label`） |
| 4.8 | 指针事件 → 相机逆变换 → 命中查询 事件桥 | `[自研]` | P1 | ✅ 已落地（`main.rs` 指针事件接线） |

---

## 5. 应用壳（compograph 主应用）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 5.1 | 窗口/画布容器 `GraphView` | `[gpui]` | P0 | ✅ 已落地（`main.rs` 的 `GraphWindow`） |
| 5.2 | 布局切换 UI（下拉/命令面板） | `[自研]` | P1 | ✅ 已落地（`switch_layout` + 顶栏菜单） |
| 5.3 | 算法执行面板（选算法→后台跑→结果高亮/面板展示） | `[自研]` | P2 | ✅ 已落地（21 类算法 + `algo_panel.rs` + 代次守卫，全部算法桥与 P3 自研算法均可跑可看） |
| 5.4 | 图导入/导出入口（文件对话框） | `[自研]` | P2 | ✅ 已落地：JSON 进出、DOT 进出、图片导出（PNG） |
| 5.5 | 状态栏（节点/边计数、缩放比、布局耗时） | `[自研]` | P2 | ✅ 已落地（含 LOD/帧耗时/索引耗时） |

---

## 6. 远期自研项（cytoscape 独有算法，v1 不做）

> 中心性三件套（度/接近/介数）已按 P1 方案自研落地（无权版本，见 `centrality.rs`），不在本表；其余决策见 [P3 设计决策](../plan/compograph-p3-plan.md)。

| 功能 | 说明 |
|---|---|
| 聚类套件（MCL、k-means/k-medoids、层次聚类、亲和传播） | petgraph 无任何聚类，远期按需单点启动（P3-1）；已落地层次单连接阈值、马尔可夫确定性归属、确定性 k 均值（`cg-graph/src/hierarchical.rs` + `markov.rs` + `kmeans.rs`），亲和传播暂不纳入，见分阶段方案 |
| Euler 路径（Hierholzer）、最小割 | ✅ 已落地：`cg-graph/src/euler.rs`（有向与无向双语义）+ `cg-graph/src/min_cut.rs`（Stoer-Wagner 确定性全局最小割），petgraph 0.8.3 无对应故自研（P3-2） |
| 复合节点 | 跨三层改造，拍板前不动（P3-3） |
| 扩展机制 | 需求出现时再设计，不预做框架（P3-4） |
| 布局动画链（含 Radial 插值） | 径向首版不做插值，插值并入本项（P3-5） |
| 实例化后端（十万级） | 先评估后决定，结论为否则不启动（P3-6） |
| 完整样式 schema（cytoscape ~208 属性） | v1 仅做常用子集（3.11，描边与透明度已接线、不新增字段），扩展见 P2 方案 |

---

## 7. 阶段汇总

> 实测进度（2026-09-30）：P0/P1/P2 项基本全部落地，P3 仅剩路线 C 未实现，PNG 导出已落地。下表保留原计划口径，"已落地"以各表状态列为准。

| 阶段 | 目标 | 覆盖功能项 |
|---|---|---|
| **P0** | "petgraph 图 → gpui 画布"闭环 | 1.1–1.4、2.1–2.4、3.1、5.1 |
| **P1** | 可交互探索小图 | 1.5、2.5–2.8、2.12–2.13、3.2–3.4、3.7–3.8、3.10、3.13、4.1–4.4、4.8、5.2 |
| **P2** | 接近 cytoscape 可用度 | 1.6–1.7、2.9–2.11、3.5、3.9、3.11–3.12、4.5–4.7、5.3–5.5、算法桥接（最短路/SCC/PageRank/MST） |
| **P3** | 大规模 | 3.6、3.14–3.16、远期算法按需；路线 C 视规模评估（3.17） |

### 7.1 实测缺口汇总（对照 [分析报告](../plan/feature-analysis-report.md)，P1 状态见 [P1 方案](../plan/compograph-p1-plan.md)，远期归属见 [P2 方案](../plan/compograph-p2-plan.md)，远期决策见 [P3 方案](../plan/compograph-p3-plan.md)）

- **P1 五项**：算法桥、中心性三件套、Radial 布局、节点形状族、箭头形状族均已落地，见上表；面板已接全部算法桥（P2-2）。
- **P2 六项**：边标签（P2-1，权重文本沿边中点）、多行标签与中文换行（P2-3）、出租车与分段曲线（P2-4，`taxi_polyline`/`segmented_polyline`/`manhattan_route`，绘制与框选共用采样；版本一为几何能力，产品默认直边、不暴露开关）、样式描边与透明度接线（P2-5，取描边与透明度子集、不新增字段）、PNG 导出（P2-6，自研编码、无图片依赖，仅覆盖节点与边几何）均已落地。
- **3.17 路线 C**：未实现（P3-6，需先拍板规模）。
- **§6 远期算法与机制**：Euler 与最小割已落地（P3-2）；聚类套件、复合节点、扩展机制、动画链均未做（P3-1、P3-3–P3-5，聚类分析见 `docs/analysis/clustering-analysis.md`）。
