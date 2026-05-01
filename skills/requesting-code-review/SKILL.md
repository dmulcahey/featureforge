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
```
## Search Before Building

Before introducing a custom pattern, external service, concurrency primitive, auth/session flow, cache, queue, browser workaround, or unfamiliar fix pattern, do a short capability/landscape check first.

Use three lenses:
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

Dispatch the `code-reviewer` sub-agent or custom agent to catch issues before they cascade. The reviewer gets precisely crafted context for evaluation — never your session's history. This keeps the reviewer focused on the work product, not your thought process, and preserves your own context for continued work.

In Codex, FeatureForge installs the `code-reviewer` custom agent alongside the shared skills checkout. In GitHub Copilot local installs, FeatureForge installs the same reviewer through the platform's custom-agent path.

**Core principle:** Review at the right checkpoints, then fail closed on the final whole-diff gate.

This skill has two valid modes:
- terminal whole-diff review (workflow-routed final gate after `featureforge:document-release`)
- non-terminal checkpoint/task-boundary review (targeted dedicated-independent validation before execution continues)

For late-stage phase/action/skill grounding, reference `review/late-stage-precedence-reference.md`.

## When to Request Review

**Mandatory:**
- For the final cross-task review gate in workflow-routed work, after `featureforge:document-release` is current for the same `HEAD`
- After completing major feature
- Before merge to the target base branch

**Optional but valuable:**
- When stuck (fresh perspective)
- Before refactoring (baseline check)
- After fixing complex bug

## How to Request

**1. If this review is for plan-routed work, capture execution state first:**

- For plan-routed final review, require the exact approved plan path and exact approved spec path from the current execution preflight handoff or session context.
- Run `featureforge plan contract analyze-plan --spec <approved-spec-path> --plan <approved-plan-path> --format json` before dispatching the reviewer.
- If `contract_state != valid` or `packet_buildable_tasks != task_count`, stop and return to the current execution flow; do not review stale or malformed approved artifacts.
- Run `featureforge workflow operator --plan <approved-plan-path>` before dispatching the reviewer.
- If workflow/operator fails, stop and return to the current execution flow; do not guess the public late-stage route from raw execution state.
- Run `featureforge plan execution status --plan <approved-plan-path>` only when you need extra execution-dirty or strategy-checkpoint diagnostics from the current workflow context.
- If diagnostic status fails when those fields are required, stop and return to the current execution flow; do not dispatch review against guessed plan state.
- When diagnostic status is required, parse `active_task`, `blocking_task`, and `resume_task` from that status JSON.
- When diagnostic status is required, if any of `active_task`, `blocking_task`, or `resume_task` is non-null, stop and return to the current execution flow; final review is only valid when all three are `null`.
- For terminal whole-diff review, keep `workflow operator` as the normal route authority; use `plan execution status` only when you need extra diagnostic fields for review context.
- For non-terminal checkpoint or task-boundary review, do not force terminal-clean execution state; follow the helper-reported blocking reason and review scope for the current task boundary.
- For terminal whole-diff review, treat workflow/operator as authoritative for the public late-stage route; status is diagnostic only.
- For terminal whole-diff review, only request a fresh external final review when workflow/operator reports `phase=final_review_pending` with `phase_detail=final_review_dispatch_required`.
- For terminal whole-diff review, if workflow/operator already reports `phase_detail=final_review_outcome_pending`, do not dispatch a second reviewer; wait for the current final-review result or return to the current execution flow.
- For terminal whole-diff review, when workflow/operator reports `final_review_dispatch_required`, keep the normal path operator-led and treat low-level dispatch commands as compatibility/debug-only.
- For terminal whole-diff review, do not route the normal path through `record-review-dispatch`; stay on workflow/operator plus the intent-level commands.
- For terminal whole-diff review, rerun `featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready` after the external review result is ready and require `phase_detail=final_review_recording_ready` before recording final-review outcome.
- After the independent reviewer returns a final-review result, rerun `featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready` and require `phase_detail=final_review_recording_ready` before recording the result with `featureforge plan execution advance-late-stage --plan <approved-plan-path> --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <final-review-summary>`.
- For non-terminal checkpoint/task-boundary review, keep command-boundary semantics explicit: low-level compatibility/debug dispatch commands are not normal intent-level progression.
- If operator/status diagnostics surface warning-only compatibility codes such as `legacy_evidence_format`, keep them in review context but do not treat them as blockers while authoritative runtime/operator gate outputs remain non-blocking.
- Pass the exact approved plan path into the reviewer context. When runtime-owned execution evidence or task-packet context is already available from the current workflow handoff, pass it through as supplemental context; do not make the public flow harvest it manually.
- If the current review is not governed by an approved FeatureForge plan, skip this execution-state gate and continue with the normal diff review.

**2. Use the provided base branch context and derive the review range:**
Keep base-branch selection runtime-aligned and stable for this review. For plan-routed final review, `BASE_BRANCH` must come from `featureforge workflow operator --plan <approved-plan-path> --json` (`base_branch`) and stay aligned with the runtime-owned release lineage/document-release context. For non-plan-routed review, provide `BASE_BRANCH` explicitly before running this step. Do not redetect it here.

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

Do not use PR metadata or repo default-branch APIs as a fallback. For workflow-routed review, require `BASE_BRANCH` from `featureforge workflow operator --plan <approved-plan-path> --json` (`base_branch`). For non-plan-routed review, require an explicitly provided `BASE_BRANCH`.

The reviewer MUST use the shared review checklist from `review/checklist.md` in the repo when available, otherwise fall back to the installed FeatureForge copy.

**3. Dispatch the code-reviewer agent:**

Use the `code-reviewer` agent and fill the template at `code-reviewer.md`

For workflow-routed final review, dispatch a dedicated fresh-context reviewer independent of the implementation context. Do not reuse the implementation agent or its session for the terminal whole-diff review gate.

The controller owns any FeatureForge runtime queries before dispatch. Fill the `code-reviewer.md` template with all context the reviewer must consider; if required runtime context is absent, return to the current execution flow instead of dispatching an under-contextualized review. The reviewer prompt owns the reviewer-only recursion contract.

When the implementation introduces unfamiliar patterns, framework APIs, dependencies, or bespoke wrappers around platform behavior, make sure the review considers built-in-before-bespoke and known ecosystem footguns.

If the approved plan already called out a likely external-pattern target, you may pass that context into the reviewer briefing, but this is optional in v1.

**Placeholders:**
- `{WHAT_WAS_IMPLEMENTED}` - What you just built
- `{PLAN_OR_REQUIREMENTS}` - What it should do, including completed task-packet context and coverage matrix details for plan-routed review
- `{APPROVED_PLAN_PATH}` - Exact approved plan path for plan-routed review, otherwise leave blank
- `{EXECUTION_EVIDENCE_PATH}` - Optional runtime-provided evidence artifact path for plan-routed review, otherwise leave blank
- `{BASE_BRANCH}` - The runtime-provided or explicitly supplied base branch name
- `{BASE_SHA}` - Starting commit
- `{HEAD_SHA}` - Ending commit
- `{DESCRIPTION}` - Brief summary

**4. Act on feedback:**
- Fix Critical issues immediately
- Fix Important issues before proceeding
- Note Minor issues for later
- Capture documentation or TODO follow-ups instead of silently dropping them
- Push back if reviewer is wrong (with reasoning)

**4.25. Enforce runtime-owned remediation checkpoints before fixes:**

- Do not jump directly into patching after actionable findings. In plan-routed execution, route through helper-owned reopen/remediation commands so runtime can record strategy checkpoints first.
- Runtime strategy checkpoints are execution-owned state, not planning-stage transitions. Do not route back to `writing-plans` or `plan-eng-review` just because remediation is needed.
- Required checkpoint behavior:
  - `review_remediation`: runtime records this automatically when reviewable execution work enters remediation after non-pass findings.
  - `cycle_break`: runtime records this automatically when the same task hits three review-dispatch/reopen cycles in one run.
- Cycle-break trigger: cap review churn at 3 cycles per task. On the third cycle, runtime enters `cycle_break` strategy automatically (no human replanning loopback required).
- Keep plan/scope fixed during remediation. Runtime strategy may change topology, lane ownership, worktree allocation, subagent assignment, and remediation order, but must not change approved scope or source plan revision.
- Carry the active runtime checkpoint fingerprint into review-result metadata so remediation and final review can be tied to the exact runtime strategy state.
- Check and surface runtime strategy status through `featureforge plan execution status --plan ...`:
  - `strategy_state`
  - `strategy_checkpoint_kind`
  - `last_strategy_checkpoint_fingerprint`
  - `strategy_reset_required`

**4.5. Keep review artifacts runtime-owned:**

- Do not add manual project-scoped markdown review artifacts as part of the normal public flow.
- If runtime emits derived reviewer projection metadata or provenance artifacts, treat them as output only; reviewed-closure and milestone records remain authoritative for routing and finish gates.

## Example

```
[Implementation is complete for the current branch and I want the final whole-diff review gate]

