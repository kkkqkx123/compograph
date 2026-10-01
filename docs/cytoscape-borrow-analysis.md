# 自研绘图库借鉴 cytoscape.js 的完整分析

> **文档状态（2026-09-28）**：本分析的定稿与落地映射已由 `docs/architecture/borrowing-design.md` 取消（借鉴总表、不借鉴清单、源文件→模块映射均已更新至 cg-* crate 命名），正文保留作为逐子系统分析依据。

> 配套：`compograph-design.md`（总体方案）。本文件聚焦**自研绘图库（L2 布局 + L3 渲染/交互）应从 cytoscape.js 借鉴什么、以及哪些不能直接参考**。
> 引用基线：cytoscape.js 随仓库 vendored 于 `ref/cytoscape-js`（版本见其 `package.json`，行号以实际源码为准）。
> 角色对照：petgraph 负责"图模型+算法"，cytoscape.js 的"布局+渲染+交互"正是自研绘图库要替代/借鉴的对象。

---

## 0. 目的与范围

自研绘图库需要补齐 cytoscape.js 提供的三类能力：**(a) 布局算法**（把图变成坐标）、**(b) 几何/坐标数学**（命中测试、曲线、箭头、坐标变换）、**(c) 渲染/交互**（把坐标画成图元、处理拾取/框选/视口）。

本分析逐子系统回答两个问题：

1. **哪些该借鉴**（附 cytoscape.js 代码位置 + 简要说明 + 借鉴程度）。
2. **哪些不能直接参考 / 必须调整**（因 cytoscape.js 是 JS + Canvas2D + 自有数据模型，而我们是 Rust + gpui `Scene` 原语 + petgraph 数据模型）。

---

## 1. 事实核查：cytoscape.js 绘图相关模块地图

| 子系统 | 目录 / 文件 | 关键符号（行号） |
|---|---|---|
| 布局注册 | `src/extensions/layout/index.mjs` | 注册表 `:11-17` |
| 布局：力导向 | `src/extensions/layout/cose.mjs` | `CoseLayout.prototype.run :126`、`step :705`（**自包含 FR 物理**） |
| 布局：BFS/同心/圆/网格/随机/预设 | `src/extensions/layout/{breadthfirst,concentric,circle,grid,random,preset}.mjs` | `run` 分别在 `:42 / :37 / :31 / :30 / random:21 / preset:25` |
| 布局桥接（写坐标+发事件） | `src/collection/layout.mjs` | `layoutPositions :41` |
| 坐标变换 | `src/math.mjs` | `modelToRenderedPosition :9`、`renderedToModelPosition :14` |
| 拾取/命中（点选/框选） | `src/extensions/renderer/base/coord-ele-math/coords.mjs` | `findNearestElement :75`、`findNearestElements :79`、`checkNode :131`、`checkEdge :157`、`getAllInBox :323`、`doLinesIntersect :413`、`projectIntoViewport :7` |
| 节点几何 | `src/extensions/renderer/base/coord-ele-math/nodes.mjs` | `getNodeShape :9` |
| 边几何（曲线/直线/自环/回环） | `src/extensions/renderer/base/coord-ele-math/edge-control-points.mjs` | `findStraightEdgePoints :251`、`findBezierPoints :257`、`findTaxiPoints :309`、`findHaystackPoints :64`、`findLoopPoints :162`、`storeAllpts :594` |
| 边采样（贝塞尔点） | `src/extensions/renderer/base/coord-ele-math/edge-projection.mjs` | `pushBezierPts :5` |
| 箭头几何 | `src/extensions/renderer/base/coord-ele-math/edge-arrows.mjs` | bearing/位移计算 |
| 标签定位 | `src/extensions/renderer/base/coord-ele-math/labels.mjs` | `addSegment :161` |
| 绘制层级 | `src/extensions/renderer/base/coord-ele-math/z-ordering.mjs` | — |
| 节点形状生成 | `src/extensions/renderer/base/node-shapes.mjs` | `generatePolygon :6`、`generateEllipse :43`、`generateRoundRectangle :143`、`generateCutRectangle :230`、`generateBarrel :295`、`registerNodeShapes :534` |
| 箭头形状 | `src/extensions/renderer/base/arrow-shapes.mjs` | `registerArrowShapes :9`、`triangle :142` |
| Canvas 节点绘制 | `src/extensions/renderer/canvas/drawing-nodes.mjs` | `CRp.drawNode :11` |
| Canvas 边绘制 | `src/extensions/renderer/canvas/drawing-edges.mjs` | `CRp.drawEdge :8`、`drawEdgePath :209`、`drawEdgeTrianglePath :299`、`drawArrowheads :320` |
| Canvas 渲染主循环 | `src/extensions/renderer/canvas/drawing-redraw.mjs` | `CRp.render :278`、`getPixelRatio :13`、`matchCanvasSize :193`、`renderTo :257` |
| 重绘调度（base） | `src/extensions/renderer/base/redraw.mjs` | `redraw :9`、`startRenderLoop :46`、`beforeRender :22` |
| 纹理/缓存（Canvas2D 专属） | `src/extensions/renderer/canvas/{ele-texture-cache,layered-texture-cache,drawing-images}.mjs` | — |
| 图片导出 | `src/extensions/renderer/canvas/export-image.mjs` | — |
| 样式 schema | `src/style/properties.mjs` | ~208 个属性定义 |
| 按元素取样式 | `src/style/get-for-ele.mjs` | `styfn`（默认导出） |
| 样式映射/触发 | `src/style/apply.mjs` | mapper/trigger 逻辑 |
| 样式解析/覆盖 | `src/style/{parse,bypass}.mjs` | — |
| WebGL 渲染（路线 C 对照） | `src/extensions/renderer/canvas/webgl/drawing-elements-webgl.mjs` | `instanced :163`、`instanceCount :530` |

