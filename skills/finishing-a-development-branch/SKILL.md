---
name: finishing-a-development-branch
description: Use when implementation is complete, verification passes, and you need to decide how to integrate the work through merge, PR, or cleanup
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


# Finishing a Development Branch

## Overview

Guide completion of development work by presenting clear options and handling chosen workflow.

**Core principle:** Verify tests → Run required pre-completion gates → Present options → Execute choice → Clean up.

**Announce at start:** "I'm using the finishing-a-development-branch skill to complete this work."

## The Process

### Step 1: Verify Tests

**Before presenting options, verify tests pass:**

```bash
# Run project's test suite
npm test / cargo test / pytest / go test ./...
```

**If tests fail:**
```
Tests failing (<N> failures). Must fix before completing:

[Show failures]

Cannot proceed with merge/PR until tests pass.
```

Stop. Don't proceed to Step 2.

**If tests pass:** Continue to Step 1.5.

### Step 1.5: Optional Pre-Landing Review Gate

If a fresh review has not already been resolved for the current diff, you may invoke `featureforge:requesting-code-review` as a non-terminal checkpoint before presenting completion options.

For workflow-routed terminal completion, this checkpoint does not replace the required terminal review gate that runs after `featureforge:document-release` and before any runtime-routed `featureforge:qa-only` handoff.

- Resolve all Critical issues before continuing
- Resolve Important issues unless the user explicitly accepts the risk
- If a fresh review already happened in the current workflow, continue silently
- A review stops being fresh as soon as new repo changes land, including release-doc or metadata edits from `featureforge:document-release`

### Step 1.6: Execution-State Gate

Before presenting completion options:

- If the current work was executed from an approved FeatureForge plan, require the exact approved plan path from the current execution workflow context before presenting completion options.
- Run `featureforge workflow operator --plan <approved-plan-path>` and require a branch-completion-ready route before presenting completion options.
- If the exact approved plan path is unavailable or workflow/operator fails, stop and return to the current execution flow instead of guessing.
- Use `featureforge plan execution status --plan <approved-plan-path>` only when you need additional diagnostics (`active_task`, `blocking_task`, `resume_task`, `evidence_path`, checkpoint fingerprints) to explain a blocker.
- For workflow-routed terminal completion, do not run the terminal review gate in this step. Run it only after `featureforge:document-release` and before any runtime-routed `featureforge:qa-only` handoff.
- If the current work is not governed by an approved FeatureForge plan, skip this execution-state gate and continue.
- rejects branch-completion handoff if the approved plan is execution-dirty or malformed
- must not allow branch completion while any checked-off plan step still lacks semantic implementation evidence
- consumes the same execution evidence artifact used by terminal final review
- must fail closed when it detects a missed reopen or stale evidence, but must not call `reopen` itself

### Step 1.75: Required Document Release Gate

For workflow-routed work, require the `document-release` pass before presenting completion options.

For workflow-routed terminal completion, keep the order strict: `featureforge:document-release` -> terminal `featureforge:requesting-code-review` -> `featureforge workflow operator --plan <approved-plan-path>` -> any required `featureforge:qa-only` handoff -> `advance-late-stage` only when operator reports `phase_detail=qa_recording_required` -> rerun `featureforge workflow operator --plan <approved-plan-path>` and follow its next finish command.

Any required `featureforge:qa-only` handoff is downstream of that terminal final-review pass. Do not move QA ahead of the post-document-release `featureforge:requesting-code-review` gate.

When in doubt on late-stage routing language, use `review/late-stage-precedence-reference.md` as the shared phase/action/skill table.

For workflow-routed work, if the repo has release-facing docs or metadata such as `CHANGELOG.md`, `RELEASE-NOTES.md`, `VERSION`, `TODOS.md`, `README.md`, or platform workflow docs, do not treat documentation as optional cleanup. Route through `featureforge:document-release` first unless a fresh pass already happened in the current workflow.

For ad-hoc or non-workflow-routed work, keep `document-release` available as an optional cleanup pass when the diff clearly changes release-facing docs or handoff material, but do not turn it into a universal pre-completion gate.

If `featureforge:document-release` writes repo files or changes release metadata, treat any earlier code review as stale and loop back through `featureforge:requesting-code-review` before presenting completion options.

A document-release rewrite also makes any earlier browser QA stale if browser QA is still required for the current HEAD, so do not reuse pre-document-release QA artifacts as the final finish-gate input.

Before moving on, perform a short Gate F-style confirmation:

