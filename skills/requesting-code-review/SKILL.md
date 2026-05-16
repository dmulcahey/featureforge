---
name: requesting-code-review
description: Use after implementation work or an intentional review checkpoint, and before merging, to verify the work meets requirements
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

# Requesting Code Review

Dispatch the `code-reviewer` sub-agent or custom agent with precisely crafted context for evaluation - never your session's history. This keeps the reviewer focused on the work product and preserves your own context for continued work.

In Codex, FeatureForge installs the `code-reviewer` custom agent alongside the shared skills checkout. In GitHub Copilot local installs, FeatureForge installs the same reviewer through the platform's custom-agent path.

**Core principle:** Review at the right checkpoints, then fail closed on the final whole-diff gate.

Invocation identity: `featureforge:requesting-code-review`. Valid modes: terminal whole-diff review when workflow/operator selects the final-review lane, or non-terminal checkpoint/task-boundary review before execution continues. For late-stage phase/action vocabulary, reference `$_FEATUREFORGE_ROOT/review/late-stage-precedence-reference.md`; do not use that reference to infer the next lane.

## When to Request Review
**Mandatory:** For the final cross-task review gate in workflow-routed work when workflow/operator selects terminal final review for the current `HEAD`, after completing major feature work, and before merge to the target base branch.

## How to Request

**1. If this review is for plan-routed work, capture execution state first:**

- For plan-routed final review, require the exact approved plan path and exact approved spec path from the current execution preflight handoff or session context.
- Run `$_FEATUREFORGE_BIN plan contract analyze-plan --spec <approved-spec-path> --plan <approved-plan-path> --format json` before dispatching the reviewer.
- If `contract_state != valid` or `packet_buildable_tasks != task_count`, stop and return to the current execution flow; do not review stale or malformed approved artifacts.
- Run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` before dispatching the reviewer.
- If workflow/operator fails, stop and return to the current execution flow; do not guess the public late-stage route from raw execution state.
- Run `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` only when you need extra execution-dirty or strategy-checkpoint diagnostics from the current workflow context.
- If diagnostic status fails when those fields are required, stop and return to the current execution flow; do not dispatch review against guessed plan state.
- When diagnostic status is required, parse `active_task`, `blocking_task`, and `resume_task` from that status JSON.
- When diagnostic status is required, if any of `active_task`, `blocking_task`, or `resume_task` is non-null, stop and return to the current execution flow; final review is only valid when all three are `null`.
- For terminal whole-diff review, keep `workflow operator` as the normal route authority; use `plan execution status` only when you need extra diagnostic fields for review context.
- For non-terminal checkpoint or task-boundary review, do not force terminal-clean execution state; follow the runtime-reported blocking reason and review scope for the current task boundary.
- For terminal whole-diff review, treat workflow/operator JSON as authoritative for the public late-stage route; status is diagnostic only.
- For terminal whole-diff review, only request a fresh external final review when workflow/operator JSON reports `phase=final_review_pending` with `phase_detail=final_review_dispatch_required`.
- For terminal whole-diff review, if workflow/operator already reports `phase_detail=final_review_outcome_pending`, do not dispatch a second reviewer; wait for the current final-review result or return to the current execution flow.
- For terminal whole-diff review, when workflow/operator JSON reports `final_review_dispatch_required`, keep the normal path operator-led and treat low-level dispatch commands as compatibility/debug-only.
- For terminal whole-diff review, do not route the normal path through low-level compatibility/debug dispatch primitives; stay on workflow/operator plus the intent-level commands.
- For terminal whole-diff review, rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready --json` after the external review result is ready.
- After the independent reviewer returns a final-review result, rerun workflow/operator with `--external-review-result-ready --json`, then follow the Installed Control Plane section and canonical route reference.
- For non-terminal checkpoint/task-boundary review, keep command-boundary semantics explicit: low-level compatibility/debug dispatch commands are not normal intent-level progression.
- If operator/status diagnostics surface warning-only compatibility codes such as `legacy_evidence_format`, keep them in review context but do not treat them as blockers while authoritative runtime/operator gate outputs remain non-blocking.
- Pass the exact approved plan path into the reviewer context. When runtime-owned execution evidence or task-packet context is already available from the current workflow handoff, pass it through as supplemental context; do not make the public flow harvest it manually.
- If the current review is not governed by an approved FeatureForge plan, skip this execution-state gate and continue with the normal diff review.

