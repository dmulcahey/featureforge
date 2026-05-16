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
  echo "featureforge: refusing non-featureforge public argv: $1" >&2
  return 2
}
```
## Installed Control Plane

Live workflow routing uses only `$_FEATUREFORGE_BIN`; never use `./bin/featureforge`, `target/debug/featureforge`, or `cargo run` for live routing. Query workflow/operator JSON with `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`.

If the installed runtime/root cannot be resolved, stop before making workflow mutations. Use only typed operator JSON route surfaces: execute `recommended_public_command_argv` when present; when `recommended_public_command_template` appears, treat `required_inputs` as validation metadata and materialize templates only by rerunning `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --input NAME=VALUE --json`. Detailed binding and route-specific stop rules live in `$_FEATUREFORGE_ROOT/references/operator-route-authority.md`. Treat display-only `recommended_command` as non-executable; if no typed executable surface exists, stop and report the route diagnostic.
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

Load the approved plan, query workflow/operator, and execute only the returned typed public argv/template route for each runtime-selected step. Stop and report the route diagnostic when no executable surface is present; let workflow/operator select execution topology, task-boundary closure, late-stage review/QA, and finish progression. Extended execution and review examples live in `$_FEATUREFORGE_ROOT/references/execution-review-qa-examples.md`.

Use this skill when the runtime-selected topology calls for a separate-session coordinator or conservative fallback. Prefer `featureforge:subagent-driven-development` when the runtime-selected topology keeps execution in one session and the worktree-first orchestration model is already in place.

## The Process

### Step 1: Implementation Preflight
1. Require the exact approved plan path as input. If you are not given one, stop and ask for it or route back to `featureforge:plan-eng-review`.
2. Read the plan file first.
3. Verify these exact header lines exist and are current: `**Workflow State:** Engineering Approved`, `**Source Spec:** <path>`, and `**Source Spec Revision:** <integer>`.
4. Read the source spec named in the plan and confirm it is still `CEO Approved`, and that the latest approved spec still matches that exact source-spec path and revision.
5. Stop immediately and redirect to `featureforge:plan-eng-review` if the plan is draft or malformed, or to `featureforge:writing-plans` if the source spec path or revision is stale.
6. Verify workspace readiness before starting: stop on protected/default branches without explicit in-place approval, detached HEAD, conflict/rebase/cherry-pick state, or dirty worktree unless the runtime-selected topology explicitly supports isolated worktree-backed execution.
7. Do not bulk-clean the workspace ad hoc. If the runtime-selected topology or protected-branch gate requires isolated execution, provision or route through a worktree-backed workspace before mutating repo state, and let the runtime-owned barrier flow reconcile reviewed work back onto the active branch and clean temporary worktrees at safe intervals.
8. The later repo-safety checks still govern any additional protected branches declared through repo or user instructions.
9. Run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` before starting execution.
10. If workflow/operator JSON does not report `phase` `executing`, stop normal execution and follow the Installed Control Plane section plus the canonical route reference for the current operator result. Treat `phase`, `phase_detail`, and `next_action` as diagnostic context.
11. If workflow/operator JSON confirms `phase` `executing`, review the plan critically for execution concerns and treat workflow/operator plus `plan execution status` as the live execution surface. Tracked checklist/evidence markdown is an optional materialized export; the event log remains authoritative for routing and gates.
12. Treat execution start as a hard gate: no code edits, no test edits, and no repo mutation are allowed after workflow/operator confirms the current execution preflight handoff and before the first `begin` for the active step. The first `begin` is the mandatory execution-tracking boundary; preflight acceptance alone is not permission to start implementation. If the workspace becomes dirty before the first `begin`, expect later execution-start checks to fail closed with reason codes such as `tracked_worktree_dirty` until reconciled or isolated. Retroactive execution tracking is recovery-only; use the canonical route reference for recovery binding details.

## Runtime-Owned Execution State

