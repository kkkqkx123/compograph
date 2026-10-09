# compograph 架构设计（Architecture Design）

> 版本：v1.0（基于 `docs/compograph-design.md` 初步设计整合、并对照本地来源逐项核验后定稿）
> 日期：2026-09-28
> 事实基线（版本以仓库内实际来源为准，文档不维持提交号，行号可能随上游同步漂移）：
>
> | 项目 | 位置 | 版本口径 | 核验状态 |
> |---|---|---|---|
> | petgraph | crates.io 依赖 | 以 `Cargo.lock` 为准（当前 0.8.3） | ✅ 与设计文档基线一致 |
> | gpui（`gpui-pre` 快照） | crates.io 依赖（无仓库内源码） | 以根 `Cargo.toml` 精确 pin 与 `Cargo.lock` 为准（当前 `=0.3.8`，zed@279fe07 快照） | ✅ 行号核验通过 |
> | cytoscape.js | `ref/cytoscape-js`（随仓库 vendored） | 见其 `package.json`（当前 3.34.3） | ✅ 行号核验通过 |
>
> 配套文档：[功能清单](./feature-list.md) · [借鉴设计说明](./borrowing-design.md) · 上游分析 [`../ref/`](../ref/README.md)

---

## 0. 一句话定位

compograph 是基于 **gpui**（Zed 的 GPU 加速 Rust 原生 GUI 框架）的桌面图可视化应用：用 **petgraph 承担"图是什么、图怎么算"**（数据模型 + 算法内核），用**自研绘图库承担"图怎么摆、怎么画、怎么交互"**（布局 + 渲染 + 交互），以此在纯 Rust 桌面端补齐 cytoscape.js 的能力空缺。

**设计总原则**：petgraph 只管图与算法、不存坐标；布局层只产坐标；渲染层只消费"结构 + 坐标"。三层通过"坐标外挂 + 变更事件"解耦。

---

## 1. 分层架构

```
┌──────────────────────────────────────────────────────────────┐
│  应用壳（gpui App / Window / Entity）                          │
│  GraphView：画布容器、命令面板、工具栏、算法结果面板              │
└───────────────┬───────────────────────────┬──────────────────┘
                │ Entity::subscribe 订阅      │ Entity::subscribe 订阅
        ┌───────▼────────┐          ┌─────────▼──────────┐
        │ L3 渲染/交互层  │          │ L2 布局引擎(自研)    │
        │ compograph-     │◄──坐标───│ LayoutEngine trait  │
        │ render/interact │          │ 力导向/层次/圆形/…    │
        │ 图元映射/相机/   │          └─────────┬──────────┘
        │ 拾取/拖拽/选择   │                    │ 只读遍历图结构
        └───────┬────────┘                    ▼
        ┌───────▼────────────────────────────────────────────┐
        │ L1 模型/算法层：cg-graph                      │
        │ Entity<GraphStore> = StableGraph + 算法桥 + 变更事件  │
        │ 只存图、只算算法，不画图、不存坐标                      │
        └──────────────────────────────────────────────────────┘
```

**数据流（事件驱动，单向依赖）**：

1. 用户/业务改动图 → `GraphStore`（L1）发 `GraphChangeEvent`（增删节点/边、改属性）。
2. `LayoutEngine`（L2）订阅变更 → 全量或增量重算坐标 → 写入 `PositionStore` 并 notify。
3. `GraphRenderer`（L3）订阅"结构 + 坐标"变更 → `cx.notify()` 触发 gpui 重绘 → `paint` 闭包把 `StableGraph` + `PositionStore` 映射为 `Quad`/`Path` 图元注入 `Scene`。
4. 指针输入 → `cg-interact` → 反变换（相机逆矩阵）→ 命中查询（空间索引）→ 改 `Camera`/`Selection`/`PositionStore` → notify 重绘。

---

## 2. 模块划分与依赖方向

> 2026-09-28 起项目结构已按本节初始化（workspace 解析验证通过）；gpui 以 crates.io 的 `gpui-pre` 快照依赖引入，仓库内不再保留上游源码。

