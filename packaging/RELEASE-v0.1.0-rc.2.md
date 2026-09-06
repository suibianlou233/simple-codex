# Simple Codex v0.1.0-rc.2

## 中文

本次为 Windows 启动窗口的小修复版本，感谢大家试用这个早期项目。

- 修复发行主程序启动时附带终端窗口的问题：将 Windows release 入口改为图形界面程序，开发模式仍保留控制台。
- 固定 Codex 内核、模型配置、数据目录及现有功能不变。
- 重新生成 Windows x64 安装包，并将桌面入口加入分发源码指纹清单。

下载 Assets 中的 `Simple-0.1.0-rc.20260906-windows-x64-234042.zip`，解压后运行其中的 setup.exe。更新前正常关闭 Simple 并备份重要项目；旧安装包不会自动更新。应用内部版本仍为 0.1.0。

需要 Windows 10/11 x64 和预装 Microsoft Edge WebView2。无需自行编译，不包含开发者密钥、模型配置或聊天记录。源码压缩包不是安装包。

验证：前端、桌面 release 和 NSIS 构建成功；最终主程序 PE Subsystem 为 2（Windows GUI）；固定内核文件、分发文件校验及 23 项源码指纹通过。未执行安装、窗口启动、真实模型或功能测试，实际体验仍需用户手动验收。

本包未签名，仍是非常初始的预发布版；长期记忆迁移、多代理完整流程和全部沙箱安全边界等既有限制没有在本次修复中解决。详见 README。旧 rc.1 安装包及校验文件已撤下，统一使用本页 rc.2 新包。

## English

A small Windows startup fix for this early project. Thank you for trying Simple Codex.

- Fixes the extra terminal window attached to the release desktop application by using the Windows GUI subsystem. Development builds retain their console.
- Leaves the pinned Codex kernel, model configuration, data location and existing features unchanged.
- Rebuilds the Windows x64 installer and includes the desktop entry point in the release source fingerprints.

Download `Simple-0.1.0-rc.20260906-windows-x64-234042.zip` under Assets, extract it and run setup.exe. Close Simple normally and back up important projects before updating. Older installers do not update automatically. The internal application version remains 0.1.0.

Requires Windows 10/11 x64 and preinstalled Microsoft Edge WebView2. No compilation is required. Developer credentials, model settings and conversations are not included. GitHub source archives are not installers.

Verification: frontend, desktop release and NSIS builds succeeded; the final desktop executable has PE Subsystem 2 (Windows GUI); fixed-kernel checksums, distribution checksums and 23 source fingerprints passed. Installation, window startup, real-model and functional testing were not performed; manual acceptance is still required.

This remains an unsigned, very early prerelease. Existing limitations, including unfinished automatic memory migration, full multi-agent acceptance and complete sandbox-boundary acceptance, are unchanged. See the README. The old rc.1 installer and checksum have been removed; use the rc.2 package on this page.

## SHA256

```text
f4dd50e463d5a66cf7bc5ed8db959d4779d08ecd97f5131a3bec1ec6af779bdb  Simple-0.1.0-rc.20260906-windows-x64-234042.zip
```
