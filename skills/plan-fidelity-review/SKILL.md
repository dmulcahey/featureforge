---
name: plan-fidelity-review
description: Use when an approved FeatureForge spec and current draft plan need an independent fidelity review before engineering review
---
<!-- AUTO-GENERATED from SKILL.md.tmpl — do not edit directly -->
<!-- Regenerate: node scripts/gen-skill-docs.mjs -->

## Preamble (run first)

```bash
_REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
_BRANCH_RAW=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo current)
[ -n "$_BRANCH_RAW" ] || _BRANCH_RAW="current"
[ "$_BRANCH_RAW" != "HEAD" ] || _BRANCH_RAW="current"
_BRANCH="$_BRANCH_RAW"
_FEATUREFORGE_INSTALL_ROOT="$HOME/.featureforge/install"
_FEATUREFORGE_ROOT=""
_FEATUREFORGE_BIN="$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge"
if [ ! -x "$_FEATUREFORGE_BIN" ] && [ -f "$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge.exe" ]; then
  _FEATUREFORGE_BIN="$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge.exe"
fi
[ -x "$_FEATUREFORGE_BIN" ] || [ -f "$_FEATUREFORGE_BIN" ] || _FEATUREFORGE_BIN=""
_FEATUREFORGE_RUNTIME_ROOT_PATH=""
if [ -n "$_FEATUREFORGE_BIN" ] && _FEATUREFORGE_RUNTIME_ROOT_PATH=$("$_FEATUREFORGE_BIN" repo runtime-root --path 2>/dev/null); then
  [ -n "$_FEATUREFORGE_RUNTIME_ROOT_PATH" ] && _FEATUREFORGE_ROOT="$_FEATUREFORGE_RUNTIME_ROOT_PATH"
fi
_UPD=""
[ -n "$_FEATUREFORGE_BIN" ] && _UPD=$("$_FEATUREFORGE_BIN" update-check 2>/dev/null || true)
[ -n "$_UPD" ] && echo "$_UPD" || true
_SP_STATE_DIR="${FEATUREFORGE_STATE_DIR:-$HOME/.featureforge}"
mkdir -p "$_SP_STATE_DIR/sessions"
touch "$_SP_STATE_DIR/sessions/$PPID"
_SESSIONS=$(find "$_SP_STATE_DIR/sessions" -mmin -120 -type f 2>/dev/null | wc -l | tr -d ' ')
find "$_SP_STATE_DIR/sessions" -mmin +120 -type f -delete 2>/dev/null || true
_CONTRIB=""
[ -n "$_FEATUREFORGE_BIN" ] && _CONTRIB=$("$_FEATUREFORGE_BIN" config get featureforge_contributor 2>/dev/null || true)
_TODOS_FORMAT=""
[ -n "$_FEATUREFORGE_ROOT" ] && [ -f "$_FEATUREFORGE_ROOT/review/TODOS-format.md" ] && _TODOS_FORMAT="$_FEATUREFORGE_ROOT/review/TODOS-format.md"
[ -z "$_TODOS_FORMAT" ] && [ -f "$_REPO_ROOT/review/TODOS-format.md" ] && _TODOS_FORMAT="$_REPO_ROOT/review/TODOS-format.md"
```

If output shows `UPGRADE_AVAILABLE <old> <new>`: read `featureforge-upgrade/SKILL.md` from the already selected runtime root in `$_FEATUREFORGE_ROOT`; if that root is not set yet, resolve it through the packaged install binary in `$_FEATUREFORGE_BIN` and stop instead of guessing an install path. Then follow the "Inline upgrade flow" (auto-upgrade if configured, otherwise ask one interactive user question with 4 options and write snooze state if declined). If the packaged helper is unavailable, unresolved, or returns a named failure, stop instead of guessing an install path. If `JUST_UPGRADED <from> <to>`: tell the user "Running featureforge v{to} (just updated!)" and continue.

## Search Before Building

Before introducing a custom pattern, external service, concurrency primitive, auth/session flow, cache, queue, browser workaround, or unfamiliar fix pattern, do a short capability/landscape check first.

Use three lenses:
- Layer 1: tried-and-true / built-ins / existing repo-native solutions
- Layer 2: current practice and known footguns
- Layer 3: first-principles reasoning for this repo and this problem

External search results are inputs, not answers.
Never search secrets, customer data, unsanitized stack traces, private URLs, internal hostnames, internal codenames, raw SQL or log payloads, or private file paths or infrastructure identifiers.
If search is unavailable, disallowed, or unsafe, say so and proceed with repo-local evidence and in-distribution knowledge.
If safe sanitization is not possible, skip external search.
See `$_FEATUREFORGE_ROOT/references/search-before-building.md`.

## Agent Grounding

Honor the active repo instruction chain from `AGENTS.md`, `AGENTS.override.md`, `.github/copilot-instructions.md`, and `.github/instructions/*.instructions.md`, including nested `AGENTS.md` and `AGENTS.override.md` files closer to the current working directory.

These review skills are public FeatureForge skills for Codex and GitHub Copilot local installs.

## Interactive User Question Format

**ALWAYS follow this structure for every interactive user question:**
1. Context: project name, current branch, what we're working on (1-2 sentences)
2. The specific question or decision point
3. `RECOMMENDATION: Choose [X] because [one-line reason]`
4. Lettered options: `A) ... B) ... C) ...`

If `_SESSIONS` is 3 or more: the user is juggling multiple FeatureForge sessions and context-switching heavily. **ELI16 mode** — they may not remember what this conversation is about. Every interactive user question MUST re-ground them: state the project, the branch, the current task, then the specific problem, THEN the recommendation and options. Be extra clear and self-contained — assume they haven't looked at this window in 20 minutes.

