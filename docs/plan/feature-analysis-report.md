# compograph 功能实现分析与补充清单

> 版本：v1.1 · 日期：2026-09-29
> P0 落地补记：v1.0 所列三项 P0（节点标签文本渲染、DOT 导入、文档回改）已按 [P0 阶段方案](./compograph-p0-plan.md) 落地，下表相关行已翻为已实现；正文历史结论保留不变。P1 细化方案见 [P1 阶段方案](./compograph-p1-plan.md)。
> 目的：以**代码实测**为准，盘点 compograph 当前已实现的功能，对照 cytoscape.js 识别待补充项，给出优先级与落点建议。
> 事实基线（本地克隆已核验）：
>
> | 项目 | 位置 | HEAD | 说明 |
> |---|---|---|---|
> | compograph | `/workspace/compograph` | 本次克隆 | 分析对象 |
> | zed-gpui（submodule） | `crates/vendor/zed-gpui` | `fbb3992`（lean 分支） | 工具链 1.98.1，编译/测试已实测通过 |
> | cytoscape.js | `/workspace/cytoscape.js` | `7ba6340`（3.35.0-unstable） | 对照参照系 |
>
> 验证方式：`cargo check --workspace --all-targets` 通过；`cargo test`（6 个自建 crate）**177 个测试全绿**（cg-types 5 / cg-geometry 22 / cg-graph 35 / cg-interact 15 / cg-layout 42 / cg-render 47，另含 2 个集成测试文件共 11 例）。

---

## 0. 文档信息

本文档为"分析报告"性质，不改动代码。用语遵循 `AGENTS.md`：文档用中文、代码注释不引用文档编号。所有功能声明均标注 `file:line` 或目录路径，可回溯核验。方法与既有 [功能清单](../architecture/feature-list.md)（计划口径）互补：**本文以实际代码为准**，区分"文档计划"与"已落地"。

---

## 1. 环境准备（本次实测）

按用户要求，在受限网络下完成克隆与构建前置配置：

- 克隆代理：`git clone https://gh-proxy.com/https://github.com/...`（compograph、cytoscape.js 均如此拉取）。
- cargo 镜像：写入 `~/.cargo/config.toml`，`crates-io` 替换为 `rsproxy-sparse`（`sparse+https://rsproxy.cn/index/`），并设 `net.git-fetch-with-cli = true`。
- Rust 工具链：`rust-toolchain.toml:2` 钉定 `1.98.1`。本机默认 `stable` 无法满足，通过 `RUSTUP_DIST_SERVER=https://rsproxy.cn` 安装 `1.98.1` 成功（`rustup toolchain list` 已含）。
- GitHub 代理：`git config --global url."https://gh-proxy.com/https://github.com/".insteadOf "https://github.com/"`。此步为**必需**——gpui 的 `Cargo.toml` 依赖 git 源 `proptest-rs/proptest`（rev `3dca198a`），无代理会 `gnutls_handshake failed`。
- submodule：`git submodule update --init --depth 1` 拉取 `crates/vendor/zed-gpui`（lean 分支快照，已展开工作区继承）。

结论：跑通构建需要**三件事同时到位**——rsproxy（cargo 索引）、gh-proxy（git/工具链）、1.98.1 工具链。缺一即失败，[实施方案 §2.1](../plan/compograph-implementation-plan.md) 已有预告，本次复现一致。

---

## 2. 项目架构实际形态

分层与 `AGENTS.md` 描述一致，依赖 DAG 无环：

```
foundation:  cg-types ← cg-geometry
             cg-types ← cg-graph（+ petgraph + gpui EventEmitter）
engine:      cg-graph + cg-types ← cg-layout（+ gpui Context）
render:      cg-graph + cg-layout + cg-geometry + cg-types ← cg-render ← cg-interact
app:         以上全部 ← compograph（+ gpui_platform）
upstream:    crates/vendor/zed-gpui（27 包，禁止反向依赖 cg-*）
```

