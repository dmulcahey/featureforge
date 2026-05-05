---
name: executing-plans
description: Use when you have an engineering-approved FeatureForge implementation plan and need to execute it in a separate session
---
<!-- AUTO-GENERATED from SKILL.md.tmpl — do not edit directly -->
<!-- Regenerate: node scripts/gen-skill-docs.mjs -->

## Preamble (run first)

```bash
_REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
_BRANCH_RAW=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo current)
[ -n "$_BRANCH_RAW" ] && [ "$_BRANCH_RAW" != "HEAD" ] || _BRANCH_RAW="current"
_BRANCH="$_BRANCH_RAW"
_FEATUREFORGE_INSTALL_ROOT="$HOME/.featureforge/install"
_FEATUREFORGE_BIN="$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge"
if [ ! -x "$_FEATUREFORGE_BIN" ] && [ -f "$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge.exe" ]; then
  _FEATUREFORGE_BIN="$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge.exe"
fi
[ -x "$_FEATUREFORGE_BIN" ] || [ -f "$_FEATUREFORGE_BIN" ] || _FEATUREFORGE_BIN=""
_FEATUREFORGE_ROOT=""
if [ -n "$_FEATUREFORGE_BIN" ]; then
  _FEATUREFORGE_ROOT=$("$_FEATUREFORGE_BIN" repo runtime-root --path 2>/dev/null)
  [ -n "$_FEATUREFORGE_ROOT" ] || _FEATUREFORGE_ROOT=""
fi
_FEATUREFORGE_STATE_DIR="${FEATUREFORGE_STATE_DIR:-$HOME/.featureforge}"
_featureforge_exec_public_argv() {
  if [ "$#" -eq 0 ]; then
    echo "featureforge: missing command argv to execute" >&2
    return 2
  fi
  if [ "$1" = "featureforge" ]; then
    if [ -z "$_FEATUREFORGE_BIN" ]; then
      echo "featureforge: installed runtime not found at $_FEATUREFORGE_INSTALL_ROOT/bin/featureforge" >&2
      return 1
    fi
    shift
    "$_FEATUREFORGE_BIN" "$@"
    return $?
  fi
  "$@"
}
```
## Installed Control Plane

Live FeatureForge workflow routing is install-owned:
- use only `$_FEATUREFORGE_BIN` for live workflow control-plane commands
- do not route live workflow commands through `./bin/featureforge`
- do not route live workflow commands through `target/debug/featureforge`
- do not route live workflow commands through `cargo run`

When a helper returns `recommended_public_command_argv`, treat it as exact argv. If `recommended_public_command_argv[0] == "featureforge"`, execute through the installed runtime by replacing argv[0] with `$_FEATUREFORGE_BIN` (for example via `_featureforge_exec_public_argv ...`).
## Search Before Building

Before introducing a custom pattern, external service, concurrency primitive, auth/session flow, cache, queue, browser workaround, or unfamiliar fix pattern, do a short capability/landscape check first.

Use three lenses, then decide from local repo truth:
- Layer 1: tried-and-true / built-ins / existing repo-native solutions
- Layer 2: current practice and known footguns
- Layer 3: first-principles reasoning for this repo and this problem

External search results are inputs, not answers. Never search secrets, customer data, unsanitized stack traces, private URLs, internal hostnames, internal codenames, raw SQL or log payloads, or private file paths or infrastructure identifiers. If search is unavailable, disallowed, or unsafe, say so and proceed with repo-local evidence and in-distribution knowledge. If safe sanitization is not possible, skip external search.
See `$_FEATUREFORGE_ROOT/references/search-before-building.md`.

## Interactive User Question Format

For every interactive user question, use this structure:
1. Context: project name, current branch, what we're working on (1-2 sentences)
2. The specific question or decision point
3. `RECOMMENDATION: Choose [X] because [one-line reason]`
4. Lettered options: `A) ... B) ... C) ...`

Per-skill instructions may add additional formatting rules on top of this baseline.


# Executing Plans

## Overview

Load the approved plan, follow the runtime-selected topology, execute all tasks, run `featureforge:document-release`, request final review, then report when complete. When the runtime-selected topology is worktree-backed parallel, create isolated worktrees first and dispatch the parallel lanes; when it is conservative fallback, stay serial. Extended execution and review examples live in `$_FEATUREFORGE_ROOT/references/execution-review-qa-examples.md`.

