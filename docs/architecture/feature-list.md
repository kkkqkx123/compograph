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
| 1.6 | 图 I/O（JSON/GraphML 导入导出） | `[自研]` | P2 | 参考 cytoscape JSON 元素格式；serde 可选 |
| 1.7 | DOT 导出/导入 | `[petgraph]` | P2 | `src/dot/`（导入需 `dot_parser` feature） |

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

> petgraph 缺失、cytoscape 独有的"网络科学"算法（度/接近/介数中心性、MCL、k-means、层次聚类、亲和传播、Euler 路径、Karger-Stein 最小割）**不在 v1 范围**，列为远期自研项（见 §6）。

---

## 2. L2 布局层（cg-layout）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 2.1 | `LayoutEngine` trait + 布局注册表 | `[cy→模式]` | P0 | 借鉴 `extensions/layout/index.mjs` 的 name→impl 注册表 |
| 2.2 | 布局桥接契约（产坐标→写回→触发重绘） | `[cy→模式]` | P0 | 借鉴 `collection/layout.mjs:41 layoutPositions` |
| 2.3 | Preset 布局（使用给定坐标） | `[cy→移植]` | P0 | `preset.mjs:25`，平凡实现，P0 画静态图即用 |
| 2.4 | Random 布局 | `[cy→移植]` | P0 | `random.mjs:21` |
| 2.5 | Force-directed（CoSE/FR 物理：斥力/引力/温度退火） | `[cy→移植]` | P1 | `cose.mjs`（`run:126`、物理步 `step:705`），自包含可直译；d3-force 风格替代为开放点 OQ-B1 |
| 2.6 | Grid 布局 | `[cy→移植]` | P1 | `grid.mjs:30` |
| 2.7 | Circle 布局 | `[cy→移植]` | P1 | `circle.mjs:31` |
| 2.8 | Breadth-first 布局 | `[cy→移植]` | P1 | `breadthfirst.mjs:42` |
| 2.9 | Concentric 布局 | `[cy→移植]` | P2 | `concentric.mjs:37` |
| 2.10 | Radial（按中心性定半径，petgraph `page_rank` 辅助排序） | `[自研]` | P2 | petgraph `page_rank.rs:64` 作子步骤 |
| 2.11 | Hierarchical（DAG 分层） | `[petgraph` 辅助`+自研]` | P2 | toposort/`tred`/`dominators` 做分层，坐标分配自研 |
| 2.12 | 增量布局（固定未变节点、局部更新） | `[自研]` | P1 | 避免整图抖动 |
| 2.13 | 后台线程布局 + 分批回写（动画式收敛） | `[gpui]` | P1 | `App::spawn`（`app.rs:2039`）/ `background_executor`（`app.rs:300`） |

---

## 3. L3 渲染层（cg-render）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 3.1 | 路线 A：`gpui::canvas` 即时绘制 `GraphView` | `[gpui]` | P0 | `elements/canvas.rs:10`；节点 `paint_quad`（`window.rs:4502`） |
| 3.2 | 边绘制（直线/折线/贝塞尔） | `[cy→移植]+[gpui]` | P1 | 几何借 `edge-control-points.mjs`（`findStraightEdgePoints:251`、`findBezierPoints:257`、`findTaxiPoints:309`、`findLoopPoints:162`），落为 `Path`（`scene.rs:840/847/859`） |
| 3.3 | 箭头（三角几何） | `[cy→移植]+[gpui]` | P1 | 借 `edge-arrows.mjs` 方位/位移 + `arrow-shapes.mjs:142` 顶点表；`Path::push_triangle`（`scene.rs:876`） |
| 3.4 | 节点形状（圆/椭圆/圆角矩形起步，多边形族后续） | `[cy→移植]+[gpui]` | P1/P2 | 借 `node-shapes.mjs` 顶点生成（`generateEllipse:43`、`generateRoundRectangle:143`、`generatePolygon:6`），绘制改 `Path`/`Quad` |
| 3.5 | 自环边 | `[cy→移植]` | P2 | `findLoopPoints`（`edge-control-points.mjs:162`） |
| 3.6 | Haystack 边聚合（大图简化边） | `[cy→移植]` | P3 | `findHaystackPoints`（`:64`） |
| 3.7 | 相机 `Camera{offset,zoom}`（平移/缩放） | `[cy→模式]+[gpui]` | P1 | 公式思路借 `math.mjs:9/14`；实现用 CPU 仿射或 `TransformationMatrix`（`scene.rs:608`） |
| 3.8 | 空间索引（四叉树/均匀网格）+ 点选命中 | `[cy→移植]` | P1 | 命中数学直译 `coords.mjs`（`findNearestElement:75`、`checkNode:131`、`checkEdge:157`） |
| 3.9 | 框选命中 | `[cy→移植]` | P2 | `getAllInBox`（`coords.mjs:323`）、`doLinesIntersect`（`:413`） |
| 3.10 | 节点/边标签 | `[自研]`（gpui 文本系统） | P1/P2 | 中文换行/锚点策略为开放点 OQ-5 |
| 3.11 | 样式属性层（颜色/描边/宽度/透明度…） | `[cy→模式]` | P2 | 借 `style/properties.mjs` schema 思想 + mapper/trigger；落点重映射为 `PaintQuad`/`Path` 字段 |
| 3.12 | 选中/高亮覆盖（bypass 式 override） | `[cy→模式]` | P2 | 借 `style/bypass.mjs` 概念 |
| 3.13 | 绘制顺序编排（先边后节点、标签最上、z-order） | `[cy→模式]` | P1 | 借 `drawing-nodes.mjs:11`、`drawing-edges.mjs:8`、`z-ordering.mjs` 的编排结构 |
| 3.14 | 路线 B：自定义 `Element` + 保留场景（增量 flush） | `[gpui]` | P3 | `Element` trait（`element.rs:53`） |
| 3.15 | LOD（缩放阈值后降级为点/线） | `[自研]` | P3 | — |
| 3.16 | 图片导出（PNG） | `[自研]`（思路参考 cytoscape `export-image.mjs`） | P3 | wgpu 截图/离屏渲染 |
| 3.17 | 路线 C：wgpu 实例化（十万级） | `[gpui_wgpu]` | 按需 | fork 的 `WgpuContext` 公开 `device/queue`（`gpui_wgpu/src/wgpu_context.rs:13-14`），可行性高于原判 |

