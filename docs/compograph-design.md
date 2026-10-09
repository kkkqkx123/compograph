# compograph 引入 petgraph 与自研绘图库 — 初步设计

> **文档状态（2026-09-28）**：本方案已由 `docs/architecture/architecture-design.md`（定稿架构）与 `docs/plan/compograph-implementation-plan.md`（分阶段实施方案）取代。过期点：共享状态类型 `Model<GraphStore>` 在当时核验的上游快照中已不存在，应为 `Entity<GraphStore>`；crate 命名已改为 `crates/foundation/cg-graph`、`crates/engine/cg-layout`、`crates/render/{cg-render,cg-interact}`、`crates/app/compograph`，gpui 及其依赖来自 crates.io 的 `gpui-pre` 快照依赖。正文保留作为设计决策依据，行号与提交号均以当时快照为准，不再维持。

> 版本：初步设计（preliminary）  ·  日期：基于当时本地源码核查
> 引用基线（历史记录，不再维持）：
> - **petgraph**（当时 0.8.3，crates.io 依赖，以 `Cargo.lock` 为准）
> - **gpui**（当时 0.2.2，submodule 快照，以指针为准）
> - 前置分析：`docs/analysis/graph-algorithms-comparison.md`、`docs/architecture/cytoscape-js.md`、`docs/architecture/petgraph.md`

---

## 0. 背景与目标

**compograph** 是基于 **gpui**（Zed 的 GPU 加速 Rust 原生 GUI 框架）开发的桌面应用。本方案回答一个问题：

> 如何在 compograph 中同时引入 **petgraph**（图数据模型 + 图论算法内核）与一套**自实现的绘图库**（不依赖 cytoscape.js），形成一个"纯 Rust、可 headless、可嵌入 gpui"的图可视化能力？

动机来自上一轮分析的结论（`docs/analysis/graph-algorithms-comparison.md`）：

- **petgraph** 覆盖经典图论/组合优化算法（最短路、流、匹配、同构、着色、极大团、SCC、PageRank…），但**完全没有布局、没有渲染、不产出坐标**。
- **cytoscape.js** 覆盖内置布局 + Canvas/WebGL 渲染 + 部分网络科学算法，但其本质是一个 JS 前端库，且与 petgraph 的"算法内核"正交。

因此本方案用 **petgraph 替代 cytoscape 的"数据/算法层"**，用**自研绘图库替代其"布局/渲染层"**，二者组合后恰好补齐 cytoscape 在纯 Rust 桌面端的空缺。

**范围**：本文件是初步设计——给出模块划分、关键接口、渲染选型对比（按用户要求逐项对比）、风险与待拍板开放点。不含完整实现代码，但给出可落地的骨架与精确 API 引述。

---

## 1. 事实核查（Facts）

下表为当时实地核实的 API 落点（历史记录，行号可能随上游同步漂移，使用时以实际源码为准）。注意：gpui 迭代极快，行号需在实际钉选的 gpui 版本上**二次复核**（见 OQ-1）。

### 1.1 petgraph 关键类型与算法（当时版本，历史记录）

| 项 | 位置（当时 petgraph 源码内相对路径） | 说明 |
|---|---|---|
| `Graph<N,E,Ty,Ix>` | `crates/petgraph/src/graph_impl/mod.rs:392` | 邻接表图，索引删除后**会复用/失效** |
| `StableGraph<N,E,Ty,Ix>` | `crates/petgraph/src/graph_impl/stable_graph/mod.rs:67` | 删除后索引稳定，适合可编辑图 |
| `NodeIndex<Ix>` / `EdgeIndex<Ix>` | `graph_impl/mod.rs:113` / `:174` | 轻量句柄，可作 HashMap key |
| `DefaultIx = u32` | `graph_impl/mod.rs:29` | 默认索引类型 |
| `dijkstra` / `astar` | `algo/dijkstra.rs:92` / `algo/astar.rs:81` | 最短路（A* 含启发式） |
| `bellman_ford` / `floyd_warshall` | `algo/bellman_ford.rs:96` / `algo/floyd_warshall.rs:113` | 负权最短路 / 全源最短路 |
| `min_spanning_tree` | `algo/min_spanning_tree.rs:86` | Kruskal/Prim（MST） |
| `tarjan_scc` / `kosaraju_scc` | `algo/scc/tarjan_scc.rs:269` / `algo/scc/kosaraju_scc.rs:95` | 强连通分量 |
| `dominators` | `algo/dominators.rs:73`（方法） | 支配树 |
| `page_rank` | `algo/page_rank.rs:64` | 网络中心性 |
| `greedy_matching` / `maximum_matching` | `algo/matching.rs:209` / `:370` | 图匹配 |
| `maximal_cliques` | `algo/maximal_cliques.rs:119` | 极大团 |
| `dsatur_coloring` | `algo/coloring.rs:57` | 图着色 |
| `is_isomorphic` / `is_isomorphic_matching` | `algo/isomorphism.rs:804` / `:830` | 图同构 |
| `greedy_feedback_arc_set` | `algo/feedback_arc_set.rs:72` | 反馈弧集（DAG 化） |
| `ford_fulkerson` | `algo/maximum_flow/ford_fulkerson.rs:164` | 最大流 |
| `steiner_tree` | `algo/steiner_tree.rs:181` | 斯坦纳树 |
| 遍历 trait 簇 | `visit/mod.rs`：`IntoNeighbors:107`、`IntoNodeIdentifiers:183`、`EdgeRef:209`、`NodeIndexable:335`、`Visitable:469` | 算法与图结构解耦的基础 |