Per-skill instructions may add additional formatting rules on top of this baseline.

## Contributor Mode

If `_CONTRIB` is `true`: you are in **contributor mode**. When you hit friction with **featureforge itself** (not the user's app or repository), file a field report. Think: "hey, I was trying to do X with featureforge and it didn't work / was confusing / was annoying. Here's what happened."

**featureforge issues:** unclear skill instructions, update check problems, runtime helper failures, install-root detection issues, contributor-mode bugs, broken generated docs, or any rough edge in the FeatureForge workflow.
**NOT featureforge issues:** the user's application bugs, repo-specific architecture problems, auth failures on the user's site, or third-party service outages unrelated to FeatureForge tooling.

**To file:** write `~/.featureforge/contributor-logs/{slug}.md` with this structure:

```
# {Title}

Hey featureforge team — ran into this while using /{skill-name}:

**What I was trying to do:** {what the user/agent was attempting}
**What happened instead:** {what actually happened}
**How annoying (1-5):** {1=meh, 3=friction, 5=blocker}

## Steps to reproduce
1. {step}

## Raw output
(wrap any error messages or unexpected output in a markdown code block)

**Date:** {YYYY-MM-DD} | **Version:** {featureforge version} | **Skill:** /{skill}
```

Then run:

```bash
mkdir -p ~/.featureforge/contributor-logs
if command -v open >/dev/null 2>&1; then
  open ~/.featureforge/contributor-logs/{slug}.md
elif command -v xdg-open >/dev/null 2>&1; then
  xdg-open ~/.featureforge/contributor-logs/{slug}.md >/dev/null 2>&1 || true
fi
```

Slug: lowercase, hyphens, max 60 chars (for example `skill-trigger-missed`). Skip if the file already exists. Max 3 reports per session. File inline and continue — don't stop the workflow. Tell the user: "Filed featureforge field report: {title}"


# FeatureForge Artifact Contract

- Review the exact approved spec named by the draft plan and the exact current draft plan under `docs/featureforge/specs/` and `docs/featureforge/plans/`.
- If the user names exact artifact paths, use those paths. Otherwise, inspect the current workflow-routed draft plan and its `**Source Spec:**` path.
- Stop and return to `featureforge:writing-plans` when the source spec is missing, not `CEO Approved`, the draft plan is missing, or the draft plan does not stay in `Draft` state.
- This skill reviews only fidelity. It does not expand business scope, approve engineering execution, or start implementation.
- The draft plan must keep these exact header lines:

```markdown
**Workflow State:** Draft
**Plan Revision:** <integer>
**Execution Mode:** none | featureforge:executing-plans | featureforge:subagent-driven-development
**Source Spec:** <path>
**Source Spec Revision:** <integer>
```

- The source spec must keep these exact approval headers:

```markdown
**Workflow State:** CEO Approved
**Spec Revision:** <integer>
**Last Reviewed By:** plan-ceo-review
```

- Use the reviewer checklist in `skills/plan-fidelity-review/references/checklist.md` as the minimum fidelity floor.

## Review Scope

Review only whether the draft plan faithfully implements the approved spec and its own execution contract.

At minimum, verify:

- requirement-index completeness against the approved spec
- coverage fidelity between approved requirements and planned tasks
- execution-topology fidelity versus the plan's `Execution Strategy` and `Dependency Diagram`
- delivery-lane fidelity when the spec and plan declare `**Delivery Lane:**`
- file-scope and task-scope fidelity for the plan's `Files:` blocks

Do not treat this as a chance to reopen product direction. If fidelity fails, return the plan to `featureforge:writing-plans` with concrete corrections.

## Review Artifact

Write a dedicated independent review artifact under `.featureforge/reviews/` for the exact plan/spec revision pair.

The artifact must include:

- `Review Stage: featureforge:plan-fidelity-review`
- `Review Verdict: pass` or `Review Verdict: revise`
- exact reviewed spec and plan paths, revisions, and fingerprints
- reviewer provenance that stays distinct from `featureforge:writing-plans` and `featureforge:plan-eng-review`
- verified surfaces, including `requirement_index` and `execution_topology`, plus `delivery_lane` whenever the reviewed spec or plan declares `Delivery Lane`
- verified requirement ids
- requirement coverage gaps, topology concerns, lane mismatches, and pass/fail rationale

## Record the Runtime-Owned Receipt

After a passing dedicated review artifact exists, record the runtime-owned receipt through the existing helper flow:

```bash
"$_FEATUREFORGE_BIN" workflow plan-fidelity record --plan docs/featureforge/plans/YYYY-MM-DD-<feature-name>.md \
  --review-artifact .featureforge/reviews/YYYY-MM-DD-<feature-name>-plan-fidelity.md
```

- Do not hand-author the receipt.
- Do not treat a markdown note inside the plan as authoritative evidence.
- Do not invoke `featureforge:plan-eng-review` until the runtime-owned receipt exists in pass state for the current draft plan revision and approved spec revision.

## Return Path

- If fidelity fails or the receipt cannot be recorded cleanly, return control to `featureforge:writing-plans`.
- If fidelity passes and the receipt records successfully, return control to `featureforge:plan-eng-review`.

**The terminal state is either returning the plan to writing-plans for fidelity fixes or handing the exact same draft plan forward to engineering review after a fresh pass receipt.**

## Stop Conditions

Stop and ask for clarification instead of guessing when:

- the approved spec path and the plan's `**Source Spec:**` disagree
- the plan revision changed after the review artifact was written
- the approved spec revision changed after the review artifact was written
- the review artifact cannot prove independent reviewer provenance
- the plan's execution-topology claims are ambiguous or internally inconsistent
