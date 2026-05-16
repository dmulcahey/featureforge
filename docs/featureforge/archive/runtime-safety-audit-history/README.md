# Runtime Safety Audit History

This directory is the single index for superseded runtime-safety audit-loop material. Active docs should link here instead of linking individual per-loop reports or remediation plans unless a current task explicitly needs that exact historical artifact.

## Retention Rule

- Keep the current active remediation plan under `docs/featureforge/plans/`.
- Move superseded runtime-safety audit reports, remediation plans, and deep-audit reference notes into this archive after a later loop replaces them.
- Preserve historical artifacts append-only unless an active reference check proves a file is unreferenced and a cleanup task explicitly approves deletion.
- Do not use archived plans or reports as live workflow authority. They are evidence for why earlier remediation happened, not current routing or implementation instructions.

## Current Active Runtime-Safety Plan

- `docs/featureforge/plans/2026-05-16-runtime-safety-thirty-ninth-audit-remediation.md`

## Archived Material

As of this index, the archive retains:

- 17 superseded audit report files at this directory root.
- 43 superseded remediation plans under `plans/`.
- 10 deep-audit reference reports under `reference/`.

Recent superseded active plans moved here during prompt/archive compaction:

- `plans/2026-05-14-runtime-safety-thirty-second-audit-remediation.md`
- `plans/2026-05-14-runtime-safety-thirty-third-audit-remediation.md`
- `plans/2026-05-15-runtime-safety-thirty-fourth-audit-remediation.md`
- `plans/2026-05-15-runtime-safety-thirty-fifth-audit-remediation.md`
- `plans/2026-05-16-runtime-safety-thirty-sixth-audit-remediation.md`
- `plans/2026-05-16-runtime-safety-thirty-seventh-audit-remediation.md`
- `plans/2026-05-16-runtime-safety-thirty-eighth-audit-remediation.md`

## Reference Policy For Active Docs

When active docs need audit-loop context, reference this index and summarize the current lesson. Avoid adding new active references to individual archived loop files; that reintroduces prompt-surface noise and makes agents choose between historical plans.

Before deleting or relocating archived files, run an active-reference check outside this archive:

```bash
rg -n "runtime-safety-audit-history|thirty-fourth|thirty-fifth|thirty-sixth|runtime-signal-noise|deep-runtime-safety" README.md docs scripts tests skills references review .codex .copilot agents --glob '!docs/featureforge/archive/runtime-safety-audit-history/**'
```
