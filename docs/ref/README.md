# 架构设计分析总览（Architecture）

本目录收纳两个图论项目的架构设计分析：

| 文档 | 项目 | 主题 |
| --- | --- | --- |
| [cytoscape.js.md](./cytoscape.js.md) | Cytoscape.js | 前端「图模型 + 渲染 + 交互」一体化库的模块划分、Core/Collection 模型、扩展机制、渲染管线 |
| [petgraph.md](./petgraph.md) | Petgraph | Rust 纯计算库的 workspace 分层、6 种图类型、访问器 trait 体系、泛型算法设计 |

## 两个项目架构的对比速览

| 维度 | Cytoscape.js | Petgraph |
| --- | --- | --- |
| 语言/生态 | JavaScript / ESM + Rollup | Rust / Cargo workspace |
| 核心定位 | 图分析 **+ 可视化 + 交互** | **纯计算**，无渲染 |
| 数据模型 | `Core` + `Collection`（活对象） | 6 种图类型（邻接表/矩阵/CSR/哈希…） |
| 算法组织 | 方法挂到 `Collection.prototype`（mixin） | 自由函数作用于**访问器 trait** |
| 扩展性 | `extension.mjs` 四类扩展点（core/collection/layout/renderer） | trait 实现 + Cargo feature 开关 |
| 渲染 | 内置 Canvas/WebGL 渲染器 + 内置布局 | 无 |
| 图 I/O | 自有的 JSON 元素格式 | DOT / graph6 / serde |
| 运行环境 | 浏览器 + Node.js | 支持 `no_std`、可并行（rayon） |

## 算法差异

图算法支持的详细差异对比见 [`./graph-algorithms-diff.md`](./graph-algorithms-diff.md)。