- documentation has been refreshed
- release notes or equivalent release-history updates are ready
- rollout and rollback are addressed
- known risks are documented
- monitoring or verification expectations are addressed when relevant

### Step 1.85: Conditional Pre-Landing QA Gate

Conditional pre-landing browser QA when the branch change surface or test-plan artifact warrants it:

```bash
_SLUG_ENV=$("$_FEATUREFORGE_BIN" repo slug 2>/dev/null || true)
if [ -n "$_SLUG_ENV" ]; then
  eval "$_SLUG_ENV"
fi
unset _SLUG_ENV
PLAN_ARTIFACT=""
for CANDIDATE in $(ls -t "$_FEATUREFORGE_STATE_DIR/projects/$SLUG"/*-test-plan-*.md 2>/dev/null); do
  [ -f "$CANDIDATE" ] || continue
  ARTIFACT_BRANCH=$(sed -n 's/^\*\*Branch:\*\* //p' "$CANDIDATE" | head -1)
  if [ "$ARTIFACT_BRANCH" = "$BRANCH" ]; then
    PLAN_ARTIFACT="$CANDIDATE"
    break
  fi
done
printf '%s\n' "$PLAN_ARTIFACT"
```

If the current work is governed by an approved FeatureForge plan, treat the approved plan's normalized `**QA Requirement:** required|not-required` metadata as authoritative for workflow-routed finish gating.

For workflow-routed work, this step validates QA applicability and current-branch test-plan freshness only. It does not authorize running `featureforge:qa-only` yet; terminal `featureforge:requesting-code-review` and the next `featureforge workflow operator --plan <approved-plan-path>` reroute still come first.

Match current-branch artifacts by their `**Branch:**` header, not by a filename substring glob, so `my-feature` cannot masquerade as `feature`.

Treat the current-branch test-plan artifact as a QA scope/provenance input only when its `Source Plan`, `Source Plan Revision`, and `Head SHA` match the exact approved plan path, revision, and current branch HEAD from the workflow context.

If that artifact names pages, routes, or browser interactions, use it to scope the required QA handoff when QA is required or when the user explicitly wants extra browser validation.

A project-wide or generic test-plan artifact may help scope ad-hoc QA, but it does not satisfy helper-backed finish readiness when approved-plan `QA Requirement` is `required`. Only the current-branch artifact counts for that freshness check.

If approved-plan `QA Requirement` is missing or invalid when deciding whether QA applies, stop and reroute through `featureforge plan execution repair-review-state --plan <path>`; do not guess from test-plan prose.

If approved-plan `QA Requirement` is `required` and no current-branch test-plan artifact exists for workflow-routed work, stop and regenerate it before invoking `featureforge:qa-only` or late-stage completion commands.

If workflow/operator reports `test_plan_refresh_required`, hand control back to `featureforge:plan-eng-review` to regenerate the current-branch test-plan artifact before QA or branch completion.

Recommendation logic:
- For workflow-routed work, do not present QA options in this step. After terminal `featureforge:requesting-code-review` resolves, rerun `featureforge workflow operator --plan <approved-plan-path>` and let `phase` plus `phase_detail` decide whether QA is still required.
- For ad-hoc or non-workflow work, recommend `A)` when browser QA is clearly warranted.
- For ad-hoc or non-workflow work, if approved-plan `QA Requirement` is `not-required`, QA remains optional unless the user explicitly wants extra browser validation.
- For ad-hoc non-workflow work without a current-branch test-plan artifact, QA remains optional for clearly non-browser work.

When browser QA is clearly warranted for ad-hoc or non-workflow work, do not present a skip option.

Possible options when browser QA is required for ad-hoc or non-workflow work:
- `A)` Run `featureforge:qa-only` now and return here after the report is written

Possible options when browser QA is optional for ad-hoc or non-workflow work:
- `A)` Run `featureforge:qa-only` now and return here after the report is written
- `B)` Skip QA handoff this time

If a fresh `qa-only` report already happened in the current workflow, continue silently.

For workflow-routed work, require the QA handoff only when workflow/operator later routes to `qa_pending`; do not infer the final QA gate from Step 1.85 alone.

If approved-plan `QA Requirement` is `required` and no current-branch test-plan artifact exists for workflow-routed work, stop and regenerate it before the operator-routed QA or finish-gate steps.

### Step 1.9: Finish Gate

If the current work is governed by an approved FeatureForge plan, use `featureforge workflow operator --plan <approved-plan-path>` as the late-stage routing source after `featureforge:document-release` and the terminal `featureforge:requesting-code-review` gate are current.

