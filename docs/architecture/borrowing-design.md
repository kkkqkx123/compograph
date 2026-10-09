# compograph 借鉴设计说明（Borrowing Design）

> 版本：v1.0 · 日期：2026-09-28
> 目的：**明确 compograph 需要从 cytoscape.js / petgraph / gpui（gpui-pre 快照）三个来源分别借鉴哪些设计**，借鉴到什么程度（整段移植 / 数学直译 / 模式借鉴 / 架构参考 / 不借鉴），以及哪些明确**不能借鉴**及原因。
> 事实基线（版本以仓库内实际来源为准，文档不维持提交号）：
>
> | 项目 | 位置 | 版本口径 | 备注 |
> |---|---|---|---|
> | cytoscape.js | `ref/cytoscape-js`（随仓库 vendored） | 见其 `package.json`（当前 3.34.3） | 借鉴主源（布局数学 + 几何数学 + 模式） |
> | petgraph | crates.io 依赖 | 以 `Cargo.lock` 为准（当前 0.8.3） | 直接依赖复用（非移植） |
> | gpui | crates.io 依赖（`gpui-pre` 快照） | 以根 `Cargo.toml` 精确 pin 与 `Cargo.lock` 为准（当前 `=0.3.8`） | 平台底座，直接使用其 API |
>
> 配套：[架构设计](./architecture-design.md) · [功能清单](./feature-list.md)

---

## 0. 结论先行（一句话版）

> **借 cytoscape.js 的"数学"（布局物理 + 拾取/曲线几何）与"契约"（布局注册表、坐标写回桥、样式 mapper/bypass 思想）；直接复用 petgraph 的"图模型与算法"；把 cytoscape 的"绘制指令、DOM 事件桥、Canvas2D 纹理缓存、WebGL 着色器"全部重写或抛弃，绘制落点换成 gpui `Quad`/`Path`。**

三大来源的角色分工：

| 项目 | 在 compograph 中的角色 | 借鉴方式 |
|---|---|---|
| petgraph | **L1 数据/算法内核** | Cargo 依赖直接复用，不复制源码 |
| gpui（gpui-pre 快照） | **平台底座**（状态/事件/绘制/异步） | Cargo 依赖直接使用 API |
| cytoscape.js | **参照系**（布局/几何/模式的来源） | 数学直译为 Rust + 模式重实现 |

---

## 1. 借鉴总表

