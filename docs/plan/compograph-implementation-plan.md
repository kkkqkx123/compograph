# compograph 分阶段实施方案

> 版本：v1.0 · 日期：2026-09-28
> 上游事实基线：zed-gpui @ `212afa4`（gpui 0.2.2）、petgraph @ `a4d94bd`（0.8.3）、cytoscape.js @ `7ba6340`
> 关联文档：[架构设计](../architecture/architecture-design.md) · [功能清单](../architecture/feature-list.md) · [借鉴设计说明](../architecture/borrowing-design.md) · [AGENTS.md](../../AGENTS.md)

---

## 0. 文档信息

本方案给出 compograph 从当前骨架状态到完整可用应用的分阶段实施路线。每个阶段以"可运行、可验证"为收口标准，行号引用均已对照本地克隆核验。方案遵循 AGENTS.md 约定：文档用中文、避免大段代码、以自然语言描述改动。

## 1. 背景与目标

- compograph 定位：petgraph 负责"图是什么、图怎么算"，自研层负责"图怎么摆、怎么画、怎么交互"，在纯 Rust 桌面端对齐 cytoscape.js 的核心能力（详见架构设计文档）。
- 项目结构已初始化：四层自建 crate（foundation/engine/render/app）+ 上游 gpui 源码（zed-gpui lean 分支展开）已就位并通过 workspace 解析。
- 目标：按阶段补齐实现，每阶段结束都有可运行产物与验收清单，避免长周期无法验证。

## 2. 事实核查（Facts）

### 2.1 项目现状（已落地，均已验证）

| 项 | 状态 | 验证方式 |
|---|---|---|
| 上游源码 | 以 git submodule 挂在 `crates/vendor/zed-gpui`（zed-gpui `lean` 分支 `212afa4`，27 个包 + `crates/refineable/derive_refineable` + `tooling/perf`），保持上游原始布局 | `git submodule status`；`.gitmodules` 记录 url/branch=lean/shallow |
| 根 Cargo.toml | members 只列自建 crate 四段 glob（统一分层，上游不再作为成员）；workspace.dependencies 只保留自建依赖 + 指向 submodule 的 `path`（如 `gpui = { path = "crates/vendor/zed-gpui/crates/gpui" }`） | `cargo metadata` 解析通过 |
| 自研 crate 骨架 | `cg-types`/`cg-geometry`/`cg-graph`/`cg-layout`/`cg-render`/`cg-interact`/`compograph` 共 7 个 | 编译 + 单测 + clippy 全绿（见下） |
| 演示主程序 | `crates/app/compograph/src/main.rs`：gpui 引导 + 12 节点链式演示图 + RandomLayout + canvas 绘制 | `cargo check --all-targets` 通过 |
| 工具链验证（沙箱实测，1.98.1） | `cargo check -p <自建 crate> --all-targets`（submodule 展开后）✅ · `cargo test -p cg-types -p cg-geometry -p cg-graph -p cg-layout -p cg-render -p cg-interact` 33/33 ✅ · clippy 自研 crate 0 警告（上游警告不修）✅ · `cargo fmt --all --check` 通过 ✅ | 命令行实测 |
| submodule 自包含验证 | 未展开的 `lean` → `error inheriting 'accesskit' from workspace root manifest's 'workspace.dependencies.accesskit'`；展开后的 `lean` → 通过 | 同一根清单两种 submodule 内容对比实测 |

> 注：`cargo test --workspace` 在本沙箱会因 gpui 自带 example 链接缺 `-lxkbcommon-x11` 而失败（缺 X11 开发库，非代码问题）；自建 crate 已全部编译并通过测试。

### 2.2 已核验的 gpui API（zed-gpui @ 212afa4）

> 下表路径省略上游前缀 `crates/vendor/zed-gpui/`，即 `crates/gpui/src/...` 实际位于 `crates/vendor/zed-gpui/crates/gpui/src/...`。