- calls `$_FEATUREFORGE_BIN workflow operator --plan ... --json` during preflight
- uses `status --plan ...` only for additional diagnostics when operator output alone is insufficient
- uses the workflow/operator execution-start handoff instead of separate compatibility/debug choreography before execution starts
- calls `begin` before starting work on a plan step
- calls `complete` after each completed step
- reports interruptions or blockers in the handoff/status surface instead of invoking command-shaped note or repair side channels
- The coordinator/runtime owns all public workflow/operator-guided execution-state mutations, including task start, task completion, reopening, transfer, task closure, repair, and late-stage advancement.
- Implementers/subagents may prepare candidate artifacts (for example task packets, coverage matrix context, and handoff drafts), but they must not directly mutate runtime state. Coordinators execute only workflow/operator-guided public argv and must not call unselected or low-level runtime mutation commands or edit runtime-owned records.
- On the first `begin` for a revision whose plan still says `**Execution Mode:** none`, initialize execution with `--execution-mode featureforge:executing-plans`
- The approved plan checklist is the human-visible execution progress projection. The event log remains authoritative for routing and gates; do not create or maintain a separate ad hoc task tracker outside those shared surfaces.
- Runtime read models are rendered under the state directory during normal execution. Repo-local projection files under `docs/featureforge/projections/` are optional human-readable exports; do not create or maintain a separate ad hoc task tracker outside workflow/operator and status.
- Use `$_FEATUREFORGE_BIN plan execution materialize-projections --plan <approved-plan-path>` for state-dir-only diagnostic projection refreshes. If the user explicitly needs repo-local human-readable projection exports, add `--repo-export --confirm-repo-export`; approved plan and evidence files are not modified, and materialization is never required for normal progress. Add `--scope execution|late-stage|all` only when a non-default export scope is needed.

## Runtime Strategy Checkpoints (Automatic, Runtime-Owned)

- Runtime strategy checkpoints are execution-owned state, not workflow-stage transitions. Keep public workflow phase in execution (`executing`) while strategy checkpoints change; remediation stays represented by checkpoint state and operator routing.
- The approved plan/spec scope is fixed during execution. Runtime strategy checkpoints may change topology, lane/worktree allocation, subagent assignment, and remediation order, but must not change approved scope, source plan revision, or required coverage.
- Required checkpoint kinds: `initial_dispatch` before repo-writing implementation starts; `cycle_break` when the same task hits three review-dispatch/reopen cycles in one run; and `review_remediation`: required after actionable independent-review findings and before remediation starts. Runtime records it automatically when reviewable runtime review state enters remediation and when remediation reopens execution work.
- Cycle-break trigger: cap remediation churn at 3 cycles per task. On the third cycle, transition to `cycle_break` strategy automatically (no human replanning loopback).
- Reviewers return summaries/results; the controller/runtime binds those results to authoritative state and runtime-owned review projections retain checkpoint fingerprints for traceability. Agents do not search for, repair, or depend on those projection files.
- Surface and respect runtime strategy status from `$_FEATUREFORGE_BIN plan execution status --plan ...`: `strategy_state`, `strategy_checkpoint_kind`, `last_strategy_checkpoint_fingerprint`, and `strategy_reset_required`.

## Execution-Phase Subagent Dispatch Policy

- Once execution is active for an approved plan (`execution_started` is `yes`), runtime-selected implementation and review subagent dispatch by workflow-owned execution skills is authorized and does not require per-dispatch user-consent prompts.
- Non-execution ad-hoc delegation still follows normal user-consent policy.

## Protected-Branch Repo-Write Gate

Before starting any plan step that mutates repo state, run the shared repo-safety preflight for that exact task slice:

```bash
$_FEATUREFORGE_BIN repo-safety check --intent write --stage featureforge:executing-plans --task-id <current-task-slice> --path <repo-relative-path> --write-target execution-task-slice
```

