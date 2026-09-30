# compograph 能力盘点与三个项目桌面化适配分析

> 版本：v1.0 · 日期：2026-09-30
> 分析对象：compograph（本项目）作为 gpui 桌面图可视化底座，能否支撑以下三个项目效仿其 Web 前端实现 gpui 桌面应用：
> - `/home/kkkqkx/code/linkrs`（GraphDB，图数据库）
> - `/home/kkkqkx/code/code-context-engine`（CCE，代码语义索引与检索）
> - `/home/kkkqkx/code/wf-agent`（工作流 + Agent 框架）

---

## 1. compograph 当前实现盘点

分层结构（详见 [feature-list](../architecture/feature-list.md)）：

| 层 | crate | 核心能力 |
|---|---|---|
| foundation | `cg-types` | 几何基元 |
| foundation | `cg-geometry` | 命中测试、曲线几何、聚合、picking（框架无关） |
| foundation | `cg-graph` | `GraphStore` + petgraph `StableGraph` 适配、变更事件广播与订阅过滤、`PositionStore` 坐标外挂、只读图视图 trait、JSON/DOT 导入导出、复合节点 |
| engine | `cg-layout` | `LayoutEngine` trait + 注册表（9 种：Preset/Random/Force(CoSE/FR)/Grid/Circle/Breadthfirst/Concentric/Radial/Hierarchical）、`LayoutDriver` 变更驱动、增量布局（pin/refine）、后台线程布局 + 分批回写 |
| render | `cg-render` | GraphView 画布、`Camera` 视口映射、空间索引、点选/框选命中、节点形状族（10 种）、箭头形状族（7 种）、直线/折线/贝塞尔/自环/Haystack 边、节点/边标签（多行换行、中文换行、标签背景）、样式层（颜色/描边/透明度/线性渐变 + mapper/谓词）、选中/高亮 bypass、绘制顺序编排、保留场景缓存、LOD 三级、PNG 导出（软件光栅化 + 自研编码） |
| render | `cg-interact` | 拖拽/平移/缩放/点选/多选/框选/悬停状态，指针事件 → 相机逆变换 → 命中查询事件桥 |
| app | `compograph` | gpui 窗口引导、布局切换 UI、算法面板（21 类算法入口，后台执行 + 代次守卫）、图导入导出文件对话框、状态栏 |

进度口径：P0/P1/P2 全部落地；P3 已落地欧拉路/最小割/层次-马尔可夫-k 均值聚类/PNG 导出/LOD/保留场景。未实现：路线 C（wgpu 十万级实例化）、复合节点、扩展机制、布局动画链。

架构上最可复用的是**变更驱动模式**：`Entity<GraphStore>` + `EventEmitter` + `subscribe`，布局层通过订阅图变更自动维护坐标（`crates/engine/cg-layout/src/driver.rs`），渲染层订阅位置变更重绘——任何后端数据源接入后都能套用同一模式。

---

## 2. 三个项目的 Web 前端形态

| 项目 | 前端技术 | 图可视化库 | 图相关代码（绝对路径） |
|---|---|---|---|
| linkrs | Svelte + TS + Vite | cytoscape.js | `/home/kkkqkx/code/linkrs/frontend/src/lib/components/common/CytoscapeCanvas.svelte`、`/home/kkkqkx/code/linkrs/frontend/src/lib/utils/cytoscapeConfig.ts`、`/home/kkkqkx/code/linkrs/frontend/src/lib/utils/graphLayout.ts` |
| code-context-engine | Svelte + TS | cytoscape.js + cose-bilkent | `/home/kkkqkx/code/code-context-engine/frontend/src/lib/components/graph/GraphCanvas.svelte`、`/home/kkkqkx/code/code-context-engine/frontend/src/lib/components/entities/CallGraph.svelte`、`/home/kkkqkx/code/code-context-engine/frontend/src/lib/components/entities/InheritanceTree.svelte` |
| wf-agent | Svelte 5 + SvelteKit + Tailwind | cytoscape.js + dagre | `/home/kkkqkx/code/wf-agent/apps/web-app/src/lib/components/domain/GraphCanvas.svelte`、`/home/kkkqkx/code/wf-agent/apps/web-app/src/lib/graph/canvas-elements.ts`、`/home/kkkqkx/code/wf-agent/apps/web-app/src/lib/graph/canvas-style.ts` |

三个项目图可视化全部基于 cytoscape.js，与 compograph 对标 cytoscape 的路线（`docs/analysis/cytoscape-clustering-reference.md`、`docs/ref/cytoscape-js-features.md`）完全吻合。

各项目前端除图画布外的页面构成：

- **linkrs**：Graph 可视化、Schema 管理、Console（Cypher REPL，Monaco 编辑器 `/home/kkkqkx/code/linkrs/frontend/src/lib/utils/monacoCypher.ts`）、DataBrowser（数据 CRUD 表格）、Login/连接管理。后端经 HTTP（`/home/kkkqkx/code/linkrs/frontend/src/lib/services/http.ts`）与 gRPC（77+ RPC）访问。
- **code-context-engine**：graph 页（`/home/kkkqkx/code/code-context-engine/frontend/src/routes/graph/+page.svelte`，含 GraphFilterPanel/GraphToolbar）、实体详情页（`/home/kkkqkx/code/code-context-engine/frontend/src/routes/entities/[id]/+page.svelte`，代码片段 + 调用图 + 继承树）。
- **wf-agent**：工作流模板编辑（WorkflowEditPanel/TemplateEditPanel）、执行可视化（GraphExplorer、NodeTracePanel、Timeline、ExecutionInspector）、会话/消息流（chat 组件）。

