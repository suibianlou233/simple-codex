# Simple Codex

**A local-first, open-source AI coding agent for Windows, built around a pinned Codex execution kernel.**

[简体中文](README.zh-CN.md) · English · [Download for Windows](https://github.com/suibianlou233/simple-codex/releases/download/v0.1.1-rc.1/Simple-0.1.1-rc.20260910-windows-x64-224336.zip) · [Report an issue](https://github.com/suibianlou233/simple-codex/issues)

Thank you to OpenAI for open-sourcing Codex. Its source code makes it possible for me to learn from its architecture and continue building Simple Codex on that foundation.

Simple Codex brings a desktop interface, a local control layer, and model API adapters around the Codex execution kernel. Open a project, describe a task, and let the agent read code, propose or make changes, and run commands under the permissions you choose. The current Windows package uses Simple's model gateway with DeepSeek; adapters for Qwen and OpenAI-compatible endpoints are also included, but compatibility depends on the model and endpoint.

> **Very early release.** This project is still under active development and may contain bugs, incomplete features, or unexpected behavior. If you try it and encounter problems, please bear with me and share a reproducible issue. I will keep improving it. Back up important work before letting an agent modify it.

## Download and get started

1. Download [the 0.1.1 Windows installer ZIP](https://github.com/suibianlou233/simple-codex/releases/download/v0.1.1-rc.1/Simple-0.1.1-rc.20260910-windows-x64-224336.zip). Do not download GitHub's automatically generated source-code archive.
2. Extract the ZIP and run the setup program. Windows 10/11 x64 and Microsoft Edge WebView2 Evergreen Runtime are required. The installer does not download WebView2 automatically.
3. Open Simple. The first-run guide takes you to model settings. Enter your endpoint, model name, and API Key, then save.
4. Choose **Open project** and select a local folder. Start with a small, read-only task and an approval-based permission level.

Example first task:

```text
Read this project and explain how to start it. Do not modify files or run commands yet.
```

Reopen the guide from **Settings → View usage guide**. The current interface is primarily Simplified Chinese; these labels appear as **设置 → 查看使用引导**.

The package includes the frontend, local backend, pinned kernel, and helper programs. End users do not need Rust, C++ Build Tools, or frontend build tools. Your own project may still require Git, Node.js, Python, or other dependencies. The package is unsigned; check its SHA256 checksum and keep your normal security precautions.

## What is included today?

- **Desktop coding workflow:** local projects and conversations, streaming answers, stop controls, task search, and conversation history.
- **Code and command tools:** project reading, file editing, and terminal execution through the kernel and Simple's permission controls.
- **Right sidebar:** embedded browser and a real interactive PowerShell terminal, with a draggable width divider. Files/Diff panel entries have been removed. Completed progress messages collapse into an execution-process section with elapsed time; the final answer stays visible.
- **Model configuration:** DeepSeek, Qwen, and OpenAI-compatible API adapters; credentials are stored in the operating system's credential store.
- **Local control:** local task storage and diagnostics, approval-based execution, and Windows sandbox integration with documented limitations.
- **Separated architecture:** the pinned Codex kernel and Simple compatibility layer are managed separately; necessary upstream patches retain provenance and checksums.

These are implemented areas, not a claim that every provider, task, or security boundary has passed complete acceptance testing.

## Local-first does not mean offline inference

Projects, task records, and settings are kept on your machine by default. With a remote model API, prompts and the project/tool context needed for the task are sent to your configured endpoint. Provider charges and privacy policies still apply.

The download contains no developer API keys, model settings, conversations, or projects. Do not post secrets or private project content in issues.

## Architecture and kernel updates

Simple owns the desktop UI, persistence, model gateway, credentials, and compatibility adapters. The distributed Codex kernel is pinned to commit [`28327355`](https://github.com/openai/codex/tree/28327355b861ab6cc76b01c7248663eb1be440cf).

The goal is manageable upstream upgrades without mixing product compatibility into upstream code. **Arbitrary Codex versions cannot be hot-swapped today.** New versions need adapter review, package verification, and behavior checks; active tasks must not be switched between kernels.

See [development setup](docs/DEVELOPMENT.md), the [kernel manifest](kernels/packages/official-283-windows-candidate-1/kernel.json), and the [patch record](kernels/packages/official-283-windows-candidate-1/PATCHES.json).

## Known limitations

- This Windows kernel is not a complete feature-equivalent replacement for the older customized kernel.
- Automatic long-term memory and forgetting have not been migrated. End-to-end multi-agent workflows and complete sandbox boundaries are not fully accepted.
- Changing models within an existing task currently requires restarting Simple.
- A Windows compound command can have an intermediate failure even when its final exit code is successful. Check the actual result.
- The recorded Windows V8 patch disables V8's internal sandbox, distinct from operating-system command sandboxing. Do not treat the app as a fully verified security boundary.
- Git workspace diffs are not exact per-turn diffs. There is no whole-project snapshot-based undo; reverting a conversation does not revert its file changes.
- A known settings issue can hide the local memory panel when capability loading fails.

## FAQ

### Is this a desktop app for using Codex's open-source kernel with DeepSeek?

Yes. Simple supplies the desktop interface and local model gateway around a pinned Codex kernel. DeepSeek is the current development integration; this does not mean every Codex feature or every DeepSeek endpoint is supported.

### Can I develop Simple Codex from source?

Yes. The frontend uses React and TypeScript; the desktop backend uses Rust and Tauri. See [development setup](docs/DEVELOPMENT.md). Compiled kernel binaries are distributed separately from Git source history.

### How can I help?

Try a small task and [report issues](https://github.com/suibianlou233/simple-codex/issues) with your Simple version, Windows version, reproduction steps, and expected/actual results. Redact API keys, private paths, prompts, and source code before sharing logs or screenshots.

## License

Simple Codex is licensed under [Apache-2.0](LICENSE). Reused components retain their applicable licenses, notices, and modification records; see [third-party notices](THIRD_PARTY_NOTICES.md). The Windows release includes dependency-license information and kernel provenance.

## 0.1.1 update

Image attachments support drag/drop, paste and previews; DeepSeek image requests automatically use the vision model. The browser and PTY terminal share a resizable right sidebar. Terminal sessions survive task/browser switches. The fixed kernel is unchanged. Frontend tests (150), release build, startup and package checks passed; full image/browser model workflows and clean-machine installation still need acceptance.