```
compograph/
├── Cargo.toml                 # workspace：依赖集中在 [workspace.dependencies]（含 gpui-pre 精确 pin）
├── rust-toolchain.toml        # 本仓库钉定工具链（当前 1.98.1）
├── crates/
│   ├── foundation/
│   │   ├── cg-types/          # 几何原语（叶子）
│   │   ├── cg-geometry/       # 命中测试与曲线几何（框架无关）
│   │   └── cg-graph/          # L1：GraphStore + petgraph 适配 + 算法桥 + 变更事件
│   │                          #     store.rs / events.rs / binding.rs / positions.rs
│   ├── engine/
│   │   └── cg-layout/         # L2：LayoutEngine trait + 注册表 + 布局实现
│   │                          #     engine.rs / registry.rs / preset.rs / random.rs
│   │                          #     reaction.rs（变更→工作量映射）/ driver.rs（订阅驱动）
│   ├── render/
│   │   ├── cg-render/         # L3：GraphView、Camera、绘制计划、空间索引
│   │   └── cg-interact/       # L3：拖拽/平移/缩放/选择 状态与事件处理
│   └── app/
│       └── compograph/        # 主应用二进制：gpui 引导、命令面板、工具栏
```

gpui 以 crates.io 快照依赖引入、自建 crate 独立分层：仓库内没有上游源码，本项目自身的代码一眼可见。gpui 与 `gpui_platform` 在根 `Cargo.toml` 的 `[workspace.dependencies]` 中以精确版本 pin（见 AGENTS.md GPUI 依赖规范）。

依赖方向（单向 DAG，禁止循环）：

```
cg-types     ← cg-geometry
cg-types     ← cg-graph（另依赖 petgraph + gpui 的 EventEmitter 标记）
cg-graph     ← cg-layout
cg-graph + cg-layout + cg-geometry ← cg-render ← cg-interact
以上全部 ← compograph（另依赖 gpui_platform）
gpui-pre 快照内部各 crate 互依，绝不反向依赖 cg-*
```

cg-graph 依赖 gpui 仅因 `EventEmitter` 标记 trait 必须在类型定义侧实现（孤儿规则）；cg-layout 依赖它是为了用 `Context`/`App` 建立变更订阅。

**解耦选项（推荐）**：L2/L3 不直接依赖 `StableGraph` 实体，而是依赖 core 暴露的只读视图 trait（基于 petgraph `visit` 的 `IntoNeighbors:107` / `IntoNodeIdentifiers:183` / `IntoEdges:147` 等，见 `petgraph/src/visit/mod.rs`）。好处：布局/渲染可用轻量 mock 图做 headless 单测，未来可替换图实现。

### 2.1 gpui 为何是 crates.io 快照而非仓库内源码

**决策：gpui 以 crates.io 的 `gpui-pre` 快照依赖引入，精确 pin；仓库内不保留上游源码（不 vendored、不挂 submodule）。**

历史上本项目曾以 submodule 挂载 zed-gpui `lean` 快照并以 path 依赖消费，2026-10-09
切换为 gpui-pre。完整分析与取舍见 gpui-kit 仓库 `docs/plan/compograph-gpui-pre-migration.md`。

| 维度 | 仓库内源码（submodule + path，已否决） | crates.io 快照（当前） |
|---|---|---|
| 与 gpui-kit 的关系 | 两套不同来源的 gpui 类型互不兼容，图视图无法嵌入 gpui-kit 应用 | 与 gpui-kit 同一精确 pin、同一 API 基线、同一依赖解析 |
| 同步职责 | 需自建 fork 的同步、workspace 展开、快照发布流程 | 升级 = 修改一个精确版本号并复核 API 签名 |
| 依赖解析 | 连带 git 依赖（proptest/scap/wasm_thread 的 zed fork），受限网络需 GitHub 代理 | 全部来自 crates.io（镜像可解析） |
| 代码可读性 | 27 个上游包进入仓库，淹没自建 crate | 仓库内只有自建 crate |
| manifest 继承 | path 依赖的 `workspace = true` 会越过仓库边界上溯到本工作区，须在发布时展开 | 发布 manifest 已规范化，本项目零配合 |

---

## 3. 核心数据模型（L1）

### 3.1 图容器：`StableGraph`（理由）

图表是**可编辑**的。petgraph 普通 `Graph`（`graph_impl/mod.rs:392`）删除节点后索引会复用，导致坐标缓存、UI 状态与算法结果错位；`StableGraph`（`graph_impl/stable_graph/mod.rs:67`）删除后索引保持稳定，是编辑器场景的正确选择。代价为轻微内存/遍历开销，可接受。

### 3.2 共享状态与变更事件（⚠️ 对初步设计的重要修正）

初步设计写的是 gpui `Model<GraphStore>`。经对照上游快照实际 API 核验：**该版本 gpui 已无 `Model<T>`，共享状态类型为 `Entity<T>`**（`crates/gpui/src/app/entity_map.rs:435`，行号以快照为准）。事件与通知机制为：