自建源码规模（`find crates -name "*.rs" -not -path "*/vendor/*"`）：约 **12207 行**，分布在 6 个库 crate + 1 个二进制 crate。`petgraph` 仅被 `cg-graph` 直接依赖（`Cargo.toml:48`），其余经 re-export 的 `NodeIndex`/`EdgeIndex` 访问。

---

## 3. 已实现功能盘点（按层）

> 判定标准：存在可编译、带单测的实现，并在 `lib.rs` 导出。仅"预留字段/参数"但无实际效果的，不计入已实现（单列于 §4）。

### 3.1 L1 数据/算法层（cg-graph）

| # | 功能 | 代码位置 | 状态 |
|---|---|---|---|
| 1.1 | 可编辑图容器（增删节点/边、清空） | `store.rs:120/129/138/151/164` | ✅ 已实现，`StableGraph` 索引稳定 |
| 1.2 | 节点/边业务数据（label / weight） | `store.rs:17/29` | ✅ 已实现 |
| 1.3 | 共享状态 + 变更事件广播 | `store.rs:53`（`GraphStore` + `Context`）、`events.rs:12`（`GraphChangeEvent`） | ✅ 已实现 |
| 1.4 | 订阅过滤（ChangeFilter） | `binding.rs:16/48/63` | ✅ 已实现 |
| 1.5 | 坐标外挂（Positions + FixedNodes） | `positions.rs`（`Positions`/`FixedNodes`） | ✅ 已实现 |
| 1.6 | 只读视图 trait（解耦 L2/L3） | `view.rs:13`（`GraphView`）+ `view.rs:44`（`MockGraph`） | ✅ 已实现，含 headless mock |
| 1.7 | 算法桥（薄封装 petgraph） | `algo.rs` | ✅ 已实现：`shortest_paths`(:30)、`shortest_path_cost`(:37)、**`shortest_path` A\*(:54)**、**`heuristic_shortest_path`(:73)**、**`strongly_connected_components` tarjan(:46)**、**`minimum_spanning_forest` Kruskal(:102)**、**`minimum_spanning_tree_single` Prim(:124)**、**`topological_order`(:144)**、**`immediate_dominators`(:151)**、**`rank_nodes` page_rank(:167)**、**`transitive_reduction`(:177)** |
| 1.8 | JSON 导入导出 | `io.rs:167/173`（`export_json`/`import_json`，`json-io` feature） | ✅ 已实现 |
| 1.9 | DOT 导出 | `io.rs:180`（`export_dot`） | ✅ 已实现 |
| 1.10 | DOT 导入 | `io.rs:190`（`import_dot` + 自写 `DotParser`） | ✅ 已实现：覆盖常用子集并复用 `validate`，往返语义测试已补 |

### 3.2 L2 布局层（cg-layout）

| # | 功能 | 代码位置 | 状态 |
|---|---|---|---|
| 2.1 | `LayoutEngine` trait | `engine.rs:16` | ✅ 已实现 |
| 2.2 | 布局注册表（name→impl） | `registry.rs:22/43/65/87` | ✅ 已实现，**8 种**注册 |
| 2.3 | Preset 布局 | `preset.rs` | ✅ 已实现 |
| 2.4 | Random 布局 | `random.rs` | ✅ 已实现 |
| 2.5 | **Force 力导向**（斥力/引力/温度退火） | `force.rs`（542 行，`ForceSimulation`/`ForceSnapshot`） | ✅ 已实现 |
| 2.6 | Grid 布局 | `grid.rs`（304 行） | ✅ 已实现 |
| 2.7 | Circle 布局 | `circle.rs`（245 行） | ✅ 已实现 |
| 2.8 | Breadth-first 布局 | `breadthfirst.rs`（497 行，含 `BfsDirection`） | ✅ 已实现 |
| 2.9 | Concentric 布局 | `concentric.rs`（328 行，含 `ConcentricScoring`） | ✅ 已实现 |
| 2.10 | Hierarchical（DAG 分层） | `hierarchical.rs`（493 行） | ✅ 已实现 |
| 2.11 | 增量布局 + 变更驱动 | `driver.rs:42`（`LayoutDriver`）、`reaction.rs`（`work_for`，`LayoutWork`） | ✅ 已实现，含 `cancel`/`pin`/`move_pinned` |
| 2.12 | 后台/分批布局 | `driver.rs:223/305`（`request_refine`/`react`）、`SYNC_LAYOUT_NODE_LIMIT` | ✅ 已实现（`App::spawn` 路径） |
| 2.13 | **Radial 布局**（按中心性定半径） | — | ❌ 未实现（文档 [feature-list 2.10](../architecture/feature-list.md) 列为 P2） |