| # | 借鉴内容 | 来源与位置 | 借鉴程度 | 落点模块 | 优先级 |
|---|---|---|---|---|---|
| B1 | 图数据模型 `StableGraph` + 算法库 | petgraph `graph_impl/stable_graph/mod.rs:67`、`algo/*` | 直接依赖 | core | P0 |
| B2 | trait 解耦思想（算法不绑图实体） | petgraph `visit/mod.rs`（`IntoNeighbors:107` 等） | 思想借鉴 | core（暴露只读视图 trait） | P1 |
| B3 | 共享状态 + 订阅 + 重绘通知 | gpui `Entity`（`entity_map.rs:435`）、`subscribe`（`app.rs:1353`）、`notify`（`context.rs:221`） | 直接使用 | core / 全部 | P0 |
| B4 | 低层自定义绘制入口 | gpui `canvas`（`elements/canvas.rs:10`） | 直接使用 | render | P0 |
| B5 | 图元：`Quad`/`Path`/`Scene` | gpui `scene.rs:41/222`、`paint_quad`（`window.rs:4550`）、Path API（`scene.rs:840/847/859/876`） | 直接使用 | render | P0/P1 |
| B6 | 相机变换 | gpui `TransformationMatrix`（`scene.rs:608`）+ cytoscape `math.mjs:9/14` 公式 | API 直接用 + 公式借 | render | P1 |
| B7 | 后台执行模型 | gpui `App::spawn`（`app.rs:2177`）、`background_executor`（`app.rs:311`） | 直接使用 | layout / core | P1 |
| B8 | 力导向物理（斥力/引力/温度退火） | cytoscape `cose.mjs`（`run:126`、物理步 `step:705`） | ★★★ 数学直译 | layout | P1 |
| B9 | 简单布局（grid/circle/breadthfirst/concentric/random/preset） | cytoscape `extensions/layout/{grid:30,circle:31,breadthfirst:42,concentric:37,random:21,preset:25}.mjs` | ★★★ 数学直译 | layout | P0–P2 |
| B10 | 布局注册表（name→impl） | cytoscape `extensions/layout/index.mjs`（`{name, impl}` 数组注册 8 种） | ★★★ 模式借鉴 | layout | P0 |
| B11 | 布局桥接契约（算完→写坐标→发事件重绘） | cytoscape `collection/layout.mjs:41 layoutPositions` | ★★★ 模式借鉴 | layout/render | P0 |
| B12 | 拾取/命中数学（点选、框选、边距离） | cytoscape `coords.mjs`（`findNearestElement:75`、`findNearestElements:79`、`checkNode:131`、`checkEdge:157`、`getAllInBox:323`、`doLinesIntersect:413`） | ★★★ 整段译 Rust | render/interact | P1 |
| B13 | 边曲线几何（直线/贝塞尔/taxi/自环/haystack） | cytoscape `edge-control-points.mjs`（`:251/:257/:309/:162/:64`） | ★★★ 整段译 Rust | render | P1–P3 |
| B14 | 贝塞尔采样（边命中距离用） | cytoscape `edge-projection.mjs:5 pushBezierPts` | ★★★ 直译 | render | P1 |
| B15 | 箭头几何 | cytoscape `edge-arrows.mjs`、`arrow-shapes.mjs`（`registerArrowShapes:9`、`triangle:142`） | ★★ 顶点数据借、绘制重写 | render | P1 |
| B16 | 节点形状顶点生成 | cytoscape `node-shapes.mjs`（`generatePolygon:6`、`generateEllipse:43`、`generateRoundRectangle:143`、`registerNodeShapes:534`） | ★★ 顶点数据借、绘制重写 | render | P1/P2 |
| B17 | 绘制编排结构（shape→border→label 顺序、z-order） | cytoscape `drawing-nodes.mjs:11`、`drawing-edges.mjs:8/209/299/320`、`z-ordering.mjs` | ★ 只借顺序/结构 | render | P1 |
| B18 | 样式 schema + mapper/trigger 思想 | cytoscape `style/properties.mjs`（~208 属性）、`apply.mjs` | ★★ 思想借、目标字段重映射 | render 样式层 | P2 |
| B19 | bypass（局部覆盖样式） | cytoscape `style/bypass.mjs` | ★★ 概念借 | render/interact | P2 |
| B20 | 扩展机制思想（四类扩展点） | cytoscape `extension.mjs`（core/collection/layout/renderer） | ★ 思想借 → Rust trait 注册 | layout/render trait 设计 | P2 |
| B21 | WebGL 实例化架构 | cytoscape `webgl/drawing-elements-webgl.mjs`（`instanced:163`） | ★ 仅架构参考 | render 路线 C | P3/按需 |
| B22 | wgpu 设备/队列访问 | gpui 快照的 `gpui_wgpu`（`gpui-pre-wgpu`，`WgpuContext` 公开 `device/queue`，`wgpu_context.rs:14-15`） | 直接使用 | render 路线 C | P3/按需 |
| B23 | headless 测试设施 | gpui `TestAppContext`（`src/app/test_context.rs`） | 直接使用 | 全部测试 | P1 |

---

## 2. 从 petgraph 借鉴什么（直接复用，不移植源码）

petgraph 是 Cargo 依赖而非"参照物"，借鉴=正确选用其能力边界：