---

## 2. 自研绘图库需要的能力清单（对照上一方案）

| 能力 | 对应 cytoscape 子系统 | 在自研库中的归属 |
|---|---|---|
| 9 类布局（力导向/BFS/同心/圆/网格/随机/预设/…） | `extensions/layout/*` | `compograph-layout` |
| 坐标变换（模型↔屏幕） | `math.mjs` | `compograph-render` 的 `Camera` |
| 节点/边命中、框选 | `coord-ele-math/coords.mjs` | `compograph-render` + `compograph-interact` |
| 边曲线/箭头几何 | `edge-control-points` / `edge-arrows` | `compograph-render` |
| 节点/箭头形状 | `node-shapes` / `arrow-shapes` | `compograph-render` |
| 节点/边/标签绘制 | `drawing-*.mjs` | `compograph-render`（映射为 gpui `Quad`/`Path`） |
| 重绘循环/脏标记 | `redraw.mjs` / `drawing-redraw.mjs` | gpui 帧循环（`cx.notify`） |
| 样式系统 | `style/*` | 可选 `compograph-render` 样式层 |

---

## 3. 需要借鉴的部分（附代码位置 + 说明 + 借鉴程度）

### 3.1 布局算法（`extensions/layout/*`）—— ★★★ 高价值，可直接移植数学

- **力导向 `cose.mjs`**：`run :126` 启动、`step :705` 每步物理（斥力/引力/温度退火）。**自包含、无外部依赖**（仅 `util/math/is`），数学可整段译为 Rust。这是 8 种布局里最复杂、最该借的。
- **`breadthfirst.mjs:42` / `concentric.mjs:37` / `circle.mjs:31` / `grid.mjs:30`**：纯几何排布，启发式清晰（`spacingFactor`/`avoidOverlap`/`startAngle` 等参数），翻译成本低。
- **`random.mjs:21` / `preset.mjs:25`**：平凡布局，仅作对照/占位。
- **注册表 `index.mjs:11-17`**：借鉴"name→impl 表 + 工厂"的模式，映射到自研 `LayoutEngine` 注册（`compograph-layout`）。
- **桥接 `collection/layout.mjs:41` `layoutPositions`**：借鉴其"布局算完坐标→统一写回→触发重绘事件"的**桥接契约**。自研对应物：`LayoutEngine::layout` 产出 `Positions` → 写入 `PositionStore` → `cx.notify()`（见总体方案第 4/5 章）。