> 关键事实：**petgraph 不提供任何布局算法，也不存储坐标**。坐标层必须自研（见第 5 章）。

### 1.2 gpui 渲染接入点（当时快照，历史记录）

| 项 | 位置（当时上游快照内相对路径） | 说明 |
|---|---|---|
| `Element` trait（含 `paint`） | `src/element.rs:53` | 自定义图元需实现的接口；`paint(bounds, …, &mut Window, &mut App)` |
| `canvas(prepaint, paint)` | `src/elements/canvas.rs:10` | **低层自定义绘制入口**，无需定义完整 Element |
| `Scene` / `Primitive` 枚举 | `src/scene.rs:41` / `:222` | 可绘制图元：`Quad`、`Path`、`Shadow`、各 Sprite、`Surface` |
| `window.paint_quad(quad)` | `src/window.rs:4502` | 绘制一个四边形（节点/矩形用） |
| `window.scene().insert_primitive(Path)` | `src/scene.rs:87` | 把矢量路径插入当前帧场景 |
| `Path::new` / `move_to` / `line_to` / `curve_to` / `push_triangle` | `path_builder.rs:329` / `scene.rs:840` / `:847` / `:859` / `:876` | 矢量路径构造（边、曲线、箭头） |
| `TransformationMatrix`（`apply`） | `src/scene.rs:658` / `:690` | 相机变换（平移/缩放） |
| 四边形辅助构造 | `quad()` / `fill()` / `outline()` / `solid_quad()` | 见 `style.rs:702/730/748`、`debug_overlay.rs:119` |

> 结论：gpui 的绘制能力是**保留模式 + 批处理**（`Scene` 按 `Primitive` 类型批量，`scene.rs:172 batches()`）。自定义绘制通过 `canvas()` 闭包拿到 `&mut Window`，再用 `paint_quad` / `scene().insert_primitive` 注入 `Quad`/`Path`。**这正是自研绘图库的落点**。

---

## 2. 总体架构

四层 + 应用壳。核心原则：**petgraph 只管"图是什么、图怎么算"，自研层只管"图怎么摆、怎么画、怎么交互"**，二者通过一层薄薄的适配（坐标外挂 + 变更事件）解耦。

```
┌──────────────────────────────────────────────────────────────┐
│  应用壳（gpui App / Window / View）                            │
│  GraphView：承载画布、命令面板、工具栏                          │
└───────────────┬───────────────────────────┬──────────────────┘
                │ 订阅                        │ 订阅
        ┌───────▼────────┐          ┌─────────▼──────────┐
        │ L3 渲染/交互层  │          │ L2 布局引擎(自研)   │
        │ (gpui canvas)   │◄──坐标───│ 图 → 坐标映射        │
        │ 图元映射/相机/  │          │ 力导向/层次/圆形/…   │
        │ 拾取/拖拽/选择  │          └─────────┬──────────┘
        └───────┬────────┘                    │ 读图结构
                │ 读结构+坐标                  ▼
        ┌───────▼────────────────────────────────────────────┐
        │ L1 模型/算法层：petgraph (StableGraph + algo + visit) │
        │ 只存图、只算算法，不画图、不存坐标                     │
        └──────────────────────────────────────────────────────┘
```