---

## 3. 能力对照

### 3.1 compograph 已直接覆盖（约 60%）

| 需求 | compograph 对应 |
|---|---|
| 图数据模型（节点/边/属性） | `GraphStore` + `NodeData`/`EdgeData` |
| 平移/缩放/拖拽/点选/框选/悬停 | `cg-interact` 全套 |
| 力导向/层次/圆形/网格等布局 | `cg-layout` 9 种（覆盖 cose-bilkent 与 dagre 的场景） |
| 样式/标签/箭头/形状/渐变/高亮 | `cg-render` style/text/shapes/arrows |
| 大图性能 | spatial 索引、LOD、RetainedCache、Haystack 边 |
| 图算法 | `cg-graph` algo 21 类入口（最短路/PageRank/中心性/连通/聚类） |
| 图导入导出 | JSON/DOT 进出 + PNG 导出 |
| 窗口壳/菜单/状态栏 | `compograph` app 层 |

### 3.2 共同缺口：通用 UI 组件库（最大缺口）

compograph 目前只有顶栏菜单、算法面板、状态栏三块自建 UI，没有可复用的组件层。而三个项目前端的主体恰恰是这类组件：

| 缺失组件 | linkrs | CCE | wf-agent |
|---|---|---|---|
| 数据表格 | DataBrowser、查询结果 | 搜索结果列表 | ExecutionRuns、IssueList |
| 表单/输入框/下拉 | Schema 编辑、连接配置 | GraphFilterPanel | 节点配置、模板编辑 |
| 代码/文本编辑器 | Monaco Cypher REPL | 代码片段展示 | TOML 模板编辑 |
| 树/列表 | Schema 树 | 实体列表、继承树 | Timeline、会话列表 |
| Tab/布局容器 | 多页签 | 路由页 | Inspector 多面板 |
| 通知/对话框 | notification store | — | 导入对话框 |

建议在 compograph 内沉淀独立组件 crate（如 `cg-ui`），三个项目共用，避免各自重写。

### 3.3 按项目增量缺口

**linkrs（缺口最多）**

- 数据模型差异：linkrs 是 Space/Tag/EdgeType 强类型属性模型（NebulaGraph 风格），compograph 的 `NodeData` 是扁平样式字段，无 schema 概念 → 需 Schema 面板 + 属性表 UI。
- Cypher 编辑器：gpui 生态无 Monaco 等价物，需自建语法高亮文本编辑组件（gpui 文本系统可承载，但高亮/补全需自写）。
- 连接层：需引入 HTTP/gRPC 客户端 crate 与异步桥接（gpui 的 background executor 可跑异步任务，compograph 后台布局已验证该模式）。
- 按 tag 着色的图例（Legend）：`cg-render` 的 style mapper/谓词雏形够用，需补按属性动态映射样式的入口与图例 UI。

**code-context-engine（契合度最高）**

- 图数据（实体=节点、调用/继承/依赖=边）与 `GraphStore` 模型几乎一一对应；cose-bilkent ↔ `crates/engine/cg-layout/src/force.rs`（同源 CoSE）。
- 缺口集中在实体详情页：代码片段高亮面板、搜索框接线、LLM/MCP 结果流式展示（需 Markdown/富文本渲染组件）。
- 建议作为第一个桌面化试点，工程量最小。

**wf-agent（缺口在"可编辑 DAG"与执行态）**

- 静态展示工作流 DAG：hierarchical 布局 + 箭头已覆盖（dagre 类分层场景）。
- **画布上的图编辑**：compograph 图是"可视化 + 算法"导向，无节点创建/连线手势（增删走 API，非 UI 拖拽连线）→ 需新增编辑态交互（创建节点、拖拽连线、删除）。
- **执行态动画**：节点运行状态着色、trace 回放、时间轴联动 → `BypassStore` 可承载样式覆盖，但缺动画循环/过渡驱动（对应 feature-list §6 未做的"布局动画链"）。
- 大量非图画布 UI（消息流、Inspector、表单编辑）依赖 §3.2 组件库。

---

## 4. 结论与建议

**结论：compograph 的图内核（存储/布局/渲染/交互/算法）已达到 cytoscape.js 的可用度，足以作为三个项目桌面版的图可视化底座；真正缺口在"应用壳层"——通用 UI 组件库与各项目的领域面板。**

桌面化能力分三档：

1. **直接复用（约 60%）**：图模型、9 种布局、全套交互、样式/标签/导出、算法面板。三个项目共用；linkrs 与 CCE 的图场景不比 compograph 自身复杂。
2. **新建通用 UI 组件库**：表格、表单、下拉、列表、树、代码/文本面板、Tab、通知。这是三个项目的共同依赖，优先级最高，建议先于任何单项目桌面化启动。
3. **按项目增量补齐**：
   - CCE：实体详情/代码高亮面板、搜索接线（最小，先做）；
   - linkrs：Cypher 编辑器、结果表格、Schema 面板、连接管理；
   - wf-agent：画布图编辑手势、执行状态动画、trace 联动。

结构性建议：三个项目均为"Rust 服务 + HTTP API"，桌面化时建议采用 compograph 同款架构——把各自后端作为 headless 服务内嵌（lib 形式，如 linkrs 的 embedded API `/home/kkkqkx/code/linkrs/crates/graphdb-api/src/embedded.rs`）或子进程 + HTTP 桥接，UI 侧用 `Entity<Store>` + `EventEmitter` + `subscribe` 变更驱动模式承接数据，compograph 的 `GraphStore` → `LayoutDriver` → `GraphView` 事件链可直接作为参照。