If the current work is governed by an approved FeatureForge plan, after `featureforge:document-release` and the terminal `featureforge:requesting-code-review` gate are current, rerun `featureforge workflow operator --plan <approved-plan-path>` and follow the exact `phase_detail`-driven next finish command before presenting completion options.

If the operator reports `qa_pending` with `phase_detail=test_plan_refresh_required`, hand control back to `featureforge:plan-eng-review` before QA or branch completion.

If the operator reports `qa_pending` with `phase_detail=qa_recording_required`, record QA with `featureforge plan execution advance-late-stage --plan <approved-plan-path> --result pass|fail --summary-file <qa-report>`, then rerun `featureforge workflow operator --plan <approved-plan-path>`.

If the operator reports `ready_for_branch_completion`, run the exact `recommended_public_command_argv` when present and rerun `featureforge workflow operator --plan <approved-plan-path>` until branch completion options are actually routable. Do not shell-parse or whitespace-split `recommended_command`; it is display-only compatibility text.

If the operator reports any other late-stage phase/detail pair, follow that exact operator result instead of forcing QA or finish-gate commands from memory.

For workflow-routed terminal completion, if no fresh post-document-release dedicated-independent review exists for the current `HEAD`, invoke `featureforge:requesting-code-review` before presenting branch completion options.

Low-level compatibility finish commands remain expert/debug-only surfaces; follow workflow/operator for normal routing instead of invoking compatibility commands from memory.

If the current work is governed by an approved FeatureForge plan and workflow/operator does not route to branch completion, stop and return to the current execution flow; do not present completion options against stale QA or release artifacts.

If the current work is not governed by an approved FeatureForge plan, skip this helper-owned finish gate and continue with the normal completion flow.

### Step 1.95: Protected-Branch Repo-Write Gate

Before executing any completion option that mutates repo state, run the shared repo-safety preflight for the chosen branch-finishing scope:

```bash
featureforge repo-safety check --intent write --stage featureforge:finishing-a-development-branch --task-id <current-branch-finish> --write-target branch-finish
```

- If the helper returns `allowed`, continue with the selected completion path.
- If it returns `blocked`, name the branch, the stage, and the blocking `failure_class`, then route to either a feature branch / `featureforge:using-git-worktrees` or explicit user approval for this exact completion scope.
- If the user explicitly approves the protected-branch completion write, approve the full completion scope you intend to use on that branch, including any follow-on git targets that are part of the same branch-finish task:

```bash
featureforge repo-safety approve --stage featureforge:finishing-a-development-branch --task-id <current-branch-finish> --reason "<explicit user approval>" --write-target branch-finish [--write-target git-merge] [--write-target git-push] [--write-target git-worktree-cleanup]
featureforge repo-safety check --intent write --stage featureforge:finishing-a-development-branch --task-id <current-branch-finish> --write-target branch-finish [--write-target git-merge] [--write-target git-push] [--write-target git-worktree-cleanup]
```

- Continue only if the re-check returns `allowed`.
- Before a follow-on `git merge`, `git push`, or worktree cleanup on the same protected-branch task, re-run the gate with the same task id and the same approved write-target set.
- If the protected-branch task scope changes, run a new `approve` plus full-scope `check` before continuing.
- Do not treat a worktree on `main`, `master`, `dev`, or `develop` as safe by itself; the branch must be non-protected or explicitly approved.

### Step 2: Determine Base Branch

If the current work is governed by an approved FeatureForge plan:

- For plan-routed completion, use the exact `base_branch` from `featureforge workflow operator --plan <approved-plan-path> --json` instead of redetecting the target branch.
- Treat release-readiness markdown as a derived handoff artifact. Do not read its `**Base Branch:**` header back into routing or branch-selection decisions; use the runtime-owned `base_branch` from workflow/operator instead.

If the current work is not governed by an approved FeatureForge plan, require an explicit `<base-branch>` value and keep it stable for this run:

```bash
if [ -z "$BASE_BRANCH" ]; then
  echo "Missing BASE_BRANCH. For non-workflow completion, set BASE_BRANCH explicitly before finishing the branch."
  exit 1
fi
```

Do not use PR metadata or repo default-branch APIs as a fallback.
The Step 2 `<base-branch>` value stays authoritative for Options A, B, and D. Do not redetect it later in the branch-finishing flow.

### Step 3: Present Options

Ask one interactive user question using the required format.

```
A) Merge back to <base-branch> locally
B) Push and create a Pull Request
C) Keep the branch as-is for follow-up later
D) Discard this work
```