### 3.3 L3 渲染层（cg-render）

| # | 功能 | 代码位置 | 状态 |
|---|---|---|---|
| 3.1 | canvas 即时绘制 `GraphView` | `view.rs:831`（`graph_view`） | ✅ 已实现 |
| 3.2 | 边绘制（直线/折线/贝塞尔） | `view.rs:507`（`paint_single_edge`）、`curves.rs` | ✅ 已实现，含二次/三次贝塞尔采样 |
| 3.3 | 箭头（三角几何） | `view.rs:691/701/708/739`（`arrow_triangle`） | ✅ 已实现，按 `DetailLevel` 分级 |
| 3.4 | 节点形状 | `view.rs:238`（`paint_single_node`，`NODE_SIDE=24`） | ⚠️ **部分**：仅方形/块状，无形状族（圆/椭圆/多边形/圆角） |
| 3.5 | 自环边 | `curves.rs:63/105`（`self_loop_controls`/`self_loop_polyline`）、`view.rs:488`（`loop_ordinal`） | ✅ 已实现 |
| 3.6 | Haystack 边聚合 | `aggregation.rs:41/55/88/119`、`view.rs:31`（`EDGE_AGGREGATION_THRESHOLD`）、`view.rs:468`（`bundle_slot`） | ✅ 已实现 |
| 3.7 | Camera 平移/缩放（光标锚点） | `camera.rs`（`world_to_viewport`/`viewport_to_world`/`zoom_at`） | ✅ 已实现 |
| 3.8 | 空间索引（均匀网格） | `spatial.rs:17`（`SpatialIndex`，`rebuild`/`query`） | ✅ 已实现，自适应格子 `adaptive_cell` |
| 3.9 | 点选/框选命中几何 | `picking.rs`（`point_hits_node`/`edge_hit`/`bezier_hit`/`polyline_intersects_rect`） | ✅ 已实现 |
| 3.10 | 样式属性层 + mapper | `style.rs`（`NodeStyle`/`EdgeStyle`/`StyleMapper`/`EdgeMapper`/`StyleSheet`/`NodePredicate`/`EdgePredicate`） | ✅ 已实现（子集） |
| 3.11 | 选中/高亮 bypass 覆盖 | `style.rs:332`（`BypassStore`，`set/clear_node/edge`） | ✅ 已实现 |
| 3.12 | 绘制顺序编排 | `view.rs`（先边后节点、箭头分层） | ✅ 已实现 |
| 3.13 | 保留模式（路线 B，增量 flush） | `retained.rs`（`RetainedCache`/`CacheVersions`/`CameraSnapshot`） | ✅ 已实现 |
| 3.14 | LOD（三级降级 + 滞回） | `lod.rs`（`DetailLevel` Full/Simplified/Minimal、`LodParams`） | ✅ 已实现 |
| 3.15 | 帧性能度量 | `metrics.rs`（`FrameMetrics`/`FrameSample`/`PlanCounts`/`measure_ms`） | ✅ 已实现 |
| 3.16 | 图片导出（软件光栅化 PPM） | `export.rs`（`ExportScope`/`ExportRequest`/`rasterize`/`encode_ppm`） | ✅ 已实现（PPM 非 PNG） |
| 3.17 | 路径 C：wgpu 实例化 | — | ❌ 未实现（评估项） |
| 3.18 | **节点标签文本渲染** | `text.rs`（`PaintedLabel`/`paint_labels_for`）+ `view.rs`（`graph_view` 第 5 参） | ✅ 已实现：单行居中，`Minimal` 隐藏，不进保留模式缓存；边标签仍缺 |

