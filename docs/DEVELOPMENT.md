# Development / 开发说明

[English README](../README.md) · [中文 README](../README.zh-CN.md)

## Boundaries / 架构边界

- `apps/desktop/src`: React/TypeScript desktop UI, conversation projection, settings and first-run guide.
- `apps/desktop/src-tauri`: Rust/Tauri local desktop backend, persistence orchestration and credential boundary.
- `crates/model/src/kernel_compat`: version-specific kernel protocol and launch adapters.
- `crates/model/src`: local model gateway and provider adapters.
- `crates/core`, `context`, `storage`, `tools`, `sandbox`: local execution/support components, including retained legacy compatibility code.
- `crates/simple-sandbox-windows`: the Simple Windows sandbox derivative and helper binaries, with upstream attribution.
- `kernels/packages/official-283-windows-candidate-1`: the fixed kernel manifest, licenses and patch records. Compiled executables are **not** stored in Git.

This first source publication excludes machine-local handoff notes, private development logs, caches and historical binary snapshots. Older migration utilities remain in `scripts`; some require historical artifacts that are not included. The instructions below describe the current packaged kernel path, not the older slim path.

本次源码发布不包含本机交接笔记、开发日志、缓存和历史二进制快照。`scripts` 保留的旧迁移工具可能依赖未分发的历史产物；以下说明使用当前固定官方候选内核，不是旧 slim 路径。

## Prerequisites / 开发环境

- Windows 10/11 x64, WebView2, Git and Windows C++ Build Tools with a Windows SDK.
- Rust as pinned in `rust-toolchain.toml` (currently 1.98.0, MSVC target).
- Node.js suitable for Vite 7 (20.19+ or 22.12+) and pnpm; install dependencies with the checked-in lockfile.
- The **matching** kernel executables. Installing Rust/Node alone is not enough to run the native agent.

## Use the matching packaged kernel / 使用随包内核

1. Download and install the matching [Windows prerelease](https://github.com/suibianlou233/simple-codex/releases/tag/v0.1.1-rc.1).
2. In the installed Simple directory, locate `kernels/official-283-windows-candidate-1`. Copy only `apply_patch.exe`, `codex-app-server.exe` and `codex-code-mode-host.exe` into the source checkout's `kernels/packages/official-283-windows-candidate-1` directory. Keep the checked-in manifest and notices. **Do not copy user data or credentials.**
3. Build Simple's own helpers and install frontend dependencies from the repository root:

```powershell
./scripts/sync-simple-sandbox-runtime.ps1 -Profile debug
Push-Location apps/desktop
pnpm install --frozen-lockfile
Pop-Location
```

4. For a development window, select the fixed manifest explicitly before launching:

```powershell
$env:SIMPLE_CODEX_KERNEL_MANIFEST = (Resolve-Path './kernels/packages/official-283-windows-candidate-1/kernel.json').Path
Push-Location apps/desktop
pnpm tauri dev
Pop-Location
```

This development window uses the same candidate data slot as the packaged application. Close the installed app first. Use a separate Windows test account if you need a completely isolated user-data environment. Never point an unverified kernel at important existing data.

开发窗口与安装版使用同一候选数据槽，运行前请关闭安装版；需要完全隔离的用户数据时，请使用单独的 Windows 测试账户。不要把未经验证的内核交给重要的已有数据。

## Upstream source / 上游源码

The current source is fixed at OpenAI Codex commit `28327355b861ab6cc76b01c7248663eb1be440cf`. The registered Windows patch is in `kernels/packages/official-283-windows-candidate-1/changes.patch` and affects V8 internal sandbox features. Read `PATCHES.json` and `MODIFICATIONS.md` before applying it. Do not silently replace the pinned binaries with an arbitrary Codex download.

To obtain a source checkout for inspection and license collection, run from this repository root:

```powershell
git clone https://github.com/openai/codex.git upstream-codex
git -C upstream-codex checkout --detach 28327355b861ab6cc76b01c7248663eb1be440cf
```

The directory is ignored by Git. Building the upstream kernel yourself is a separate, platform-sensitive process: follow that revision's toolchain/build instructions, apply the recorded patch in a separate checkout, then register and verify the resulting package. A locally rebuilt executable will not necessarily have the distributed binary's checksum. Do not edit checksum fields merely to bypass verification.

## Build the Windows installer / 构建安装包

After preparing the matching kernel, frontend dependencies and pinned upstream source:

```powershell
./scripts/sync-simple-sandbox-runtime.ps1 -Profile release
./scripts/build-release.ps1 -KernelSource './upstream-codex' -Jobs 1
```

Output: `target/release/bundle/nsis/Simple_0.1.1_x64-setup.exe`. Add `-Offline` only after required dependencies and build tools are cached. This does not make the initial setup offline.

The distributed package was built on the development machine. These clean-checkout setup instructions have not been independently exercised on a fresh machine; report missing steps rather than assuming complete reproducibility.

分发包已在开发机完成构建；以上从零配置步骤尚未在全新机器上独立验收，不承诺完全可复现。遇到缺失步骤欢迎反馈。

## Optional checks / 可选检查

Run these only when you choose to test your changes:

```powershell
cargo test -p local-agent-model -p local-agent-storage -p local-agent-desktop --lib --locked
Push-Location apps/desktop
pnpm test
pnpm run typecheck
pnpm run test:ui
Pop-Location
```

UI checks use local fixtures, not real model credentials. Some integration tests need a separately configured kernel and are ignored by default. The existing memory-panel capability-loading case is a known failure; see the README limitations. A successful build is not complete functional or security acceptance.