- Use one stable task id per repo-writing task slice and pass concrete repo-relative paths when known. If `allowed`, continue; if `blocked`, name the branch, the stage, and the blocking `failure_class`, then route to a feature branch / `featureforge:using-git-worktrees` or explicit user approval for this exact task slice.
- If the user approves protected-branch writes, approve the full task-slice scope with `$_FEATUREFORGE_BIN repo-safety approve --stage featureforge:executing-plans --task-id <current-task-slice> --reason "<explicit user approval>" --path <repo-relative-path> --write-target execution-task-slice [--write-target git-commit] [--write-target git-merge] [--write-target git-push]`, then re-check. Before follow-on `git commit`, `git merge`, or `git push`, re-run the gate with the same task id, paths, and approved write-target set; if scope changes, approve and re-check the new full scope.
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

2. treat it as the exact task contract for that execution segment. Coordinator logistics may clarify branch/cwd/base commit, but must not reinterpret approved requirements: the packet's `Goal`, `Context`, indexed `CONSTRAINT_N` obligations, indexed `DONE_WHEN_N` obligations, covered requirements, and file scope are authoritative; `CONSTRAINT_N` obligations must be checked by task reviewers; objectively reviewable `Done when` obligations remain mandatory even when verified by diff inspection or targeted evidence rather than one command; Separate-session handoffs must paste the generated task packet verbatim; if packet content conflicts with `$_FEATUREFORGE_ROOT/review/plan-task-contract.md`, stop and route back to plan review.
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
   - after the dedicated-independent review result is in hand, query `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready --json`, then follow the Installed Control Plane section and canonical route reference for the returned route; verification results are `close-current-task` inputs, not a reason to pass that flag
   - if workflow/operator still reports a review-dispatch or recording route after an external review result is ready, keep rerouting through workflow/operator and the canonical route reference; do not expand the normal path into low-level dispatch-lineage management
   - no exceptions: only after close-current-task succeeds may Task `N+1` begin
8. If the packet is malformed, stale, or still leaves ambiguity unresolved, stop and route back to review instead of guessing.
9. Call `complete` as soon as a step is truly satisfied so the authoritative event log records the completed step. Do not manually flip tracked plan checkboxes during normal execution.

### Reviewed-Closure Route Authority

Reviewed-closure and late-stage handoffs are operator-owned. Keep workflow/operator JSON as the route authority; use `$_FEATUREFORGE_ROOT/references/operator-route-authority.md` for route-specific binding detail and `$_FEATUREFORGE_ROOT/docs/featureforge/reference/2026-04-01-review-state-reference.md` only for the review-state mental model.

Use `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready --json` only after an external task-review or final-review result is in hand; do not use that hint for release-readiness, document-release, or QA routing.

Do not duplicate route tables, infer low-level dispatch-lineage commands, or manually edit runtime-owned records, derived markdown projections, or `**Execution Note:**` lines to recover routing state. Follow operator JSON and the canonical route reference.

### Step 3: Resolve Late-Stage Routes

After all tasks complete and verified:
- Query `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`.
- Apply the shared route law (Installed Control Plane section plus canonical route reference) for the returned route instead of running a remembered late-stage sequence.
- Use route-specific late-stage skills only when workflow/operator selects that lane; companion references provide examples and binding detail, not route-selection authority.
- When the selected route requires terminal code review, announce and use `featureforge:requesting-code-review`, then resolve any Critical or Important findings before proceeding.

### Step 4: Finish Via Operator Route

After any selected final review route is resolved:
- Rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`.
- Apply the same shared route law to the returned operator result.
- Invoke `featureforge:finishing-a-development-branch` only when workflow/operator selects branch completion.
- If workflow/operator selects release readiness, final review, QA, repair, or another task-boundary route first, resolve that selected route before branch completion.

- **featureforge:writing-plans** - Creates the plan this skill executes
- **featureforge:plan-eng-review** - Provides the approved plan and the execution preflight handoff
- **featureforge:requesting-code-review** - Route-selected final review gate after execution completes
- **featureforge:finishing-a-development-branch** - Route-selected branch completion after workflow/operator reports ready