**数据流（事件驱动）**：

1. 业务/用户改动图 → `GraphStore`（L1 封装）发出变更事件（增/删节点边、改属性）。
2. `LayoutEngine`（L2）订阅变更 → 全量或增量重算坐标 → 写入 `PositionStore`。
3. `GraphRenderer`（L3）订阅"结构+坐标"变更 → 调用 `cx.notify()` 让 gpui 重绘 → `paint` 闭包把 `petgraph` 结构 + `PositionStore` 映射为 `Quad`/`Path` 注入 `Scene`。

引用：`Element::paint`（`element.rs:53`）、`canvas`（`canvas.rs:10`）、`Scene/Primitive`（`scene.rs:41/222`）。

---

## 3. 模块划分（Workspace 布局）

建议把绘图能力拆成独立 crate，与 compograph 主应用解耦，便于 headless 测试与复用：

```
compograph/                              # gpui 桌面应用 workspace 根
├── crates/
│   ├── compograph/                      # 主应用：gpui App、窗口、命令面板
│   ├── compograph-core/           # L1：GraphStore + petgraph 适配 + 算法桥接 + 变更事件
│   ├── compograph-layout/         # L2：LayoutEngine trait + 具体布局实现
│   ├── compograph-render/         # L3：GraphView Element、图元映射、Camera、拾取(空间索引)
│   └── compograph-interact/       # L3：平移/缩放/拖拽/选择/框选 输入处理
└── ...
```

**依赖方向（单向，避免循环）**：

```
compograph-core   → petgraph
compograph-layout → compograph-core
compograph-render → compograph-core, compograph-layout
compograph-interact → compograph-render
compograph              → 以上全部 + gpui
```

> 若希望 L2/L3 完全不依赖 petgraph 具体类型，可在 `core` 暴露一个轻量只读视图 trait（基于 `visit` 的 `IntoNeighbors`/`IntoNodeIdentifiers` 等，见 `visit/mod.rs:107/183`），使渲染/布局层只依赖 trait 而非 `StableGraph` 实体。这是推荐的可选解耦（见 OQ-4）。

---

## 4. 核心数据模型与 petgraph 适配

### 4.1 为什么用 `StableGraph` 而非 `Graph`

图表是**可编辑**的：用户会删除节点/边。普通 `Graph`（`graph_impl/mod.rs:392`）删除后索引会被复用，导致缓存坐标、UI 状态与算法结果错位。`StableGraph`（`stable_graph/mod.rs:67`）删除后索引**稳定保留**（`swap_remove` 语义），更适合编辑器式场景。代价是轻微内存/遍历开销，对桌面图可视化可接受。

### 4.2 坐标层外挂（petgraph 不存坐标）

petgraph 的图模型里没有"位置"概念。坐标由布局层维护在独立的 `PositionStore`：

```rust
// compograph-core/src/store.rs（拟新增文件，骨架）
use petgraph::stable_graph::{StableGraph, NodeIndex, EdgeIndex};
use petgraph::Directed;

#[derive(Clone, Debug)]
pub struct NodeData {
    pub label: String,
    // 业务/样式字段（颜色、大小、分组…）按需扩展
}

#[derive(Clone, Debug)]
pub struct EdgeData {
    pub weight: f32,
}

pub struct GraphStore {
    pub graph: StableGraph<NodeData, EdgeData, Directed, u32>, // 见 stable_graph/mod.rs:67
    // 坐标不在 petgraph 内，由布局层写入 PositionStore（见第 5 章）
}

impl GraphStore {
    pub fn add_node(&mut self, data: NodeData) -> NodeIndex { self.graph.add_node(data) }
    pub fn remove_node(&mut self, n: NodeIndex) { self.graph.remove_node(n); }
    pub fn add_edge(&mut self, a: NodeIndex, b: NodeIndex, w: f32) -> EdgeIndex {
        self.graph.add_edge(a, b, EdgeData { weight: w })
    }
    // 变更后通过 subscribe/notify 广播（见 4.4）
}
```

### 4.3 算法桥接（直接复用 petgraph，无需重写）

算法调用示例（均来自 petgraph 已核实函数）：