| API | 位置 |
|---|---|
| `canvas(prepaint, paint)`（paint 为 FnOnce，T 由 prepaint 产出） | `crates/gpui/src/elements/canvas.rs:10-19` |
| `fill(bounds, background) -> PaintQuad` / `outline` / `quad` | `crates/gpui/src/window.rs:7620/7632/7601` |
| `Window::paint_quad(PaintQuad)` | `crates/gpui/src/window.rs:4502` |
| `Path::move_to/line_to/curve_to/push_triangle` | `crates/gpui/src/scene.rs:840/847/859/876` |
| `Scene`/`Primitive`（按类型批处理） | `crates/gpui/src/scene.rs:41/222` |
| `TransformationMatrix`（compose） | `crates/gpui/src/scene.rs:608/658` |
| `Entity<T>` 共享状态（无 `Model`） | `crates/gpui/src/app/entity_map.rs:435` |
| `EventEmitter<E>` 标记 trait / `Context::emit`（要求 T: EventEmitter） | `crates/gpui/src/gpui.rs:317`、`app/context.rs:760-763` |
| `App::subscribe` | `crates/gpui/src/app.rs:1272` |
| `notify`（Context/App） | `app/context.rs:221`、`app.rs:2794` |
| `App::spawn` / `background_executor` | `app.rs:2039`、`app.rs:300` |
| `application()` 引导（gpui_platform） | `crates/gpui_platform/src/lib.rs:13` |
| `WgpuContext` 公开 `device`/`queue` | `crates/gpui_wgpu/src/wgpu_context.rs:13-14` |
| `TestAppContext` | `crates/gpui/src/app/test_context.rs` |

### 2.3 已核验的 petgraph API（@ a4d94bd）

`StableGraph`（`stable_graph/mod.rs:67`）、`add_node:366`/`remove_node:411`/`add_edge:467`/`remove_edge:619`/`node_weights:668`；算法入口 `dijkstra.rs:92`、`astar.rs:81`、`tarjan_scc.rs:269`、`page_rank.rs:64`、`min_spanning_tree.rs:86/255`、`ford_fulkerson.rs:164`；visit trait 簇 `visit/mod.rs`（`IntoNeighbors:107`、`IntoNodeIdentifiers:183`、`IntoEdges:147`）。

### 2.4 已核验的 cytoscape.js 移植源（@ 7ba6340）

布局物理 `cose.mjs`（`run:126`、`step:705`）；布局族 `grid.mjs:30`/`circle.mjs:31`/`breadthfirst.mjs:42`/`concentric.mjs:37`/`random.mjs:21`/`preset.mjs:25`；注册表 `extensions/layout/index.mjs`（8 项 name→impl）；桥接 `collection/layout.mjs:41`；拾取 `coords.mjs`（`findNearestElement:75`、`findNearestElements:79`、`checkNode:131`、`checkEdge:157`、`getAllInBox:323`、`doLinesIntersect:413`）；边几何 `edge-control-points.mjs`（`:251/:257/:309/:162/:64`）；贝塞尔采样 `edge-projection.mjs:5`；仿射公式 `math.mjs:9/14`；形状 `node-shapes.mjs`（`generatePolygon:6` 等）、`arrow-shapes.mjs:142`；样式 schema `style/properties.mjs`；bypass `style/bypass.mjs`。

## 3. 现状与差距（Gap）

> 2026-09-29 实测校准：下表"缺口"列原按阶段填写，多数已在后续阶段落地。已落地项标注 ✅，剩余缺口见[功能实测分析报告](./feature-analysis-report.md)与[P0 方案](./compograph-p0-plan.md)。