### 3.4 L3 交互层（cg-interact）

| # | 功能 | 代码位置 | 状态 |
|---|---|---|---|
| 4.1 | 画布平移 | `input.rs:39`（`PanState`） | ✅ 已实现 |
| 4.2 | 滚轮缩放 | `handlers.rs:204`（`wheel_zoom_factor`）+ `camera.zoom_at` | ✅ 已实现 |
| 4.3 | 节点拖拽 | `input.rs:19/24/28`（`DragState`/`DragGesture`）、`handlers.rs:52`（`drag_position`） | ✅ 已实现 |
| 4.4 | 点选 / 多选 | `input.rs:113`（`SelectionState`） | ✅ 已实现 |
| 4.5 | 框选 | `input.rs:66`（`BoxSelectState`）、`handlers.rs:70/123`（`nodes_in_rect`/`edges_in_rect`） | ✅ 已实现 |
| 4.6 | 悬停高亮 | `handlers.rs:191`（`hover_node`）+ `BypassStore` | ✅ 已实现 |
| 4.7 | 悬停 tooltip（popover） | `main.rs:319`（`hover_label`） | ✅ 已实现；3.18 落地后画布标签与 tooltip 双通道齐备 |
| 4.8 | 指针事件桥 | `main.rs`（`MouseDownEvent`/`MouseMoveEvent`/`MouseUpEvent`/`ScrollWheelEvent` 接线） | ✅ 已实现 |

### 3.5 应用壳（compograph）

| # | 功能 | 代码位置 | 状态 |
|---|---|---|---|
| 5.1 | 窗口/画布容器 | `main.rs:73`（`GraphWindow`）、`Render`(:847) | ✅ 已实现 |
| 5.2 | 布局切换 UI | `main.rs:211`（`switch_layout`）+ 顶栏菜单 | ✅ 已实现 |
| 5.3 | 算法执行面板 | `main.rs:420/441/463/474/487` + `algo_panel.rs`（outcome 映射） | ✅ 已实现：最短路 / 引导搜索 / 连通分量 / PageRank / 生成树 5 类 |
| 5.4 | 图导入/导出入口 | `main.rs:498/537/599`、`file_io.rs`（对话框 + `/tmp` 兜底） | ✅ 已实现：JSON 进出、DOT 导出 |
| 5.5 | 状态栏（计数/缩放/LOD/耗时） | `main.rs`（lod 标签、`FrameMetrics`） | ✅ 已实现 |
| 5.6 | 图片导出入口 | `main.rs:681`（`export_image`），视口/全图两档 | ✅ 已实现（PPM） |
| 5.7 | 后台算法任务 + 代次守卫 | `main.rs:364/374/397`（`begin_algo_run`/`commit_outcome`/`spawn_algo_task`） | ✅ 已实现（防旧结果覆盖） |
| 5.8 | 保留模式/聚合开关 | `main.rs`（`retained:on/off`、`aggregate:on/off`） | ✅ 已实现 |

---

## 4. 文档与代码的偏差（重要）

文档 [feature-list.md](../architecture/feature-list.md) 与 [实施方案](../plan/compograph-implementation-plan.md) 停留在 P0/P1 口径，**代码已远超文档**。以下为按 `AGENTS.md` "文档同步"要求需回改的过期点：

