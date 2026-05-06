---
name: document-release
description: Use when implementation is complete and release notes, changelog, TODO, or handoff documentation need a release-quality pass before merge
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


# Document Release

Audit and update project documentation after implementation work is complete. This skill is mostly automatic for factual corrections and conservative everywhere else.

For workflow-routed implementation work, this is the required `document-release` handoff before branch completion. Treat it as the repo-facing release-readiness pass, not as an optional polish step.

For workflow-routed terminal sequencing, `featureforge:document-release` must run before the terminal `featureforge:requesting-code-review` whole-diff gate.

`featureforge:document-release` does not replace checkpoint reviews and does not own review-dispatch minting. Keep command-boundary semantics explicit: low-level compatibility/debug commands stay out of the normal-path flow.

When you need explicit late-stage phase/action/skill grounding while updating docs, cite `review/late-stage-precedence-reference.md`.

Extended audit examples live in `$_FEATUREFORGE_ROOT/references/execution-review-qa-examples.md`.

## Step 0: Require base branch context

For workflow-routed work, get `BASE_BRANCH` from `$_FEATUREFORGE_BIN workflow operator --json` (`base_branch`) for the concrete approved plan path; any `<approved-plan-path>` command text here is input shape, not exact argv.

For non-workflow work, require `BASE_BRANCH` explicitly and keep it stable for this run:

```bash
if [ -z "$BASE_BRANCH" ]; then
  echo "Missing BASE_BRANCH. Set it explicitly before writing release-facing docs."
  exit 1
fi
git fetch origin "$BASE_BRANCH" --quiet 2>/dev/null || true
```

Do not use PR metadata or repo default-branch APIs as a fallback.

## Optional Project Memory Follow-Up

- When the release pass surfaces durable knowledge worth preserving, use `featureforge:project-memory` to update the relevant `docs/project_notes/` file.
- Keep this follow-up narrow: record durable bugs in `docs/project_notes/bugs.md`, durable decisions in `docs/project_notes/decisions.md`, stable key facts in `docs/project_notes/key_facts.md`, or breadcrumb issues in `docs/project_notes/issues.md` only when the implementation or release pass surfaced knowledge worth reusing later.

## Protected-Branch Repo-Write Gate

Before editing release-facing docs or metadata on disk, run the shared repo-safety preflight for the exact release-write scope:

```bash
$_FEATUREFORGE_BIN repo-safety check --intent write --stage featureforge:document-release --task-id <current-release-doc-pass> --path <release-doc-path> --write-target release-doc-write
```

- If the helper returns `allowed`, continue with the doc or metadata write.
- If it returns `blocked`, name the branch, the stage, and the blocking `failure_class`, then route to either a feature branch / `featureforge:using-git-worktrees` or explicit user approval for this exact release-doc scope.
- If the user explicitly approves the protected-branch release write, approve the full release-doc scope you intend to use on that branch, including the release-doc path:

```bash
$_FEATUREFORGE_BIN repo-safety approve --stage featureforge:document-release --task-id <current-release-doc-pass> --reason "<explicit user approval>" --path <release-doc-path> --write-target release-doc-write
$_FEATUREFORGE_BIN repo-safety check --intent write --stage featureforge:document-release --task-id <current-release-doc-pass> --path <release-doc-path> --write-target release-doc-write
```

- Continue only if the re-check returns `allowed`.
- If the protected-branch task scope changes, run a new `approve` plus full-scope `check` before continuing.
- This skill may edit docs or metadata, but it does not own `git commit`, `git merge`, or `git push`; leave branch-integration actions to the next workflow stage.

## Step 1: Pre-flight and diff analysis

Run repo-appropriate commands such as:

```bash
git diff "origin/$BASE_BRANCH...HEAD" --stat 2>/dev/null || git diff "$BASE_BRANCH...HEAD" --stat 2>/dev/null || git diff --stat
git diff "origin/$BASE_BRANCH...HEAD" --name-only 2>/dev/null || git diff "$BASE_BRANCH...HEAD" --name-only || git diff --name-only
```

Classify the diff into:
- New features or new public workflows
- Changed behavior
- Removed functionality
- Infrastructure or contributor workflow changes

## Step 2: Per-file documentation audit

Read relevant docs, classify safe factual corrections as `Auto-update`, and mark risky narrative, security, architecture, versioning, or TODO changes as `Ask user`.

## Step 3: Apply safe factual updates

Make clear factual updates directly and report one line per changed file. Do not silently rewrite positioning, philosophy, or security promises.

## Step 4: Ask about risky changes

For each risky update, use one interactive user question naming the file, decision, recommendation, and skip option. Apply approved changes immediately.

## Step 5: CHANGELOG or release-notes voice polish

**CRITICAL — NEVER CLOBBER CHANGELOG ENTRIES**

This step polishes voice. It does not replace history.

If the repo keeps release history in `CHANGELOG.md`, use that file. Otherwise, use the equivalent release-notes file (for example `RELEASE-NOTES.md`) for this step.

Rules: read the full file first, preserve entries and ordering, polish only the current entry without changing meaning, and ask before fixing apparently wrong or incomplete entries.

If the diff does not touch the current release-history file, skip this step.

## Step 6: Cross-doc consistency and discoverability

After auditing files individually, do one discoverability pass: README/install workflow consistency, release-history/version consistency, and links from README or contributor docs to important docs.

If a doc exists but nothing links to it, flag it as a discoverability issue and make the smallest safe fix.

## Step 6.5: Release-readiness pass