Use this skill when the runtime-selected topology calls for a separate-session coordinator or conservative fallback. Prefer `featureforge:subagent-driven-development` when the runtime-selected topology keeps execution in one session and the worktree-first orchestration model is already in place.

## The Process

### Step 1: Implementation Preflight
1. Require the exact approved plan path as input. If you are not given one, stop and ask for it or route back to `featureforge:plan-eng-review`.
2. Read the plan file first.
3. Verify these exact header lines exist and are current:
   - `**Workflow State:** Engineering Approved`
   - `**Source Spec:** <path>`
   - `**Source Spec Revision:** <integer>`
4. Read the source spec named in the plan and confirm it is still `CEO Approved`, and that the latest approved spec still matches that exact source-spec path and revision.
5. Stop immediately and redirect:
   - to `featureforge:plan-eng-review` if the plan is draft or malformed
   - to `featureforge:writing-plans` if the source spec path or revision is stale
6. Verify workspace readiness before starting:
   - stop on a default protected branch (`main`, `master`, `dev`, or `develop`) unless the user explicitly approves in-place execution
   - stop on detached HEAD
   - stop if merge conflicts, unresolved index entries, rebase, or cherry-pick state is present
   - if the working tree is dirty, stop unless the helper-selected topology and workspace-prepared context explicitly support isolated worktree-backed execution for this run
7. Do not bulk-clean the workspace ad hoc. If the helper-selected topology or protected-branch gate requires isolated execution, provision or route through a worktree-backed workspace before mutating repo state, and let the runtime-owned barrier flow reconcile reviewed work back onto the active branch and clean temporary worktrees at safe intervals.
8. The later repo-safety checks still govern any additional protected branches declared through repo or user instructions.
9. Run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` before starting execution.
10. If workflow/operator does not report `phase` `executing`, stop and follow the reported `phase`, `phase_detail`, `next_action`, and `recommended_public_command_argv` instead of reopening execution through compatibility helpers. Treat `recommended_command` as display-only compatibility text.
11. If workflow/operator confirms `phase` `executing`, review the plan critically for execution concerns and treat workflow/operator plus `plan execution status` as the live execution surface. Tracked checklist/evidence markdown is an optional materialized export; the event log remains authoritative for routing and gates.
12. Treat execution start as a hard gate, not a reminder:
   - no code edits and no test edits are allowed after workflow/operator confirms the current execution preflight handoff and before the first `begin` for the active step
   - no repo mutation is allowed until that first `begin` is recorded
   - the first `begin` is the mandatory execution-tracking boundary; preflight acceptance alone is not permission to start implementation
   - if the workspace becomes dirty before the first `begin`, expect later execution-start checks to fail closed (for example `tracked_worktree_dirty`) until the workspace is reconciled or isolated
   - retroactive execution tracking is recovery-only and must never be treated as the normal execution path
   - five-step recovery runbook for dirty-before-begin failures:
     1. reconcile or isolate the workspace
     2. rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` and confirm the current route is still `executing` for the current approved plan revision
     3. use that helper-backed route before any recovery mutation
     4. backfill only factual-only completed steps using authoritative helper mutations; never infer completion from dirty diffs
     5. resume from the task-boundary review and verification gate before any next-task `begin`

## Helper-Owned Execution State

- calls `$_FEATUREFORGE_BIN workflow operator --plan ...` during preflight
- uses `status --plan ...` only for additional diagnostics when operator output alone is insufficient
- uses the workflow/operator execution-start handoff instead of separate compatibility-helper choreography before execution starts
- calls `begin` before starting work on a plan step
- calls `complete` after each completed step
- reports interruptions or blockers in the handoff/status surface instead of invoking a removed execution-note command
- `record-contract`, `record-evaluation`, `record-handoff`, `begin`, removed `note`, `complete`, `reopen`, and `transfer` are authoritative helper mutation boundaries.
- Coordinators and subagents may prepare candidate artifacts (for example task packets, coverage matrix context, and handoff drafts), but they must not directly invoke these commands; the runtime helper owns and executes execution-state mutations.
- On the first `begin` for a revision whose plan still says `**Execution Mode:** none`, initialize execution with `--execution-mode featureforge:executing-plans`
- The approved plan checklist is the human-visible execution progress projection. The event log remains authoritative for routing and gates; do not create or maintain a separate ad hoc task tracker outside those shared surfaces.
- Runtime read models are rendered under the state directory during normal execution. Repo-local projection files under `docs/featureforge/projections/` are optional human-readable exports; do not create or maintain a separate ad hoc task tracker outside workflow/operator and status.
- Use `$_FEATUREFORGE_BIN plan execution materialize-projections --plan <approved-plan-path>` for state-dir-only diagnostic projection refreshes. If the user explicitly needs repo-local human-readable projection exports, add `--repo-export --confirm-repo-export`; approved plan and evidence files are not modified, and materialization is never required for normal progress. Add `--scope execution|late-stage|all` only when a non-default export scope is needed.