| 文档声明 | 代码实测 | 建议 |
|---|---|---|
| feature-list 标 Grid/Circle/BFS/Concentric 为 P1/P2"待做" | 均**已实现**（`grid.rs`/`circle.rs`/`breadthfirst.rs`/`concentric.rs`） | 更新阶段标记为"已落地" |
| 实施方案 §3 称 render 缺"边/箭头、标签、LOD、保留模式" | 边/箭头/LOD/保留模式**均已实现**；仅标签真缺 | 更新差距表 |
| 实施方案 §3 称 engine 缺"力导向与布局族" | 力导向 + 6 种布局族**均已实现** | 更新 |
| feature-list 3.16 图片导出标 P3 | **已实现**（软件光栅化 PPM） | 更新，并注明为 PPM 而非 PNG |
| 各处称算法桥仅 4 个函数 | 实际 **11 个**（见 §3.1 #1.7） | 更新清单 |
| 实施方案称 interact 缺"框选、平移缩放" | **均已实现** | 更新 |

> 建议：另起一次"文档回改"提交，按上表逐条更新 `feature-list.md`、`architecture-design.md §9`、`compograph-implementation-plan.md §3/§5`，避免读者按旧口径误判进度。

---

## 5. 对照 cytoscape.js 的功能差距

对照基线：`/workspace/cytoscape.js`（`7ba6340`）。

### 5.1 布局族（cytoscape 内置 8 种）

cytoscape 注册表 `src/extensions/layout/index.mjs:11-18`：`breadthfirst / circle / concentric / cose / grid / null / preset / random`。

compograph 注册表 `registry.rs:43`：`force / random / preset / grid / circle / breadthfirst / concentric / hierarchical`。

> 差异：compograph 用 `force` 对应 cose（**已实现**）、`hierarchical` 为自研增项（cytoscape 无内置）；compograph 缺 `null`（占位布局，简单）与 `radial`（文档计划的 P2 项）。

### 5.2 算法族（cytoscape 内置 19 模块）

cytoscape `src/collection/algorithms/index.mjs:2-19` 共 19 个算法模块：`bfsDfs / dijkstra / kruskal / aStar / floydWarshall / bellmanFord / kargerStein / pageRank / degreeCentrality / closenessCentrality / betweennessCentrality / markovClustering / kClustering / hierarchicalClustering / affinityPropagation / hierholzer / hopcroftTarjanBiconnected / tarjanStronglyConnected`（+ index）。

compograph 已桥接 11 个（见 §3.1 #1.7），**未桥接**：

| 缺口 | 是否有 petgraph 对应 | 落点建议 |
|---|---|---|
| `floyd_warshall` 全源最短路 | ✅ petgraph 有 | 低工作量，直接加封装 |
| `bellman_ford` 负权最短路 | ✅ petgraph 有 | 低工作量 |
| `karger_stein` 最小割 | ❌ 0.8.3 注册表源码无此实现 | 远期自研 |
| `hopcroft_tarjan` 双连通分量 | ⚠️ 对应函数存在但仅适配无向图语义 | 自研无向深度优先实现（P1 落地） |
| `hierholzer` 欧拉路径 | ❌ 需自研 | 远期 |
| `degree/closeness/betweenness_centrality` 中心性 | ⚠️ 仅 page_rank | 需自研（介数最贵） |
| `markov/kmeans/hierarchical/affinity` 聚类套件 | ❌ 需自研 | 远期 |
| `bfs_dfs` 显式遍历 API | ✅ petgraph visit | 低工作量 |

### 5.3 渲染与样式

| 维度 | cytoscape.js | compograph | 差距 |
|---|---|---|---|
| 节点形状 | 20+ 种（`registerNodeShapes`：triangle/rectangle/round-*/polygon/barrel/cut-rectangle/…） | 仅方形块 | **大**，需形状族 |
| 箭头形状 | `arrow-shapes.mjs` 多族（triangle/vee/tee/circle/diamond/…） | 仅三角 | 中 |
| 样式属性 | ~194 个（`style/properties.mjs`） | 约 10+ 常用字段（`style.rs`） | **大**，长尾 |
| 文本/标签 | 完整（`drawing-label-text.mjs`，含换行/锚点/背景） | 节点单行居中标签已落地，边标签/多行/锚点仍缺 | 中，边标签列 P2 |
| 曲线类型 | straight/bezier/taxi/round-taxi/segments/haystack/loop | 直线+贝塞尔+自环+haystack | 中（缺 taxi/segments） |
| 导出 | PNG/JPG（`export-image.mjs`） | PPM（软件光栅化） | 小（格式限制） |
| 渲染后端 | Canvas2D + WebGL | gpui canvas + 保留模式 | 架构不同，非差距 |

