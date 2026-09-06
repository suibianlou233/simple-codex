# Upstream and modification record

This crate is a source-derived Simple component, not a runtime wrapper around
`codex.exe`.

- Upstream project: OpenAI Codex (`https://github.com/openai/codex`)
- Upstream path: `codex-rs/windows-sandbox-rs`
- Imported tag: `rust-v0.130.0`
- Imported commit: `3ebb3e033d9be7fb48fc957216d4a97eb24bec1f`
- Upstream copyright: Copyright 2025 OpenAI
- License: Apache License 2.0 (see the repository root `LICENSE`)

Simple modifications are substantial and include standalone dependency and
policy types, Simple-owned accounts and Windows object names, local helper
binaries, removal of Codex app-server/agent/telemetry integration, removal of
legacy and TTY execution, a smaller pipe-only command runner, cancellation
propagation, local packaging, health reporting, and Simple-specific boundary
tests.

Files in this crate should be treated as modified unless a future provenance
manifest says otherwise.
