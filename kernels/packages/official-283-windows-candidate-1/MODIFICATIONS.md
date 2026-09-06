# 第二个内核：官方 28327355 候选版

状态：隔离后端验收候选，不允许替换当前桌面内核。不代表全部 Simple 功能已迁移。

## 来源与边界

- 官方固定提交：`28327355b861ab6cc76b01c7248663eb1be440cf`，来自本地官方 Git 仓库。
- 原样 checkout：`E:/快速开发/kernel-upstreams/codex-28327355`。
- 补丁构建 checkout：`E:/快速开发/kernel-upstreams/codex-283-win`。
- Simple 适配器：`crates/model/src/kernel_compat/upstream_283.rs`。
- 历史 `simple-codex-slim`、现有三件套、前端、生产 SQLite 与配置均不替换。

## 唯一源码补丁

`codex-rs/code-mode-runtime/Cargo.toml`：仅 Windows 不启用 `v8_enable_sandbox`，其他平台保持原值。
官方构建请求的 `rusty_v8_ptrcomp_sandbox_release_x86_64-pc-windows-msvc.lib.gz` 在 2026-09-06 返回 HTTP 404；
Windows 使用已有 150.4.0 普通发行 archive，SHA-256：
`571bf6a028576ac1413c8a942383f637f91e94b0c964bbeefff8a098637aaa40`。

这会关闭 V8 的内部内存 sandbox，是明确的防护取舍，不是“无行为变化”。它不是 Windows 文件/命令沙箱，后者配置仍为 unelevated。
不宣称 JS 引擎的安全性与启用 V8 sandbox 的构建等价，也不宣称完整系统沙箱已验收。
当上游提供可用 Windows sandbox 构建，或本项目接受自行构建同配置 V8 时移除此补丁并重验。
确切 patch、原因、顺序、校验与测试记录见 `patches/upstream-283-windows-v8/`。

## Simple 层适配

- 官方 provider 配置指向 Simple 的随机回环端口；模型凭据只由 Simple 网关持有，内核仅接收临时网关 token。
- token 不写入配置/argv；shell 环境明确排除 Simple 网关与常见凭据变量。
- 使用官方 `model_catalog_json` 提供启动模型别名及工具元数据，无需修改模型识别源码。当前只验收一个启动别名，不宣称动态模型切换完成。
- 明确关闭远程模型目录、插件后台同步、应用、远程控制、自动更新、遥测导出和自动记忆。配置约束不替代完整网络抓包审计。
- 仅允许空目录建立新数据合同；有旧文件但无对应合同标记时拒绝启动。可以恢复本候选版自己创建的历史，不能直接打开旧内核数据。
- 桌面选择候选包时明确拒绝，不悄悄把记忆关闭当成正常完整版本。

## 不兼容项与后续决策

| 项目 | 本候选状态 | 正式替换之前需要做什么 |
|---|---|---|
| 项目记忆隔离、memory/forget、禁用语义 | 未迁移；拒绝旧记忆合同 | 在 Simple 建独立项目 home/记忆治理，或导出最小可重放补丁；必须专门验收 |
| 旧 SQLite/rollout 回退 | 不读不迁移旧数据 | 全数据副本/数据槽切换及回退证明，不能仅换 exe |
| 动态模型配置版本 | 仅单启动别名 | 更新官方模型目录的生命周期方案，不能只向网关注册新别名 |
| 补丁 Windows alias / 失败状态 / 冲突保护 | 不假定继承历史定制 | 用真实后端行为逐项验证，失败记录，不伪造“同等能力” |
| 多代理、长期记忆、完整网络/沙箱边界 | 不在基本流程通过的结论内 | 独立验收后再解除桌面候选限制 |

## 构建复现（Windows）

固定上述提交，另建 detached checkout，并在其上 `git apply --check` 后应用登记 patch。
在 `codex-rs` 下使用 Rust 1.98.0：

```powershell
cargo +1.98.0 build --locked --profile dev-small -p codex-app-server --bin codex-app-server -p codex-code-mode-host --bin codex-code-mode-host -p codex-apply-patch --bin apply_patch
```

本机构建设置 `RUSTY_V8_ARCHIVE` 为上面已校验的本地 archive。
Cargo registry 在 C 盘、target 在 E 盘且没有符号链接权限时，预先将 target 的 `dev-small/gn_root` 建为对应 `v8-150.4.0` crate 源目录的 Junction，解决依赖构建要求；不修改依赖源码。
三件套必须同批构建，使用 `scripts/import-candidate-kernel.ps1` 登记为不可覆盖版本包；该脚本检查源码与 patch 对应，但不声称二进制可重复构建哈希一致。

## 验证记录

此节将在本批实际后端验证完成后填写。未经验证项不得从能力清单推断为通过。
上游 `just fmt` 的 Bazel 部分缺 dotslash，`just bazel-lock-update` 缺 Bazel，均未通过；当前是 Cargo Windows 候选构建，不宣称 Bazel 发行链合格。
