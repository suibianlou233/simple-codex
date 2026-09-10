# Third-party notices

## Image and browser dependencies (2026-09-08)

Simple uses unmodified image 0.25.9 (MIT OR Apache-2.0,
https://github.com/image-rs/image) for image format and dimension validation,
and tokio-tungstenite 0.28.0 (MIT,
https://github.com/snapview/tokio-tungstenite) with tungstenite 0.28.0
(MIT OR Apache-2.0, https://github.com/snapview/tungstenite-rs) for local
browser CDP transport. Existing base64 0.22.1 and reqwest dependencies are
reused. Cargo.lock pins versions and registry checksums, including transitive
dependencies. scripts/collect-release-licenses.mjs collects the exact resolved
dependency license and copyright files into every release before packaging.
No upstream source was copied or modified for these integrations.

tokio-tungstenite MIT notice:

Copyright (c) 2017 Daniel Abramov
Copyright (c) 2017 Alexey Galakhov

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## process-wrap 10.0.0

Simple uses the unmodified `process-wrap` 10.0.0 crate for managed command and
native-kernel process lifetimes. The model host now directly uses the existing
locked dependency with Tokio, Job Object, KillOnDrop and CreationFlags support.
Source: https://github.com/watchexec/process-wrap . Cargo.lock records the
crates.io checksum `0e3f4237d0e4741eb50bc5584db701f1299c85fa31ff0274dd6445e79dc42d12`.
The Apache-2.0 license option applies; a copy is provided in `LICENSE`.
The exact crate's COPYRIGHT identifies these retained authorship notices:

Copyrights in this project are retained by their contributors. No copyright
assignment is required to contribute. Some files include explicit copyright
notices and/or license notices. For full authorship information, see the version
control history. Except as otherwise noted, the project is licensed under
Apache-2.0 or MIT at the user's option.

The read2() method in src/std/core.rs is adapted from the read2() function in
the Rust standard library at sys/unix/pipe.rs, copyright the Rust Contributors,
licensed under Apache-2.0 and MIT.
Reference: https://github.com/rust-lang/rust/blob/c9aa2595d9ba2313bbfcc8e0244231baa9c5d9e0/library/std/src/sys/unix/pipe.rs#L76-L122

The job_object() and resume_threads() methods in src/windows.rs are adapted
from the Windows version of the new() method in watchexec at src/process.rs,
copyright Matt Green, licensed under Apache-2.0 only.
Reference: https://github.com/watchexec/watchexec/blob/07974e0d1411e79e653a5a5f610f923834628038/lib/src/process.rs#L305-L393

## dirs 6.0.0

Simple directly uses the already locked Tauri dependency `dirs` 6.0.0 to resolve
the OS user-local data location for cross-installation project coordination.
The crate is unmodified; source: https://github.com/dirs-dev/dirs-rs .
Cargo.lock records the crates.io checksum. Licensed under MIT OR Apache-2.0;
the MIT notice from this exact crate version follows:

Copyright (c) 2018-2019 dirs-rs contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## OpenAI Codex Windows sandbox source

Simple's `simple-windows-sandbox` component is a modified derivative of the
`codex-rs/windows-sandbox-rs` module from OpenAI Codex. It was imported from
tag `rust-v0.130.0`, commit `3ebb3e033d9be7fb48fc957216d4a97eb24bec1f`,
and then adapted into a standalone Simple component.

Source: https://github.com/openai/codex

Copyright 2025 OpenAI

Licensed under the Apache License, Version 2.0. A copy of the license is
provided in this repository's `LICENSE` file. Simple's modifications include
removing Codex agent/app-server/telemetry dependencies and legacy/TTY paths,
renaming identities, resources, IPC endpoints and WFP objects, and adding the
Simple adapter, packaging, cancellation and verification layers.

No OpenAI trademark or endorsement is implied.

## Current redistributable kernel (2026-09-06)

The `bundled-official-kernel` distribution includes OpenAI Codex at commit
`28327355b861ab6cc76b01c7248663eb1be440cf`, package
`official-283-windows-candidate-1`, under Apache-2.0. Source:
https://github.com/openai/codex/tree/28327355b861ab6cc76b01c7248663eb1be440cf .
Its original LICENSE and NOTICE, exact changes.patch, PATCHES.json and
MODIFICATIONS.md are installed in `kernels/official-283-windows-candidate-1`.
One recorded Windows V8 build-feature patch is applied; this is not an
unmodified upstream binary. Simple's compatibility layer is maintained outside
the upstream tree. No OpenAI trademark or endorsement is implied.

The following older slim-kernel notes are retained as development history.
That older kernel binary is NOT included in the current redistributable;
its memory/forgetting and execution-hardening changes must not be assumed
present in the current candidate. The external release README describes the
current product limitations; the immutable package documents record provenance.

## Historical pinned Simple agent kernel

DEBUG-029 adds a private `codex-state` source-exclusion module, dynamic persisted
cutoff checks, coordinated memory forgetting and the experimental `memory/forget`
app-server API. Stable/experimental schema fixtures are regenerated from the fixed
fork; original conversation history and the Apache-2.0 attribution are retained.

The DEBUG-028 maintenance follow-up modifies `codex-memories-write` startup,
consolidation lifetime and reset handling, with a new private OS-file-lock activity
module and tests. App-server routes memory reset through the coordinated handler.
These remain changes to the same fixed Apache-2.0 derivative, not a new dependency.

The 2026-09-05 project-memory follow-up also pins app-server memory scope across
configuration requests/reloads, with explicit in-process startup binding.
Module changes and verification are recorded in Simple DEBUG-026 and the fork's
SIMPLE_SLIM.md; the upstream revision and license remain unchanged.

The app-server, Code Mode host and standalone apply_patch tool are Apache-2.0 derivatives of OpenAI Codex,
fixed at upstream commit `2b7c279735d0d096cf7b34fe98938f46792f4d4f`.
Source: https://github.com/openai/codex . The development handoff includes the
source, original `LICENSE` and `NOTICE` in `../simple-codex-slim/`; its
`SIMPLE_SLIM.md` records removals and modifications, including the 2026-09-05
live memory-switch semantics, native Windows patch aliases, BYOK patch-tool
availability, PowerShell execution-status preservation, and the staged project-memory
SQLite partition API and explicit native project-scope configuration (separate
generated memory/jobs, aligned read/write/reset artifact roots, project-cwd guards
on memory contributions/startup/extra read grants, native inherited-source
inspection and extraction/consolidation eligibility checks, unchanged native
history; desktop activation and complete forgetting semantics remain pending).
Release resources retain `codex-LICENSE`,
`codex-NOTICE`, and `codex-MODIFICATIONS.md` through the kernel sync script.
Simple owns the local gateway, UI, task-tree cancellation adapter, and Windows
native sandbox launch configuration; these do not imply full OS isolation.

## Interactive terminal (2026-09-08)

Unmodified portable-pty 0.9.0 (MIT, https://github.com/wez/wezterm), @xterm/xterm 5.5.0 and @xterm/addon-fit 0.10.0 (MIT, https://github.com/xtermjs/xterm.js) provide the OS PTY and terminal rendering. Cargo.lock and pnpm-lock.yaml pin exact package integrity. Simple-owned bridge and panel code connect them; the fixed agent kernel is unchanged.

MIT License

Copyright (c) 2018 Wez Furlong

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.


Copyright (c) 2017-2019, The xterm.js authors (https://github.com/xtermjs/xterm.js)
Copyright (c) 2014-2016, SourceLair Private Company (https://www.sourcelair.com)
Copyright (c) 2012-2013, Christopher Jeffrey (https://github.com/chjj/)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.


Copyright (c) 2019, The xterm.js authors (https://github.com/xtermjs/xterm.js)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
