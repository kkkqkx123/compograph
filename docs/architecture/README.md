# compograph 架构文档（Architecture）

> 日期：2026-09-28 · 基于 `docs/` 现有分析与本地克隆源码核验产出

## 文档索引

| 文档 | 内容 |
|---|---|
| [architecture-design.md](./architecture-design.md) | **总体架构设计**：分层架构、模块划分与依赖方向、核心数据模型（`Entity<GraphStore>` + `StableGraph` + 坐标外挂）、布局引擎、渲染三路线选型（A/B/C）、交互、性能策略、开放点、实施路线 P0–P3 |
| [feature-list.md](./feature-list.md) | **功能清单**：按 L1 数据/算法（含 petgraph 算法桥接全表）、L2 布局、L3 渲染、L3 交互、应用壳逐项列出，标注来源（`[petgraph]` 直接复用 / `[cy→移植]` 数学直译 / `[cy→模式]` 模式借鉴 / `[自研]` / `[gpui]` 平台能力）与阶段映射 |
| [borrowing-design.md](./borrowing-design.md) | **借鉴设计说明**：从 cytoscape.js / petgraph / gpui 各借什么、借鉴程度（整段移植/数学直译/模式借鉴/架构参考/不借鉴）、23 项借鉴总表、明确不借鉴清单及原因、源文件→模块落地映射、行号核验记录 |
| [petgraph-integration-review.md](./petgraph-integration-review.md) | **petgraph 引入方式与设计评审**：workspace 版本集中 + 各层直接依赖的事实描述、`StableGraph` 选型与算法桥的合理性评审、4 项可商榷点与改进建议 |

## 上游参考分析（`docs/ref/`）

| 文档 | 项目 |
|---|---|
| [`../ref/cytoscape.js.md`](../ref/cytoscape.js.md) | cytoscape.js：Core/Collection 模型、扩展机制、渲染管线 |
| [`../ref/petgraph.md`](../ref/petgraph.md) | petgraph：workspace 分层、6 种图类型、visit trait 体系 |
| [`../ref/graph-algorithms-diff.md`](../ref/graph-algorithms-diff.md) | petgraph 与 cytoscape.js 算法差异总表 |

## 事实基线

| 项目 | 位置 | 版本口径 | 关键结论 |
|---|---|---|---|
| petgraph | crates.io 依赖 | 以 `Cargo.lock` 为准 | 纯计算无布局；`StableGraph` 适配编辑场景；算法直接复用 |
| gpui | crates.io 依赖（`gpui-pre` 快照） | 以根 `Cargo.toml` 精确 pin 与 `Cargo.lock` 为准 | ⚠️ `Model<T>` 已不存在，用 `Entity<T>`；`canvas`/`Scene`/`paint_quad`/`TransformationMatrix` 为绘制落点；`gpui_wgpu` 公开 device/queue |
| cytoscape.js | `ref/cytoscape-js`（随仓库 vendored） | 见其 `package.json` | 借布局物理与几何数学；内置布局注册表为 8 种（`extensions/layout/index.mjs`） |

## 一图流

```
应用壳 gpui(app/compograph) → L3 渲染/交互(render/cg-render + cg-interact, 借 cytoscape 几何数学)
  → L2 布局(engine/cg-layout, 借 cytoscape 物理数学) → L1 图模型(foundation/cg-graph:
  Entity<GraphStore> = petgraph StableGraph + 算法桥 + 变更事件)
gpui 来自 crates.io 的 `gpui-pre` / `gpui-pre-platform` 快照依赖（精确 pin，与 gpui-kit 同版本），仓库内无上游源码、无 submodule；manifest 已由发布流程规范化，本项目无需复述 gpui 的 workspace 表。
```