| 层 | 已有 | 缺口 |
|---|---|---|
| workspace/上游源码 | 结构、展开、解析与编译验证完成 | 沙箱无图形环境，窗口显示需目标机验证 |
| foundation | cg-types 几何原语+单测；cg-geometry 命中/曲线/聚合几何+单测；cg-graph 存储 + 变更事件广播 + 订阅过滤（`binding.rs`）+ 算法桥 11 个（`algo.rs`）+ JSON/DOT 进出（`io.rs`） | ✅ 只读视图 trait 已落地（`view.rs` 的 `GraphView`/`MockGraph`） |
| engine | LayoutEngine trait、注册表、**8 种布局**（force/random/preset/grid/circle/breadthfirst/concentric/hierarchical）+ LayoutDriver 增量维护 | ✅ 力导向与布局族均已落地；Radial 布局未做 |
| render | Camera、canvas 绘制、视口剔除、边/箭头/自环/haystack、空间索引、样式层与 bypass、**保留模式**、LOD、帧度量、PPM 导出、**节点标签文本** | ⚠️ 节点形状族、边标签、PNG 导出未做 |
| interact | 平移/缩放/拖拽/点选/框选/悬停 全套状态机 + 指针事件桥 | ✅ 均已落地 |
| app | 引导+演示场景、布局切换 UI、算法面板（5 类）、JSON/DOT/图片出入口、状态栏 | ✅ 均已落地 |
| 文档 | architecture/plan 四件套 + AGENTS.md | 随代码落地持续回改（本次已回改 P0 相关项） |

## 4. 总体阶段划分

| 阶段 | 主题 | 收口产物 |
|---|---|---|
| 阶段 0 | 骨架闭环 | 全量编译通过；事件流（变更→订阅→重布局）闭环；应用窗口显示演示图 |
| 阶段 1 | 可交互 | 力导向布局收敛；平移/缩放/拖拽/点选可用 |
| 阶段 2 | 功能对齐 | 布局族齐备；样式层；框选/高亮；算法面板 |
| 阶段 3 | 规模化 | 保留模式渲染 + LOD；后台布局线程；路线 C 评估结论 |

依赖关系：阶段 1 依赖阶段 0 的事件流接线；阶段 2 依赖阶段 1 的拾取与相机；阶段 3 依赖阶段 2 的绘制计划稳定。

## 5. 阶段 0：骨架闭环

**已完成**

1. 全链路编译/单测/clippy/fmt（见 2.1），仅图形窗口显示需真实桌面环境。
2. 上游以 submodule 挂载于 `crates/vendor/zed-gpui`；zed-gpui 发布 `lean` 时展开工作区继承，本项目根清单无需复述 gpui 的 workspace 表，仅用 `path` 依赖消费（实测对比见 2.1 与 architecture-design §2.1）。
3. 事件流接线，落地为三个文件：
   - `cg-graph/src/store.rs`：`add_node`/`remove_node`/`add_edge`/`remove_edge`/`clear` 接收 `&mut Context<Self>`，在改动成功后 `cx.emit(GraphChangeEvent)` + `cx.notify()`。删除类方法仅在确实删掉元素时发事件，避免空通知。
   - `cg-graph/src/binding.rs`：`ChangeFilter`（`ALL`/`NODES`/`EDGES`）把「哪些变更与我相关」抽成可测的值；`subscribe_graph` 统一订阅入口。
   - `cg-layout/src/reaction.rs`：`work_for(event) -> LayoutWork` 映射变更到所需工作量（新增节点→`PlaceNew`，边改/删点→`RefreshEdgeGeometry`，清空→`Full`）。
   - `cg-layout/src/driver.rs`：`LayoutDriver` 自持订阅，位置随变更增量维护；连续单点新增累计 64 次后回落全量重算，约束环形摆放的漂移。
