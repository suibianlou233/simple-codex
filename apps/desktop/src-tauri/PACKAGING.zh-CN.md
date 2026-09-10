# Windows 公开测试版打包

Local Agent 当前只生成 Windows x64 NSIS 安装包。安装范围是当前用户，不需要管理员权限；应用不包含自动更新器、遥测或账号模块。

## 构建环境

- Windows 10 或 Windows 11 x64；
- Rust 1.98 MSVC 工具链；
- Node.js 20+ 与 pnpm 11；
- Microsoft C++ Build Tools；
- 系统已安装 Microsoft Edge WebView2 Runtime。

安装包不会联网下载 WebView2。Windows 11 和仍受支持的 Windows 10 通常已经包含它；缺失时应由用户从 Microsoft 官方渠道单独安装。

## 生成安装包

```powershell
cd apps/desktop
pnpm install --frozen-lockfile
pnpm test
pnpm tauri build
```

首次构建会下载并校验 Tauri 使用的 NSIS 打包工具；这是构建机行为，不会给产品加入联网能力。预期产物位于仓库根目录下：

```text
target/release/local-agent-desktop.exe
target/release/bundle/nsis/Local Agent_0.1.0_x64-setup.exe
```

发布前为安装包生成 SHA-256：

```powershell
Get-FileHash -Algorithm SHA256 "target/release/bundle/nsis/Local Agent_0.1.0_x64-setup.exe"
```

如果终端仍位于 `apps/desktop`，上述路径应写为 `../../target/release/bundle/nsis/Local Agent_0.1.0_x64-setup.exe`。

## 发布验收

1. 在未安装旧版本的普通 Windows 用户账户中运行安装包；
2. 确认安装界面可选择简体中文或英文；
3. 确认开始菜单图标、窗口图标与卸载项图标清晰；
4. 启动应用，打开一个临时项目并创建任务；
5. 重启应用，确认项目、任务和本地事件仍可恢复；
6. 卸载应用，确认程序文件被移除；本地任务数据是否保留应在发布说明中明确告知用户。

当前测试版没有代码签名证书，Windows SmartScreen 可能显示“未知发布者”。公开分发前应使用项目所有者持有的合法代码签名证书签署最终安装包；不得提交证书或私钥到仓库。

## 图标来源

图标源稿是仓库内独立绘制的 `icons/source.svg`，由深色圆角方块、终端箭头和光标组成，不使用参考项目的品牌或图形资源。修改源稿后运行：

```powershell
cd apps/desktop
pnpm exec tauri icon src-tauri/icons/source.svg
```

该命令会重新生成 `.ico`、`.icns` 和各平台 PNG 尺寸。
