# Cytoscape.js 架构设计分析

> 分析对象：cytoscape.js（随仓库 vendored 于 `ref/cytoscape-js`，版本见其 `package.json`，当前 3.34.3）
> 定位：图论（网络）库，同时提供**图数据模型**与**可选的交互式渲染器**，面向浏览器与 Node.js 服务端分析。

---

## 0. 一句话定位

Cytoscape.js 是一个**「图模型 + 渲染 + 交互」三位一体的前端图论库**。它既能在无头（headless）模式下做纯图分析，也能挂载 Canvas/WebGL 渲染器做可视化。这与 petgraph（纯计算、无渲染）形成鲜明对照。

---

## 1. 工程与构建结构

| 维度 | 说明 | 依据 |
| --- | --- | --- |
| 入口 | `src/index.mjs` 导出 `cytoscape` 工厂函数；`cytoscape(opts)` 返回 `Core` 实例，`cytoscape('name')` 走扩展注册 | `src/index.mjs:11-18` |
| 构建 | Rollup 打包，输出 ESM（`dist/cytoscape.esm.mjs`）与 CJS（`dist/cytoscape.cjs.js`），Babel 转译 | `package.json:33-40`、`rollup.config.mjs`、`.babelrc` |
| 类型 | `index.d.ts` 提供完整 TS 类型 | `package.json:30` |
| 模块粒度 | 源码全部为 ES Module（`.mjs`），按**领域职责**而非算法划分目录 | `src/` 目录树 |

源码根目录 `src/` 划分为：

```
src/
  index.mjs            # 顶层工厂 + extension 注册入口
  core/                # Core 类（图模型编排器）
  collection/          # Collection 类（元素集合 + 算法/操作 mixin）
  extensions/          # 内置扩展：layout、renderer
  selector/            # 选择器（CSS 式查询）解析与匹配
  style/               # 样式表解析、应用、bypass
  define/              # define 辅助（data/events/animation）
  util/  is.mjs  emitter.mjs  event.mjs  extension.mjs  stylesheet.mjs ...
```

---

## 2. 核心对象模型：Core 与 Collection

### 2.1 Core（图实例编排器）

`Core` 构造函数位于 `src/core/index.mjs:19-187`，所有实例状态集中在 `this._private`（`src/core/index.mjs:61-96`）：

- `elements`：一个 `Collection` 实例，承载全部节点/边（`core/index.mjs:65`）
- `renderer` / `layout`：渲染器与布局器实例（`core/index.mjs:70-71`）
- 视口状态：`zoom`、`pan`、`minZoom/maxZoom`、`boxSelectionEnabled` 等（`core/index.mjs:74-95`）

Core 通过 **mixin 模式**把若干职责模块挂到原型上（`src/core/index.mjs:506-520`）：

```
addRemove  animation  events  export  layout
notification  renderer  search  style  viewport  data
```

即 Core 同时承担：增删元素、动画、事件、导出、布局驱动、渲染通知、查询（search）、样式、视口控制、核心数据。这是典型的「胖核心 + 横切 mixin」组织方式。

### 2.2 Collection（元素集合）

`Collection`（`src/collection/index.mjs`）是对一组元素（node/edge）的轻量包装，**所有图算法都挂在 `Collection.prototype` 上**（见第 4 节）。其 mixin 包括：

```
algorithms/  animation.mjs  class.mjs  comparators.mjs  compounds.mjs
data.mjs  degree.mjs  dimensions/  element.mjs  events.mjs  filter.mjs
group.mjs  iteration.mjs  layout.mjs  style.mjs  traversing.mjs  zsort.mjs
```

`collection/algorithms/index.mjs:21-44` 把 18 个算法模块 `util.extend` 合并进 `elesfn`（即 `Collection.prototype`）。

---

## 3. 扩展机制（Extension System）

`src/extension.mjs` 是整个库**可插拔架构**的支柱，定义了四种扩展类型（type）：

| type | 注册目标 | 行为 | 依据 |
| --- | --- | --- | --- |
| `core` | `Core.prototype[name]` | 新增 Core 方法；若原型已存在则禁止覆盖（`extension.mjs:23-28`） | `extension.mjs:23-28` |
| `collection` | `Collection.prototype[name]` | 新增集合方法 | `extension.mjs:30-35` |
| `layout` | 包装后的 `Layout` 类 | 自动合成 `.run()/.start()`、`.stop()`、`.destroy()` 与事件系统 | `extension.mjs:37-133` |
| `renderer` | 继承 `base` 的 `Renderer` 类 | 用户渲染器以 `BaseRenderer` 为父类，未实现的方法报缺失（`clientFunctions` 清单） | `extension.mjs:137-173` |

机制要点：