4. **算法桥**（`cg-graph/src/algo.rs`）：以薄封装暴露 petgraph 算法，返回普通 owned 数据、不泄漏 petgraph 泛型到调用方。首批函数：`shortest_paths` / `shortest_path_cost`（dijkstra）、`strongly_connected_components`（tarjan_scc）、`rank_nodes`（page_rank）。纯 headless，5 个单测覆盖。注意 petgraph 0.8 的 `dijkstra` 返回 `hashbrown::HashMap`，封装层转为 `std::HashMap`。
5. **渲染订阅**（`cg-render/src/refresh.rs`）：`subscribe_repaint(cx, store)` 让画布视图直接订阅 `GraphStore` 的结构变更并 `notify` 自身，结构变更不再只依赖 `LayoutDriver` 的位置变化间接触发重绘。因 gpui `Context::subscribe` 订阅的是**调用方自身**实体，该接线必须在画布视图的 `Context<T>` 内调用（`crates/render/cg-render/tests/refresh_flow.rs` 以 `App::observe` 计数验证 2 例）。
6. **petgraph 依赖收敛**（详见 [petgraph 引入方式与设计评审](../architecture/petgraph-integration-review.md)）：`cg-graph` 成为唯一直接依赖 petgraph 的 crate，re-export `NodeIndex`/`EdgeIndex`；`GraphStore` 新增语义化查询方法（`node_ids`/`node_count`/`edge_count`/`node_data`/`edge_endpoints`/`successors`/`predecessors`）。下游 `cg-layout`/`cg-render`/`cg-interact`/`app` 的 petgraph 直接引用（原 7 处）全部移除，对应 `Cargo.toml` 依赖一并删除；`cg-interact` 改依赖 `cg-graph`。

**剩余工作**

1. 目标机运行验证：`cargo run -p compograph` 打开窗口并显示 12 节点演示图（唯一需真实桌面环境的收尾项）。

**验收**：窗口出现 12 节点链式演示图；`cargo test -p cg-types -p cg-geometry -p cg-graph -p cg-layout -p cg-render -p cg-interact` 全绿（33/33）；自研 crate 无 clippy 告警（上游警告不在修复范围）；`cargo fmt --all --check` 通过。

> 本沙箱内 `cargo test --workspace` 会因 gpui 自带 example 缺 `-lxkbcommon-x11` 链接失败（环境缺 X11 开发库）。在装有 X11/Wayland 开发库的目标机上应全绿；若仍失败，先确认 submodule 已展开（`error inheriting ...` 即表示未展开）。

**已知实现约束（供阶段 1 参考）**

- gpui 的事件派发是**队列化**的：`Context::emit` 只把 `Effect::Emit` 压入 `pending_effects`，真正的派发发生在**最外层** `App::update` 结束时的 `flush_effects`（`app.rs:1176` 的 `finish_update` 判定 `pending_updates == 1`）。因此「变更 → 断言」必须拆到两次 update 调用中，`crates/engine/cg-layout/tests/event_flow.rs` 即按此组织。

## 6. 阶段 1：几何数学层 + 力导向 + 基础交互

**移植（cytoscape.js → compograph，数学直译）**

| 源 | 落点 | 内容 |
|---|---|---|
| `cose.mjs:126/705` | `cg-layout/src/force.rs` | 斥力/引力/温度退火主循环；迭代参数（repulsion/gravity/maxIterations）做成结构体选项 |
| `coords.mjs:75/79/131/157` | `cg-geometry/src/picking.rs`（扩展） | 最近元素检索、节点包围盒命中、边距离命中（配合 `edge-projection.mjs:5` 采样） |
| `math.mjs:9/14` | `cg-render/src/camera.rs`（校对） | 平移缩放仿射公式与现有实现对拍 |
| `edge-control-points.mjs:251/:257` | `cg-geometry/src/curves.rs`（扩展） | 直线/贝塞尔控制点计算；绘制侧接 `Path::curve_to`（scene.rs:859） |

**新增**

- `cg-render/src/spatial.rs`：均匀网格空间索引（插入/点查/矩形查），供点选与框选共用。
- `cg-interact/src/handlers.rs`：gpui 指针事件 → `Camera::viewport_to_world` 反变换 → 空间索引命中 → 改状态 → notify。事件入口用 gpui 的元素级鼠标事件（div 包裹 canvas 或 window 级监听，接线方式见 OQ-10）。
- 布局后台执行：力导向迭代放 `App::spawn`（app.rs:2039），分批回写 `Positions` 并 notify，形成动画收敛。