| 设计点 | 内容 | 核验位置 |
|---|---|---|
| 可编辑图容器 | 用 `StableGraph` 而非 `Graph`：删除节点后索引稳定（`Graph` 在 `graph_impl/mod.rs:392` 会复用索引，导致坐标/选中态错位） | `stable_graph/mod.rs:67` |
| 算法库 | 最短路/MST/最大流/匹配/SCC/支配树/传递归约/着色/极大团/同构/PageRank 等直接调用，**零重写**；完整清单见[功能清单 §1A](./feature-list.md) | `algo/*`（如 `dijkstra.rs:92`、`page_rank.rs:64`） |
| trait 解耦 | 算法作用于 `visit` trait 而非具体图类型——compograph 沿用此思想：L2 布局/L3 渲染只依赖 core 暴露的只读视图 trait，不依赖 `StableGraph` 实体，可 mock 单测 | `visit/mod.rs`（`IntoNeighbors:107`、`IntoNodeIdentifiers:183`、`IntoEdges:147`） |
| 能力边界 | petgraph **无布局、无坐标、无渲染**——坐标层必须自建 `PositionStore` 外挂；布局数学从 cytoscape 借（见 §4） | `lib.rs` 全文无布局 API |
| 布局子步骤复用 | `toposort`、`tred`（传递归约）、`dominators`（`dominators.rs:73`）、`page_rank`（`page_rank.rs:64`）作为分层/径向布局的子步骤 | `algo/` |

---

## 3. 从 gpui（gpui-pre 快照）借鉴什么（平台 API 直接使用）

| 设计点 | gpui API（以 gpui-pre 快照为准，行号可能随升级漂移） | compograph 用途 |
|---|---|---|
| 共享状态 | `Entity<T>`（`src/app/entity_map.rs:435`） | `Entity<GraphStore>`、`Entity<PositionStore>`。⚠️ 初步设计所写 `Model<T>` 在该版本已不存在，统一改为 `Entity`（见 [架构设计 §3.2](./architecture-design.md)） |
| 变更广播 | `EventEmitter` + `App::subscribe`（`src/app.rs:1353`） | `GraphChangeEvent` 驱动布局/渲染 |
| 重绘触发 | `Context::notify`（`src/app/context.rs:221`）/ `App::notify`（`src/app.rs:2932`） | 状态变更 → gpui 帧 → `paint` |
| 自定义绘制 | `canvas(prepaint, paint)`（`src/elements/canvas.rs:10`）；`Element` trait（`src/element.rs:53`，路线 B 用） | 路线 A 的 `GraphView` |
| 绘制图元 | `Scene`/`Primitive`（`src/scene.rs:41/222`，按类型批处理）；`paint_quad`（`src/window.rs:4550`）；`Path::move_to/line_to/curve_to/push_triangle`（`src/scene.rs:840/847/859/876`） | 节点=Quad、边/曲线=Path、箭头=三角 |
| 相机 | `TransformationMatrix`（`src/scene.rs:608`，compose `:658`） | GPU 级平移/缩放（进阶） |
| 异步 | `App::spawn`（`src/app.rs:2177`）、`background_executor`（`src/app.rs:311`） | 力导向后台迭代、大图算法 |
| 测试 | `TestAppContext`（`src/app/test_context.rs`） | headless 渲染/拾取单测（OQ-8） |
| 路线 C 通道 | 快照提供的 `gpui_wgpu`（`gpui-pre-wgpu`）：`WgpuContext` 公开 `pub device: Arc<wgpu::Device>` / `pub queue: Arc<wgpu::Queue>`（`crates/gpui_wgpu/src/wgpu_context.rs:14-15`） | 十万级实例化渲染时可直接拿 GPU 上下文，比原判"最脆弱"更可行 |

---

## 4. 从 cytoscape.js 借鉴什么（重点：数学与契约）

> 原则：**借"数学与几何"、借"模式与契约"；不借"绘制指令、数据访问、事件桥、线程/动画模型"**（不借原因见 §5）。

### 4.1 布局数学（→ cg-layout）★★★ 最高价值之一