> 借鉴程度：布局**数学与参数语义**可直接移植；**调用形态**（原型方法、`eles.nodes()` 迭代）需改为 petgraph 遍历。

### 3.2 几何/坐标数学层（`coord-ele-math/*`）—— ★★★ 最高价值，框架无关

这一层是 cytoscape 最该借的部分：**纯数学，不依赖 Canvas 也不依赖 DOM**，与 Rust/gpui 完全兼容。

- **拾取 `coords.mjs`**：`findNearestElement :75` / `findNearestElements :79` 是点选入口；`checkNode :131` / `checkEdge :157` 是节点/边命中判定（含包围盒与边距离）；`getAllInBox :323` 是框选；`doLinesIntersect :413` 是线段相交工具。**整段可译为 Rust**，仅把 `ele` 换成 `(NodeIndex, bbox)` / `(EdgeIndex, pts)`。
- **边曲线 `edge-control-points.mjs`**：`findBezierPoints :257`（贝塞尔控制点）、`findStraightEdgePoints :251`、`findTaxiPoints :309`（折线）、`findHaystackPoints :64`（大量边聚合）、`findLoopPoints :162`（自环）、`storeAllpts :594`（汇总）。这是"边怎么弯"的全部几何，**应整体借鉴**。
- **边采样 `edge-projection.mjs:5` `pushBezierPts`**：把贝塞尔离散成点，用于边命中距离计算。
- **箭头 `edge-arrows.mjs`**：箭头方位/位移几何。
- **坐标变换思路 `math.mjs:9/14`**：`modelToRenderedPosition`/`renderedToModelPosition` 的**(zoom, pan) 仿射公式**可借；但实现由 gpui `Camera` 接管（见 4.3）。
- **标签 `labels.mjs:161`、层级 `z-ordering.mjs`**：标签分段与绘制顺序策略可借。

> 借鉴程度：**整层可移植**（这是纯函数式几何）。是工作量性价比最高的借鉴区。

### 3.3 节点/箭头形状（`base/node-shapes.mjs`、`base/arrow-shapes.mjs`）—— ★★ 可借几何

- `node-shapes.mjs`：`generatePolygon :6`、`generateEllipse :43`、`generateRoundRectangle :143`、`generateCutRectangle :230`、`generateBarrel :295`、`registerNodeShapes :534` —— 把"形状名→多边形顶点/路径"的几何生成。**顶点计算可借**，但"如何把顶点变成可见像素"在 cytoscape 是 `context` 调用，需改为 gpui `Path`（`scene.rs` 的 `move_to/line_to`，见总体方案 6.1）。
- `arrow-shapes.mjs:9` `registerArrowShapes`、`:142` `triangle` —— 箭头多边形定义（triangle/vee/tee/…）可借为"形状常量表"。

> 借鉴程度：**形状顶点/多边形数据可借**，绘制指令需重写。

### 3.4 绘制流程结构（`canvas/drawing-nodes.mjs`、`drawing-edges.mjs`）—— ★ 借"顺序"不借"实现"

- `CRp.drawNode :11`：流程为 shape→border→outline→label→overlay，且有 `drawPie`/`drawStripe`/`drawOverlay` 等细节。**借鉴其绘制顺序与层级分解**（先填色、再描边、再标签、再覆盖层），不借 Canvas2D `context` 调用。
- `CRp.drawEdge :8` + `drawEdgePath :209` + `drawEdgeTrianglePath :299` + `drawArrowheads :320`：借鉴"先算路径点（来自 3.2）→ 描边 → 画箭头"的**编排结构**。
- `drawing-redraw.mjs:278` `CRp.render` 的"清屏→按 z-order 遍历元素→逐个 draw→处理 pixelRatio"主循环结构可借为**渲染管线编排**；`redraw.mjs:9/46/22` 的脏标记/帧循环思路可借（但由 gpui 帧循环承载）。

