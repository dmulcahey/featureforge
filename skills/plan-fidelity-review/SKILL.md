---
name: plan-fidelity-review
description: Use when an engineering-reviewed draft FeatureForge implementation plan needs a final first-class fidelity review against the CEO-approved spec before engineering approval
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


# FeatureForge Artifact Contract

- Review the current engineering-reviewed draft plan in `docs/featureforge/plans/YYYY-MM-DD-<feature-name>.md`.
- Read the plan's `**Source Spec:**` and load that exact spec path.
- This stage is verification-only. Do not rewrite the plan here.
- If no draft plan exists, stop and route to `featureforge:writing-plans`.
- If the source spec is not workflow-valid `CEO Approved` with `**Last Reviewed By:** plan-ceo-review`, stop and route to `featureforge:plan-ceo-review`.
- If the draft plan has not been handed off by engineering review with `**Last Reviewed By:** plan-eng-review`, stop and route to `featureforge:plan-eng-review`.

## Independent Reviewer Requirement

- This stage must run with an independent fresh-context subagent.
- The reviewer must be distinct from both `featureforge:writing-plans` and `featureforge:plan-eng-review`.
- Use `skills/plan-fidelity-review/reviewer-prompt.md` when briefing the reviewer.
- The reviewer verifies exact Requirement Index coverage, execution-topology fidelity, and task-contract fidelity for the current draft plan revision.
- Task-contract fidelity is governed by `review/plan-task-contract.md`; missing required task fields, wrong field ordering, non-deterministic `Done when` bullets, insufficient `Context`, missing required spec references, and weak self-containment are review failures.
- The review artifact must record exactly these `Verified Surfaces`: `requirement_index`, `execution_topology`, `task_contract`, `task_determinism`, and `spec_reference_fidelity`.

## Review Artifact Contract

- Persist exactly one review artifact at `.featureforge/reviews/YYYY-MM-DD-<feature-name>-plan-fidelity.md`.
- When `$_FEATUREFORGE_BIN workflow status --json` or `$_FEATUREFORGE_BIN plan contract analyze-plan --format json` returns `plan_fidelity_review.required_artifact_template`, write the template's `artifact_path` using the template `content` verbatim.
- Fill only the reviewer-owned placeholders in that template: reviewer id, review verdict, and findings/summary content.
- Do not invent, rename, reorder, omit, or hand-type parseable artifact headers when a runtime template is available.
- The artifact must include these parseable fields:
  - `Review Stage`
  - `Review Verdict`
  - `Reviewed Plan`
  - `Reviewed Plan Revision`
  - `Reviewed Plan Fingerprint`
  - `Reviewed Spec`
  - `Reviewed Spec Revision`
  - `Reviewed Spec Fingerprint`
  - `Reviewer Source`
  - `Reviewer ID`
  - `Distinct From Stages`
  - `Verified Surfaces`
  - `Verified Requirement IDs`
- `Review Verdict` must be either `pass` or `fail`; only `pass` advances this gate.
- Review artifacts missing any required verified surface are stale or invalid for the expanded plan-fidelity gate, even if they verified requirement coverage and topology under an older artifact shape.

## Review Completion

After the reviewer writes the artifact, hand off the review artifact path and verdict to the normal planning/review authority. Do not call removed workflow helper commands from this skill.

- If the review verdict is not pass, return to `featureforge:plan-eng-review` with the artifact diagnostics.
- If the artifact is a current pass for the current plan/spec revision pair and fingerprints, continue to `featureforge:plan-eng-review` for final approval.

**The terminal state is invoking `featureforge:plan-eng-review` only after a matching pass plan-fidelity review artifact exists.**