## Runtime Strategy Checkpoints (Automatic, Runtime-Owned)

- Runtime strategy checkpoints are execution-owned state, not workflow-stage transitions. Keep public workflow phase in execution (`executing`) while strategy checkpoints change; remediation stays represented by checkpoint state and operator routing.
- The approved plan/spec scope is fixed during execution. Runtime strategy checkpoints may change topology, lane/worktree allocation, subagent assignment, and remediation order, but must not change approved scope, source plan revision, or required coverage.
- Required checkpoint kinds:
  - `initial_dispatch`: required before repo-writing implementation starts. Runtime records it automatically on first dispatch/begin when missing.
  - `review_remediation`: required after actionable independent-review findings and before remediation starts. Runtime records it automatically when reviewable dispatch lineage enters remediation and when remediation reopens execution work.
  - `cycle_break`: required when churn is detected. Runtime records it automatically when the same task hits three review-dispatch/reopen cycles in one run.
- Cycle-break trigger: cap remediation churn at 3 cycles per task. On the third cycle, transition to `cycle_break` strategy automatically (no human replanning loopback).
- Reviewers return summaries/results; the controller/runtime binds those results to authoritative state and runtime-owned review projections retain checkpoint fingerprints for traceability. Agents do not search for, repair, or depend on those projection files.
- Surface and respect runtime strategy status from `$_FEATUREFORGE_BIN plan execution status --plan ...`:
  - `strategy_state`
  - `strategy_checkpoint_kind`
  - `last_strategy_checkpoint_fingerprint`
  - `strategy_reset_required`

## Execution-Phase Subagent Dispatch Policy

- Once execution is active for an approved plan (`execution_started` is `yes`), runtime-selected implementation and review subagent dispatch is authorized and does not require per-dispatch user-consent prompts.
- This authorization is limited to execution-phase dispatch performed by workflow-owned execution skills (`featureforge:executing-plans` and `featureforge:subagent-driven-development`).
- Non-execution ad-hoc delegation still follows normal user-consent policy.

## Protected-Branch Repo-Write Gate

Before starting any plan step that mutates repo state, run the shared repo-safety preflight for that exact task slice:

```bash
$_FEATUREFORGE_BIN repo-safety check --intent write --stage featureforge:executing-plans --task-id <current-task-slice> --path <repo-relative-path> --write-target execution-task-slice
```

- Use one stable task id per repo-writing task slice and pass the concrete repo-relative paths when they are known.
- If the helper returns `allowed`, continue with that task slice.
- If it returns `blocked`, name the branch, the stage, and the blocking `failure_class`, then route to either a feature branch / `featureforge:using-git-worktrees` or explicit user approval for this exact task slice.
- If the user explicitly approves protected-branch writes, approve the full task-slice scope with `$_FEATUREFORGE_BIN repo-safety approve --stage featureforge:executing-plans --task-id <current-task-slice> --reason "<explicit user approval>" --path <repo-relative-path> --write-target execution-task-slice [--write-target git-commit] [--write-target git-merge] [--write-target git-push]`, then re-check before continuing.
- Before a follow-on `git commit`, `git merge`, or `git push`, re-run the gate with the same task id, paths, and approved write-target set.
- If the protected-branch task scope changes, run a new approval plus full-scope check before continuing.
- Do not treat a worktree on `main`, `master`, `dev`, or `develop` as safe by itself; the branch must be non-protected or explicitly approved.

### Step 2: Execute Tasks

For each task:
1. Before starting a task, build the canonical task packet:

```bash
"$_FEATUREFORGE_BIN" plan contract build-task-packet \
  --plan <approved-plan-path> \
  --task <task-number> \
  --format markdown \
  --persist yes
```

2. treat it as the exact task contract for that execution segment. Coordinator-added logistics may clarify branch, cwd, or base commit, but they may not reinterpret approved requirements.
   - The packet's `Goal`, `Context`, indexed `CONSTRAINT_N` obligations, indexed `DONE_WHEN_N` obligations, covered requirements, and file scope are authoritative.
   - `CONSTRAINT_N` obligations must be checked by task reviewers and must not be softened into advice.
   - `Done when` obligations must not be reinterpreted or replaced with prose summaries; objectively reviewable obligations remain mandatory even when verified by diff inspection or targeted evidence rather than one command.
   - Separate-session handoffs must paste the helper-built packet verbatim and may not replace it with a coordinator-written summary.
   - If packet content conflicts with `review/plan-task-contract.md`, stop and route back to plan review instead of guessing.
3. Use workflow/operator and `plan execution status` as the live step-progress surface for the task's steps; tracked checklist/evidence markdown is optional materialized output and is not routing authority.
4. Follow each step exactly (plan has bite-sized steps).
5. Run verifications as specified.
6. For FeatureForge-on-FeatureForge execution, every execution-evidence update must include a runtime provenance section with:
   - installed runtime path used for live workflow routing
   - installed runtime hash used for live workflow routing
   - workspace runtime hash used for tests/fixtures (or `none` when no workspace runtime was used)
   - state dir used for live workflow commands
   - explicit confirmation that workspace runtime did not mutate live workflow state (or the explicit approved override record when it did)
7. After the implementation steps for a task are complete, enforce the mandatory task-boundary closure loop before beginning the next task:
   - MUST dispatch dedicated-independent task review in a fresh-context subagent; coordinator or implementer self-review never satisfies this gate
   - if review fails, reopen/remediate/re-review until green
   - Review findings must use deterministic repair-packet fields: `Finding ID`, `Severity`, `Task`, `Violated Field or Obligation`, `Evidence`, `Required Fix`, and `Hard Fail`.
   - when remediation churn reaches 3 cycles for the same task, follow runtime cycle-break handling before retry
   - after review is green, run `verification-before-completion` and collect the verification result inputs needed by `close-current-task`
   - Task `N+1` may begin only after Task `N` has a current positive task-closure record
   - dedicated-independent review loops plus verification are required inputs to `close-current-task`; they are not separate begin-time authority once Task `N` has a current positive closure
   - rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; when `recommended_public_command_argv` is absent, treat the closure command shape as an input contract and provide concrete review/verification values through `required_inputs` before rerunning workflow/operator
   - workflow/operator must route normal task-boundary closure through `task_closure_recording_ready` / `close-current-task`, not `task_review_dispatch_required`; if a task-review dispatch phase appears, treat it as a runtime diagnostic bug instead of manual low-level command choreography
   - if workflow/operator remains in a `*_dispatch_required` lane after an external review result is ready, keep rerouting through workflow/operator and the intent-level commands; do not expand the normal path into low-level dispatch-lineage management
   - no exceptions: only after close-current-task succeeds may Task `N+1` begin
8. If the packet is malformed, stale, or still leaves ambiguity unresolved, stop and route back to review instead of guessing.
9. Call `complete` as soon as a step is truly satisfied so the authoritative event log records the completed step. Do not manually flip tracked plan checkboxes during normal execution.

### Reviewed-Closure Command Matrix

For the reviewed-closure mental model, read `docs/featureforge/reference/2026-04-01-review-state-reference.md` before acting on late-stage routing. A current reviewed closure matches the current reviewed state. A superseded closure was valid for earlier reviewed work but is no longer authoritative after later reviewed work lands. A stale-unreviewed state means unreviewed edits exist, so the runtime MUST repair review state before recording another closure or late-stage milestone.

`$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` is the only normal-path routing authority for reviewed-closure and late-stage progression.
Treat `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` as authoritative for `phase`, `phase_detail`, `review_state_status`, `next_action`, `recommended_public_command_argv`, and `required_inputs`. Treat `recommended_command` as display-only compatibility text.
Treat `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` as optional diagnostic detail.
When executing `recommended_public_command_argv`, if argv[0] is `featureforge`, run it through `$_FEATUREFORGE_BIN` (or `_featureforge_exec_public_argv`) instead of PATH or workspace-runtime resolution.