### 5.4 交互与生态

| 维度 | cytoscape.js | compograph | 差距 |
|---|---|---|---|
| 选择器 DSL | CSS 式 `src/selector/` | 类型化谓词（`NodePredicate`/`EdgePredicate`） | 设计取舍（AGENTS 已定不借 DSL） |
| 复合节点（compound） | `collection/compounds.mjs` | 无 | 中，需拍板 |
| 动画系统 | `animation.mjs` 独立 | 无（依赖 gpui 帧） | 中 |
| 事件系统 | 完整 DOM 事件模型 | gpui 指针事件 | 架构不同 |
| 扩展机制 | 四类扩展点 `extension.mjs` | 仅 `LayoutEngine` trait | 中 |
| 布局动画（animate 选项） | ✅ | 部分（分批 refine） | 小 |
| 编辑操作（增删 UI） | API 完备 | 有 API，UI 操作入口少 | 小 |

---

## 6. 待补充功能清单（建议优先级）

> 综合"价值 / 工作量 / 依赖"排序。P0 = 影响可用度与验证，P1 = 对齐 cytoscape 主要能力，P2 = 长尾。

| 优先级 | 功能 | 落点 | 说明 |
|---|---|---|---|
| ✅ **P0** | 节点标签文本渲染 | `cg-render/src/text.rs`（新建）+ `view.rs`（`graph_view` 接 gpui 文本系统） | 已落地；OQ-A1 按 gpui 文本系统拍板，边标签移 P2 |
| ✅ **P0** | **DOT 导入** | `cg-graph/src/io.rs`（自写轻量解析） | 已落地；OQ-A6 按自写解析拍板 |
| ✅ **P0** | **文档回改** | `docs/architecture`、`docs/plan` | 已落地：`feature-list`、设计文档与实施方案进度口径已校准 |
| **P1** | 节点形状族（圆/椭圆/圆角矩形/多边形） | `cg-render`（新 `shapes.rs`） | 移植 cytoscape `node-shapes.mjs` 顶点生成；当前只有方块，视觉差距最直观 |
| **P1** | 算法桥补 4–5 个（floyd_warshall / bellman_ford / 割点桥 / 双连通 / 显式 BFS-DFS） | `cg-graph/src/algo.rs` | petgraph 现成，封装即得，性价比最高 |
| **P1** | 中心性三件套（degree / closeness / betweenness） | `cg-graph/src/algo.rs`（自研） | cytoscape 有；介数中心性较贵，需后台跑 |
| **P1** | Radial 布局 | `cg-layout`（新 `radial.rs`） | 文档计划内 P2 项，用 `rank_nodes` 作半径排序 |
| **P1** | 箭头形状族（vee / tee / circle / diamond） | `cg-render/src/view.rs` | 借 `arrow-shapes.mjs` 顶点表 |
| **P2** | 曲线类型补全（taxi / segments） | `cg-geometry/src/curves.rs` + `aggregation.rs`（已有 `ortho_polyline` 基础） | 对齐 cytoscape `curve-style` |
| **P2** | 样式属性长尾（border/opacity/text-* 等） | `cg-render/src/style.rs` | 从 ~194 属性取常用扩展 |
| **P2** | PNG 导出 | `cg-render/src/export.rs` | 现为 PPM，需加 PNG 编码（可引入 `png` crate） |
| **P2** | 聚类套件（MCL / k-means / 层次 / 亲和传播） | `cg-graph`（自研，新模块） | cytoscape 独有，petgraph 无对应 |
| **P2** | compound 复合节点 | `cg-graph` + `cg-layout` + `cg-render` | 跨三层，需先拍板是否纳入范围 |
| **P2** | 扩展机制（Renderer/Overlay trait） | `cg-render` | 对齐 cytoscape 四类扩展点思想 |
| **P3** | 布局动画（`animate` 选项链） | `cg-layout/src/driver.rs` | 现有分批 refine 可扩展 |
| **P3** | 路径 C（wgpu 实例化，十万级） | `cg-render`（新后端） | 需先拍板目标规模（OQ-3） |
| **P3** | Euler 路径 / Karger-Stein 最小割 | `cg-graph`（自研） | 远期 |

