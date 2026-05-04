---
name: plan-ceo-review
description: Use when a written FeatureForge design or architecture spec needs CEO or founder review before implementation planning, including scope expansion, selective expansion, hold-scope rigor, or scope reduction
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

- Review the written spec artifact in `docs/featureforge/specs/YYYY-MM-DD-<topic>-design.md`.
- If the user names a specific spec path, use that path. Otherwise, inspect `docs/featureforge/specs/` and review the newest matching design doc.
- If no current spec exists, stop and direct the agent back to `featureforge:brainstorming`.
- The spec must include these exact header lines immediately below the title:

```markdown
**Workflow State:** Draft | CEO Approved
**Spec Revision:** <integer>
**Last Reviewed By:** brainstorming | plan-ceo-review
```

- If any header line is missing or malformed, normalize the spec to this contract before continuing and treat it as `Draft`.
- `brainstorming` is only valid while the spec remains `Draft`. A `CEO Approved` spec must end with `**Last Reviewed By:** plan-ceo-review`.
- When review decisions change the written spec, update the spec document before continuing.
- After each spec edit (including final approval edits), keep using the same repo-relative spec path in later workflow/operator and writing-plans handoffs; do not route through compatibility-only `workflow sync`.

**Protected-Branch Repo-Write Gate:**

- Before editing the spec body or changing approval headers on disk, run the shared repo-safety preflight for the exact review-write scope:

```bash
featureforge repo-safety check --intent write --stage featureforge:plan-ceo-review --task-id <current-spec-review> --path docs/featureforge/specs/YYYY-MM-DD-<topic>-design.md --write-target repo-file-write
```

- When the mutation is specifically an approval-header edit, use the same command shape with `--write-target approval-header-write`.
- If the helper returns `blocked`, name the branch, the stage, and the blocking `failure_class`, then route to either a feature branch / `featureforge:using-git-worktrees` or explicit user approval for this exact review scope.
- If the user explicitly approves the protected-branch review write, run:

```bash
featureforge repo-safety approve --stage featureforge:plan-ceo-review --task-id <current-spec-review> --reason "<explicit user approval>" --path docs/featureforge/specs/YYYY-MM-DD-<topic>-design.md --write-target repo-file-write
featureforge repo-safety check --intent write --stage featureforge:plan-ceo-review --task-id <current-spec-review> --path docs/featureforge/specs/YYYY-MM-DD-<topic>-design.md --write-target repo-file-write
```

- Repeat the same approve -> re-check pattern for `approval-header-write` before flipping `**Workflow State:**` or any other approval header on a protected branch.
- Keep the spec in `Draft` until the review is fully resolved.
- When approving the written spec, set `**Workflow State:** CEO Approved` and `**Last Reviewed By:** plan-ceo-review`.
- `**Spec Revision:**` starts at `1`. If this review materially changes a previously approved spec, increment the revision and reset the spec to `Draft` until it is re-approved.
- When the review is resolved and the written spec is approved, invoke `featureforge:writing-plans`.
- `featureforge:writing-plans` owns plan creation after approval. Do not draft a plan or offer implementation options from `plan-ceo-review`.

**The terminal state is invoking writing-plans.**

# Spec Review Mode

You are not here to rubber-stamp the spec. Review only the written spec, do not make code changes, do not start implementation, and do not draft the implementation plan from this skill.

Use the detailed rubrics, examples, and output templates in `$_FEATUREFORGE_ROOT/references/plan-ceo-review-rubric.md`. That reference is guidance only; the terminal decisions, write gates, workflow headers, stop rules, and approval law in this top-level skill remain authoritative.

## Accelerated Review

- Accelerated review is available only when the user explicitly requests `accelerated` or `accelerator` mode for the current CEO review.
- Do not activate accelerated review from heuristics, vague wording like "make this fast", saved preferences, or agent-only judgment.
- Use the existing CEO review sections as canonical boundaries and brief the accelerated reviewer with `skills/plan-ceo-review/accelerated-reviewer-prompt.md`.
- The reviewer prompt plus `review/review-accelerator-packet-contract.md` define section-packet schema and keep the reviewer limited to draft-only output.
- Persist accelerated CEO section packets under `~/.featureforge/projects/<slug>/...`; resume only from the last approved-and-applied section boundary.
- If the source artifact fingerprint changes, treat saved packets as stale and regenerate them before reuse.
- Final explicit human approval remains unchanged, and only the main review agent may write authoritative artifacts, apply approved patches, or change approval headers.

## Review Standard

In every mode, the user owns scope. Every scope change is explicit opt-in via an interactive user question. Never silently add or remove scope, and once the user selects a mode, keep that posture through the review.

Modes:

- `SCOPE EXPANSION`: push scope up and surface ambitious improvements.
- `SELECTIVE EXPANSION`: hold the current scope as baseline, then present each expansion opportunity as its own individual interactive user question.
- `HOLD SCOPE`: make the current scope bulletproof without silent expansion or reduction.
- `SCOPE REDUCTION`: identify the minimum version that still delivers the core outcome.

Present each expansion opportunity as its own individual interactive user question.

CEO approval is blocked when the written spec materially lacks any delivery-floor item: clear problem and outcome, scope boundaries, relevant interfaces or dependencies, failure-mode thinking, observability expectations, rollout and rollback expectations, credible risks, and testable acceptance criteria.

## Required Audit And Step 0

Before reviewing the spec, run a local system audit. Read `AGENTS.md`, `AGENTS.override.md`, `.github/copilot-instructions.md`, `.github/instructions/*.instructions.md`, `TODOS.md`, and relevant architecture docs when present. Check git log, branch diff, stashes, relevant TODO/FIXME/HACK/XXX markers, and touched-area conventions.