**验收**：100~500 节点随机图力导向收敛不抖动；拖拽节点后局部重排；滚轮以光标为锚缩放；点选高亮。

## 7. 阶段 2：布局族 + 样式 + 框选/高亮 + 算法 UI

- **布局族移植**：`grid.mjs:30`/`circle.mjs:31`/`breadthfirst.mjs:42`/`concentric.mjs:37` → `cg-layout/src/{grid,circle,breadthfirst,concentric}.rs`；Hierarchical 用 petgraph toposort/`tred`/`dominators`（`dominators.rs:73`）做分层子步骤，坐标分配自研。全部注册进 `LayoutRegistry`，应用侧提供切换菜单。
- **样式层**：新建 `cg-render/src/style.rs`，定义节点/边视觉属性子集（填充、描边、宽度、透明度、标签字号），设计借 cytoscape `properties.mjs` 的 schema 思想与 `apply.mjs` 的 mapper 思想；目标字段重映射为 `fill`（window.rs:7620）/`outline`/`Path` 描边。选中/高亮走 bypass 式覆盖（`style/bypass.mjs` 概念），不改主样式。
- **框选**：`coords.mjs:323 getAllInBox` + `:413 doLinesIntersect`（几何已在 cg-geometry 补齐）接空间索引矩形查询；高亮改对应 quad 填充。
- **算法 UI 桥接**：命令面板选算法 → 后台 `App::spawn` 执行 petgraph 算法 → 结果以着色/面板展示（最短路高亮、SCC 分组着色、PageRank 半径映射）。首批暴露：dijkstra、astar、tarjan_scc、page_rank、min_spanning_tree。
- **图 I/O**：JSON 导入导出（节点/边/位置），serde 可选。

**验收**：≥6 种布局可切换；框选/高亮正确；三类算法结果可视化；JSON 往返无损。

## 8. 阶段 3：规模化

- **保留模式渲染（路线 B）**：实现自定义 `Element`（element.rs:53），缓存节点/边图元，按 `GraphChangeEvent` 与位置变更做差异 flush；`Scene` 批处理特性（scene.rs:41）保持低状态切换。
- **LOD**：缩放低于阈值时边退化为直线、标签隐藏、节点退化为方块。
- **边聚合**：移植 `findHaystackPoints`（edge-control-points.mjs:64）用于大图边简化。
- **后台布局线程化**：布局完全移出主线程，分批回写（阶段 1 已打基础，此阶段补取消/重启语义）。
- **路线 C 评估**：若目标规模达十万级，基于 fork 的 `gpui_wgpu::WgpuContext`（wgpu_context.rs:13-14 公开 device/queue）做实例化渲染原型；架构参考 cytoscape `webgl/drawing-elements-webgl.mjs:163` 的实例化抽象，着色器全新实现。产出评估结论后决定是否保留该路线。

**验收**：万级节点可交互（平移/缩放帧率稳定）；内存占用与帧时间有量化记录。

## 9. 验证策略

- **纯计算层 headless 单测**：cg-types/cg-geometry/cg-layout 全部 `#[cfg(test)]` 同文件单测（已建立此模式）；几何移植函数用 cytoscape 原实现算例做数值对拍。
- **事件接线**：gpui `TestAppContext`（test_context.rs）做 subscribe/emit 的 headless 测试，已落地于 `crates/engine/cg-layout/tests/event_flow.rs`（5 例，覆盖新增/删除/清空/顺序/换引擎）。注意 gpui 事件队列化派发，断言需与变更分处两次 update（见第 5 章约束）。
- **每阶段验收清单**：见各阶段"验收"行；阶段 1 起补手动操作脚本（窗口操作步骤）。
- **环境注记**：受限网络下 cargo 需 rsproxy 镜像 + github 代理（gh-proxy insteadOf）；沙箱已验证此链路。

## 10. 风险与待拍板开放点