- 扩展注册表是二维/多维 map：`extensions[type][name]` 与 `modules[type][name][moduleType][moduleName]`（`extension.mjs:180-207`）。
- `cytoscape.use(ext)` 即把扩展挂进注册表（`src/index.mjs:18-25`）。
- 内置扩展通过 `incExts.forEach(... setExtension ...)` 自动注册（`extension.mjs:240-244`）。
- 安全护栏：禁止注册 `__proto__/constructor/prototype` 类型，防原型污染（`extension.mjs:175-178`）。

---

## 4. 内置算法（Collection 算法 mixin）

算法按文件挂到 `Collection.prototype`（`collection/algorithms/index.mjs:1-46`）：

| 文件 | 算法 |
| --- | --- |
| `dijkstra.mjs` | Dijkstra 最短路径 |
| `a-star.mjs` | A* 最短路径 |
| `bellman-ford.mjs` | Bellman-Ford（含负权检测） |
| `floyd-warshall.mjs` | 全源最短路 |
| `bfs-dfs.mjs` | BFS / DFS 遍历 |
| `kruskal.mjs` | 最小生成树（Kruskal） |
| `tarjan-strongly-connected.mjs` | 强连通分量（Tarjan） |
| `hopcroft-tarjan-biconnected.mjs` | 双连通分量 / 关节点 |
| `page-rank.mjs` | PageRank |
| `degree-centrality.mjs` | 度中心性 |
| `closeness-centrality.mjs` | 接近中心性 |
| `betweenness-centrality.mjs` | 介数中心性 |
| `markov-clustering.mjs` | Markov 聚类（MCL） |
| `k-clustering.mjs` | k-means / k-medoids / 模糊 C-means |
| `hierarchical-clustering.mjs` | 层次聚类（HCA） |
| `affinity-propagation.mjs` | 亲和传播聚类 |
| `hierholzer.mjs` | Euler 路径/回路（Hierholzer） |
| `karger-stein.mjs` | 最小割（Karger-Stein 随机化） |

聚类辅助：`clustering-distances.mjs`（距离度量，供聚类算法复用）。

> 注意：这些是**方法式算法**，运行在「活的」图模型之上，结果可直接写回元素 data / 样式，适合做交互式可视化分析。

---

## 5. 渲染管线（Renderer）

渲染器是可选扩展，体现「模型与视图解耦」：

- **BaseRenderer**（`extensions/renderer/base/index.mjs:11`）：定义通知机制 `notify(eventName, eles)`（`base/index.mjs:120-167`）、渲染循环、选择框、指针/触摸状态、节点/箭头形状注册。
- **CanvasRenderer**（`extensions/renderer/canvas/`）：基于 2D Canvas 的绘制，细分 `drawing-nodes/edges/labels/images/shapes`，并配 `ele-texture-cache`、`layered-texture-cache` 做纹理缓存加速（`canvas/index.mjs` 及相关文件）。
- **WebGL 子管线**（`canvas/webgl/`）：`drawing-elements-webgl`、`atlas`、`shader-sdf`、`fxaa-upscaler` 等，用于大规模图的高性能渲染。
- **null 渲染器**（`renderer/null/`）：headless 模式，仅做数据模型不做绘制。

渲染器以 `clientFunctions = ['redrawHint','render','renderTo','matchCanvasSize','nodeShapeImpl','arrowShapeImpl']`（`base/index.mjs:15`）作为必须实现的契约接口。

---

## 6. 选择器与样式

- **选择器**（`selector/`）：支持 CSS 式字符串选择器、集合选择器、函数过滤器；`Selector` 构造后 `parse()` 编译、`matching()` 执行匹配（`selector/index.mjs:8-51`）。子模块：`parse`、`matching`、`query-type-match`、`tokens`、`type`、`state`、`data`、`expressions`。
- **样式**（`style/`）：样式表解析（`parse`）、应用（`apply`）、bypass（临时覆盖）、JSON 序列化、属性定义（`properties`）。

---

## 7. 架构特征小结

| 特征 | 表现 |
| --- | --- |
| 设计范式 | 对象模型（Core/Collection）+ 横切 mixin + 插件式扩展 |
| 关注点分离 | 模型（core/collection）、视图（renderer）、查询（selector）、样式（style）分目录 |
| 可扩展性 | `extension.mjs` 统一 core/collection/layout/renderer 四类扩展点 |
| 可视化能力 | 内置 Canvas/WebGL 渲染 + 8 种布局算法 |
| 计算能力 | 18 个图算法，偏「网络分析 + 聚类 + 中心性」 |
| 语言/构建 | 纯 ES Module + Rollup + Babel |

**与 petgraph 的本质差异**：Cytoscape.js 是「图分析 + 可视化」一体化前端库；petgraph 是纯服务端/嵌入式计算库，无渲染、无交互，以泛型 trait 抽象图类型。
