# cytoscape-js 源码功能清单（ref/cytoscape-js）

> 分析对象：`ref/cytoscape-js`（cytoscape.js v3.34.3，MIT 协议，纯 JS 图可视化库）。
> 目的：整理其功能模块清单，为 compograph 自研布局/渲染/交互提供参照。
> 说明：本文只描述 cytoscape.js 实现了"什么"，不涉及 compograph 的采用决策。

## 总体架构

cytoscape.js 是面向浏览器的图理论与可视化库，核心分层：

| 层 | 位置 | 职责 |
|---|---|---|
| 核心 API | `src/core/` | 实例生命周期、数据增删、视口、事件、导出 |
| 元素集合 | `src/collection/` | 元素（节点/边/复合节点）查询、遍历、算法 |
| 选择器 | `src/selector/` | CSS 风格选择器解析与匹配 |
| 样式系统 | `src/style/` | 样式表、属性定义、解析与映射 |
| 扩展机制 | `src/extension.mjs`、`src/extensions/` | 布局与渲染器的注册挂载点 |
| 渲染器 | `src/extensions/renderer/canvas/` | Canvas 2D 与 WebGL 渲染实现 |
| 布局 | `src/extensions/layout/` | 内置布局算法 |
| 基础工具 | `src/util/`、`src/math.mjs`、`src/heap.mjs` 等 | 集合、颜色、排序、堆、几何数学 |

## 1. 核心（src/core/）

- `index.mjs`：Cytoscape 实例构造与初始化装配。
- `add-remove.mjs`：图元素的批量增删（`add` / `remove` / `removeAll`）。
- `data.mjs`：实例级数据与 scratch 内存区。
- `events.mjs`：核心级事件（点击背景、resize 等）。
- `viewport.mjs`：视口控制——平移/缩放（`pan`/`zoom`/`center`/`fit`）、选中/锁定模式（`autolock`、`autoungrabify`、`autounselectify`）、单选/加选模式（`selectionType`）、框选与点击选择状态。
- `layout.mjs`：布局实例的创建、运行、停止回调管理。
- `style.mjs`：样式表挂载与按需重算。
- `search.mjs`：按 id/selector 查找元素。
- `export.mjs`：导出图片（jpg/png）与 JSON 图数据。
- `notification.mjs`：警告/错误通知通道。
- `renderer.mjs`：渲染器实例管理（渲染器可通过扩展替换）。
- `animation/`：核心动画框架——easing 库（cubic-bezier、spring 等）、逐帧步进（`step.mjs`/`step-all.mjs`）、动画队列启动。

## 2. 元素集合（src/collection/）

- `element.mjs`/`index.mjs`：节点/边/复合节点（compound）统一抽象，父子关系由复合节点表达。
- `data.mjs`：元素 data/scratch 字段读写，支持批量与映射。
- `class.mjs`：CSS 风格 class 的增删查。
- `filter.mjs`、`group.mjs`、`traversing.mjs`：集合过滤、按 group 分类、邻接遍历（`neighbors`、`connectedEdges`、`edgesWith`、深度/广度闭包等）。
- `iteration.mjs`：集合的 map/reduce/排序/去重等迭代工具。
- `comparators.mjs`、`zsort.mjs`：比较器与 z 序（绘制顺序）排序。
- `degree.mjs`：节点度数（入度/出度/自环）计算。
- `dimensions/`：包围盒（`bounds.mjs`）、边的端点与拐点（`edge-points.mjs`）、位置与宽高读写。
- `compounds.mjs`：复合节点（父/子/祖先/后代）关系操作，包含折叠语义相关查询。
- `style.mjs`：元素级样式读写、bypass（内联覆盖）样式。
- `events.mjs`：元素级事件绑定与触发。
- `layout.mjs`：元素集合上驱动布局。
- `animation.mjs`：元素动画（位置、样式的补间）。
- `cache-traversal-call.mjs`：遍历调用的记忆化缓存。

## 3. 图算法（src/collection/algorithms/）

全部作为集合方法注册（`index.mjs` 汇总）：

| 类别 | 算法 |
|---|---|
| 遍历 | BFS/DFS（`bfs-dfs.mjs`）、欧拉回路（`hierholzer.mjs`） |
| 最短路径 | Dijkstra、A*、Bellman-Ford、Floyd-Warshall |
| 生成树 | Kruskal 最小生成树 |
| 连通性 | Tarjan 强连通分量、Hopcroft-Tarjan 双连通分量/割点 |
| 中心性 | PageRank、度中心性、接近中心性、介数中心性 |
| 聚类 | Markov 聚类、k-means（`k-clustering.mjs`）、层次聚类、亲和传播聚类、聚类距离度量（`clustering-distances.mjs`） |

## 4. 选择器（src/selector/）

CSS 风格选择器的完整实现：词法分析（`tokens.mjs`）、语法解析（`parse.mjs`）、查询对象构建（`new-query.mjs`）、类型匹配（`type.mjs`/`query-type-match.mjs`）、匹配执行（`matching.mjs`）、表达式求值（`expressions.mjs`，支持 `data()` 比较、class、id、伪类如 `:selected`、`:parent`、 compound 关系选择符 `>`、`<` 等）。

## 5. 样式系统（src/style/）

