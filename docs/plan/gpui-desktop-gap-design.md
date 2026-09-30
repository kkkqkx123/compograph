# 桌面化缺口设计方案

> 版本：v1.0 · 日期：2026-09-30
> 上游分析：[gpui 桌面化适配分析](../analysis/gpui-desktop-adaptation-analysis.md)
> 目标：补齐 compograph 作为三个项目（linkrs / code-context-engine / wf-agent）gpui 桌面应用底座时的缺口。
> 设计原则：遵循 AGENTS.md 的分层 DAG、纯 Rust、无中文注释；所有新增 crate 挂入现有 foundation/engine/render/app 分层；不做超前抽象，按落地顺序单点启动。

---

## 0. 缺口清单与启动顺序

| 缺口 | 优先级 | 归属 crate | 阻塞谁 |
|---|---|---|---|
| D1 通用 UI 组件库 | P0（共同依赖，最先） | 新建 `crates/render/cg-ui` | 三者全部 |
| D2 异步数据接入模式（后端桥接） | P0 | app 层模式约定，不新建 crate | linkrs、CCE、wf-agent |
| D3 图编辑手势（建点/连线/删除） | P1 | `cg-interact` + `cg-render` | wf-agent |
| D4 样式动画/过渡驱动 | P1 | `cg-render` | wf-agent |
| D5 代码/文本高亮编辑组件 | P2 | `cg-ui` 扩展 | linkrs、CCE |
| D6 领域面板（按项目） | P2 | 各项目自有 app crate | 单项目 |

启动顺序：D1 → D2 →（试点 CCE）→ D3/D4 →（wf-agent）→ D5/D6 →（linkrs）。

---

## 1. D1 通用 UI 组件库（cg-ui）

### 1.1 定位与分层

- 新 crate `crates/render/cg-ui`，依赖 gpui（与 cg-render 同层：都是"gpui 元素/绘制"层，不依赖任何 cg-* 图 crate，保持可独立用于非图界面）。
- 内容：无业务语义的通用组件 + 设计令牌。领域面板（Cypher 编辑器、Schema 表单）不放这里，放在各消费项目。
- 与 compograph 现有三块自建 UI（顶栏菜单、算法面板、状态栏）的关系：落地后迁移重构为 cg-ui 组件，作为组件库的第一个消费者与验收标准。

### 1.2 组件清单（按落地批次）

| 批次 | 组件 | 说明 |
|---|---|---|
| U1 | `Button`、`TextInput`、`Label`、`Divider`、`Select`（下拉） | 最小交互集，覆盖 GraphFilterPanel、连接配置 |
| U2 | `ListView`（虚拟化列表）、`TreeList` | 覆盖实体列表、会话列表、Schema 树；虚拟化用 gpui `list` 元素 |
| U3 | `DataTable`（排序/选择/虚拟滚动） | 覆盖 DataBrowser、查询结果、执行记录 |
| U4 | `TabContainer`、`SplitPane`、`Modal`/`Drawer`、`Notification`（toast） | 应用壳布局 |
| U5 | `CodeView`（只读代码展示 + 语法高亮） | CCE 代码片段；见 D5 |
| U6 | `CodeEditor`（可编辑 + 高亮） | linkrs Cypher REPL；见 D5 |

### 1.3 关键设计决策

- **状态管理**：组件本身是无状态渲染 + 事件回调（对齐 gpui 的 `RenderOnce`/element 风格）；有状态交互（输入焦点、选中项）由调用方以 `Entity` 持有，cg-ui 只提供 `impl Render` 的包装类型。避免组件库内部自建全局 store。
- **主题/令牌**：`Theme` 结构体（颜色、字号、间距、圆角）经 gpui 的 `Global` 注入，组件从 `App`/`Window` 读取。wf-agent 的 Tailwind token 表与 linkrs 的 `theme.ts` 均可映射为一份 `Theme`。
- **文本输入**：基于 gpui 文本系统自建，不复用 Zed 内部的 editor（不在 submodule 范围）。U1 的 `TextInput` 只做单行 + 光标/选区/剪贴板，复杂能力推迟到 U6。
- **DataTable 虚拟滚动**：行高固定档先行（单行/双行两档），不做自适应行高；列宽用显式像素 + `fr` 比例两种。

### 1.4 验收

- compograph 顶栏/面板/状态栏迁移完成后不回退任何功能；
- 用 cg-ui 搭一个最小"连接 + 查询 + 结果表"示例页（demo 二进制，不进主应用）验证组件完备性。

---