| 能力 | API | 位置（gpui-pre 快照） |
|---|---|---|
| 共享状态容器 | `Entity<T>` | `src/app/entity_map.rs:435` |
| 事件广播 | `EventEmitter` trait + `App::subscribe` | `src/app.rs:1353` |
| 重绘通知 | `Context::notify` / `App::notify` | `src/app/context.rs:221`、`src/app.rs:2932` |

因此 L1 的形态为：

```rust
// crates/foundation/cg-graph/src/store.rs
pub struct GraphStore {
    graph: StableGraph<NodeData, EdgeData, Directed>,
}

// 每个变更方法都接收 Context，改动成功后广播事件并请求重绘
pub fn add_node(&mut self, cx: &mut Context<Self>, label: impl Into<String>) -> NodeIndex;

// crates/foundation/cg-graph/src/events.rs
pub enum GraphChangeEvent {
    NodeAdded(NodeIndex), NodeRemoved(NodeIndex),
    EdgeAdded(EdgeIndex), EdgeRemoved(EdgeIndex),
    StructureReset, // 批量清空等
}

// 主应用侧：
// let store: Entity<GraphStore> = cx.new(|_| GraphStore::new());
// store.update(cx, |graph, cx| graph.add_node(cx, "a")) → 订阅者（布局/渲染）响应
```

消费侧不直接 match 全部变体，而是用 `ChangeFilter` 声明关心的类别：

```rust
// crates/foundation/cg-graph/src/binding.rs
pub struct ChangeFilter { pub structure: bool, pub topology: bool, pub reset: bool }
// ChangeFilter::ALL / ::NODES / ::EDGES

pub fn subscribe_graph<T>(
    cx: &mut Context<T>,
    store: &Entity<GraphStore>,
    filter: ChangeFilter,
    on_change: impl FnMut(&mut T, &GraphChangeEvent, &mut App) + 'static,
) -> Subscription;
```

布局侧的响应策略见 `crates/engine/cg-layout/src/reaction.rs`：`work_for(event) -> LayoutWork` 把变更映射为所需工作量（新增节点只需 `PlaceNew`；边改动或删点需要 `RefreshEdgeGeometry`；清空则 `Full`），由 `LayoutDriver` 执行，避免每次改动都全量重算。

> **派发时序**：`Context::emit` 只把 `Effect::Emit` 压入 `pending_effects`，真正的派发在最外层 `App::update` 结束时由 `flush_effects` 统一处理（`app.rs:1176`）。因此测试中「变更」与「断言」必须分处两次 update 调用，见 `crates/engine/cg-layout/tests/event_flow.rs`。

### 3.3 坐标外挂（petgraph 不存坐标）

petgraph 图模型中无"位置"概念，坐标由 L2 维护在独立 `PositionStore`：

```rust
pub type Positions = HashMap<NodeIndex, (f32, f32)>;   // 模型坐标
pub struct PositionStore {
    positions: Positions,
    fixed: HashSet<NodeIndex>,   // 被用户拖拽固定的节点
}
```

### 3.4 算法桥（直接复用 petgraph，零重写）

已在 petgraph 当前版本（以 `Cargo.lock` 为准）逐项核验的算法入口（全部纯 headless、不依赖渲染/坐标）：

| 算法 | API | 位置 |
|---|---|---|
| 最短路 | `dijkstra` / `astar` | `algo/dijkstra.rs:92` / `algo/astar.rs:81` |
| SCC | `tarjan_scc` | `algo/scc/tarjan_scc.rs:269` |
| 中心性 | `page_rank` | `algo/page_rank.rs:64` |
| MST | `min_spanning_tree` / `min_spanning_tree_prim` | `algo/min_spanning_tree.rs:86` / `:255` |
| 最大流 | `ford_fulkerson` | `algo/maximum_flow/ford_fulkerson.rs:164` |
| 遍历 trait 簇 | `IntoNeighbors:107` / `IntoNodeIdentifiers:183` / `IntoEdges:147` | `visit/mod.rs` |

完整算法清单见 [功能清单 §算法 UI 桥接](./feature-list.md)。

---

## 4. 布局引擎设计（L2，自研主体）

### 4.1 统一接口

```rust
// crates/engine/cg-layout/src/lib.rs
pub trait LayoutEngine: 'static {
    /// 由图结构 + 上一次坐标计算本次坐标；prev 非空时支持增量
    fn layout(&self, graph: &dyn GraphView, prev: &Positions) -> Positions;
    fn name(&self) -> &'static str;
}
```