You: Let me request the final code review gate before branch completion.

APPROVED_PLAN_PATH=docs/featureforge/plans/deployment-plan.md
SOURCE_SPEC_PATH=docs/featureforge/specs/deployment-plan-design.md
ANALYZE_JSON=$("$_FEATUREFORGE_BIN" plan contract analyze-plan --spec "$SOURCE_SPEC_PATH" --plan "$APPROVED_PLAN_PATH" --format json)
CONTRACT_STATE=$(printf '%s\n' "$ANALYZE_JSON" | node -e 'const fs = require("fs"); const parsed = JSON.parse(fs.readFileSync(0, "utf8")); process.stdout.write(parsed.contract_state || "")')
PACKET_BUILDABLE_TASKS=$(printf '%s\n' "$ANALYZE_JSON" | node -e 'const fs = require("fs"); const parsed = JSON.parse(fs.readFileSync(0, "utf8")); process.stdout.write(String(parsed.packet_buildable_tasks ?? ""))')
TASK_COUNT=$(printf '%s\n' "$ANALYZE_JSON" | node -e 'const fs = require("fs"); const parsed = JSON.parse(fs.readFileSync(0, "utf8")); process.stdout.write(String(parsed.task_count ?? ""))')
if [ "$CONTRACT_STATE" != "valid" ] || [ "$PACKET_BUILDABLE_TASKS" != "$TASK_COUNT" ]; then
  echo "Stop and return to execution: approved artifacts are stale or malformed."
  exit 1
