# Simple Codex v0.1.0-rc.1

## 中文

首个 GitHub 预发布版本：一个基于固定 Codex 开源执行内核、本地优先的 Windows 桌面 AI 编程助手。

非常感谢 OpenAI 开源 Codex，使本项目能够在其基础上继续开发。

- 包含 Windows 安装程序、本地控制层、固定内核与辅助程序。
- 支持在界面填写模型配置，新增首次启动引导与设置中的引导入口。
- 完成后的过程说明折叠，显示本轮用时，保留最终回答。
- 包含本地项目/会话、流式回复、命令与文件工具、审批/停止及 Git 工作区差异等功能。

下载 Assets 中的 `Simple-0.1.0-rc.20260906-windows-x64-222419.zip`，解压后运行 setup.exe。需要 Windows 10/11 x64 和预装 WebView2；无需编译，也不包含开发者密钥或聊天记录。旁边的 `.sha256.txt` 用于核对下载完整性。源码压缩包不是安装包。

这只是非常初始的版本，可能存在错误，遇到问题请见谅并提供脱敏反馈；我会持续开发。未签名，不是完整验收的稳定版。长期记忆迁移、多代理完整流程、完整沙箱边界仍未完成；Windows V8 内部沙箱有已记录的构建差异。请先备份重要项目。更多限制见 README。

## English

The first GitHub prerelease of Simple Codex: a local-first Windows desktop AI coding agent built around a pinned open-source Codex execution kernel.

Thank you to OpenAI for open-sourcing Codex and making this work possible.

- Windows installer with the local backend, pinned kernel and helpers.
- Model API configuration, a first-run guide, and a guide entry in settings.
- Collapsed completed progress messages with elapsed time and a separate final answer.
- Local projects/conversations, streaming replies, command/file tools, approvals, stop controls and Git workspace diffs.

Download `Simple-0.1.0-rc.20260906-windows-x64-222419.zip` under Assets, extract it, and run setup.exe. Requires Windows 10/11 x64 and preinstalled WebView2. No compilation is needed; developer credentials and conversations are not included. Use the adjacent `.sha256.txt` to verify integrity. GitHub's source archives are not installers.

This is a very early, unsigned release, not a fully accepted stable product. Bugs may occur; please bear with me, share redacted reports, and expect continued development. Automatic memory migration, complete multi-agent workflows and full sandbox acceptance remain unfinished. The Windows V8 build has a documented internal-sandbox difference. Back up important work and read the README limitations.

## SHA256

```text
dd271d58591119cc65b322c979fcab396b2da50f24dcfbb72f948703b7976f6d  Simple-0.1.0-rc.20260906-windows-x64-222419.zip
```