注册模式借鉴 cytoscape.js 的"name→impl 表"（`src/extensions/layout/index.mjs`，一个 `{name, impl}` 数组注册 8 种内置布局）；桥接契约借鉴其 `layoutPositions`（`src/collection/layout.mjs:41`）——"布局算完坐标 → 统一写回 → 触发重绘事件"。

### 4.2 布局清单

| 布局 | 数学来源 | 说明 |
|---|---|---|
| Force-directed (CoSE/FR) | cytoscape `cose.mjs`（`run:126`，物理步 `step:705`） | 斥力/引力/温度退火，自包含可直译；或改用 d3-force 风格（开放点） |
| Circular / Grid / Random / Preset | cytoscape 对应 `*.mjs` | 纯几何排布，启发式（spacingFactor/avoidOverlap 等）直译 |
| Breadth-first / Concentric / Radial | cytoscape `breadthfirst.mjs:42` / `concentric.mjs:37` | 按层/按半径排布 |
| Hierarchical（DAG 分层） | petgraph 辅助 + 自研坐标分配 | petgraph 提供 toposort / `tred` / `dominators`（`algo/dominators.rs:73`）作分层子步骤 |

### 4.3 执行模型

- **增量布局**：结构小改时固定未变节点，只对新节点/被拖节点做局部力导向，避免整图抖动。
- **后台线程**：力导向迭代昂贵，放 gpui `App::spawn`（`src/app.rs:2177`）/ `background_executor`（`src/app.rs:311`）执行，分批回写 `PositionStore` 并 notify，形成动画式收敛，不阻塞 UI。

---

## 5. 渲染层设计（L3）

### 5.1 三条路线与选型

| 维度 | A. `gpui::canvas` 即时模式 | B. 自定义 Element + 保留场景 | C. wgpu 实例化 |
|---|---|---|---|
| 实现复杂度 | 低（闭包即可，`elements/canvas.rs:10`） | 中（自管缓存/脏标记，`element.rs:53`） | 高（深入 GPU 内部） |
| 单图规模 | 数百~数千节点 | 数千~数万 | 十万级 |
| 与 gpui 升级解耦 | 高 | 中 | 中（gpui-pre 提供 `gpui_wgpu`，见 5.4） |
| 每帧开销 | 高（重建图元） | 低（增量 flush） | 最低 |
| 建议 | **v1 落地** | **v2 演进** | 按需（v3） |

结论：**A → B 渐进**；C 仅在目标规模确证十万级且 B 不够时评估。

### 5.2 图元映射（gpui 原语，行号已核验）

| 视觉元素 | gpui API | 位置（gpui-pre 快照） |
|---|---|---|
| 边（直线） | `Path::move_to`/`line_to` → `scene().insert_primitive` | `scene.rs:840/847` |
| 边（曲线） | `Path::curve_to` | `scene.rs:859` |
| 节点 | `window.paint_quad(PaintQuad)` | `window.rs:4550` |
| 箭头 | `Path::push_triangle` | `scene.rs:876` |
| 场景容器 | `Scene` / `Primitive`（Quad/Path/Shadow/…，按类型批处理） | `scene.rs:41/222` |
| 相机变换 | `TransformationMatrix` | `scene.rs:608`（compose `:658`） |

### 5.3 相机与拾取

- `Camera { offset: (f32,f32), zoom: f32 }`：v1 用 CPU 侧仿射（公式思路借 cytoscape `math.mjs:9/14` 的 `modelToRenderedPosition`），进阶用 `TransformationMatrix` 做 GPU 级变换。
- 拾取：gpui 只有元素级命中，**节点/边级命中需自建空间索引**（四叉树/均匀网格）。命中数学直译 cytoscape `coords.mjs`：`findNearestElement:75`、`getAllInBox:323`（框选）；边命中用贝塞尔采样距离（`edge-projection.mjs:5`）或简化为中垂线距离（开放点）。

### 5.4 路线 C 的新证据（快照特有）

gpui 快照把 wgpu 后端独立为 `gpui_wgpu` crate（gpui-pre 对应 `gpui-pre-wgpu`），且 `WgpuContext` 公开暴露 `pub device: Arc<wgpu::Device>`、`pub queue: Arc<wgpu::Queue>`（`wgpu_context.rs:14-15`）。相比原设计"路线 C 依赖 gpui 内部上下文、最脆弱"的判断，**该快照上路线 C 的可行性显著提高**，但仍属后期选项。