fi
OPERATOR_JSON=$("$_FEATUREFORGE_BIN" workflow operator --plan "$APPROVED_PLAN_PATH" --json)
PHASE=$(printf '%s\n' "$OPERATOR_JSON" | node -e 'const fs = require("fs"); const parsed = JSON.parse(fs.readFileSync(0, "utf8")); process.stdout.write(parsed.phase || "")')
PHASE_DETAIL=$(printf '%s\n' "$OPERATOR_JSON" | node -e 'const fs = require("fs"); const parsed = JSON.parse(fs.readFileSync(0, "utf8")); process.stdout.write(parsed.phase_detail || "")')
if [ "$PHASE" != "final_review_pending" ] || [ "$PHASE_DETAIL" != "final_review_dispatch_required" ]; then
  echo "Stop and return to execution: workflow/operator did not expose final-review dispatch as the current route."
  exit 1
fi
BASE_BRANCH=<same runtime-owned base branch from document-release when plan-routed, otherwise the explicit BASE_BRANCH provided in Step 2>
BASE_SHA=$(git merge-base HEAD "origin/$BASE_BRANCH" 2>/dev/null || git merge-base HEAD "$BASE_BRANCH" 2>/dev/null)
if [ -z "$BASE_SHA" ]; then
  echo "Could not derive merge-base for BASE_BRANCH=$BASE_BRANCH. Stop and provide a valid base branch."
  exit 1
fi
HEAD_SHA=$(git rev-parse HEAD)

