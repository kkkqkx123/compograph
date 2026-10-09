# zed-gpui submodule 拉取指南

**当前已改用longbridge的gpui-pre，该文档作为参考，如果后续要换回gpui-zed则可以参考该文档**

上游 gpui 源码以 git submodule 挂载在 `crates/vendor/zed-gpui`，指向 [kkkqkx123/zed-gpui](https://github.com/kkkqkx123/zed-gpui) 的 `lean` 分支。`lean` 分支是精简历史快照（提交数很少，无完整 zed 历史），因此**可以直接拉取，无需浅克隆**。

## 首次克隆项目后初始化

```shell
git submodule update --init
```

受限网络环境可临时使用代理：

```shell
export http_proxy="http://localhost:7890" https_proxy="http://localhost:7890"
git submodule update --init
```

## 更新到 lean 分支最新

```shell
git submodule update --remote
```

更新后若 gitlink 变化，在主仓库提交该变更。

## 配置说明

`.gitmodules` 已声明：

```ini
[submodule "crates/vendor/zed-gpui"]
    path = crates/vendor/zed-gpui
    url = https://gh-proxy.com/https://github.com/kkkqkx123/zed-gpui.git
    branch = lean
    shallow = true
```

- `branch = lean`：`submodule update --remote` 只跟踪 lean 分支。
- `shallow = true`：允许 `update --depth` 浅拉取；因 lean 历史极短，直接全量拉取即可。
- fetch refspec 已固化为 `+refs/heads/lean:refs/remotes/origin/lean`，任何 fetch 都只触达 lean 分支，不会拉取其他分支。

## 验证

```shell
git submodule status                 # 应显示 lean 分支的提交（指向上游定期同步的快照，不在文档维持具体 hash）
git -C crates/vendor/zed-gpui branch --show-current   # 应为 lean
git -C crates/vendor/zed-gpui log --oneline           # 应为少量提交的精简历史
```

## 注意事项

- 上游代码禁止修改、不格式化、不加业务依赖（见 AGENTS.md「上游源码规范」）。
- 上游同步只能在 zed-gpui 仓库内进行，本项目绝不单独 fetch/merge 上游仓库的其他分支。
- 升级上游后需复核所有引用 gpui API 的代码签名。上游定期同步，文档不维持具体提交号，以 submodule 指针为准。
