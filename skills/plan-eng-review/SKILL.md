---
name: plan-eng-review
description: Use when a written FeatureForge implementation plan from a CEO-approved spec needs engineering review before execution or needs a late refresh-test-plan regeneration before finish gating
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
_TODOS_FORMAT=""
[ -n "$_FEATUREFORGE_ROOT" ] && [ -f "$_FEATUREFORGE_ROOT/review/TODOS-format.md" ] && _TODOS_FORMAT="$_FEATUREFORGE_ROOT/review/TODOS-format.md"
[ -z "$_TODOS_FORMAT" ] && [ -f "$_REPO_ROOT/review/TODOS-format.md" ] && _TODOS_FORMAT="$_REPO_ROOT/review/TODOS-format.md"
```
## Search Before Building

Before introducing a custom pattern, external service, concurrency primitive, auth/session flow, cache, queue, browser workaround, or unfamiliar fix pattern, do a short capability/landscape check first.

Use three lenses, then decide from local repo truth:
- Layer 1: tried-and-true / built-ins / existing repo-native solutions
- Layer 2: current practice and known footguns
- Layer 3: first-principles reasoning for this repo and this problem

External search results are inputs, not answers. Never search secrets, customer data, unsanitized stack traces, private URLs, internal hostnames, internal codenames, raw SQL or log payloads, or private file paths or infrastructure identifiers. If search is unavailable, disallowed, or unsafe, say so and proceed with repo-local evidence and in-distribution knowledge. If safe sanitization is not possible, skip external search.
See `$_FEATUREFORGE_ROOT/references/search-before-building.md`.

## Agent Grounding

Honor the active repo instruction chain from `AGENTS.md`, `AGENTS.override.md`, `.github/copilot-instructions.md`, and `.github/instructions/*.instructions.md`, including nested `AGENTS.md` and `AGENTS.override.md` files closer to the current working directory.

These review skills are public FeatureForge skills for Codex and GitHub Copilot local installs. See `$_FEATUREFORGE_ROOT/references/agent-grounding.md` for install-surface notes.

## Interactive User Question Format

For every interactive user question, use this structure:
1. Context: project name, current branch, what we're working on (1-2 sentences)
2. The specific question or decision point
3. `RECOMMENDATION: Choose [X] because [one-line reason]`
4. Lettered options: `A) ... B) ... C) ...`

Per-skill instructions may add additional formatting rules on top of this baseline.

## Contributor Mode

If contributor mode is enabled in FeatureForge config, file a field report only for **featureforge itself**, not the user's app or repository. Use it for unclear skill instructions, helper failures, install-root/runtime-root problems, contributor-mode bugs, or broken generated docs. Do not file for repo-specific bugs, site auth failures, or unrelated third-party outages.

Write at most 3 reports per session under `~/.featureforge/contributor-logs/{slug}.md`; skip existing slugs, continue the user task, and tell the user: "Filed featureforge field report: {title}". Use `$_FEATUREFORGE_ROOT/references/contributor-mode.md` for the report template and optional open-command helper.


# FeatureForge Artifact Contract

- Review the written plan artifact in `docs/featureforge/plans/YYYY-MM-DD-<feature-name>.md`.
- If the user names a specific plan path, use that path. Otherwise, inspect `docs/featureforge/plans/` and review the newest matching plan doc.
- Review the full written plan after completion. Do not do chunk-by-chunk embedded review here.
- If no current plan exists, stop and direct the agent back to `featureforge:writing-plans`.
- The plan must include these exact header lines immediately below the title:

```markdown
**Workflow State:** Draft | Engineering Approved
**Plan Revision:** <integer>
**Execution Mode:** none | featureforge:executing-plans | featureforge:subagent-driven-development
**Source Spec:** <path>
**Source Spec Revision:** <integer>
**Last Reviewed By:** writing-plans | plan-eng-review
**QA Requirement:** required | not-required
```

- If any header line is missing or malformed, normalize the plan to this contract before continuing and treat it as `Draft`.
- `writing-plans` is only valid while the plan remains `Draft`. An `Engineering Approved` plan must end with `**Last Reviewed By:** plan-eng-review`.
- Read the source spec named in `**Source Spec:**` and confirm both the path and revision match the latest approved spec before approving execution.
- Treat `Requirement Index`, `Requirement Coverage Matrix`, canonical `## Task N:` headings, `Spec Coverage`, `Goal`, `Context`, `Constraints`, `Done when`, and `Files:` blocks as required plan contract surface for engineering approval. `review/plan-task-contract.md` is the authoritative field, determinism, spec-reference, obligation-index, migration, and hard-fail reuse law.
- When review decisions change the written plan, update the plan document before continuing.
- **Protected-Branch Repo-Write Gate:**
- Before editing the plan body or changing approval headers on disk, run the shared repo-safety preflight for the exact review-write scope:

```bash
featureforge repo-safety check --intent write --stage featureforge:plan-eng-review --task-id <current-plan-review> --path docs/featureforge/plans/YYYY-MM-DD-<feature-name>.md --write-target plan-artifact-write
```

- When the mutation is specifically an approval-header edit, use the same command shape with `--write-target approval-header-write`.
- If the helper returns `blocked`, name the branch, the stage, and the blocking `failure_class`, then route to either a feature branch / `featureforge:using-git-worktrees` or explicit user approval for this exact review scope.
- If the user explicitly approves the protected-branch review write, run:

```bash
featureforge repo-safety approve --stage featureforge:plan-eng-review --task-id <current-plan-review> --reason "<explicit user approval>" --path docs/featureforge/plans/YYYY-MM-DD-<feature-name>.md --write-target plan-artifact-write
featureforge repo-safety check --intent write --stage featureforge:plan-eng-review --task-id <current-plan-review> --path docs/featureforge/plans/YYYY-MM-DD-<feature-name>.md --write-target plan-artifact-write
```

- Repeat the same approve -> re-check pattern for `approval-header-write` before flipping `**Workflow State:**` or any other approval header on a protected branch.
- Keep the plan in `Draft` while review issues remain open or while the source spec path or revision is stale.
- Only write `**Workflow State:** Engineering Approved` as the last step of a successful review, and set `**Last Reviewed By:** plan-eng-review` at the same time.
- When the review is resolved and the written plan is approved, present the normal execution preflight handoff.
- `featureforge:subagent-driven-development` and `featureforge:executing-plans` own implementation. Do not start implementation inside `plan-eng-review`.

**The terminal state is presenting the execution preflight handoff with the approved plan path.**

plan-eng-review also owns the late refresh-test-plan lane when approved-plan `QA Requirement` is `required` and finish readiness reports `test_plan_artifact_missing`, `test_plan_artifact_malformed`, `test_plan_artifact_stale`, `test_plan_artifact_authoritative_provenance_invalid`, or `test_plan_artifact_generator_mismatch` for the current approved plan revision.

In that late-stage lane, the terminal state is returning to the finish-gate flow with a regenerated current-branch test-plan artifact, not reopening execution preflight.

# Plan Review Mode

Review the plan thoroughly before code changes. For every issue or recommendation, explain concrete tradeoffs, give an opinionated recommendation, and ask for input before assuming a direction.

Use the detailed rubrics, domain overlays, examples, and output templates in `$_FEATUREFORGE_ROOT/references/plan-eng-review-rubric.md`. That reference is guidance only; top-level plan headers, analyzer gates, plan-fidelity sequencing, write gates, workflow/operator routing, and approval law remain authoritative here.

## Accelerated Review

- Accelerated review is available only when the user explicitly requests `accelerated` or `accelerator` mode for the current engineering review.
- Do not activate accelerated review from heuristics, vague wording like "make this fast", saved preferences, or agent-only judgment.
- Use the existing ENG review sections as canonical section boundaries and brief the reviewer with `skills/plan-eng-review/accelerated-reviewer-prompt.md`.
- That reviewer prompt plus `review/review-accelerator-packet-contract.md` defines required section-packet schema and keeps the reviewer limited to draft-only output.
- Accelerated ENG `SMALL CHANGE` review still limits the reviewer to one primary issue per canonical section and may not collapse into one bundled approval round.
- Persist packets under `~/.featureforge/projects/<slug>/...`, resume only from the last approved-and-applied section boundary, and regenerate stale packets when the source artifact fingerprint changes.
- Final explicit human approval remains unchanged, and only the main review agent may write authoritative artifacts, apply approved patches, or change approval headers.

## Before You Start

Step 0 must answer:

- What existing code already partially or fully solves each sub-problem?
- What is the minimum set of changes that achieves the stated goal?
- If the plan touches more than 8 files or introduces more than 2 new classes or services, is there a simpler decomposition?
- Do `TODOS.md` entries block, unlock, or deserve follow-up from this plan?

Run a targeted search check when the plan introduces a new/custom auth/session/token flow, cache, queue/scheduler/background job, concurrency primitive, search/indexing subsystem, browser/platform workaround, framework wrapper, infrastructure dependency, or unfamiliar integration pattern. Annotate review prose with `[Layer 1]`, `[Layer 2]`, `[Layer 3]`, or `[EUREKA]` when those findings affect recommendations.

Ask the user to choose one review posture:

- `SCOPE REDUCTION`: the plan is overbuilt; propose a minimal version.
- `BIG CHANGE`: work through architecture, code quality, tests, and performance one section at a time.
- `SMALL CHANGE`: in normal non-accelerated review, do Step 0 plus one combined pass with the single most important issue per section.

If the user does not select `SCOPE REDUCTION`, respect that decision fully.

## Approval Gate

Before moving into review sections:

1. Read `**Source Spec:**` and confirm the file exists.
2. Read that spec's `**Workflow State:**`, `**Spec Revision:**`, and `**Last Reviewed By:**`.
3. If the spec is not workflow-valid `CEO Approved` with `**Last Reviewed By:** plan-ceo-review`, stop and direct the agent back to `featureforge:plan-ceo-review`.
4. If the plan's `**Source Spec:**` path or `**Source Spec Revision:**` does not match the latest approved spec, stop and direct the agent back to `featureforge:writing-plans`.
5. Start engineering review for a structurally parseable draft even when no plan-fidelity review artifact exists yet.
6. If you make plan edits during this review, keep `**Workflow State:** Draft` and keep `**Last Reviewed By:** writing-plans` until every engineering-review issue is resolved.
7. When all engineering-review issues are resolved, update only `**Last Reviewed By:** plan-eng-review` and hand control to `featureforge:plan-fidelity-review` for the final independent fidelity pass.
8. After a current pass plan-fidelity review artifact exists for the final draft fingerprint, return here for the final approval-header mutation.
9. Do not look for or require a runtime-owned plan-fidelity projection file. The authoritative fidelity evidence is the parseable review artifact surfaced by workflow routing and `plan contract analyze-plan` as `plan_fidelity_review`.

## Plan-Contract Gate

Before `**Workflow State:** Engineering Approved`, run:

```bash
PLAN_ANALYSIS_JSON="$("$_FEATUREFORGE_BIN" plan contract analyze-plan \
  --spec <source-spec-path> \
  --plan <plan-path> \
  --format json)"
```

Engineering approval must fail closed unless `contract_state == valid` and `packet_buildable_tasks == task_count`.

Engineering approval must also fail closed unless `plan_fidelity_review.state == pass` for the current plan/spec fingerprint.

Engineering approval must also fail closed unless `execution_strategy_present`, `dependency_diagram_present`, `execution_topology_valid`, `serial_hazards_resolved`, `parallel_lane_ownership_valid`, and `parallel_workspace_isolation_valid` are all `true`.

Engineering approval must also fail closed unless `task_contract_valid`, `task_goal_valid`, `task_context_sufficient`, `task_constraints_valid`, `task_done_when_deterministic`, and `tasks_self_contained` are all `true`.

Treat `reason_codes` and `diagnostics` from `analyze-plan` as the authoritative contract feedback for approval law.

Engineering approval must fail closed when `analyze-plan` reports missing or malformed `Requirement Index`, `Requirement Coverage Matrix`, `Execution Strategy`, `Dependency Diagram`, unknown or uncovered requirement IDs, tasks without `Spec Coverage`, missing task `Goal`, `Context`, `Constraints`, `Done when`, `Spec Coverage`, or `Files`, non-deterministic, non-atomic, or under-specified `Done when`, insufficient task `Context`, invalid `Files:` block structure, fake-parallel hotspot files, exact isolated workspace truth failures, parallel lanes without ownership, serial execution without hazard or reintegration reason, invalid task headings, ambiguous wording, requirement drift, or avoidable duplicate implementation of substantive production behavior.

If `coverage_complete`, `task_structure_valid`, `files_blocks_valid`, `execution_strategy_present`, `dependency_diagram_present`, `execution_topology_valid`, `serial_hazards_resolved`, `parallel_lane_ownership_valid`, `parallel_workspace_isolation_valid`, `task_contract_valid`, `task_goal_valid`, `task_context_sufficient`, `task_constraints_valid`, `task_done_when_deterministic`, or `tasks_self_contained` is not `true`, keep the plan in `Draft` and continue review or route back to `featureforge:writing-plans`.

Do not use legacy task-level `Open Questions` review as the primary approval model after cutover. Legacy `Task Outcome`, `Plan Constraints`, and task-level `Open Questions` are invalid through the task-contract analyzer and `review/plan-task-contract.md`; approval depends on the canonical `Goal`, `Context`, `Constraints`, `Done when`, `Spec Coverage`, and `Files` contract.

Separately, use reviewer judgment against `review/plan-task-contract.md` for reuse and architecture hygiene that cannot be fully proven by structural booleans. Keep the plan in `Draft` when any task duplicates substantive production behavior without an approved exception, fails to name the shared implementation home when reuse is required, or is too broad or under-specified for two independent reviewers to reach the same verdict.

The reuse gate is a hard approval gate, not advisory design feedback. When a task plans substantive parser, normalizer, validator, routing, eligibility, policy, prompt-assembly, state-transition, artifact-binding, or freshness behavior, the plan must either extend the named shared implementation home or name one approved exception category from `review/plan-task-contract.md` with its boundary rationale. Generated code, fixtures or test data, tiny test-only setup repetition, platform-specific adapters, controlled migration shims, and explicit layer-boundary separation are the only approved exception categories.

For every concrete engineering-review issue, use the deterministic review finding shape from `review/plan-task-contract.md`: `Finding ID`, `Severity`, `Task`, `Violated Field or Obligation`, `Evidence`, `Required Fix`, and `Hard Fail: yes|no`. Do not use general feedback when a failed task field, analyzer boolean, packet-assigned obligation, or checklist law can be named. `Required Fix` must be the smallest repair delta needed to make the plan approvable without the next author paraphrasing reviewer prose into a new interpretation.

Before approval, explicitly answer these questions:

- Does the `Requirement Coverage Matrix` cover every approved requirement without orphaned or over-broad tasks?
- Do `Files:` blocks stay within the minimum file scope needed for the covered requirements, or do they signal file-scope drift that should be split or reapproved?
- Does execution topology and `Dependency Diagram` agree?
- Does each task have one exact `Goal`?
- Is `Context` self-contained?
- Are `Done when` bullets deterministic?
- If an exception is claimed, does it name one approved exception category?

## Review Sections

Run architecture, code quality, test, and performance review in order. After each section, stop for user feedback unless normal non-accelerated `SMALL CHANGE` mode is using the bundled end-of-review round. In accelerated review, keep routine issues in section packets and break out only escalated high-judgment issues as direct human questions.

- **Architecture review:** boundaries, dependency graph, data flow, coupling, scaling, security architecture, production failure scenarios, rollout/rollback, and risk.
- **Code quality review:** organization, DRY, error handling, edge cases, technical debt, complexity, documentation, evidence expectations, and domain overlays from the companion reference.
- **Test review:** coverage graph for new UX, data flow, code paths, and branch outcomes. Every meaningful path must be exactly one of `automated`, `manual QA`, or `not required` with written justification.
- **Performance review:** N+1, memory, caching, slow or high-complexity paths, and repeated fetch patterns.

For browser-visible or multi-step interaction flows, produce an `E2E Test Decision Matrix`:

```text
FLOW | REQUIRED? | WHY | COVERAGE
```

**REGRESSION RULE:** every new meaningful path or branch outcome must land in exactly one of `automated`, `manual QA`, or `not required` with written justification before approval.

Browser-facing plans must cover loading, empty, error, success, partial, navigation, responsive, and accessibility-critical states where relevant. Non-browser paths must cover compatibility, retry/timeout semantics, replay or backfill behavior, and rollback or migration verification where relevant.

For LLM or prompt changes, check the repo's prompt or evaluation docs, name the eval suites and cases, then use one interactive user question to confirm eval scope.

## Test Plan Artifact

After the coverage graph, write a QA handoff artifact to `$_FEATUREFORGE_STATE_DIR/projects/$SLUG/{user}-{safe-branch}-test-plan-{datetime}.md`.

```bash
_SLUG_ENV=$("$_FEATUREFORGE_BIN" repo slug 2>/dev/null || true)
if [ -n "$_SLUG_ENV" ]; then
  eval "$_SLUG_ENV"
fi
unset _SLUG_ENV
USER=$(whoami)
DATETIME=$(date +%Y%m%d-%H%M%S)
mkdir -p "$_FEATUREFORGE_STATE_DIR/projects/$SLUG"
```

The artifact must include these headers and sections:

```markdown
# Test Plan
**Source Plan:** `docs/featureforge/plans/...`
**Source Plan Revision:** 3
**Branch:** {branch}
**Repo:** {slug}
**Head SHA:** {current-head}
**Browser QA Required:** yes
**Generated By:** featureforge:plan-eng-review
**Generated At:** 2026-03-22T14:30:00Z

## Affected Pages / Routes
## Key Interactions
## Edge Cases
## Critical Paths
## Coverage Graph
## E2E Test Decision Matrix
## Browser Matrix
## Non-Browser Contract Checks
## Regression Risks
## Manual QA Notes
## Engineering Review Summary
```

Include only tester-facing guidance: what to test, where to test it, and why it matters. Preserve the current core sections (`Affected Pages / Routes`, `Key Interactions`, `Edge Cases`, `Critical Paths`) and treat the richer sections as additive context; finish-gate freshness still depends on the existing required headers.

Set `**Browser QA Required:** yes` when approved-plan `**QA Requirement:** required` and the branch-specific routes or interactions define the tester-facing browser QA handoff. Otherwise write `no`. This field scopes the QA artifact for testers; it is not the authoritative finish-gate policy source. Set `**Head SHA:**` to the current `git rev-parse HEAD` for the branch state that this test-plan artifact covers.

## Outside Voice

After all review sections are complete, optionally get an outside voice. It is informative by default and actionable only if the main reviewer explicitly adopts a finding and patches the authoritative plan body.

- Use `skills/plan-eng-review/outside-voice-prompt.md` when briefing the outside voice.
- Label the source as `cross-model` only when the outside voice definitely uses a different model/provider than the main reviewer.
- If model provenance is the same, unknown, or only a fresh-context rerun of the same reviewer family, label the source as `fresh-context-subagent`.
- If the transport truncates or summarizes the outside-voice output, disclose that limitation plainly in review prose instead of overstating independence.
- If skipped, record `Outside Voice: skipped`; if unavailable, record `Outside Voice: unavailable`.

## Engineering Review Summary Writeback

After review decisions are applied to the authoritative plan body, write or replace a single trailing summary block at the end of the plan:

```markdown
## Engineering Review Summary

**Review Status:** clear | issues_open
**Reviewed At:** <ISO-8601 UTC>
**Review Mode:** big_change | small_change | scope_reduction
**Reviewed Plan Revision:** <integer>
**Critical Gaps:** <integer>
**QA Requirement:** required | not-required
**Test Plan Artifact:** `<artifact path>`
**Outside Voice:** skipped | unavailable | cross-model | fresh-context-subagent
```

Accepted review findings must patch the authoritative plan body before approval. The summary is descriptive only. Run the plan-artifact-write gate before editing the summary body and the approval-header-write gate separately before flipping approval headers. Replace any older `## Engineering Review Summary`, move the summary to the end, and leave the plan in `Draft` if freshness cannot be re-established after one retry.

## Critical Rule - How To Ask Questions

Follow the Interactive User Question format above. Additional rules for plan reviews:

- Normal review: one issue equals one interactive user question. In accelerated review, this applies only to escalated high-judgment issues.
- Present 2-3 options, including "do nothing" where reasonable; each option needs effort, risk, and maintenance burden in one line.
- Label with issue NUMBER + option LETTER, for example `3A`.
- Escape hatch: if a section has no issues, say so and move on. If an issue has an obvious fix with no real alternatives, state what you will do and move on.
- In normal non-accelerated `SMALL CHANGE` mode, batch one issue per section into a single interactive user question round at the end; accelerated `SMALL CHANGE` still uses section packets and per-section approvals.

## Required Outputs

Every engineering review must produce `NOT in scope`, `What already exists`, TODO proposals as individual interactive user questions, diagrams for non-trivial data/state/processing flows, failure modes, a completion summary, `Test Plan Artifact`, outside-voice status, and `Engineering Review Summary`.

## Retrospective Learning

Check the git log for this branch. If prior commits suggest a previous review cycle, note what changed and review repeated areas more aggressively.

## Execution Handoff

Before presenting the final execution preflight handoff, if `$_FEATUREFORGE_BIN` is available, call `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`.

- Treat workflow/operator `phase`, `phase_detail`, `review_state_status`, `next_action`, `recommended_public_command_argv`, and `required_inputs` as authoritative for public routing. `recommended_command` is display-only compatibility text.
- If workflow/operator returns `phase` `executing`, present the normal execution preflight handoff below.
- If workflow/operator returns a later phase such as `task_closure_pending`, `document_release_pending`, `final_review_pending`, `qa_pending`, or `ready_for_branch_completion`, follow that reported `phase`, `phase_detail`, `next_action`, and `recommended_public_command_argv` when present instead of reopening execution preflight; when argv is absent, satisfy typed `required_inputs` or the prerequisite named by `next_action`, then rerun workflow/operator.
- `featureforge plan execution status --plan <approved-plan-path>` is supporting diagnostic detail only; do not let it override workflow/operator routing.
- Only fall back to manual artifact inspection if the helper is unavailable or fails.
- Present the runtime-selected execution owner skill as the default path with the approved plan path.
- During handoff, name the exact approved plan path and approved plan revision, and remind the execution skill to reject draft or stale plans.
- If any task packet is missing, stale, or non-buildable for the approved plan revision, stop and route back to review instead of handing off execution.
- If isolated-agent workflows are available, show the other valid execution skill as an explicit override.
- If isolated-agent workflows are unavailable, do not present `featureforge:subagent-driven-development` as an available override.

Do not start implementation before the review is satisfied.

## Unresolved Decisions

If the user does not respond to an interactive user question or interrupts to move on, note which decisions were left unresolved. At the end of the review, list these as "Unresolved decisions that may bite you later". Never silently default.