```rust
use petgraph::algo::{dijkstra, tarjan_scc, page_rank};
// 最短路：algo/dijkstra.rs:92
let dist = dijkstra(&store.graph, src, None, |e| e.weight as f64);
// 强连通分量：algo/scc/tarjan_scc.rs:269
let sccs = tarjan_scc(&store.graph);
// 中心性：algo/page_rank.rs:64
let rank = page_rank(&store.graph, 0.85, 50);
```

> 这些算法**不依赖任何渲染/坐标**，纯 headless，可直接在测试或后台线程调用。

### 4.4 变更事件（驱动布局与重绘）

`GraphStore` 的增删改应发出变更事件（自研轻量事件总线，或复用 gpui 的 `Model`/`AppContext::subscribe`）。渲染层订阅后调用 `cx.notify()` 触发 `paint`。gpui 的 `Model`/`Entity` 机制天然支持"状态变更 → 订阅者重绘"，建议直接用 gpui 的 `Model<GraphStore>` 作为共享状态容器，省去自研事件总线。

---

## 5. 布局引擎设计（必须自研的主体之一）

petgraph **零布局能力**，因此"把图摆开"是纯自研工作。这是相对 cytoscape.js 最大的补差项（`docs/architecture/cytoscape-js.md` 列出 cytoscape 的内置布局）。

### 5.1 统一接口

```rust
// compograph-layout/src/lib.rs（拟新增，骨架）
use compograph_core::GraphStore;

pub trait LayoutEngine {
    /// 由图结构 + 上一次坐标 计算本次坐标；支持增量（prev 非空时局部更新）
    fn layout(&self, store: &GraphStore, prev: &Positions) -> Positions;
}

pub type Positions = std::collections::HashMap<petgraph::stable_graph::NodeIndex, (f32, f32)>;
```

### 5.2 必须自研的布局（petgraph 无对应物）

| 布局 | 说明 | 是否需自研 |
|---|---|---|
| Force-directed（Fruchterman–Reingold / d3-force 风格） | 力导向，通用默认布局 | **自研** |
| Circular | 环形排布 | **自研** |
| Grid | 网格排布 | **自研** |
| Concentric | 按某属性半径分层 | **自研** |
| Breadth-first（BFS 树） | 以某节点为根的辐射树 | **自研** |
| Radial | 同心圆辐射 | **自研** |
| CoSE / fCoSE | 复合弹簧/约束 | **自研** |
| Hierarchical（DAG 分层） | 有向无环图的层状排布 | **部分复用 petgraph** |

### 5.3 petgraph 可辅助的布局子步骤（减少自研量）

| 子步骤 | petgraph 引述 | 用在哪种布局 |
|---|---|---|
| 传递归约 / 分层 | `tred`（algo 目录） | Hierarchical |
| 拓扑排序分层 | `visit`/`algo` 的 toposort | Hierarchical / BFS |
| 支配树（确定根—叶） | `dominators`（`algo/dominators.rs:73`） | BFS / Radial |
| 中心性排序（决定半径） | `page_rank`（`algo/page_rank.rs:64`） | Radial / Concentric |
| 最短路径定坐标 | `dijkstra`/`bellman_ford` | 某些坐标分配策略 |

### 5.4 增量布局与后台执行

- **增量**：结构小改时，固定未变节点坐标，仅对新节点/被拖节点做局部力导向，避免整图抖动。
- **后台线程**：力导向迭代昂贵，应在 gpui 的 `App::spawn` / `background_executor` 上跑，分批把中间坐标回写 `PositionStore` 并 `notify`，实现动画式收敛，避免阻塞 UI 主线程。

---

## 6. 渲染层设计（核心，含选型对比）

### 6.1 接入点

两种落点，对应不同复杂度：

- **路线 A（推荐 v1）**：用 `gpui::canvas(prepaint, paint)`（`canvas.rs:10`）拿到 `&mut Window`，在闭包内把图映射为图元：
  - 边：`Path::new(start)` → `move_to`/`line_to`（`scene.rs:840/847`），必要时 `curve_to`（`:859`）做贝塞尔；经 `window.scene().insert_primitive(path)`（`scene.rs:87`）入场景。
  - 节点：`window.paint_quad(fill/outline(...))`（`window.rs:4502`）画圆角矩形/圆。
  - 箭头：`Path::push_triangle`（`scene.rs:876`）。
  - 标签：gpui 文本系统（`text_system`）或离屏纹理精灵。