Run an explicit release-readiness pass before finishing: refreshed docs for changed behavior/workflows, release-history updates for user/operator-visible behavior, rollout notes, non-trivial rollback notes, known risks or operator-facing caveats, and monitoring or verification expectations.

If any of these are materially missing, stop and fix them or ask the user before calling the branch ready to finish.

## Step 7: TODOS.md cleanup

If `TODOS.md` exists:
- Mark obviously completed items when the diff closes them
- Add new follow-up items only when they are concrete and justified by the diff
- Ask the user before large reorganizations or subjective reprioritization

## Step 7.5: Structured Release-Readiness Companion Artifact

For workflow-routed implementation work, runtime emits a project-scoped release-readiness companion artifact:

- Treat `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path> --result ready|blocked --summary-file <release-summary>` as an input shape only; substitute concrete plan, result, and existing summary file, or invoke `recommended_public_command_argv` exactly when present.
- Derive `Source Plan` and `Source Plan Revision` from that exact approved plan; do not leave placeholders or guess from prose.
- If the approved plan path or revision is unavailable, stop and return to the current workflow instead of writing a partial artifact.
- Use the runtime-provided base branch from Step 0 exactly as written; do not substitute a different branch name when persisting the artifact.
- This markdown output is a derived operator handoff companion. Runtime-owned reviewed-closure and late-stage milestone records remain the authoritative gate-truth surface.

Inspect runtime-emitted artifact locations after recording only when needed; do not hand-write companion artifacts.

For workflow-routed release-readiness, the runtime writes the derived companion artifact to:
- `$_FEATUREFORGE_STATE_DIR/projects/$SLUG/featureforge-{safe-branch}-release-readiness-{datetime}.md`

Do not hand-write or edit this artifact. Provide the release summary to the runtime-owned `advance-late-stage` command and let the runtime render the derived markdown projection.

Derived artifact minimum fields under `# Release Readiness Result` include:
- `**Current Reviewed Branch State ID:** git_tree:abc1234`
- `**Branch Closure ID:** branch-release-closure`
- `**Result:** pass`
Allowed `**Result:**` values:
- `pass`
- `blocked`
Artifact `pass` is the runtime-rendered form of CLI input `--result ready`.

## Step 7.6: Runtime-Owned Release-Readiness Recording (Workflow-Routed)

For workflow-routed implementation work, the derived companion artifact above is not the release gate itself.

- workflow-routed release-readiness must be recorded through runtime-owned commands, not inferred from the companion markdown artifact alone.
- For reviewed-closure late-stage routing, use the workflow/operator input shape `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` with the concrete plan; workflow/operator remains authoritative for `phase`, `phase_detail`, `next_action`, and `recommended_public_command_argv`. `recommended_command` is display-only compatibility text.
- If `recommended_public_command_argv` is present, invoke it exactly. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic; otherwise satisfy typed `required_inputs` or the prerequisite named by `next_action`, then rerun workflow/operator.
- Confirm the current `phase_detail` before recording release-readiness.
- If workflow/operator reports `phase_detail=branch_closure_recording_required_for_release_readiness`, use input shape `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path>` with the concrete plan and rerun workflow/operator.
- When workflow/operator reports `phase_detail=release_readiness_recording_ready`, use input shape `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path> --result ready|blocked --summary-file <release-summary>` only after substituting concrete values.
- When workflow/operator reports `phase_detail=release_blocker_resolution_required`, resolve the blocker and then use that same concrete release-readiness input shape.
- The `advance-late-stage --result ready|blocked` input shape renders `**Result:** pass|blocked` in the derived companion artifact; do not rewrite the artifact to mirror the command input.
- If workflow/operator reports any other phase or phase_detail, stop and return to the current workflow flow instead of forcing release-readiness recording from stale assumptions.

Example runtime-owned path after substituting concrete values:

```bash
OPERATOR_JSON=$("$_FEATUREFORGE_BIN" workflow operator --plan "$APPROVED_PLAN_PATH" --json)
PHASE_DETAIL=$(printf '%s\n' "$OPERATOR_JSON" | node -e 'const fs = require("fs"); const parsed = JSON.parse(fs.readFileSync(0, "utf8")); process.stdout.write(parsed.phase_detail || "")')
if [ "$PHASE_DETAIL" = "branch_closure_recording_required_for_release_readiness" ]; then
  "$_FEATUREFORGE_BIN" plan execution advance-late-stage --plan "$APPROVED_PLAN_PATH"
  OPERATOR_JSON=$("$_FEATUREFORGE_BIN" workflow operator --plan "$APPROVED_PLAN_PATH" --json)
  PHASE_DETAIL=$(printf '%s\n' "$OPERATOR_JSON" | node -e 'const fs = require("fs"); const parsed = JSON.parse(fs.readFileSync(0, "utf8")); process.stdout.write(parsed.phase_detail || "")')
fi
if [ "$PHASE_DETAIL" != "release_readiness_recording_ready" ] && [ "$PHASE_DETAIL" != "release_blocker_resolution_required" ]; then
  echo "Stop and return to workflow: release-readiness recording is not currently routable."
  exit 1
fi
"$_FEATUREFORGE_BIN" plan execution advance-late-stage --plan "$APPROVED_PLAN_PATH" --result ready --summary-file release-summary.md
```

## Output

Report files audited, files changed, risky changes deferred, and any remaining discoverability, VERSION, or TODO questions. If documentation still looks stale after the safe pass, say so explicitly.
