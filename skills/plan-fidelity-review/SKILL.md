---
name: plan-fidelity-review
description: Use when a draft FeatureForge implementation plan needs a first-class fidelity review against the CEO-approved spec before engineering review
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


# FeatureForge Artifact Contract

- Review the current draft plan in `docs/featureforge/plans/YYYY-MM-DD-<feature-name>.md`.
- Read the plan's `**Source Spec:**` and load that exact spec path.
- This stage is verification-only. Do not rewrite the plan here.
- If no draft plan exists, stop and route to `featureforge:writing-plans`.
- If the source spec is not workflow-valid `CEO Approved` with `**Last Reviewed By:** plan-ceo-review`, stop and route to `featureforge:plan-ceo-review`.

## Independent Reviewer Requirement

- This stage must run with an independent fresh-context subagent.
- The reviewer must be distinct from both `featureforge:writing-plans` and `featureforge:plan-eng-review`.
- Use `skills/plan-fidelity-review/reviewer-prompt.md` when briefing the reviewer.
- The reviewer verifies exact Requirement Index coverage and execution-topology fidelity for the current draft plan revision.

## Review Artifact Contract

- Persist exactly one review artifact at `.featureforge/reviews/YYYY-MM-DD-<feature-name>-plan-fidelity.md`.
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
- `Review Verdict` must be `pass` for this gate to advance.

## Receipt Recording

After the reviewer writes the artifact, hand off the review artifact path and verdict to the normal planning/review authority. Do not call removed workflow helper commands from this skill.

- If receipt recording fails or the review verdict is not pass, return to `featureforge:writing-plans`.
- If the receipt records successfully in pass state for the current plan/spec revision pair, continue to `featureforge:plan-eng-review`.

**The terminal state is invoking `featureforge:plan-eng-review` only after a matching pass runtime-owned plan-fidelity receipt exists.**