- **路线 B（推荐 v2/大规模）**：实现自定义 `Element`（`element.rs:53`），内部维护**保留模式场景**（缓存节点/边几何），`paint` 时增量 flush 差异图元，避免每帧重建。
- **路线 C（可选 v3）**：直接取 gpui 的 wgpu `Device`/`Queue` 做实例化绘制，支撑十万级节点。最脆弱（依赖 gpui 内部 GPU 上下文），仅在对性能极端敏感时考虑。

### 6.2 图元映射小结

| 视觉元素 | gpui 实现 | 引述 |
|---|---|---|
| 边（直线） | `Path` + `move_to`/`line_to` → `insert_primitive` | `scene.rs:840/847/87` |
| 边（曲线） | `Path` + `curve_to` | `scene.rs:859` |
| 节点 | `paint_quad(fill/outline)` | `window.rs:4502` |
| 箭头 | `Path::push_triangle` | `scene.rs:876` |
| 阴影/高亮 | `Shadow` / 改 `Quad` 描边 | `Primitive::Shadow`（`scene.rs:223`） |

### 6.3 相机（平移/缩放）

`Camera { offset: (f32,f32), zoom: f32 }`。两种实现：
- 简单：先对坐标做 `offset + zoom` 变换，再构造 `Path`/`Quad`（坐标变换在 CPU 完成）。
- 进阶：用 `TransformationMatrix`（`scene.rs:658`，`apply` 在 `:690`）对整个场景做 GPU 级变换。

### 6.4 拾取 / 命中测试（自研）

gpui 只提供"元素级"命中（canvas 整个 bounds）。**图内节点级点选需自管空间索引**：四叉树 / 均匀网格，在指针事件中做点—包围盒查询。拖拽命中节点时，将其标记为 `fixed` 并触发局部重布局（见 5.4）。

### 6.5 渲染载体选型对比（按用户要求逐项对比）

| 维度 | A. gpui::canvas 即时模式 | B. 自定义 Element + 保留场景 | C. 直接 wgpu 实例化 |
|---|---|---|---|
| 实现复杂度 | 低（闭包即可） | 中（自管缓存/脏标记） | 高（深入 gpui GPU 内部） |
| 单图规模 | 数百~数千节点 | 数千~数万 | 十万级 |
| 与 gpui 升级解耦 | 高（只用公开 API） | 中（依赖 Element trait） | 低（依赖内部上下文） |
| 每帧开销 | 高（重建图元） | 低（增量 flush） | 最低 |
| 风险 | 低 | 中 | 高（api 易碎） |
| **建议** | **v1 落地** | **v2 演进** | 按需（v3 可选） |

**结论**：采用 **A → B 渐进**。v1 用 `canvas` 验证数据/布局/交互闭环；规模上来后再升级到保留模式（B）。C 仅在目标规模确证为十万级且 B 不够时再评估。

### 6.6 GraphView 骨架（路线 A）

```rust
// compograph-render/src/view.rs（拟新增，骨架）
use gpui::{canvas, Bounds, Pixels, Window, App, IntoElement, Element};
use compograph_core::GraphStore;
use compograph_layout::Positions;

pub fn graph_view(store: Model<GraphStore>, positions: Positions) -> impl IntoElement {
    canvas(
        move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        move |bounds, (), window: &mut Window, _cx: &mut App| {
            // 1) 应用相机变换（offset/zoom）
            // 2) 遍历 store.graph.edge_references() 画边：Path + move_to/line_to
            // 3) 遍历节点画 Quad（paint_quad）
            // 4) 命中/拾取状态反馈（高亮）
            let _ = bounds;
        },
    )
}
```

> 注意：`store` 应为 gpui `Model<GraphStore>`，变更时 `cx.notify()` 触发 `paint` 重绘（见 4.4）。

---

## 7. 交互系统

| 交互 | 实现 | 备注 |
|---|---|---|
| 平移 | 空白处拖拽 → 改 `Camera.offset` | 即时重绘 |
| 缩放 | 滚轮/捏合 → 改 `Camera.zoom`（以光标为锚） | 用 `TransformationMatrix` 或 CPU 变换 |
| 节点拖拽 | 命中后 `fixed` 该节点 + 移动坐标 | 触发局部重布局（5.4） |
| 点选 / 框选 | 指针事件 + 空间索引（四叉树） | 框选需自研矩形命中 |
| 高亮 | 改对应 `Quad` 描边/填充色 | `paint_quad(outline/fill)` |
| 悬停 tooltip | gpui popover | 复用 gpui 组件 |