[Dispatch code-reviewer agent]
  WHAT_WAS_IMPLEMENTED: Final branch diff for the deployment plan
  PLAN_OR_REQUIREMENTS: Approved plan plus any runtime-provided supplemental context for the current review handoff
  APPROVED_PLAN_PATH: docs/featureforge/plans/deployment-plan.md
  EXECUTION_EVIDENCE_PATH:
  BASE_BRANCH: main
  BASE_SHA: a7981ec
  HEAD_SHA: 3df7661
  DESCRIPTION: Final whole-diff review gate before branch completion

[Subagent returns]:
  Strengths: Clean architecture, real tests, checklist pass covered enum consumers
  Issues:
    ### Finding FINAL_REVIEW_PROGRESS_INDICATORS
    **Finding ID:** FINAL_REVIEW_PROGRESS_INDICATORS
    **Severity:** important
    **Task:** Task 2
    **Violated Field or Obligation:** DONE_WHEN_1
    **Evidence:** The final diff lacks progress reporting for the packet-required long-running deployment step.
    **Required Fix:** Add progress indicators to the long-running deployment path and include evidence in the final-review response.
    **Hard Fail:** no
  Assessment: Ready to proceed

You: [Fix progress indicators]
RECORDING_READY_JSON=$("$_FEATUREFORGE_BIN" workflow operator --plan "$APPROVED_PLAN_PATH" --external-review-result-ready --json)
RECORDING_PHASE_DETAIL=$(printf '%s\n' "$RECORDING_READY_JSON" | node -e 'const fs = require("fs"); const parsed = JSON.parse(fs.readFileSync(0, "utf8")); process.stdout.write(parsed.phase_detail || "")')
if [ "$RECORDING_PHASE_DETAIL" != "final_review_recording_ready" ]; then
  echo "Stop and return to execution: workflow/operator did not expose final-review recording readiness."
  exit 1
fi
"$_FEATUREFORGE_BIN" plan execution advance-late-stage --plan "$APPROVED_PLAN_PATH" --reviewer-source fresh-context-subagent --reviewer-id 019d3550-c932-7bb2-9903-33f68d7c30ca --result pass --summary-file review-summary.md
[Continue to QA or branch completion through workflow/operator]
```

## Integration with Workflows

**Subagent-Driven Development:**
- Per-task spec-compliance and code-quality reviews happen inside `subagent-driven-development`
- Use `featureforge:requesting-code-review` as the final whole-diff gate only after `featureforge:document-release` is current for the same `HEAD`, or earlier when you intentionally want an extra checkpoint
- Resolve Critical and Important findings before handing off to branch completion

**Executing Plans:**
- Use `featureforge:requesting-code-review` for both terminal whole-diff review and runtime-requested task-boundary dedicated-independent review (`prior_task_review_*` reason families)
- For terminal whole-diff review, run it after `featureforge:document-release`
- After the reviewer returns for terminal whole-diff review, rerun `featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready` and record the final-review result through `featureforge plan execution advance-late-stage --plan <approved-plan-path> --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <final-review-summary>` before QA or branch completion
- Use an earlier review only when intentionally checkpointing or when runtime boundary reasons explicitly require it

**Ad-Hoc Development:**
- Review before merge
- Review when stuck

## Execution-State Gate

- rejects final review if the plan has invalid execution state or required unfinished work not truthfully represented
- when diagnostic status is required, treats non-null `active_task`, `blocking_task`, or `resume_task` as execution-dirty and rejects final review until execution returns to a clean state
- uses `workflow operator --plan ...` as the authoritative late-stage route, requests external final review when operator reports `final_review_dispatch_required`, and records final-review outcome through `advance-late-stage` only after `--external-review-result-ready` exposes `final_review_recording_ready`
- accepts runtime-provided supplemental review context when the current workflow handoff already includes it, but does not require manual status/evidence/task-packet harvesting in the normal path
- treats any derived reviewer projection metadata or provenance artifacts as runtime-owned output, not routing authority
- must fail closed when it detects a missed reopen or stale evidence, but must not call `reopen` itself

## Red Flags

**Never:**
- Skip review because "it's simple"
- Ignore Critical issues
- Proceed with unfixed Important issues
- Argue with valid technical feedback

**If reviewer wrong:**
- Push back with technical reasoning
- Show code/tests that prove it works
- Request clarification

See template at: code-reviewer.md