**2. Use the provided base branch context and derive the review range:**
Keep base-branch selection runtime-aligned and stable for this review. For plan-routed final review, `BASE_BRANCH` must come from `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` (`base_branch`) and stay aligned with the runtime-owned release lineage/document-release context. For non-plan-routed review, provide `BASE_BRANCH` explicitly before running this step. Do not redetect it here.

```bash
if [ -z "$BASE_BRANCH" ]; then
  echo "Missing BASE_BRANCH. Set it from workflow/operator base_branch and runtime-owned release lineage (plan-routed) or provide it explicitly (non-plan-routed) before continuing."
  exit 1
fi
git fetch origin "$BASE_BRANCH" --quiet 2>/dev/null || true
BASE_SHA=$(git merge-base HEAD "origin/$BASE_BRANCH" 2>/dev/null || git merge-base HEAD "$BASE_BRANCH" 2>/dev/null)
if [ -z "$BASE_SHA" ]; then
  echo "Could not derive merge-base for BASE_BRANCH=$BASE_BRANCH. Stop and provide a valid base branch."
  exit 1
fi
HEAD_SHA=$(git rev-parse HEAD)
```

Do not use PR metadata or repo default-branch APIs as a fallback. For workflow-routed review, require `BASE_BRANCH` from `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` (`base_branch`). For non-plan-routed review, require an explicitly provided `BASE_BRANCH`.

The reviewer MUST use `$_REPO_ROOT/review/checklist.md` when available, otherwise `$_FEATUREFORGE_ROOT/review/checklist.md`.

**3. Dispatch the code-reviewer agent:**

Use the `code-reviewer` agent and fill the template at skill-local `code-reviewer.md`.

For workflow-routed final review, dispatch a dedicated fresh-context reviewer independent of the implementation context. Do not reuse the implementation agent or its session for the terminal whole-diff review gate.

The controller owns any FeatureForge runtime queries before dispatch. Fill the skill-local `code-reviewer.md` template with all context the reviewer must consider, including unfamiliar platform patterns or known footguns when relevant; if required runtime context is absent, return to the current execution flow instead of dispatching an under-contextualized review. The reviewer prompt owns the reviewer-only recursion contract.

Use skill-local `code-reviewer.md` as the placeholder source of truth. Fill all plan, diff, runtime-provenance, skill-root, live-state, and description fields before dispatch.

**3.5. Required review-dispatch provenance for FeatureForge-on-FeatureForge work:**
- Before dispatch, include a provenance section in the review handoff with: base branch, base SHA, head SHA, working-tree diff hash, installed runtime path/hash used for live routing, workspace runtime hash used for tests (if any), live state dir, active FeatureForge skill source/roots, installed skill root, and workspace skill root.
- If any required provenance field is missing, stop and treat review dispatch as blocked until the missing provenance is provided.
- Reviewers must fail review when live workflow mutation used workspace runtime without an explicit approved override record.
- Reviewers must fail review when active FeatureForge skills resolve from the workspace skill root instead of the installed skill root without an explicit approved self-hosting exception.
- Reviewers must fail review when FeatureForge-on-FeatureForge provenance is missing or incomplete.

**4. Act on feedback:**
Fix Critical issues immediately and Important issues before proceeding. Note Minor issues for later, push back on incorrect findings with code/test evidence, and require deterministic repair-packet fields: `Finding ID`, `Severity`, `Task`, `Violated Field or Obligation`, `Evidence`, `Required Fix`, and `Hard Fail`.

**4.25. Enforce runtime-owned remediation checkpoints before fixes:**

- Do not jump directly into patching after actionable findings. In plan-routed execution, route through runtime-owned public reopen/remediation commands so runtime can record strategy checkpoints first.
- Runtime strategy checkpoints are execution-owned state, not planning-stage transitions. Do not route back to `writing-plans` or `plan-eng-review` just because remediation is needed.
- Required checkpoint behavior:
  - `review_remediation`: runtime records this automatically when reviewable execution work enters remediation after non-pass findings.
  - `cycle_break`: runtime records this automatically when the same task hits three review-dispatch/reopen cycles in one run.