事件流：`gpui 指针事件` → `compograph-interact` → 改 `Camera`/`Selection`/`PositionStore` → `notify` 重绘。

---

## 8. 性能与规模

- **渲染**：路线 A 每帧重建 `Path` 有开销；升级到 B（保留模式）+ 空间索引 + **LOD**（缩小到阈值后仅画节点点/边线）可支撑大图。gpui `Scene` 已按 `Primitive` 类型批处理（`scene.rs:172`），同类 `Quad`/`Path` 减少状态切换。
- **布局**：力导向放后台线程（gpui `background_executor` / `App::spawn`），分批回写坐标形成动画收敛。
- **目标规模**是开放点（见 OQ-3）：单图节点量级（千/万/十万）直接决定是否需要路线 B/C。

---

## 9. 风险与待拍板开放点

> 以下为需在落地前拍板的开放点，编号 OQ-1..OQ-8。

- **OQ-1（API 版本复核）**：本方案所有 gpui 行号基于当时的上游快照（历史记录）。gpui 迭代快，必须在实际钉选的 gpui 版本上复核 `canvas`/`Scene::Path`/`paint_quad`/`TransformationMatrix` 的签名与可用性。
- **OQ-2（渲染路线）**：A/B/C 三选（默认 A→B 渐进），需确认 v1 是否接受即时模式的上限。
- **OQ-3（目标规模）**：单图节点量级？决定 LOD、空间索引、是否上 B/C。
- **OQ-4（解耦粒度）**：布局/渲染层是否仅依赖 `visit` trait（`visit/mod.rs:107/183`）而非直接依赖 `StableGraph`？影响可测试性与替换成本。
- **OQ-5（标签/文本渲染）**：节点标签用 gpui 文本系统还是离屏纹理？中文换行/锚点策略。
- **OQ-6（算法 UI 范围）**：哪些 petgraph 算法需在 UI 暴露（最短路/SCC/PageRank/最大流…）？大图算法是否走后台线程避免卡顿？
- **OQ-7（视觉保真度）**：是否需要边弯曲（贝塞尔）、箭头、分组/子图（subgraph）、布局动画过渡？
- **OQ-8（测试策略）**：是否引入 gpui `TestAppContext`（`src/elements/canvas.rs` 等测试中已用）做 headless 渲染/拾取单测？

---

## 10. 实施路线（分阶段）

| 阶段 | 内容 | 产出 |
|---|---|---|
| **P0** | workspace + `compograph-core`（`Model<GraphStore>` + `StableGraph`）+ 路线 A 的静态 `GraphView`（能画出固定坐标的图） | 跑通"petgraph 图 → gpui 画布"闭环 |
| **P1** | `compograph-layout` 力导向 + `Camera` 平移/缩放 + 节点拖拽 + 空间索引拾取 | 可交互探索小图 |
| **P2** | 其余布局（圆形/网格/层次/径向/同心/BFS/CoSE）+ `compograph-interact` 框选/高亮 + 算法 UI 桥接（dijkstra/SCC/PageRank…） | 接近 cytoscape 可用度 |
| **P3** | 按需升级路线 B（保留模式）+ LOD + 后台布局线程 | 支撑大图 |

---

## 11. 下一步与参考

**下一步（建议顺序）**：
1. 钉选 gpui 版本并复核 OQ-1 的 API 行号。
2. 确认 OQ-2/OQ-3（渲染路线与目标规模），锁定 P0 技术栈。
3. 先实现 P0，用一张手绘固定坐标的小图验证 gpui `canvas` 绘制闭环。
4. 回填本方案随代码落地而**过期**的部分（按你的规范：更新而非仅标记）。

**参考（历史记录，位置与版本均以当时为准，不再维持）**：
- petgraph（crates.io 依赖，以 `Cargo.lock` 为准）
- gpui（submodule 快照，以指针为准）
- 前置分析：`docs/analysis/graph-algorithms-comparison.md`、`docs/architecture/cytoscape-js.md`、`docs/architecture/petgraph.md`
- cytoscape.js 的内置布局可作为自研布局的对照基线（`docs/architecture/cytoscape-js.md`，注册表以 `ref/cytoscape-js/src/extensions/layout/index.mjs` 为准）