| 编号 | 事项 | 建议 |
|---|---|---|
| OQ-1 | gpui 升级会漂移 API（本仓已证实 `Model`→`Entity` 演进） | 保持 submodule 指向 zed-gpui `lean` 分支；升级在 zed-gpui 内完成并发布新快照，本项目只更新 submodule 指向 + 复核 2.2 表 |
| OQ-2 | 力导向物理选型：CoSE 直译 vs d3-force 风格 | 先直译 cose（:705），手感不满意再评估 |
| OQ-3 | 目标图规模（千/万/十万） | 决定 LOD 与路线 C 启动时机，需拍板 |
| OQ-5 | 标签渲染：gpui 文本系统 vs 离屏纹理；中文换行/锚点 | ✅ 已定并落地（节点标签走 gpui 文本系统，单行居中）；边标签与中文换行待做 |
| OQ-7 | 视觉保真度：贝塞尔/箭头/自环优先级 | 建议顺序：直线+箭头 → 贝塞尔 → 自环 → taxi/haystack |
| OQ-8 | headless 渲染测试覆盖面 | 拾取几何 + 事件接线优先 |
| OQ-10 | 指针事件入口：元素级（div 包 canvas）vs window 级监听 | 阶段 1 以最小可用为标准先接 div 级，必要时上 window 级 |
| OQ-11 | Cargo.lock 是否入库 | 建议入库（应用类仓库，锁定依赖解析）；注意 submodule 内的 `Cargo.lock` 不参与本项目解析（其清单已展开，与本项目锁各自独立） |

## 11. 下一步行动

1. 目标机执行阶段 0 收口：`cargo run -p compograph` 演示窗口验证（事件流接线已完成，见第 5 章）。
2. 阶段 1 开工顺序：空间索引 → 拾取几何移植 → force.rs 移植 → 指针事件接线 → 相机交互。
3. 文档与代码一致性（已完成第三轮，2026-09-29）：architecture 三文档改用 cg-* 命名；上游结构描述已从"平铺在 crates/"更正为"submodule 挂 crates/vendor/zed-gpui"，并补 zed-gpui 侧展开工作区继承的机制说明（architecture-design §2.1）；原作者文档（compograph-design.md / cytoscape-borrow-analysis.md）已加取代指引注；本轮按[功能实测分析报告](./feature-analysis-report.md)回改 `feature-list.md`/`architecture-design.md`/本文的进度口径。
4. 每阶段结束在本文件补记"实际落地差异"，供后续阶段参考。

### 11.1 P0 落地记录（2026-09-29）

按 [P0 方案](./compograph-p0-plan.md) 完成三项：

- **节点标签文本渲染**：新增 `cg-render/src/text.rs`（`PaintedLabel` + `paint_labels_for`，含 headless 单测）；`graph_view` 增第 5 参并在节点之后、框选之前经 gpui 文本系统（`shape_line` + `ShapedLine::paint`）绘制；`main.rs` 复用既有 `labels` 生成计划；`DetailLevel::Minimal` 隐藏标签。
- **DOT 导入**：`cg-graph/src/io.rs` 的 `import_dot` 由占位改为自写轻量解析器（头/节点/边链/属性/注释/引号字符串），产出 `GraphDocument` 并复用 `validate`；应用层新增 `import_dot_file` 与 `import dot` 按钮。
- **文档回改**：`feature-list.md`/`architecture-design.md`/本文按实测校准进度口径。
- **验证**：`cargo check --workspace --all-targets` 通过；`cargo test` 6 自建 crate 全绿（cg-types 5 / cg-geometry 22 / cg-graph 40 / cg-interact 15 / cg-layout 42+9 / cg-render 51+2 / compograph 14）；`cargo fmt --all --check` 通过；clippy 仅 `cg-render` 一处**既有**告警（`metrics.rs` 的 `len` 无 `is_empty`，非本次引入）。
- **遗留未闭环**：图形窗口目视确认需目标机。