## 2. D2 异步数据接入模式（后端桥接）

三个项目的桌面版都需要 UI 进程访问后端（HTTP/gRPC/内嵌）。约定统一模式，不新建 crate：

### 2.1 内嵌模式（首选）

- 后端 crate 以 lib 形式直接链接进桌面 app（如 linkrs 的 `graphdb-api` embedded API）。
- 桥接层（各项目自写，模式统一）：
  1. 定义 `BackendStore` 实体（`Entity<T>`），内含后端客户端句柄与本地缓存快照；
  2. 后台请求用 gpui `background_executor.spawn`（async 客户端）或 `smol::block_on` 包装同步 SDK；
  3. 结果写回 `BackendStore` 并 emit 事件，UI 与 `GraphStore` 同样 `subscribe` 驱动刷新——**与 cg-layout 订阅图变更完全同构**；
  4. 长任务（批量导入、执行流）复用 compograph 算法面板的"后台执行 + 代次守卫"模式（`crates/app/compograph/src/algo_panel.rs`），过期结果按代次丢弃。

### 2.2 子进程模式（备选）

后端不便内嵌时（wf-agent 的沙箱执行、LLM 进程），桌面 app 拉起子进程 + HTTP/stdio 通信，桥接层不变，仅客户端实现不同。

### 2.3 图数据映射约定

后端图数据 → `GraphStore` 的映射层是各项目唯一需要写的"胶水"，约定：

- 节点/边 id 用后端稳定 id 字符串，`GraphStore` 的 `NodeIndex`/`EdgeIndex` 通过 `attrs.rs` 的属性表或独立 `HashMap<String, NodeIndex>` 双向映射（compograph 的 file_io 已有同款导入映射可参照）；
- 增量同步：后端事件流（linkrs 的 event_dispatch、CCE 的 file watcher、wf-agent 的执行事件）→ diff → `GraphStore` 增删改 → 事件自动驱动布局与重绘，不做全量重建。

---

## 3. D3 图编辑手势（建点/连线/删除）

wf-agent 工作流编辑的核心缺口。当前 `cg-interact` 只有"查看态"手势，需新增"编辑态"。设计为**手势模式机**，不引入新 crate：

### 3.1 状态机

`EditState`（cg-interact 新文件 `edit.rs`），与现有 `PanState`/`SelectionState` 并列，由工具模式决定激活哪个手势集：

| 工具模式 | 指针手势 | 产出 |
|---|---|---|
| Select（现有） | 保持现状 | — |
| AddNode | 点击空白 → 创建节点草稿（跟随光标）→ 点击确认落点 | `AddNode { pos, data }` |
| Connect | 按住节点 port/边缘起拖 → 拉出预览线 → 落到目标节点/空白 | `ConnectEdge { from, to }` 或 `ConnectNode { from, pos }`（连到空白时先建节点再连线，二合一） |
| Delete | 选中后 Del 键，或右键菜单 | `RemoveSelected` |

### 3.2 动作出口（不直接改图）

- 手势层只产生**编辑动作**（`GraphEditAction` 枚举），由 app 层统一应用到 `GraphStore`——与 cg-interact 现有"事件桥不改图"的边界一致，方便 wf-agent 接 undo/校验（DAG 环检测挂在这里：wf-agent 校验拒绝后回滚动作即可）。
- 连线预览：`Connect` 拖拽期间的橡皮筋线由 app 层作为额外绘制层叠加（cg-render 已有 bypass 绘制通道可承载），不进入 GraphStore。

### 3.3 实现要点

- 起拖命中：复用 `cg-render` 空间索引的节点命中；port 概念首版不做（整节点即可连），port 锚点作为后续扩展。
- 拖拽中的增量布局：新节点落点写入 `PositionStore` 后由 `LayoutDriver` 现有事件链自动收敛，无需新机制。
- 删除联动：`GraphStore` 的 `StableGraph` 删除已保证索引稳定，选中集需同步清理（在动作应用层做）。

---

## 4. D4 样式动画/过渡驱动

wf-agent 执行态（节点运行着色、trace 回放）所需。设计为**轻量插值层**，不引动画库：

### 4.1 结构

- cg-render 新文件 `animation.rs`：
  - `StyleTransition { from: StyleValue, to: StyleValue, t0: Instant, duration, easing }`，`StyleValue` 覆盖颜色/透明度/描边宽度三个可插值属性（渐变不做插值，切换直接跳变）；
  - `TransitionStore`（`Entity`）：活跃过渡表，按 `(元素 id, 属性)` 键控，新过渡覆盖旧过渡（取旧值当前插值结果为起点，避免跳变）。
