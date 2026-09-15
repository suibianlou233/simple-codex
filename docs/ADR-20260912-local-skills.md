# Simple local skill library

Decision: keep skill installation, selection and persistence in the Simple desktop;
reuse the audited official-283 kernel's native skills service and catalog prompt.
The fixed kernel package and upstream source are unchanged.

## Storage and activation

`%LOCALAPPDATA%/dev.localagent.desktop/skills/library.json` is shared across model
profiles/history partitions. Import copies the complete selected skill folder into
an immutable UUID revision under `packages`. Updates switch the index atomically;
previous revisions and original sources remain untouched. No credentials, plugin
cache, Codex account state or personal skills are included in the source repository.

Before creating or resuming a turn, register only enabled revision directories with
`skills/extraRoots/set { extraRoots }`. Native `skills/list` performs forced reload.
This adapter path is enabled for the official-283 package, preserving legacy launch.
Disabling affects future discovery; it cannot erase instructions already read into
an existing conversation or interrupt work already underway. Project/.agents skills
outside this managed library remain governed by the kernel's own discovery rules.

The model receives names/descriptions and chooses skills based on the task. There is
no novel-specific keyword router or automatic execution of imported scripts. A UI
label reports a completed shell read of valid SKILL.md frontmatter; it is evidence
of reading, not a guarantee of full compliance. Alternate read tools/output formats
may not produce that label. Native assistant commentary is still displayed normally.

## Import boundary

The main desktop alone can invoke management commands; embedded browser pages are
excluded by the existing invoke-handler boundary. The renderer supplies candidate
IDs or opens a native folder picker, never arbitrary filesystem paths. YAML metadata
is parsed as data. Complete resource/script folders are copied without execution.
Known credential filenames are rejected case-insensitively; links/reparse ancestors,
non-regular files, oversized files/packages and excessive depth/count are rejected.
Limits: 16 MiB/file, 32 MiB/package, 2000 files/directories, 12 levels, 128 installed skills.
Source checks are basic migration checks, not a full secret scanner or process sandbox.
Concurrent mutation by another local process is outside this import check's guarantee.

Python/Node availability, dependency manifests, agents/openai.yaml tool dependencies,
known Codex-specific tools and machine-specific paths are inspected. Findings disable
new imports by default and appear in the manager; explicit enable remains available.
No dependency installation, external network or plugin service is triggered by import.

## Verification (2026-09-12)

- Native fixed executable: register/discover/remove/re-enable, twice across restart.
- Rust persistence/security tests: full-folder import, atomic update, disabled state,
  original and prior revision preservation, credentials/name/size rejection, read label.
- Frontend: 151 tests; application and UI TypeScript checks; Edge import/view/disable/
  Escape test. Skill source renders as inert text; screenshot checked.
- Real configured model through Simple's own gateway, isolated home and project:
  prompt asks only to initialize a Chinese suspense novel, without any skill name.
  Native successful SKILL.md read and `.novel-project/manifest.json` creation observed.
  The first run omitted a verbal skill announcement; a successful-read UI label was
  added and tested instead of relying on model self-report. Second live run passes.
- Evidence: target/skill-live-20260912-a and -b, target/skills-manager.png. These local
  test artifacts/rollouts are not publication inputs. Novel project validator passes.
- Personal write-serialized-novel imported through the same product import function:
  six files, enabled, no basic dependency findings. The source folder is unchanged.

No release build or GitHub publication is part of this iteration. The YAML parser
is pinned in Cargo.lock and listed in THIRD_PARTY_NOTICES; release-time license
collection must include its resolved dependencies before the next distribution.

Batch migration extends the bounded asset limit for the comic skill’s 10.7 MB demo image. Plugin-origin skills default to pending environment validation; importing a skill does not install its plugin.