> 借鉴程度：**只借"画什么、按什么顺序画"的编排**，具体光栅化指令必须替换为 gpui `Scene` 原语。

### 3.5 样式系统概念（`style/*`）—— ★★ 借"schema + mapper 思想"

- `properties.mjs`：~208 个样式属性的**schema 定义**（类型、默认值、mapper、trigger）。可借作自研库的"样式属性清单"起点（颜色/宽度/形状/透明度/标签字体…）。
- `get-for-ele.mjs` + `apply.mjs`：借鉴"**按元素求值 + 函数映射器(mapper) + 变更触发器(trigger)**"的设计——这正是把"业务数据→视觉属性"解耦的标准做法。
- `bypass.mjs`：借鉴"局部覆盖样式"的概念（选中/高亮时用 override 而非改主样式）。

> 借鉴程度：借**数据模型与映射思想**；目标值要重映射到 gpui `PaintQuad`/`Path` 的 `color`/`stroke` 字段（见 4.5）。

### 3.6 图片导出 / 纹理（`export-image.mjs`、各类 cache）—— △ 仅参考

- `export-image.mjs`（PNG 导出）：若自研库需"导出图"，可参考其"离屏渲染→toDataURL"思路；gpui 下改用 `wgpu` 截图/`Scene` 离屏渲染。
- `ele-texture-cache` / `layered-texture-cache` / `drawing-images`：**Canvas2D 纹理缓存专属**，gpui 用 `Scene` 批处理而非 2D 纹理缓存，**不直接借**（见 4.9）。

### 3.7 WebGL 路线（路线 C 对照）—— ★ 仅借架构

- `webgl/drawing-elements-webgl.mjs:163` `instanced`、`instanceCount :530`：cytoscape 的 GPU 实例化架构，正好对应总体方案"路线 C"。**借其整体架构（实例化几何 + 样式键合批 + 纹理图集拾取）**，但着色器与 wgpu 管线完全不同，必须从零写。

---

## 4. 不能直接参考 / 必须调整的部分

> 根因：cytoscape.js = **JavaScript + Canvas2D `context` + 自有 Element/Collection 数据模型 + DOM 事件 + 单线程 rAF**；自研库 = **Rust + gpui `Scene`(Quad/Path) + petgraph `StableGraph` + gpui 指针事件 + 后台 executor**。四类不匹配导致以下必须调整。

### 4.1 渲染原语模型不同（最关键的调整）

- cytoscape 所有绘制都通过 **Canvas2D `context`**（fill/stroke/arc/bezierCurveTo…）。
- gpui 的绘制原语是 **`Scene` 中的 `Primitive::Quad` 与 `Primitive::Path`**（`scene.rs:222`），通过 `window.paint_quad()`（`window.rs:4502`）和 `window.scene().insert_primitive(Path)`（`scene.rs:87`）注入。
- **结论**：第 3.4 节的"绘制流程结构"可借，**但每个 `context.fill()/stroke()` 必须改为构造 `Quad`/`Path` 图元**。Canvas2D 的 `ctx.save/restore/translate/rotate` 也要换成 gpui 的 `TransformationMatrix`（`scene.rs:658`）或先 CPU 变换坐标。

### 4.2 数据模型不同（所有"按 ele 取数据"需改）

- cytoscape 布局/几何/绘制都通过 `ele.data()`、`ele.boundingBox()`、`eles.nodes()` 等**自有 Collection API** 取数据。
- 自研库数据在 **petgraph `StableGraph`**（`stable_graph/mod.rs:67`）。每次"取节点/边/邻居"都要改成 petgraph 遍历（`graph.node_weights()` / `edge_references()` / `neighbors()`）。
- 影响面：3.1 布局的 `eles.nodes().layoutPositions(...)`（`layout.mjs:41`）→ 改为 `LayoutEngine` 遍历 `StableGraph`；3.2 拾取的 `checkNode/checkEdge` 入参从 `ele` 改为 `(NodeIndex, bbox)`。

