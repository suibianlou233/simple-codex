# Simple Codex contributor instructions

- Keep projects, orchestration, credentials and task records local. Model requests use the user's explicitly configured endpoint; never add telemetry or proprietary cloud-service dependencies.
- Never commit API keys, tokens, user databases, chat histories, private project content, or machine-local configuration. Redact diagnostics before sharing.
- Keep Simple compatibility in `crates/model/src/kernel_compat`. Pin upstream commits; preserve LICENSE/NOTICE and record necessary patches with reasons and hashes. Do not claim arbitrary-version hot swapping.
- Do not restore the removed whole-workspace file snapshot feature. Git workspace diff and legacy per-file undo are separate; conversation revert does not undo file edits.
- Respect permission/approval boundaries. Do not describe incomplete sandbox acceptance as proven isolation.
- Preserve existing user changes. Add focused checks for code changes, but do not run tests or model calls when the user asks for manual testing only.
- Read `docs/DEVELOPMENT.md` for the current source/build boundaries. Historical migration utilities are not the default launch path.


- Simple conversation mode exposes Terminal and Browser in the resizable right sidebar. The user-approved Simple Code mode adds a project tree and text editor, with AI/Terminal/Browser on the right. Preserve conversation state and unsaved editor buffers on mode changes. Terminal uses an interactive PTY; do not restore the command input/run form or the removed whole-workspace Files/Diff snapshot panels.
