# compograph 方案文档（Plan）

| 文档 | 内容 |
|---|---|
| [compograph-implementation-plan.md](./compograph-implementation-plan.md) | **分阶段实施方案**（0~11 章）：事实核查、现状与差距、阶段 0~3 的工作项/移植清单/验收标准、验证策略、风险与待拍板开放点、下一步行动 |
| [phase-1-detailed-plan.md](./phase-1-detailed-plan.md) | **阶段 1 细化方案**：几何数学层、空间索引、力导向移植、后台执行、边箭头绘制、指针事件桥、应用接线的任务分解、顺序与验收 |
| [phase-2-detailed-plan.md](./phase-2-detailed-plan.md) | **阶段 2 细化方案**：布局族、样式与旁路、多选框选、算法桥扩展与面板、图出入口、应用组装的任务分解、顺序与验收 |
| [phase-3-detailed-plan.md](./phase-3-detailed-plan.md) | **阶段 3 细化方案**：度量基线、细节层次、边聚合、索引与剔除改造、保留模式、后台调度、图片导出、路线 C 评估的任务分解、顺序与验收 |
| [feature-analysis-report.md](./feature-analysis-report.md) | **功能实测分析报告**：以代码为准盘点已落地功能、对照 cytoscape.js 的差距、待补充清单与优先级、文档与代码的偏差表（供回改） |
| [compograph-p0-plan.md](./compograph-p0-plan.md) | **P0 阶段方案**：节点标签文本渲染、DOT 导入、应用接线、文档回改的实施设计与验证 |
| [compograph-p1-plan.md](./compograph-p1-plan.md) | **P1 阶段方案**：算法桥补齐、中心性三件套、Radial 布局、节点形状族、箭头形状族的实施设计与验证（0/5 未开工） |
| [compograph-p2-plan.md](./compograph-p2-plan.md) | **P2 与远期方案**：边标签、界面接线、换行、曲线、样式扩展、PNG，以及聚类/复合节点/动画链/实例化后端的归属与拍板条件 |
| [compograph-p3-plan.md](./compograph-p3-plan.md) | **P3 设计决策**：聚类、欧拉路、最小割、复合节点、扩展机制、动画链、实例化后端的是否做、前置条件、落点与验收口径 |
| [compograph-p3-clustering-plan.md](./compograph-p3-clustering-plan.md) | **聚类分阶段实施方案**：基于 cytoscape 对照结论的层次、马尔可夫、k 均值三阶段落地与亲和传播暂缓结论、契约与验收 |
| [compograph-p3-next-design.md](./compograph-p3-next-design.md) | **P3 后续任务细化设计**：欧拉路与最小割接线、聚类接线、布局动画链、规模评估的取舍、落点与验收 |
| [cytoscape-gap-supplement-design.md](./cytoscape-gap-supplement-design.md) | **差距补充功能设计**：基于差距分析与参照清单的自由属性、选择器、复合节点、渲染填充、交互、动画、算法补齐的设计与交付顺序 |
| [cytoscape-tasks-1-3-refinement.md](./cytoscape-tasks-1-3-refinement.md) | **任务 1-3 细化修改方案**：自由属性与数据映射、类集合与轻量选择器、复合节点的落点模块、行为契约、跨层影响与验收口径 |
| [cytoscape-tasks-4-6-refinement.md](./cytoscape-tasks-4-6-refinement.md) | **任务 4-6 细化修改方案**：节点形状与箭头扩充、渐变填充、标签增强的落点模块、行为契约、跨层影响与验收口径 |
| [cytoscape-tasks-7-9-refinement.md](./cytoscape-tasks-7-9-refinement.md) | **任务 7-9 细化修改方案**：节点背景图、用户拐点边、力导向补强的落点模块、行为契约、跨层影响与验收口径 |
| [cytoscape-tasks-10-12-refinement.md](./cytoscape-tasks-10-12-refinement.md) | **任务 10-12 细化修改方案**：邻域高亮锁定与选择模式、元素动画、算法补齐的落点模块、行为契约、跨层影响与验收口径 |

## 约定

- 方案遵循 `AGENTS.md`：文档中文、避免大段代码、代码注释不得引用文档编号。
- 引用的 gpui / petgraph / cytoscape.js 行号均基于本地克隆核验（基线见方案第 0 章与 `../architecture/README.md` 事实基线表）。
- 代码落地后同步回改本文档与 `../architecture/` 中过期内容（更新而非仅标记）。