| 借鉴项 | cytoscape 源（已核验） | 移植说明 |
|---|---|---|
| 力导向物理 | `cose.mjs`：入口 `CoseLayout.prototype.run:126`，**物理步 `step:705`**（斥力/引力/温度退火，自包含无外部依赖） | `step` 内数学整段直译为 Rust；调用形态（`eles.nodes()` 迭代）改为 petgraph 遍历。是否改用 d3-force 风格物理 = 开放点 OQ-B1 |
| Grid/Circle/Breadthfirst/Concentric | `grid.mjs:30`、`circle.mjs:31`、`breadthfirst.mjs:42`、`concentric.mjs:37` | 纯几何排布，参数语义（`spacingFactor`/`avoidOverlap`/`startAngle`…）连同数学一起移植 |
| Random/Preset | `random.mjs:21`、`preset.mjs:25` | 平凡实现，P0 起步即用 |
| 布局注册表 | `extensions/layout/index.mjs`：`{name, impl}` 数组注册 8 种内置布局 | 映射为 `LayoutEngine` 注册表（trait object + name 查找） |
| 布局桥接契约 | `collection/layout.mjs:41 layoutPositions`：布局算完坐标→统一写回→触发重绘事件 | 对应自研链路：`LayoutEngine::layout` 产 `Positions` → 写 `PositionStore` → `notify` |

### 4.2 几何/坐标数学层（→ cg-render）★★★ 最高价值、框架无关

> 这一层是**纯数学、不依赖 Canvas 也不依赖 DOM**，与 Rust/gpui 完全兼容，整层可移植，性价比最高。

| 借鉴项 | cytoscape 源（已核验） | 移植说明 |
|---|---|---|
| 点选/最近元素 | `coords.mjs`：`findNearestElement:75`、`findNearestElements:79` | 入参 `ele` → `(NodeIndex, bbox)` / `(EdgeIndex, pts)` |
| 节点/边命中判定 | `checkNode:131`、`checkEdge:157` | 直译；边命中可用 4.2-B 的采样距离 |
| 框选 | `getAllInBox:323`、`doLinesIntersect:413` | 直译为框选矩形与线段相交测试 |
| 边曲线控制点 | `edge-control-points.mjs`：直线 `:251`、贝塞尔 `:257`、taxi 折线 `:309`、自环 `:162`、haystack 聚合 `:64` | "边怎么弯"的全部几何，逐函数直译 |
| 贝塞尔采样 | `edge-projection.mjs:5 pushBezierPts` | 离散采样点用于命中距离 |
| 箭头几何 | `edge-arrows.mjs`（bearing/位移） | 直译；落点为 `Path::push_triangle` |
| 坐标变换公式 | `math.mjs`：`modelToRenderedPosition:9`、`renderedToModelPosition:14` | **只借 (zoom, pan) 仿射公式**；实现由 gpui `Camera`/`TransformationMatrix` 承担 |
| 标签定位/z 序 | `labels.mjs:161`、`z-ordering.mjs` | 借分段与绘制顺序策略 |

### 4.3 形状几何（→ cg-render）★★

| 借鉴项 | cytoscape 源 | 移植说明 |
|---|---|---|
| 节点形状顶点 | `node-shapes.mjs`：`generatePolygon:6`、`generateEllipse:43`、`generateRoundRectangle:143`、`generateCutRectangle:230`、`generateBarrel:295`、`registerNodeShapes:534` | "形状名→顶点序列"的生成函数可直译；**顶点→像素的绘制必须改为 gpui `Path`/`Quad`** |
| 箭头形状表 | `arrow-shapes.mjs`：`registerArrowShapes:9`、`triangle:142` | 借为形状常量表（triangle/vee/tee…） |

### 4.4 绘制编排结构（→ cg-render）★

| 借鉴项 | cytoscape 源 | 借什么 |
|---|---|---|
| 节点绘制顺序 | `drawing-nodes.mjs:11 drawNode`（shape→border→label→overlay） | 只借**顺序与层级分解**，不借 Canvas2D `context` 指令 |
| 边绘制编排 | `drawing-edges.mjs`：`drawEdge:8`、`drawEdgePath:209`、`drawEdgeTrianglePath:299`、`drawArrowheads:320` | "先算路径点→描边→画箭头"的编排 |
| 主循环结构 | `drawing-redraw.mjs:278 render`（清屏→按 z-order 遍历→draw→pixelRatio） | 借为渲染管线编排；帧循环由 gpui 承载，不复制 |
| 脏标记思路 | `redraw.mjs:9 redraw`、`:46 startRenderLoop`、`:22 beforeRender` | 思路借；gpui 的 `notify`/帧循环已等价承载 |

### 4.5 样式系统思想（→ render 样式层）★★

