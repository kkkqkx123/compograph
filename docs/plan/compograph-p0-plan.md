# compograph P0 阶段方案

> 版本：v1.0 · 日期：2026-09-29
> 关联文档：[功能实测分析报告](./feature-analysis-report.md) · [功能清单](../architecture/feature-list.md) · [分阶段实施方案](./compograph-implementation-plan.md) · [AGENTS.md](../../AGENTS.md)
> 事实基线：compograph 本次克隆；工具链 1.98.1；已实测 `cargo check --workspace --all-targets` 通过、6 个自建 crate 共 177 测试全绿。

---

## 0. 文档信息

本方案承接 [功能实测分析报告](./feature-analysis-report.md) 的结论，落地其中的 P0 项。按 AGENTS.md 约定：文档用中文、避免大段代码、以自然语言描述改动；代码/注释一律英文且不得引用文档编号。行号引用基于本次克隆核验。

---

## 1. 背景与目标

实测分析确认 compograph 实现程度远超文档口径，但存在三项 P0 缺口：

1. **节点标签文本完全未渲染**：`crates/render/cg-render/src/style.rs:38` 已有 `label_size` 字段、`style.rs:204` 已有 `NodePredicate::LabelIs`，`crates/app/compograph/src/main.rs:860` 已收集 `labels`，但绘制侧无任何字形输出。
2. **DOT 导入是占位**：`crates/foundation/cg-graph/src/io.rs:188` 的 `import_dot` 忽略入参、直接返回错误。
3. **文档进度口径过期**：`feature-list.md`、`architecture-design.md`、`compograph-implementation-plan.md` 仍按 P0/P1 描述，与实际差距大。

目标：补齐前两项能力，回改第三项使文档与代码一致，全部改动编译/测试/clippy/fmt 通过。

---

## 2. 事实核查（Facts）

| 项 | 现状 | 位置（已核验） |
|---|---|---|
| 标签样式字段 | `NodeStyle.label_size` / `EdgeStyle.label_size` 存在，默认 12.0 / 11.0 | `style.rs:38/49/62/72` |
| 标签谓词 | `NodePredicate::LabelIs(String)` 已实现匹配 | `style.rs:204/212/215` |
| 标签数据收集 | `render()` 内已构造 `labels: Vec<(NodeIndex, String, usize)>` | `main.rs:860` |
| 悬停文本 | `hover_label` 已生成 tooltip 文本 | `main.rs:319` |
| 字形绘制 | **无**——`graph_view` 只画 edge/arrow/node/rubber band | `view.rs:831` |
| gpui 文本 API | `Window::text_system()`；`TextSystem::shape_line`；`ShapedLine::paint` | `window.rs:2358`、`text_system.rs:638`、`text_system/line.rs:108` |
| `TextRun` | 字段含 `len`/`font`/`color`/… | `text_system.rs:1228` |
| `Font` | 结构体 | `text_system.rs:1292` |
| DOT 导出 | `export_dot` 用 petgraph `Dot` 格式化 | `io.rs:180` |
| DOT 导入 | **占位**：`import_dot(_encoded)` 忽略入参 | `io.rs:188` |
| JSON 导入导出 | 完整实现 + 校验 | `io.rs:167/173` |
| 应用 DOT 入口 | 仅有 `export dot` 按钮，无导入 | `main.rs:1066` |
| 文件 IO | `import_json_file`/`export_json_file`/`write_text_file` 齐全 | `file_io.rs` |

---

## 3. 现状与差距（Gap）

| 能力 | 现状 | 差距 |
|---|---|---|
| 节点标签 | 样式与数据齐备，无绘制 | 缺"计划生成 + 文本整形 + 绘制"三段 |
| DOT 导入 | 仅报错占位 | 缺解析器 |
| 应用 DOT 导入 | 无入口 | 缺文件读取封装 + 按钮 + 处理器 |
| 文档 | 停留在 P0/P1 | 与实际差 6 项以上 |

---

## 4. P0-1 节点标签文本渲染

### 4.1 设计

标签渲染拆为"与 gpui 无关的计划层"和"gpui 绘制层"，前者可 headless 单测，符合项目既有的 `paint_nodes_for` / `graph_view` 分离模式。

- **新建 `crates/render/cg-render/src/text.rs`**：
  - `PaintedLabel { id, text, origin, size, color }`：屏幕像素坐标下的标签绘制项，字段对齐 `PaintedNode` 风格。
  - `paint_labels_for(nodes: &[PaintedNode], labels: &[(NodeIndex, String)], level: DetailLevel) -> Vec<PaintedLabel>`：消费已筛选的可视节点集合，产出标签计划；文本为空或 `DetailLevel::Minimal` 时跳过；位置取节点方块下沿（`origin = (node 中心 x, node.origin.y + node.side)`），水平居中交由绘制侧 `align_width` 完成。
  - 字号来自 `NodeStyle.label_size`，首版以常量默认值（与 `style.rs:49` 一致），后续可经 style 闭包细化。
  - 同文件 `#[cfg(test)]`：跟随可视集合、空文本剔除、零尺寸节点、Minimal 隐藏。
- **`lib.rs`**：声明 `pub mod text;` 与 `pub use text::{PaintedLabel, paint_labels_for};`。
- **`view.rs`**：`graph_view` 增第 5 参 `labels: Vec<PaintedLabel>`，在节点之后、rubber band 之前绘制：
  - `window.text_system().shape_line(text.into(), px(size), &[TextRun], None)`；
  - `ShapedLine::paint(origin, line_height, TextAlign::Center, Some(width), window, cx)`；
  - 颜色 `u32` → `Hsla`（复用现有 `rgb`/`with_opacity` 思路）；整形或绘制失败静默跳过，不 panic（遵循禁 `unwrap`）。
  - 更新文档注释说明绘制顺序 edge → arrow → node → label → rubber band。