- Cycle-break trigger: cap review churn at 3 cycles per task. On the third cycle, runtime enters `cycle_break` strategy automatically (no human replanning loopback required).
- Keep plan/scope fixed during remediation. Runtime strategy may change topology, lane ownership, worktree allocation, subagent assignment, and remediation order, but must not change approved scope or source plan revision.
- Carry the active runtime checkpoint fingerprint into review-result metadata so remediation and final review can be tied to the exact runtime strategy state.
- Check and surface runtime strategy status through `$_FEATUREFORGE_BIN plan execution status --plan ...`:
  - `strategy_state`
  - `strategy_checkpoint_kind`
  - `last_strategy_checkpoint_fingerprint`
  - `strategy_reset_required`

**4.5. Keep review artifacts runtime-owned:**

- Do not add manual project-scoped markdown review artifacts as part of the normal public flow.
- If runtime emits derived reviewer projection metadata or provenance artifacts, treat them as output only; reviewed-closure and milestone records remain authoritative for routing and finish gates.

## Terminal Final-Review Route

Use `ANALYZE_JSON=$("$_FEATUREFORGE_BIN" plan contract analyze-plan --spec "$SOURCE_SPEC_PATH" --plan "$APPROVED_PLAN_PATH" --format json)` and stop unless `contract_state=valid` and `packet_buildable_tasks=task_count`.

Use `OPERATOR_JSON=$("$_FEATUREFORGE_BIN" workflow operator --plan "$APPROVED_PLAN_PATH" --json)` and request final review only for `phase=final_review_pending` plus `phase_detail=final_review_dispatch_required`.

Set `BASE_BRANCH` from workflow/operator for plan-routed review and derive `BASE_SHA`/`HEAD_SHA`. Stop here: dispatch the dedicated fresh-context reviewer, wait for its result, then set REVIEWER_SOURCE=<actual-source>, REVIEWER_ID=<actual-reviewer-id>, REVIEW_RESULT=pass|fail, and SUMMARY_FILE=<actual-final-review-summary>.

After the reviewer returns, obtain `RECORDING_READY_JSON=$("$_FEATUREFORGE_BIN" workflow operator --plan "$APPROVED_PLAN_PATH" --external-review-result-ready --json)`, then follow the Installed Control Plane section and canonical route reference. The final-review materializer lives in the `Final-Review Recording Route Materializer` section of `$_FEATUREFORGE_ROOT/references/operator-route-authority.md`; use that materializer's `--input NAME=VALUE` operator query for any template inputs and execute only the returned `recommended_public_command_argv` through `_featureforge_exec_public_argv`.

Do not hand-write `advance-late-stage` for final review recording. If workflow/operator does not return an executable final-review recording route, stop and report `RECORDING_READY_JSON`.

See `$_FEATUREFORGE_ROOT/references/execution-review-qa-examples.md` for example findings and dispatch logistics.

## Execution-State Gate

- rejects final review if the plan has invalid execution state or required unfinished work not truthfully represented
- when diagnostic status is required, treats non-null `active_task`, `blocking_task`, or `resume_task` as execution-dirty and rejects final review until execution returns to a clean state
- uses `$_FEATUREFORGE_BIN workflow operator --plan ... --json` as the authoritative late-stage route, requests external final review when routed, and records final-review outcome only through the returned public route after `--external-review-result-ready --json`
- accepts runtime-provided supplemental review context when the current workflow handoff already includes it, but does not require manual status/evidence/task-packet harvesting in the normal path
- treats any derived reviewer projection metadata or provenance artifacts as runtime-owned output, not routing authority
- must fail closed when it detects a missed reopen or stale evidence, but must not call `reopen` itself

## Red Flags

Never skip review because "it's simple", ignore Critical issues, proceed with unfixed Important issues, or argue with valid technical feedback.

If reviewer feedback is wrong, push back with technical reasoning, code/tests, or a clarification request.

See template at skill-local `code-reviewer.md`.