| 借鉴项 | cytoscape 源 | 借什么 |
|---|---|---|
| 属性 schema | `style/properties.mjs`（~208 个属性：类型/默认值/mapper/trigger） | 作为自研样式属性**清单起点**（v1 只取常用子集） |
| mapper/trigger | `style/apply.mjs` | "业务数据→视觉属性"的解耦标准做法 |
| bypass | `style/bypass.mjs` | 选中/高亮走**局部覆盖**而非改主样式 |
| ⚠️ 落点重映射 | — | cytoscape 属性最终落 Canvas2D（`fillStyle`/`lineWidth`）；自研落 `PaintQuad`/`Path` 的 `color`/`stroke` 字段，映射表必须重写 |

### 4.6 扩展机制思想（→ trait 设计）★

cytoscape `extension.mjs` 定义 core/collection/layout/renderer 四类扩展点（如 layout 扩展自动合成 `run/stop/destroy` 与事件，`extension.mjs:37-133`）。**思想可借**：compograph 用 Rust trait 表达同类扩展（`LayoutEngine`、后续 `Renderer`/`Overlay` trait），注册表+工厂模式对应 B10。

---

## 5. 明确不借鉴清单（及原因）

| 不借鉴项 | cytoscape 源 | 原因 |
|---|---|---|
| Canvas2D 绘制指令 | `drawing-*.mjs` 全部 `context.fill()/stroke()/arc()…` | 渲染原语不同：gpui 是 `Scene` 的 `Quad`/`Path`（`scene.rs:222`），`ctx.save/restore/translate` 换 `TransformationMatrix`（`scene.rs:608`）或 CPU 变换 |
| 活对象数据模型（`Core`/`Collection`/`ele.data()`） | `src/core/index.mjs`、`src/collection/` | 数据模型不同：自研用 petgraph `StableGraph` + 权重类型；所有 `eles.nodes()` 式访问改为 petgraph 遍历（`node_weights()`/`edge_references()`/`neighbors()`） |
| DOM 事件→拾取的桥接 | `coords.mjs` 依赖的 `projectIntoViewport:7` 及 DOM 监听 | 事件体系不同：改为 gpui 指针事件 → `Camera` 逆变换 → 自研空间索引。**几何函数可借，事件桥必须重写** |
| 单线程 rAF 分步布局 | `cose.mjs` 的 rAF 步进（`run` 内循环 `:175` 调 `step:705`） | gpui 有 `background_executor`/`App::spawn`；改为后台线程分批迭代 + 回写 + notify |
| 独立动画系统 | `animation.mjs` 及各布局 `animate` 选项链 | 挂 gpui 自身帧循环/动画机制 |
| Canvas2D 纹理缓存 | `ele-texture-cache` / `layered-texture-cache` / `drawing-images` | gpui `Scene` 已按 `Primitive` 类型批处理；性能手段改为 **LOD + 空间索引 + 保留模式（路线 B）** |
| WebGL 着色器/图集管线 | `webgl/shader-sdf.mjs`、`atlas.mjs` 等 | GLSL→wgsl、管线完全不同；仅借"实例化架构"抽象（B21） |
| 网络科学算法 | 度/接近/介数中心性、MCL、k-means、层次聚类、亲和传播、`hierholzer.mjs`、`karger-stein.mjs` | petgraph 无对应、v1 范围外；列为远期自研（见[功能清单 §6](./feature-list.md)） |
| CSS 式选择器 | `src/selector/` | Rust 侧用类型化查询/过滤（petgraph 遍历 + 谓词）即可，无需字符串 DSL（远期按需再议） |

---

## 6. 借鉴落地映射表（源文件 → compograph 模块）