When an external task-review or final-review result is already in hand, use `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready` to expose recording-ready routes. Do not use that hint for release-readiness, document-release, or QA routing.

Do not reconstruct closure routing from memory or duplicate route tables in this skill. Follow operator-reported `phase`, `phase_detail`, `review_state_status`, `next_action`, `recommended_public_command_argv`, `required_inputs`, and `recording_context` directly, with these guardrails:
- `task_closure_recording_ready` requires `recording_context.task_number`.
- `release_readiness_recording_ready` and `release_blocker_resolution_required` require `recording_context.branch_closure_id`.
- `final_review_recording_ready` requires `recording_context.branch_closure_id`.
- Treat `resume_task` and `resume_step` in diagnostic status output as advisory-only fields; if they disagree with workflow/operator `recommended_public_command_argv`, follow the argv from workflow/operator.
- When `phase_detail=task_closure_recording_ready`, replay is already complete enough for closure refresh; run `close-current-task` and do not reopen the same step again.
- When workflow/operator reports `review_state_status` as stale or missing closure context, do not invent a repair command. If `recommended_public_command_argv` is present, invoke it exactly. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic. Otherwise satisfy `required_inputs` or run `$_FEATUREFORGE_BIN plan execution repair-review-state --plan <approved-plan-path>` only when the non-diagnostic route owns that repair lane.
- After `repair-review-state`, MUST follow that command's returned `recommended_public_command_argv` when present before any additional recording commands. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic; otherwise satisfy typed `required_inputs` or the prerequisite named by `next_action`, then rerun the route owner. Do not shell-parse or whitespace-split `recommended_command`.
- The returned `recommended_public_command_argv` is authoritative for the immediate reroute only when present; `recommended_command` is only the human-readable rendering of the same route.
- Use `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` only when additional diagnostics are required.
- Keep compatibility/debug-only runtime primitives out of the normal path unless explicitly debugging a compatibility boundary.
- Hidden compatibility/debug command entrypoints are removed from the public CLI; normal routing must use public commands only.
- In `*_dispatch_required` lanes, request the review and keep rerouting through workflow/operator; do not expand the normal path into low-level dispatch-lineage management.
- MUST NOT manually edit runtime-owned execution records.
- MUST NOT manually edit `**Execution Note:**` lines to recover runtime state.
- MUST NOT manually edit derived markdown projection artifacts.
- MUST NOT repair runtime progress by editing tracked plan, evidence, review, readiness, QA, or strategy projection files.
- MUST NOT use the internal task-closure recording service boundary directly.
- MUST use `close-current-task` for task closure.

Late-stage aggregate command coverage:
- `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path>`
- `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path> --result ready|blocked --summary-file <release-summary>` is an input shape after substituting concrete values
- `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path> --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <final-review-summary>` is an input shape after substituting concrete values
- `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path> --result pass|fail --summary-file <qa-report>` is an input shape after substituting concrete values
- Compatibility-only escape hatch: use low-level runtime primitives only when explicitly debugging or preserving compatibility.

### Step 3: Request Final Review

After all tasks complete and verified:
- Run `featureforge:document-release` first, then route to `featureforge:requesting-code-review` for the terminal final review pass.
- Announce: "I'm using the requesting-code-review skill for the final review pass."
- **REQUIRED SUB-SKILL:** Use `featureforge:requesting-code-review`
- Resolve any Critical or Important findings before proceeding

### Step 4: Complete Development

After the final review is resolved:
- Announce: "I'm using the finishing-a-development-branch skill to complete this work."
- **REQUIRED SUB-SKILL:** Use featureforge:finishing-a-development-branch
- Follow that skill to verify tests, require `qa-only` when browser QA is warranted, require `document-release` for workflow-routed work, present options, and execute the chosen completion path

- **featureforge:writing-plans** - Creates the plan this skill executes
- **featureforge:plan-eng-review** - Provides the approved plan and the execution preflight handoff
- **featureforge:requesting-code-review** - REQUIRED: Final review gate after execution completes
- **featureforge:finishing-a-development-branch** - Complete development after all tasks