- **`main.rs`**：复用 `labels`（`main.rs:860`），在 `render()` 内由 `visible_ids` 生成 `Vec<PaintedLabel>` 并传入 `graph_view`（`main.rs:1128`）。
- **LOD 联动**：`DetailLevel::Minimal` 隐藏标签；在 `lod.rs` 补 `draws_labels` 谓词保持与其他降级项一致的口径。

### 4.2 缓存说明

首版标签**不进入 `RetainedCache`**，每帧按可视集合生成。理由：标签是文本整形产物，其缓存键与节点/边图元不同，混入保留模式会引入额外的失效维度；首版以正确性优先，性能策略单列后续。

---

## 5. P0-2 DOT 导入（自写轻量解析）

### 5.1 范围

`crates/foundation/cg-graph/src/io.rs` 的 `import_dot` 由占位改为真实实现，支持 DOT 常用子集：

- 头：`strict? (di)?graph <name>? { ... }`；
- 语句：节点 `a [attrs]`、边 `a -> b [attrs]`、`a -- b`（视为有向边写入）、`node`/`edge`/`graph` 默认属性块（可忽略或仅记录）；
- 分隔：`;` 与 `,`；
- 注释：`//` 行注释、`#` 行注释、`/* */` 块注释；
- 字符串：双引号（含 `\"` 转义）与裸标识符；
- 属性：`key=value` 与 `key="value"`，值可为数字或字符串。

### 5.2 产出映射

- 节点 id：按首次出现顺序从 0 递增；内部维护 `name -> id` 表。
- `label`：取 `label` 属性，缺省用节点名。
- `weight`：取 `weight` 属性，缺省 `1.0`；非有限值在 `validate` 阶段被拒。
- `position`：取 `pos="x,y"`（`!` 后缀忽略），可选。
- 解析失败返回 `IoError::invalid(含记号/位置)`，绝不 panic；成功后复用 `GraphDocument::validate`。

### 5.3 与导出的对齐

`export_dot` 走 petgraph `Dot`，输出形如 `digraph { 0 [ label = "a" ] ... }`。自写解析器需能读回该格式。测试用**语义比较**（节点数、边数、权重集合、标签集合）而非逐字节，避免与格式化细节耦合。

### 5.4 测试

同文件 `#[cfg(test)]`：往返（export→import 语义等价）、最小图、带权重/标签、孤立节点、行/块注释、语法错误报错、空输入。

---

## 6. P0-3 应用接线

- **`file_io.rs`**：新增 `import_dot_file(path) -> Result<GraphDocument, String>`（读文本 → `cg_graph::import_dot`），与 `import_json_file` 对称；补单测（含缺失文件报错）。
- **`main.rs`**：新增 `import_dot` 处理器（与 `import_json` `main.rs:537` 对称，走文件对话框 + `apply_document`）；顶栏在 `export dot` 旁加 `import dot` 按钮。

---

## 7. P0-4 文档回改

按 [分析报告 §4](./feature-analysis-report.md) 偏差表逐条更新（更新而非仅标记）：

| 文档 | 更新点 |
|---|---|
| `feature-list.md` | Grid/Circle/BFS/Concentric、力导向、边/箭头、LOD、保留模式、图片导出（注明 PPM）、算法桥 11 个 → 标为已落地 |
| `architecture-design.md` | §9 实施路线与 §8 开放点按实际收口更新 |
| `compograph-implementation-plan.md` | §3 差距表刷新；§5 阶段 0/1 的实际落地差异补记 |
| 本文 | 记录标签渲染与 DOT 导入的落地 |

---

## 8. 交付顺序

1. P0-2 DOT 导入（纯 `cg-graph` 内改动，零渲染风险）
2. P0-3 应用接线
3. P0-1 标签渲染（跨 `view.rs` / `main.rs`）
4. P0-4 文档回改
5. 全量验证

---

## 9. 验证策略

| 步骤 | 命令 | 期望 |
|---|---|---|
| 编译检查 | `cargo check --workspace --all-targets` | 通过 |
| 单元/集成测试 | `cargo test`（6 自建 crate） | 全绿，且在 177 基础上新增标签/DOT 用例 |
| 静态检查 | `cargo clippy --all-targets --all-features` | 自研 crate 0 警告 |
| 格式 | `cargo fmt --all --check` | 通过 |
| 环境 | rsproxy + gh-proxy + 1.98.1 | 已就绪 |

---

## 10. 风险与开放点

| 编号 | 事项 | 说明 | 处置 |
|---|---|---|---|
| OQ-P1 | 标签是否进保留模式缓存 | 缓存键维度不同 | 首版不进，后续单列 |
| OQ-P2 | 边标签 | 本轮仅节点标签 | 列入后续阶段 |
| OQ-P3 | 中文换行/锚点 | 首版单行居中 | 多行后续 |
| OQ-P4 | 字号来源细化 | 首版常量默认 | 后续接 `NodeStyle.label_size` 全链路 |
| OQ-P5 | DOT 语法覆盖度 | 首版常用子集 | 记录不支持的语法，按需扩展 |

---

## 11. 下一步

1. 落地 P0-2/P0-3（DOT 导入 + 接线）。
2. 落地 P0-1（标签渲染）。
3. 执行 P0-4 文档回改。
4. 全量验证；目标机 `cargo run -p compograph` 目视确认标签与 DOT 导入。