Do not use PR metadata or repo default-branch APIs as a fallback; keep the system audit locally derivable from repository state.

Map current system state, in-flight work, relevant known pain points, prior review cycles, TODO dependencies, and whether the spec involves UI scope. If UI scope is detected, note `UI_SCOPE` for Section 11.

Run a short landscape check after the audit and before mode selection:

- reuse the spec's `Landscape Snapshot` when it exists and is still relevant
- refresh only when the spec lacks it or the review introduces materially new market, category, or architecture assumptions
- surface common incumbent approaches, overbuilt/failure patterns, solved-problem risks, and any simplification or differentiation insight
- if search is unavailable, disallowed, or unsafe, say so plainly and continue with repo/local reasoning

Step 0 must challenge the premise, existing-code leverage, 12-month dream state, mode-specific scope posture, temporal implementation decisions, and final mode selection. Stop after Step 0 until the chosen mode and any required issue decisions are resolved.

## Review Sections

Run these sections in order. The companion reference contains the full rubrics, tables, examples, and completion-summary template.

1. **Architecture Review:** component boundaries, dependency graph, data flow, state machines, coupling, scaling, failure scenarios, rollback, and required ASCII diagrams.
2. **Error & Rescue Map:** named failures, exception classes, rescue behavior, user-visible impact, logging, retries, and silent-failure gaps.
3. **Security & Threat Model:** attack surface, input validation, authorization, secrets, dependency risk, data classification, injection, and audit logging.
4. **Data Flow & Interaction Edge Cases:** nil, empty, invalid, slow, stale, duplicate, partial, and user-interaction edge cases.
5. **Code Quality Review:** organization, DRY, naming, error patterns, complexity, over/under-engineering, and missing edge cases.
6. **Test Review:** new UX flows, data flows, code paths, async work, integrations, error paths, test types, flakiness, and load/stress needs.
7. **Performance Review:** N+1, memory, indexing, caching, background sizing, slow paths, and connection pressure.
8. **Observability & Debuggability Review:** logging, metrics, tracing, alerts, dashboards, runbooks, admin tooling, and operating ergonomics.
9. **Deployment & Rollout Review:** migration safety, feature flags, rollout order, rollback, risk windows, environment parity, and smoke tests.
10. **Long-Term Trajectory Review:** debt, path dependency, reversibility, ecosystem fit, knowledge concentration, and 12-month readability.
11. **Section 11: Design & UX Review:** run only when `UI_SCOPE` is present; cover information architecture, interaction states, journey coherence, responsive intent, accessibility, and AI-slop risk.

After each section, stop. In normal review, use one interactive user question per issue. In accelerated review, keep routine issues in the section packet and break out only escalated high-judgment issues as direct human questions. Do NOT batch escalated issues. If no issues or the fix is obvious, state what you will do and move on.

## Outside Voice

After all sections are complete, optionally get an outside voice. It is informative by default and actionable only if the main reviewer explicitly adopts a finding and patches the authoritative spec body.

- Use `skills/plan-ceo-review/outside-voice-prompt.md` when briefing the outside voice.
- Prefer `codex exec` when available.
- Label the source as `cross-model` only when the outside voice definitely uses a different model/provider than the main reviewer.
- If model provenance is the same, unknown, or only a fresh-context rerun of the same reviewer family, label the source as `fresh-context-subagent`.
- If the transport truncates or summarizes the outside-voice output, disclose that limitation plainly in review prose instead of overstating independence.
- If neither outside-voice path is available, record `Outside Voice: unavailable`; if skipped, record `Outside Voice: skipped`.

## CEO Review Summary Writeback

After review decisions are applied to the authoritative spec body, write or replace a single trailing summary block at the end of the spec:

```markdown
## CEO Review Summary

**Review Status:** clear | issues_open
**Reviewed At:** <ISO-8601 UTC>
**Review Mode:** hold_scope | selective_expansion | expansion | scope_reduction
**Reviewed Spec Revision:** <integer>
**Critical Gaps:** <integer>
**UI Design Intent Required:** yes | no
**Outside Voice:** skipped | unavailable | cross-model | fresh-context-subagent
```

Accepted selective-expansion candidates must patch the authoritative spec body before approval. The summary is descriptive only. Run the repo-file-write gate before editing the summary body and the approval-header-write gate separately before flipping approval headers. Replace any older `## CEO Review Summary`, move the summary to the end, and leave the spec in `Draft` if freshness cannot be re-established after one retry.

## Critical Rule - How To Ask Questions

Follow the Interactive User Question format above. Additional rules for spec reviews:

- Normal review: one issue equals one interactive user question. In accelerated review, this applies only to escalated high-judgment issues.
- Describe the problem concretely, with file and line references when relevant.
- Present 2-3 options, including "do nothing" where reasonable.
- For each option: effort, risk, and maintenance burden in one line.
- Map reasoning to the engineering preferences in the companion reference.
- Label with issue NUMBER + option LETTER, for example `3A`.
- Escape hatch: if a section has no issues, say so and move on. If an issue has an obvious fix with no real alternatives, state what you will do and move on.

## Required Outputs

Every CEO review must produce these outputs before approval:

- `NOT in scope`
- `What already exists`
- `Dream state delta`
- `Error & Rescue Registry`
- `Failure Modes Registry`
- `TODOS.md` proposals, each as its own interactive user question
- `Delight Opportunities` for `SCOPE EXPANSION` and `SELECTIVE EXPANSION`
- Required ASCII diagrams for architecture, data flow, state machine, error flow, deployment, and rollback when applicable
- `Stale Diagram Audit`
- `Completion Summary`
- `Unresolved Decisions` for unanswered questions

Use **CRITICAL GAP**, **WARNING**, and **OK** for scannability. Never silently default unresolved decisions.