---

## 7. 待拍板开放点

| 编号 | 事项 | 影响 | 建议 |
|---|---|---|---|
| OQ-A1 | 标签文本方案（gpui 文本系统 vs 离屏纹理） | P0 标签渲染选型 | ✅ 已拍板：gpui 文本系统，中文换行单独验证 |
| OQ-A2 | 目标图规模（千/万/十万） | 是否启动路径 C | 需产品拍板 |
| OQ-A3 | 是否纳入 compound 复合节点 | 三层改造范围 | 影响架构，尽早定 |
| OQ-A4 | 样式属性覆盖面（v1 取多少） | 视觉保真度 vs 工作量 | 建议先补形状+标签，属性长尾按需 |
| OQ-A5 | 图片导出格式（PPM 是否够） | 交付可用性 | 建议加 PNG |
| OQ-A6 | DOT 导入实现方式（petgraph feature vs 自写） | 依赖体积 vs 可控性 | ✅ 已拍板：自写轻量解析 |

---

## 8. 验证结论

| 检查项 | 结果 |
|---|---|
| `cargo check --workspace --all-targets` | ✅ 通过（1m26s） |
| `cargo test`（6 crate） | ✅ **177 passed / 0 failed** |
| submodule 初始化 | ✅ `crates/vendor/zed-gpui` @ `fbb3992` |
| 受限网络三件套 | ✅ rsproxy + gh-proxy + 1.98.1 工具链 |
| 图形窗口显示 | ⏳ 沙箱无桌面环境，需目标机验证（唯一未闭环项） |

---

## 9. 下一步行动

1. ✅ ~~**P0**：实现节点/边标签文本渲染（补 `cg-render` 文本层），拍板 OQ-A1。~~已落地，见 P0 阶段方案。
2. ✅ ~~**P0**：实现 DOT 导入（`io.rs:188`），拍板 OQ-A6。~~已落地，自写解析。
3. ✅ ~~**P0**：按 §4 偏差表回改 `docs/architecture/` 与 `docs/plan/` 过期内容。~~已落地。
4. **P1**：补 4–5 个 petgraph 现成算法（floyd_warshall / bellman_ford / 割点桥 / 显式 BFS-DFS）。
5. **P1**：节点形状族（`shapes.rs`）与箭头形状族。
6. **目标机**：`cargo run -p compograph` 验证窗口与交互（阶段 0 唯一收尾项）。

---

## 10. 复核记录

- 本文所有 `file:line` 引用均基于本次克隆逐项打开核验；代码位置以函数/结构体定义行为准（非调用点）。
- 测试数为实测输出（`cargo test` 各 crate `test result` 行汇总）。
- cytoscape.js 对照项基于 `src/extensions/layout/index.mjs`、`src/collection/algorithms/index.mjs`、`src/extensions/renderer/base/node-shapes.mjs`（形状注册 `:534`）、`src/style/properties.mjs`（属性 ~194）核验。

## 11. 附：本文与既有文档的关系

| 文档 | 定位 | 关系 |
|---|---|---|
| [feature-list.md](../architecture/feature-list.md) | 功能"计划"清单 | 本文为其"实际落地"校验版 |
| [architecture-design.md](../architecture/architecture-design.md) | 架构设计 | §4/§9 阶段口径需按本文回改 |
| [compograph-implementation-plan.md](../plan/compograph-implementation-plan.md) | 分阶段实施方案 | §3 差距表需按本文回改 |
| 本文 | 实测分析报告 | 供回改与排期参考 |
