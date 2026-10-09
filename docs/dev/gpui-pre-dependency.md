# gpui-pre 依赖说明

gpui 及其平台后端来自 crates.io 上的 `gpui-pre` 快照 crate（`gpui` 这个库名由
`gpui-pre` 提供，`gpui_platform` 由 `gpui-pre-platform` 提供），在本仓库根
`Cargo.toml` 的 `[workspace.dependencies]` 中以精确版本声明，无 submodule、
无 vendored 源码、无 path 依赖。

## 与 gpui-kit 的版本关系

gpui-kit 工作区消费同一条 `gpui-pre-*` 快照线。两者的 pin 必须完全一致
（当前 `=0.3.8`），否则 compograph 与 gpui-kit 应用各自链接两套不兼容的
gpui 类型，图视图无法在 gpui-kit 应用中使用。gpui-kit 侧的 pin 由
`script/check-gpui-pin.ts` 在 CI 强制为精确版本。

## 升级步骤

1. 修改根 `Cargo.toml` 中 `gpui` 与 `gpui_platform` 的精确版本，与 gpui-kit
   根 `Cargo.toml` 的对应 pin 同步修改。
2. 执行 `cargo update -p gpui-pre -p gpui-pre-platform`（或直接重建）以刷新
   `Cargo.lock`。
3. 按编译错误逐一复核引用 gpui API 的代码签名，重点是窗口、绘制与测试 API。
4. 运行 `cargo clippy --workspace --all-targets` 完成全量编译检查。

## 后端 feature

`gpui_platform` 启用 `x11`、`wayland`（Linux 桌面后端）与 `font-kit`、
`runtime_shaders`（macOS 字体与着色器），与 gpui-kit 的取值一致。缺少
`x11`/`wayland` 时，`guess_compositor` 在桌面会话下返回 Wayland/X11 会触发
`unreachable!`，应用无法打开窗口；两者至少启用其一才能在 Linux 桌面运行。

## 受限网络

依赖全部来自 crates.io（经镜像解析），不再依赖 GitHub git 依赖；受限网络下
只需为 cargo 配置 crates.io 镜像（如 rsproxy）。