---

## 6. 交互系统（L3）

| 交互 | 实现 | 备注 |
|---|---|---|
| 平移 | 空白处拖拽 → 改 `Camera.offset` | 即时重绘 |
| 缩放 | 滚轮/捏合 → 改 `Camera.zoom`（光标锚点） | CPU 仿射或 `TransformationMatrix` |
| 节点拖拽 | 命中后 `fixed` 节点 + 移动坐标 | 触发局部重布局（§4.3） |
| 点选 / 框选 | 指针事件 + 空间索引 | 框选直译 `getAllInBox`（`coords.mjs:323`） |
| 高亮 | 改 `Quad` 描边/填充色 | 借鉴 cytoscape `bypass` 式覆盖而非改主样式 |
| 悬停 tooltip | gpui popover 组件 | 复用 gpui 生态 |

---

## 7. 性能与规模策略

- **渲染**：路线 A 每帧重建图元有上限；v2 升级保留模式（B）+ 空间索引 + **LOD**（缩放低于阈值后仅画点/线）。gpui `Scene` 按 `Primitive` 类型批处理（`scene.rs` batches），同类图元减少状态切换。
- **布局**：后台线程分批迭代回写，动画式收敛。
- **算法**：大图算法（Floyd-Warshall、同构、最大流等）一律 `App::spawn` 后台执行，结果经事件回传 UI。
- **目标规模**为开放点（千/万/十万），直接决定是否需要路线 B/C 与 LOD。

---

## 8. 风险与待拍板开放点

| 编号 | 开放点 | 影响 | 建议 |
|---|---|---|---|
| OQ-1 | gpui API 版本复核 | 全部渲染代码 | ✅ 已部分解决：上游快照行号已核验（行号可能随同步漂移，使用时以实际源码为准）；`Model`→`Entity` 修正见 §3.2 |
| OQ-2 | 渲染路线 A/B/C | 架构 | 默认 A→B 渐进 |
| OQ-3 | 目标图规模 | LOD/索引/路线 | 需产品拍板 |
| OQ-4 | L2/L3 是否仅依赖 visit trait | 可测试性 | 推荐是（§2 解耦选项） |
| OQ-5 | 标签/文本渲染方案 | 标签可读性 | ✅ 已定并落地：节点标签走 gpui 文本系统（`TextSystem::shape_line` + `ShapedLine::paint`），单行居中；边标签与多行/中文换行待做 |
| OQ-6 | 算法 UI 暴露范围 | 功能面 | 见功能清单 §6 |
| OQ-7 | 视觉保真度（贝塞尔/箭头/子图/动画） | 自研量 | v1 先直线+箭头 |
| OQ-8 | headless 测试策略 | 质量门 | gpui `TestAppContext`（`src/app/test_context.rs`） |
| OQ-B1 | 力导向物理选型（CoSE 直译 vs d3-force 风格） | 布局手感 | 先直译 cose，后评估 |

---

## 9. 实施路线

> 实测进度（2026-09-29）：下表 P0–P3 绝大部分已落地，仅"节点形状族""PNG 导出""路线 C"等少数项待做。逐项落地状态见[功能清单](./feature-list.md)与[功能实测分析报告](../plan/feature-analysis-report.md)。

| 阶段 | 内容 | 产出 |
|---|---|---|
| **P0** | workspace + `cg-graph`（`Entity<GraphStore>` + `StableGraph`）+ 路线 A 静态 `GraphView` | ✅ "petgraph 图 → gpui 画布"闭环 |
| **P1** | 力导向布局 + `Camera` 平移/缩放 + 节点拖拽 + 空间索引拾取 + 几何数学层（cytoscape 直译） | ✅ 可交互探索小图 |
| **P2** | 其余布局 + 框选/高亮 + 算法 UI 桥接（dijkstra/SCC/PageRank/最大流…） | ✅ 接近 cytoscape 可用度 |
| **P3** | 保留模式（B）+ LOD + 后台布局线程；必要时评估路线 C（`gpui_wgpu` device/queue） | ✅ 保留模式/LOD/后台布局已落地；路线 C 未启动 |

---

## 10. 下一步

1. 按 §3.2 修正结论落地 P0 workspace 骨架（`Entity<GraphStore>`）。
2. 拍板 OQ-2/OQ-3，锁定 P0 技术栈。
3. 用一张固定坐标小图验证 gpui `canvas` 绘制闭环。
4. 代码落地后同步更新本目录与 `docs/compograph-design.md` 中过期部分（更新而非标记）。