- `properties.mjs`：全部可视化样式属性的类型化定义（约 900 行）——节点形状（30+ 种）、边曲线样式（bezier/haystack/segments/taxi 等）、箭头形状、颜色/渐变填充、标签排版、背景图片、z-index 策略等；支持 `data()`/`mapData()` 数据映射、函数映射、单位（px/em/%/deg）。
- `stylesheet.mjs`（根目录）：样式表对象（selector + style 块）。
- `apply.mjs`：样式应用到元素并触发失效重算。
- `parse.mjs`：属性值解析与校验。
- `bypass.mjs`：运行时内联样式覆盖。
- `json.mjs`：样式表 JSON 导入导出。
- `get-for-ele.mjs`：按元素解析最终样式值（含映射求值）。
- `align.mjs`、`container.mjs`、`string-sheet.mjs`：对齐辅助、容器样式、字符串池。

## 6. 渲染器（src/extensions/renderer/）

- `base/`：渲染器公共基类契约。
- `null/`：空渲染器（headless 模式，无 DOM 也能跑算法）。
- `canvas/`：默认 Canvas 渲染器，多层画布结构：
  - 分层：SELECT_BOX（框选层）、DRAG（拖拽层）、NODE（主层），可选第 4 层 WEBGL；另配 3 个离屏 buffer（纹理缓存、节点/拖拽运动模糊 buffer）。
  - `drawing-nodes.mjs` / `drawing-edges.mjs` / `drawing-shapes.mjs` / `arrow-shapes.mjs` / `node-shapes.mjs`：节点/边/形状/箭头几何绘制，含形状命中路径。
  - `drawing-label-text.mjs`：文本排版与绘制（换行、省略、旋转、背景）。
  - `drawing-images.mjs`：图片加载、缓存与绘制。
  - `drawing-elements.mjs`：元素绘制调度与可见性裁剪。
  - `drawing-redraw.mjs`：脏区域重绘调度与运动模糊。
  - `ele-texture-cache.mjs` / `layered-texture-cache.mjs` / `ele-texture-cache-lookup.mjs` / `texture-cache-defs.mjs`：元素级纹理缓存（把节点渲染为位图，缩放/平移时直接贴图，是大数据量性能的关键）。
  - `export-image.mjs`：离屏导出高清图片。
  - `webgl/`：WebGL2 增强路径——SDF 着色器（`shader-sdf.mjs`）绘制形状、图集纹理管理（`atlas.mjs`）、FXAA 上采样（`fxaa-upscaler.mjs`），用于超大规模图。

## 7. 布局（src/extensions/layout/）

内置布局（注册表以 `src/extensions/layout/index.mjs` 为准，当前 8 种），统一 Layout 契约（options/stop 生命周期）：

| 布局 | 策略 |
|---|---|
| `null` | 不动 |
| `random` | 随机布点 |
| `preset` | 使用预设坐标 |
| `grid` | 网格 |
| `circle` | 圆环 |
| `concentric` | 同心圆（按度数等排序值分层） |
| `breadthfirst` | BFS 树状分层（支持有向/无向、复合节点） |
| `cose` | 力导向（compound spring embedder，支持复合节点、节点重叠避免） |

（另有无 `cola`/`dagre`/`elk` 等外部布局，属独立扩展包，不在此源码树内。）

## 8. 扩展机制与基础设施

- `extension.mjs` / `extensions/index.mjs`：`cytoscape.use()` 注册布局/渲染器扩展的机制。
- `define/`：给核心与集合批量挂载方法（data/events/animation 的 API 定义辅助）。
- `emitter.mjs` / `event.mjs`：自研发布订阅（命名空间、去抖事件）。
- `map.mjs` / `set.mjs`： ES Map/Set 的垫片与辅助。
- `math.mjs`：向量运算、包围盒（`makeBoundingBox`）等几何数学。
- `heap.mjs`：二叉堆（算法用）。
- `is.mjs`：类型判断。
- `promise.mjs`、`window.mjs`、`round.mjs`、`cjs.mjs`：环境适配与工具。
- `util/`：颜色解析转换（`colors.mjs`）、深浅拷贝扩展（`extend.mjs`）、哈希（`hash.mjs`）、memoize（`memoize.mjs`）、正则集（`regex.mjs`）、排序（`sort.mjs`）、时间格式（`timing.mjs`）、字符串（`strings.mjs`）、位置（`position.mjs`）。

## 9. 与 compograph 关注点的对照线索

cytoscape.js 与 compograph 目标相似（图模型 + 布局 + 渲染 + 交互），其实现中值得对照研读的点：

1. **纹理缓存策略**（`canvas/ele-texture-cache*.mjs`）：节点渲染为位图按缩放级别缓存，是 Canvas 路径支撑大规模图的核心手段——对 cg-render 的绘制计划设计有直接参照价值。
2. **分层画布 + 脏区重绘**（`drawing-redraw.mjs`）：框选/拖拽与静态内容分层、最小重绘范围，对应 cg-render 的绘制计划分层。
3. **数据映射样式**（`style/properties.mjs` 的 `data()`/`mapData()`）：以数据驱动视觉映射（如节点大小 ∝ 度数），比手写映射函数更声明式。
4. **复合节点语义**（`collection/compounds.mjs`）：父子包含、折叠、邻接计算贯穿遍历/布局/渲染，是图模型设计的重要参考。
5. **布局生命周期契约**：统一 options/init/stop + promise 回调，cg-layout 的 `LayoutEngine` trait 可对照其接口划分。
6. **WebGL SDF 路径**：Canvas 之上叠加 WebGL2/SDF 提升大规模渲染上限，是渲染层扩展的演进方向示例。
7. **headless 能力**：null 渲染器使算法层完全脱离 DOM，验证了"内核与渲染解耦"的可行结构，与 compograph 的 crate 分层理念一致。