| cytoscape.js 源 | → compograph 落点 | 移植方式 | 工作量 | 阶段 |
|---|---|---|---|---|
| `extensions/layout/cose.mjs:126,705` | `cg-layout/src/force.rs` | 数学直译 | 中-高 | P1 |
| `extensions/layout/{grid,circle,breadthfirst,concentric,random,preset}.mjs` | `cg-layout/src/{grid,circle,bfs,concentric,random,preset}.rs` | 数学直译 | 低-中 | P0–P2 |
| `extensions/layout/index.mjs` | `cg-layout/src/registry.rs` | 模式重实现 | 低 | P0 |
| `collection/layout.mjs:41` | `cg-layout` 桥接 + `render` notify | 模式重实现 | 低 | P0 |
| `coord-ele-math/coords.mjs` | `cg-render/src/picking.rs` | 整段译 Rust | 中 | P1 |
| `coord-ele-math/edge-control-points.mjs` | `cg-render/src/edge_geometry.rs` | 整段译 Rust | 中 | P1–P3 |
| `coord-ele-math/edge-projection.mjs:5` | `cg-render/src/edge_geometry.rs` | 直译 | 低 | P1 |
| `coord-ele-math/edge-arrows.mjs`、`base/arrow-shapes.mjs` | `cg-render/src/arrows.rs` | 数据借+绘制重写 | 低-中 | P1 |
| `base/node-shapes.mjs` | `cg-render/src/shapes.rs` | 数据借+绘制重写 | 低-中 | P1/P2 |
| `canvas/drawing-nodes.mjs:11`、`drawing-edges.mjs:8` | `cg-render/src/paint.rs`（仅编排） | 结构借 | 低 | P1 |
| `style/properties.mjs`、`apply.mjs`、`bypass.mjs` | `cg-render/src/style/`（子集） | 思想借+映射重写 | 中 | P2 |
| `math.mjs:9/14` | `cg-render/src/camera.rs`（公式） | 公式借 | 低 | P1 |
| `webgl/drawing-elements-webgl.mjs:163` | 路线 C 抽象参考 | 架构借 | 高 | P3/按需 |

---

## 7. 行号核验说明

各表引用的上游文件行号以核验时的快照为准；cytoscape.js 随仓库 vendored（`ref/cytoscape-js`）、petgraph 以 `Cargo.lock` 为准、gpui-pre 快照按版本升级，行号可能漂移，使用时以实际源码为准：

- cytoscape.js：`CoseLayout.prototype.run`(cose.mjs:126)、`step`(cose.mjs:705)、`layoutPositions`(layout.mjs:41)、`modelToRenderedPosition`/`renderedToModelPosition`(math.mjs:9/14)、`findNearestElement`/`findNearestElements`/`getAllInBox`(coords.mjs:75/79/323)、`findHaystackPoints`/`findLoopPoints`/`findStraightEdgePoints`/`findBezierPoints`/`findTaxiPoints`(edge-control-points.mjs:64/162/251/257/309)、布局注册表 `index.mjs`（8 项 `{name, impl}`）、算法 index（18 模块 import，`collection/algorithms/index.mjs:2-19`）。
- petgraph：`StableGraph`(stable_graph/mod.rs:67)、`Graph`(graph_impl/mod.rs:392)、`dijkstra`(dijkstra.rs:92)、`astar`(astar.rs:81)、`tarjan_scc`(tarjan_scc.rs:269)、`page_rank`(page_rank.rs:64)、`min_spanning_tree`/`min_spanning_tree_prim`(min_spanning_tree.rs:86/255)、`ford_fulkerson`(ford_fulkerson.rs:164)、`IntoNeighbors`/`IntoNodeIdentifiers`/`IntoEdges`(visit/mod.rs:107/183/147)。
- gpui（gpui-pre）：`canvas`(elements/canvas.rs:10)、`Element`(element.rs:53)、`Scene`/`Primitive`(scene.rs:41/222)、`paint_quad`(window.rs:4550)、`Path::move_to/line_to/curve_to/push_triangle`(scene.rs:840/847/859/876)、`TransformationMatrix`(scene.rs:608)、`Entity`(app/entity_map.rs:435)、`subscribe`(app.rs:1353)、`notify`(app/context.rs:221)、`App::spawn`(app.rs:2177)、`background_executor`(app.rs:311)、`WgpuContext{device,queue}`(gpui_wgpu/src/wgpu_context.rs:14-15)。

> 注：cytoscape.js 内置布局注册表实际为 **8 种**（breadthfirst/circle/concentric/cose/grid/null/preset/random，`index.mjs`）；此前文档表述"9 种"口径含 headless null/扩展生态，使用时以注册表为准。