- 驱动：gpui 每帧回调（canvas request redraw）中推进 `t`，写 `BypassStore` 覆盖样式；过渡结束自动摘除。compograph 状态栏已显示帧耗时，绘制循环已存在，接入点现成。

### 4.2 执行态着色约定

wf-agent 的"节点运行中/成功/失败"映射为三组 bypass 样式 + 进入过渡；trace 回放 = 按时间轴顺序提交一组 bypass 变更，动画层只负责平滑，不感知业务。

### 4.3 与"布局动画链"的关系

feature-list §6 的布局动画链（位置插值）不在本项内：本项只做**样式**插值（颜色/透明度/宽度），位置动画待布局收敛分批回写（已实现的 `LayoutProgress`）满足需求后再评估，避免重复建设。

---

## 5. D5 代码/文本高亮组件

### 5.1 只读 CodeView（U5，先做）

- 分词用 tree-sitter（wf-agent/cce 生态已有依赖先例），注入 theme 颜色渲染为 gpui 富文本段落；首版只覆盖各项目主语言（Rust、TOML、Cypher 文法可用简单 lexer 代替 tree-sitter，Cypher 无现成 grammar）。
- 长文件虚拟化：按行分段绘制，只整形可见窗口（复用 cg-render `text.rs` 的多行/换行经验）。

### 5.2 可编辑 CodeEditor（U6，后做）

- 基于 gpui 文本系统自建单 buffer 编辑器：光标/选区/撤销栈/剪贴板；语法高亮按行重算（编辑行 ±N 窗口）。
- 明确不做：LSP、多 buffer、折叠。linkrs Cypher REPL 首版 = CodeEditor + 历史列表（U2 ListView）+ 结果表（U3 DataTable）。

---

## 6. D6 按项目落地路线

| 项目 | 试点顺序 | 复用 | 需自建 |
|---|---|---|---|
| code-context-engine | 第一个（工程量最小） | 图内核全部（cose-bilkent ↔ force.rs 同源）、U1/U2/U5、D2 内嵌模式 | 实体详情面板、搜索框接线、Markdown 结果渲染（cg-ui 补一个 `MarkdownView`，只做标题/列表/代码块子集） |
| wf-agent | 第二个 | hierarchical 布局 + 箭头（dagre 场景）、U1–U4、D3、D4、D2 子进程模式（沙箱执行） | 工作流编辑面板、trace 联动逻辑、DAG 校验接入 |
| linkrs | 第三个（缺口最多） | 图内核 + tag 着色 mapper、U1–U3/U5/U6、D2 内嵌模式（graphdb-api embedded） | Schema 面板（表格 + 表单即够）、Cypher REPL 组装、连接管理页、图例（Legend）组件（cg-ui 补 `Legend`：色块 + 文本列表，数据来自样式 mapper） |

### 6.1 各项目仓库内结构约定

桌面 app 作为各项目 workspace 的新成员（如 `crates/studio-desktop`），依赖：`gpui`（path 指向本项目 submodule 或经 crates.io 发布后的 gpui）+ `cg-graph`/`cg-layout`/`cg-render`/`cg-interact`/`cg-ui`（本项目发布为 crates.io 包，或各项目以 git 依赖引入——推荐 git 依赖 + 固定 rev，与 zed-gpui submodule 同步节奏绑定）。

### 6.2 本项目需要新增的对外承诺

- cg-* 各 crate 的公开 API 视为稳定面（当前 "no-backward-compatible" 口径不变，但三个消费项目落地后，破坏性改动需同步通知）；
- `cg-graph` 事件类型与 `GraphStore` 增删改 API 是胶水层依赖的核心契约，改动需走文档同步（AGENTS.md 已有要求）。

---

## 7. 风险与开放问题

- **gpui 文本编辑器工作量**：U6 CodeEditor 是最大单点工程，若超预算，linkrs REPL 可先降级为"多行 TextInput + 发送按钮"（无高亮），高亮后补。
- **tree-sitter 依赖进入 cg-ui**：与 compograph 纯净依赖面冲突，倾向放可选 feature（`syntax` feature 默认关），无该 feature 时 CodeView 降级纯文本。
- **cg-ui 是否依赖 cg-render**：不依赖。动画层（D4）属于 cg-render；cg-ui 保持纯 gpui，保证非图项目可用。
- **多窗口**：三个项目首版均单窗口，gpui 多窗口能力不纳入本设计。
