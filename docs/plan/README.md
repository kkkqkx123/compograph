# compograph 方案文档（Plan）

| 文档 | 内容 |
|---|---|
| [compograph-implementation-plan.md](./compograph-implementation-plan.md) | **分阶段实施方案**（0~11 章）：事实核查、现状与差距、阶段 0~3 的工作项/移植清单/验收标准、验证策略、风险与待拍板开放点、下一步行动 |
| [phase-1-detailed-plan.md](./phase-1-detailed-plan.md) | **阶段 1 细化方案**：几何数学层、空间索引、力导向移植、后台执行、边箭头绘制、指针事件桥、应用接线的任务分解、顺序与验收 |
| [phase-2-detailed-plan.md](./phase-2-detailed-plan.md) | **阶段 2 细化方案**：布局族、样式与旁路、多选框选、算法桥扩展与面板、图出入口、应用组装的任务分解、顺序与验收 |
| [phase-3-detailed-plan.md](./phase-3-detailed-plan.md) | **阶段 3 细化方案**：度量基线、细节层次、边聚合、索引与剔除改造、保留模式、后台调度、图片导出、路线 C 评估的任务分解、顺序与验收 |

## 约定

- 方案遵循 `AGENTS.md`：文档中文、避免大段代码、代码注释不得引用文档编号。
- 引用的 gpui / petgraph / cytoscape.js 行号均基于本地克隆核验（基线见方案第 0 章与 `../architecture/README.md` 事实基线表）。
- 代码落地后同步回改本文档与 `../architecture/` 中过期内容（更新而非仅标记）。