---

## 4. L3 交互层（cg-interact）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 4.1 | 画布平移（空白处拖拽） | `[自研]+[gpui]` | P1 | 改 `Camera.offset` |
| 4.2 | 滚轮/捏合缩放（光标锚点） | `[自研]` | P1 | — |
| 4.3 | 节点拖拽（fixed + 局部重布局） | `[自研]` | P1 | 与 2.12 联动 |
| 4.4 | 点选 / 多选 | `[cy→移植]` | P1 | 命中经空间索引（3.8） |
| 4.5 | 框选 | `[cy→移植]` | P2 | 命中经 3.9 |
| 4.6 | 高亮/悬停效果 | `[cy→模式]` | P2 | bypass 式覆盖（3.12） |
| 4.7 | 悬停 tooltip（popover） | `[gpui]` | P2 | 复用 gpui 组件 |
| 4.8 | 指针事件 → 相机逆变换 → 命中查询 事件桥 | `[自研]` | P1 | cytoscape 的 DOM 事件桥不可借，需按 gpui 指针事件重写 |

---

## 5. 应用壳（compograph 主应用）

| # | 功能 | 来源 | 阶段 | 说明 |
|---|---|---|---|---|
| 5.1 | 窗口/画布容器 `GraphView` | `[gpui]` | P0 | — |
| 5.2 | 布局切换 UI（下拉/命令面板） | `[自研]` | P1 | 对接布局注册表（2.1） |
| 5.3 | 算法执行面板（选算法→后台跑→结果高亮/面板展示） | `[自研]` | P2 | 大图算法走 `App::spawn` |
| 5.4 | 图导入/导出入口（文件对话框） | `[自研]` | P2 | 对接 1.6/1.7 |
| 5.5 | 状态栏（节点/边计数、缩放比、布局耗时） | `[自研]` | P2 | — |

---

## 6. 远期自研项（cytoscape 独有算法，v1 不做）

| 功能 | 说明 |
|---|---|
| 度/接近/介数中心性 | petgraph 仅 PageRank，其余需自研（`algo/` 无对应） |
| 聚类套件（MCL、k-means/k-medoids、层次聚类、亲和传播） | petgraph 无任何聚类 |
| Euler 路径（Hierholzer）、最小割（Karger-Stein） | petgraph 无对应 |
| 完整样式 schema（cytoscape ~208 属性） | v1 仅做常用子集（3.11） |

---

## 7. 阶段汇总

| 阶段 | 目标 | 覆盖功能项 |
|---|---|---|
| **P0** | "petgraph 图 → gpui 画布"闭环 | 1.1–1.4、2.1–2.4、3.1、5.1 |
| **P1** | 可交互探索小图 | 1.5、2.5–2.8、2.12–2.13、3.2–3.4、3.7–3.8、3.10、3.13、4.1–4.4、4.8、5.2 |
| **P2** | 接近 cytoscape 可用度 | 1.6–1.7、2.9–2.11、3.5、3.9、3.11–3.12、4.5–4.7、5.3–5.5、算法桥接（最短路/SCC/PageRank/MST） |
| **P3** | 大规模 | 3.6、3.14–3.16、远期算法按需；路线 C 视规模评估（3.17） |