### 4.3 坐标与视口由 gpui `Camera` 接管

- cytoscape 的 `(zoom, pan)` 与 `modelToRenderedPosition`（`math.mjs:9`）是自带 viewport 系统。
- gpui 没有"图视口"概念，需自研 `Camera{offset, zoom}`（总体方案 6.3）。**公式思路可借 `math.mjs`，实现必须自写**，且要接入 gpui 的 `Window` 尺寸与 DPI（`drawing-redraw.mjs:13 getPixelRatio` 的 DPI 处理也要在 gpui 侧重做）。

### 4.4 事件/拾取入口需重写（几何可借，事件桥不可借）

- cytoscape 拾取链路 `findNearestElement`（`coords.mjs:75`）建立在 **DOM 鼠标事件 + `projectIntoViewport :7`** 之上。
- gpui 用自己的**指针事件系统**（见总体方案 7）。**几何部分（`checkNode`/`checkEdge`/`getAllInBox`/`doLinesIntersect`）可整段译 Rust**，但"监听 DOM 事件→换算屏幕坐标→调拾取"这层桥必须重写，改为 gpui 的 pointer handler → 经 `Camera` 反变换 → 调自研空间索引/拾取函数。

### 4.5 样式映射目标不同

- cytoscape 样式最终落到 **Canvas2D context 属性**（`fillStyle`/`lineWidth`/`font`…）。
- 自研库样式要落到 **gpui `PaintQuad`/`Path` 的 `color`/`stroke`/`background` 字段**。属性名与类型需**重映射**（如 `background-color`→`PaintQuad` 填充色、`width`→`stroke` 宽度、`label`→gpui 文本系统）。`properties.mjs` 的 schema 可借，但"属性→目标字段"的映射表要重写。

### 4.6 异步/线程模型不同

- cytoscape 是**单线程 JS**，布局在 `step` 里同步迭代，靠 `requestAnimationFrame` 分步。
- gpui 有 **`background_executor` / `App::spawn`**。力导向（3.1 的 `cose.step`）应改为**后台线程分批迭代、回写 `PositionStore` 并 `notify`**（总体方案 5.4）。这是结构性调整，不能照抄单线程循环。

### 4.7 动画模型不同

- cytoscape 有独立 `animation.mjs` + rAF 驱动。
- gpui 有自身**帧循环与动画 API**。节点拖拽/布局收敛动画应挂在 gpui 动画机制上，而非复制 cytoscape 的 `animate` 选项链路（`layout/*` 里大量 `animate`/`animateFilter` 选项（`cose.mjs:39`）在自研库里需重新对接）。

### 4.8 WebGL 路线 C 的着色器/管线不可复用

- cytoscape 的 `webgl/*.mjs` 是对 **WebGL + 浏览器 GLSL** 的实现。`getKey`/`drawElement`/`getBoundingBox` 接口（`drawing-elements-webgl.mjs:86-109`）值得借为"实例化渲染元素接口"的**抽象**，但 wgpu 管线、SDF 着色器（`shader-sdf.mjs`）、atlas（`atlas.mjs`）全部要重写。

### 4.9 Canvas2D 专属优化不适用

- `ele-texture-cache` / `layered-texture-cache` / 离屏纹理缓存是 **Canvas2D 性能手法**（缓存节点为位图）。gpui 的 `Scene` 已**按 `Primitive` 类型批处理**（`scene.rs:172`），无需位图缓存。自研库应改用 **LOD + 空间索引 + 保留模式（路线 B）** 而非纹理缓存。

---

## 5. 借鉴优先级与移植工作量评估