Recommendation logic:
- Recommend `B)` when a normal PR flow is available and the user has not signaled a different preference
- Recommend `A)` when local integration is clearly preferred or PR tooling is unavailable
- Recommend `C)` when the user has indicated they want to continue later
- Never recommend `D)` by default

### Step 4: Execute Choice

#### Option A: Merge Locally

```bash
# Refresh the base branch first
git fetch origin <base-branch> --quiet || true

# Switch to base branch
git checkout <base-branch>

# Fast-forward to the latest merged remote state
git merge --ff-only "origin/<base-branch>" 2>/dev/null || git pull --ff-only origin <base-branch>

# Stop if the local base branch still does not match the merged remote state
REMOTE_BASE=$(git rev-parse "origin/<base-branch>" 2>/dev/null || echo "")
LOCAL_BASE=$(git rev-parse HEAD)
if [ -n "$REMOTE_BASE" ] && [ "$LOCAL_BASE" != "$REMOTE_BASE" ]; then
  echo "Base branch is not at the latest merged remote state. Stop and resolve that divergence before merging."
  exit 1
fi

# Merge feature branch
git merge <feature-branch>

# Verify tests on merged result before cleanup
<test command>

# If tests pass
git branch -d <feature-branch>
```

Then: Cleanup worktree (Step 5)

#### Option B: Push and Create PR

Use the exact `<base-branch>` resolved in Step 2. Do not redetect it during PR creation.

```bash
# Push branch
git push -u origin <feature-branch>

# Create PR
gh pr create --base "<base-branch>" --title "<title>" --body "$(cat <<'EOF'
## Summary
<2-3 bullets of what changed>

## Test Plan
- [ ] <verification steps>
EOF
)"
```

Then: Keep the branch and worktree for follow-up until the PR is merged.

#### Option C: Keep As-Is

Report: "Keeping branch <name>. Worktree preserved at <path>."

**Don't cleanup worktree.**

#### Option D: Discard

**Confirm first:**
```
This will permanently delete:
- Branch <name>
- All commits: <commit-list>
- Worktree at <path>

Type 'discard' to confirm.
```

Wait for exact confirmation.

If confirmed:
```bash
git checkout <base-branch>
git branch -D <feature-branch>
```

Then: Cleanup worktree (Step 5)

### Step 5: Cleanup Worktree

**For Options A and D:**

Locate the feature branch worktree by branch name, not the current branch:
```bash
FEATURE_WORKTREE=$(git worktree list --porcelain | awk '
  /^worktree / { wt=$2 }
  /^branch refs\/heads\/<feature-branch>$/ { print wt }
')
```

If found:
```bash
[ -n "$FEATURE_WORKTREE" ] && git worktree remove "$FEATURE_WORKTREE"
```

**For Option C:** Keep worktree.

### Step 6: Document Release Follow-Through

If the document-release step already ran in this flow, summarize the release-readiness result and continue. Do not offer a skip path here for workflow-routed work.

## Quick Reference

| Option | Merge | Push | Keep Worktree | Cleanup Branch |
|--------|-------|------|---------------|----------------|
| A. Merge locally | ✓ | - | - | ✓ |
| B. Create PR | - | ✓ | ✓ | - |
| C. Keep as-is | - | - | ✓ | - |
| D. Discard | - | - | - | ✓ (force) |

## Common Mistakes

**Skipping test verification**
- **Problem:** Merge broken code, create failing PR
- **Fix:** Always verify tests before offering options

**Open-ended questions**
- **Problem:** "What should I do next?" → ambiguous
- **Fix:** Present exactly 4 structured options

**Automatic worktree cleanup**
- **Problem:** Remove worktree when might need it (Option B, C)
- **Fix:** Only cleanup for Options A and D

**No confirmation for discard**
- **Problem:** Accidentally delete work
- **Fix:** Require typed "discard" confirmation

## Red Flags

**Never:**
- Proceed with failing tests
- Merge without verifying tests on result
- Delete work without confirmation
- Force-push without explicit request

**Always:**
- Verify tests before offering options
- Present exactly 4 options
- Get typed confirmation for Option D
- Clean up worktree for Options A & D only

## Integration

**Called by:**
- **subagent-driven-development** - After the final review passes and all tasks are complete
- **executing-plans** - After the final review is resolved and all tasks are complete

**Pairs with:**
- **qa-only** - Conditional pre-landing browser QA when the branch change surface or test-plan artifact warrants it
- **document-release** - Required release-readiness pass for workflow-routed work before completion
- **using-git-worktrees** - Optional cleanup for a worktree created by that skill
