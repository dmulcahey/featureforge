# FeatureForge for Codex

This document is the Codex-specific overview for the FeatureForge runtime.

## Install

Use the checked-in installer instructions in [.codex/INSTALL.md](../.codex/INSTALL.md). That file is the source of truth for symlink, copy, and platform-specific setup details.

For a fresh Codex session, the minimal instruction is:

```text
Follow the checked-in instructions in .codex/INSTALL.md from this repository.
```

For the canonical validation matrix after install or update, see [docs/testing.md](testing.md).

## Discovery Layout

FeatureForge installs through one shared checkout:

- `~/.featureforge/install/skills`
- `~/.featureforge/install/.codex/agents/code-reviewer.toml`

Codex discovers those artifacts through:

- `~/.agents/skills/featureforge -> ~/.featureforge/install/skills`
- `~/.codex/agents/code-reviewer.toml -> ~/.featureforge/install/.codex/agents/code-reviewer.toml`

On Windows, the reviewer artifact may be copied instead of symlinked. Refresh that copy after updates.

## Runtime State

Runtime state lives under `~/.featureforge/`.

- config: `~/.featureforge/config/config.yaml`
- sessions: `~/.featureforge/sessions/`
- project artifacts and workflow manifests: `~/.featureforge/projects/`
- contributor logs: `~/.featureforge/contributor-logs/`

## Command Families

The supported command families are:

- `featureforge workflow`
- `featureforge repo-safety`
- `featureforge plan contract`
- `featureforge plan execution`
- `featureforge config`
- `featureforge update-check`
- `featureforge repo runtime-root`
- `featureforge repo slug`

## Workflow Summary

FeatureForge routes product work conservatively from repo-visible artifacts.

Accelerated review is an opt-in branch inside `plan-ceo-review` and `plan-eng-review`, not a separate workflow stage.

- `using-featureforge` is the human-readable entry router that consults `featureforge workflow` directly from repo-visible artifacts.
- `featureforge:project-memory` is an opt-in supportive memory skill for `docs/project_notes/*`; use it only for explicit memory-oriented requests or later follow-up updates, not as a default workflow stage or gate
- generated skill preambles always invoke the packaged install binary under `~/.featureforge/install/bin/` (`featureforge` on Unix, `featureforge.exe` on Windows), and that runtime resolves the active root through `featureforge repo runtime-root --path` before update checks or contributor-mode reads
- the generated `using-featureforge` skill routes through `featureforge workflow operator --plan <approved-plan-path>` directly when an approved plan path is already known; if no approved plan path is known, resolve it through the normal planning/review handoff, then route with workflow/operator
- `featureforge plan contract` compiles approved markdown into exact execution and review inputs
- workflow/operator and approved-plan execution metadata select the execution owner skill before work starts; do not route from status-only compatibility fields
- task closure is task-boundary gated: Task `N+1` may begin only after Task `N` has a current positive task-closure record; dedicated-independent fresh-context review loops and task verification are inputs to `featureforge plan execution close-current-task --plan <approved-plan-path> ...`; keep normal progression on operator-led intent-level commands and do not require low-level review-dispatch primitives in the normal path
- once approved-plan execution has started, execution-phase implementation/review subagent dispatch is pre-authorized and does not require per-dispatch user-consent prompts
- `featureforge workflow operator --plan <approved-plan-path>` is the normal routing surface after handoff; use `featureforge plan execution status --plan <approved-plan-path>` only for deeper diagnostics
- `resume_task` / `resume_step` from `featureforge plan execution status --plan <approved-plan-path>` are advisory-only diagnostics; if they conflict with workflow/operator `recommended_command`, follow `recommended_command`
- when workflow/operator reports `phase_detail=task_closure_recording_ready`, replay is complete enough to refresh closure truth; run the routed `close-current-task` command and do not reopen the same step again
- do not manually edit `**Execution Note:**` lines to recover runtime state; those markdown notes are projection-only
- after `featureforge plan execution repair-review-state --plan <approved-plan-path>`, run the returned `recommended_command` directly as the one exact next command before issuing any additional command
- `featureforge plan execution status --plan <approved-plan-path>` surfaces runtime strategy checkpoint state (`strategy_state`, `strategy_checkpoint_kind`, `last_strategy_checkpoint_fingerprint`, `strategy_reset_required`)
- for workflow-routed terminal sequencing, run `featureforge:document-release` before terminal `featureforge:requesting-code-review`, then continue to `featureforge:qa-only` (when required) and `featureforge:finishing-a-development-branch`
- compatibility/debug command boundaries (low-level `record-*` and related compatibility commands) must not be required in the normal path; normal progression stays on `workflow operator`, `close-current-task`, and `advance-late-stage`
- hidden compatibility/debug commands have been removed from the public CLI surface; normal routing and recommendations must use public commands only

Runtime strategy checkpointing is execution-owned, not planning-owned. The runtime records:

- `initial_dispatch` before repo-writing execution starts
- `review_remediation` when reviewable dispatch lineage enters remediation and when remediation reopens execution work
- `cycle_break` automatically when the same task reaches three reviewable dispatch/remediation cycles

This does not send the workflow back to planning stages; it keeps remediation in execution while preserving approved plan scope.

Checkpoint history is runtime-owned authoritative state (`strategy_checkpoints`). Runtime-owned task-review state carries the active strategy checkpoint fingerprint. Agents do not repair state-dir projection files directly; public commands regenerate derived metadata when needed.

Review note: this runtime strategy checkpoint layer is intentional contract hardening and should not be removed as "out-of-plan" cleanup when branch tests and runtime contracts require it.

Default planning pipeline:

`featureforge:brainstorming -> featureforge:plan-ceo-review -> featureforge:writing-plans -> featureforge:plan-eng-review`; `featureforge:plan-fidelity-review` runs only after engineering-review edits are complete, then `featureforge:plan-eng-review` performs final approval before implementation.

## Updating

Update the shared checkout with:

```bash
git -C ~/.featureforge/install pull
```

Then refresh any copied reviewer artifact if your platform does not use symlinks.

## Troubleshooting

1. Verify the skills link exists: `ls -la ~/.agents/skills/featureforge`
2. Verify the reviewer artifact exists: `ls -la ~/.codex/agents/code-reviewer.toml`
3. Verify the runtime responds: run the packaged install binary under `~/.featureforge/install/bin/` (`featureforge` on Unix, `featureforge.exe` on Windows) with `workflow help`
4. Re-run the checked-in install instructions if any link or copied artifact is missing