| 子系统 | 借鉴内容 | 来源行号 | 借鉴程度 | 移植工作量 | 优先 |
|---|---|---|---|---|---|
| 几何/坐标数学（命中/曲线/箭头/变换） | 整层几何函数 | `coord-ele-math/*` | ★★★ 整段译 Rust | 中 | **P1 最高** |
| 布局数学（尤其 cose 力导向） | FR 物理 + 排布算法 | `cose.mjs:126/705`、`breadthfirst` 等 | ★★★ 数学直译 | 中-高 | **P1 最高** |
| 形状几何（顶点数据） | 多边形/箭头顶点 | `node-shapes.mjs`、`arrow-shapes.mjs` | ★★ 数据借、绘制改 | 低-中 | P1 |
| 绘制编排（顺序/管线） | draw 流程结构 | `drawing-nodes.mjs:11`、`drawing-edges.mjs:8`、`drawing-redraw.mjs:278` | ★ 只借结构 | 低（重写绘制） | P2 |
| 样式 schema + mapper 思想 | 属性表/映射/覆盖 | `style/*` | ★★ 思想借 | 中 | P2 |
| 布局注册/桥接契约 | name→impl、写坐标+事件 | `index.mjs:11-17`、`layout.mjs:41` | ★★★ 模式借 | 低 | P1 |
| 重绘脏标记/帧循环思路 | 脏矩形/帧调度 | `redraw.mjs:9/46` | ★ 思路借 | 低（gpui 承载） | P2 |
| WebGL 实例化架构（路线 C） | 抽象接口 | `webgl/drawing-elements-webgl.mjs:163` | ★ 架构借 | 高（重写） | P3 |
| 纹理缓存/导出 | Canvas2D 缓存 | `ele-texture-cache*`、`export-image.mjs` | △ 仅参考 | — | P3（按需） |

**一句话结论**：**借"数学与几何"（3.1/3.2/3.3）与"布局契约"（3.1 注册+桥接），重写"绘制指令"（4.1/4.5）、"数据访问"（4.2）、"事件桥"（4.4），并改造"线程/动画模型"（4.6/4.7）**。

---

## 6. 待拍板开放点 / 下一步

- **OQ-B1（借鉴边界）**：力导向是否整段译 `cose.mjs` 的 `step`（`cose.mjs:705`），还是改用更现代的 d3-force 风格物理？两者几何可借、物理可选。
- **OQ-B2（形状覆盖度）**：v1 是否需要 `node-shapes.mjs` 的全部形状（barrel/cut-rectangle 等），还是先圆/矩形/椭圆？
- **OQ-B3（样式系统是否做）**：v1 是否引入完整 `style/*` 式 schema，还是先用硬编码视觉属性（选中/高亮走 `bypass` 式 override）？
- **OQ-B4（命中精度）**：边命中是否采用 `edge-projection.mjs:5` 的贝塞尔采样距离，还是简化成"中垂线距离"？
- **OQ-B5（路线 C 时机）**：若目标规模到十万级（见总体方案 OQ-3），何时启动 WebGL/wgpu 实例化（`webgl/*.mjs` 架构参考）？

**下一步**：从 P1 的"几何层 + 力导向布局"起步，把 `coord-ele-math/*` 与 `cose.mjs` 的关键函数译为 Rust 原型并单测（gpui `TestAppContext` 做 headless 拾取测试，对应总体方案 OQ-8），再接 gpui `canvas` 绘制（路线 A）。

---

## 7. 参考

- 参照源码：cytoscape.js 见 `ref/cytoscape-js`、petgraph 为 crates.io 依赖（版本以 `Cargo.lock` 为准）、gpui 见 `crates/vendor/zed-gpui`（submodule，行号以实际快照为准）
- 总体方案：`docs/plan/compograph-design.md`
- 架构分析：`docs/architecture/cytoscape-js.md`、`docs/architecture/petgraph.md`
- 算法差异：`docs/analysis/graph-algorithms-comparison.md`
