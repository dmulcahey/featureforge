# FeatureForge for GitHub Copilot Local Installs

This document is the GitHub Copilot-specific overview for the FeatureForge runtime.

## Install

Use the checked-in installer instructions in [.copilot/INSTALL.md](../.copilot/INSTALL.md). That file is the source of truth for symlink, copy, and platform-specific setup details.

For a fresh Copilot session, the minimal instruction is:

```text
Follow the checked-in instructions in .copilot/INSTALL.md from this repository.
```

For the canonical validation matrix after install or update, see [docs/testing.md](testing.md).

## Discovery Layout

FeatureForge installs through one shared checkout:

- `~/.featureforge/install/skills`
- `~/.featureforge/install/agents/code-reviewer.md`

GitHub Copilot discovers those artifacts through:

- `~/.copilot/skills -> ~/.featureforge/install/skills`
- `~/.copilot/agents/code-reviewer.agent.md -> ~/.featureforge/install/agents/code-reviewer.md`

On Windows, the reviewer artifact is typically copied instead of symlinked. Refresh that copy after updates.

Do not register workspace-local `<repo>/skills` as an active Copilot discovery root during FeatureForge development; those files are generated product artifacts under test only.

## Installed Control Plane

Live workflow execution must use the installed runtime and installed skills:

- runtime: `~/.featureforge/install/bin/featureforge` (or `featureforge.exe` on Windows)
- skills: `~/.featureforge/install/skills`
- Copilot discovery: `~/.copilot/skills` must resolve to the installed skills directory, not to a workspace-local `<repo>/skills` directory

Workspace-local `./bin/featureforge`, `target/debug/featureforge`, and
`cargo run -- ...` are test subjects only. They may run fixture or shell-smoke
commands only with an isolated temp or fixture `FEATUREFORGE_STATE_DIR`; they
must not mutate live `~/.featureforge` workflow state.

The guard override
`FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1` is intentionally
auditable and should almost never be used. Approved uses must be recorded in
execution evidence and review provenance.

Verify discovery and runtime provenance with:

```bash
readlink ~/.copilot/skills
~/.featureforge/install/bin/featureforge doctor self-hosting --json
```

## Runtime State

Runtime state lives under `~/.featureforge/`.

- config: `~/.featureforge/config/config.yaml`
- sessions: `~/.featureforge/sessions/`
- project artifacts and workflow manifests: `~/.featureforge/projects/`
- contributor logs: `~/.featureforge/contributor-logs/`

## Command Families

The supported command families are:

- `$_FEATUREFORGE_BIN workflow`
- `featureforge doctor`
- `$_FEATUREFORGE_BIN repo-safety`
- `$_FEATUREFORGE_BIN plan contract`
- `$_FEATUREFORGE_BIN plan execution`
- `featureforge config`
- `featureforge update-check`
- `featureforge repo runtime-root`
- `featureforge repo slug`

## Workflow Summary

FeatureForge routes product work conservatively from repo-visible artifacts.

Accelerated review is an opt-in branch inside `plan-ceo-review` and `plan-eng-review`, not a separate workflow stage.

- `using-featureforge` is the human-readable entry router that consults `$_FEATUREFORGE_BIN workflow` directly from repo-visible artifacts.
- `featureforge:project-memory` is an opt-in supportive memory skill for `docs/project_notes/*`; use it only for explicit memory-oriented requests or later follow-up updates, not as a default workflow stage or gate
- generated skill preambles always invoke the packaged install binary under `~/.featureforge/install/bin/` (`featureforge` on Unix, `featureforge.exe` on Windows), and that runtime resolves the active root through `featureforge repo runtime-root --path` before update checks or contributor-mode reads
- the generated `using-featureforge` skill routes through `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` directly when an approved plan path is already known; if no approved plan path is known, resolve it through the normal planning/review handoff, then route with workflow/operator
- `$_FEATUREFORGE_BIN plan contract` compiles approved markdown into exact execution and review inputs
- workflow/operator and approved-plan execution metadata select the execution owner skill before work starts; do not route from status-only compatibility fields
- task closure is task-boundary gated: Task `N+1` may begin only after Task `N` has a current positive task-closure record; dedicated-independent fresh-context review loops and task verification are inputs to `$_FEATUREFORGE_BIN plan execution close-current-task --plan <approved-plan-path> ...`; keep normal progression on operator-led intent-level commands and do not require low-level review-dispatch primitives in the normal path
- once approved-plan execution has started, execution-phase implementation/review subagent dispatch is pre-authorized and does not require per-dispatch user-consent prompts
- `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` is the normal routing surface after handoff; use `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` only for deeper diagnostics
- `resume_task` / `resume_step` from `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` are advisory-only diagnostics; if they conflict with workflow/operator `recommended_public_command_argv`, follow the argv from workflow/operator
- when workflow/operator reports `phase_detail=task_closure_recording_ready`, replay is complete enough to refresh closure truth; run the routed `close-current-task` command and do not reopen the same step again
- do not manually edit `**Execution Note:**` lines to recover runtime state; those markdown notes are projection-only
- after `$_FEATUREFORGE_BIN plan execution repair-review-state --plan <approved-plan-path>`, run the returned `recommended_public_command_argv` directly when present as the one exact next command before issuing any additional command, except installed-control-plane rebinding (`featureforge` argv[0] must execute as `~/.featureforge/install/bin/featureforge`); if argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic; otherwise satisfy typed `required_inputs` or the prerequisite named by `next_action`, then rerun the route owner; `recommended_command` is display-only compatibility text and must not be shell-parsed for invocation
- `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` surfaces runtime strategy checkpoint state (`strategy_state`, `strategy_checkpoint_kind`, `last_strategy_checkpoint_fingerprint`, `strategy_reset_required`)
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

1. Verify the skills link exists: `ls -la ~/.copilot/skills`
2. Verify the reviewer artifact exists: `ls -la ~/.copilot/agents/code-reviewer.agent.md`
3. Verify the runtime responds: run the packaged install binary under `~/.featureforge/install/bin/` (`featureforge` on Unix, `featureforge.exe` on Windows) with `workflow help`
4. Re-run the checked-in install instructions if any link or copied artifact is missing
