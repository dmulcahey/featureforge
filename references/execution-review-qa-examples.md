# Execution, Review, QA, and Finish Examples

This companion keeps examples and rationale out of top-level skill prompts. It is not routing authority. When it disagrees with a top-level skill or `featureforge workflow operator --plan <approved-plan-path>`, follow the top-level skill and workflow/operator.

## Subagent Orchestration Examples

Use fresh isolated agents for implementation and review tasks when the runtime-selected topology supports that approach. Give each child the helper-built task packet verbatim, plus only transient logistics such as branch, working directory, and base commit.

Typical role mapping:
- Implementer: built-in `worker`
- Spec reviewer: built-in `explorer` for read-heavy checks or `default` for broader judgment
- Code-quality reviewer: installed `code-reviewer` custom agent when available
- Custom agent: only when the built-ins do not fit the task

Implementer status handling:
- `DONE`: proceed to spec review.
- `DONE_WITH_CONCERNS`: read concerns and resolve correctness or scope doubts before review.
- `NEEDS_CONTEXT`: answer from the packet, or stop if the packet leaves the task ambiguous.
- `BLOCKED`: change context, model, task size, or escalate the plan issue instead of retrying blindly.

Mini workflow:

```text
Task packet -> implementer -> spec review -> code quality review -> verification -> close-current-task
Repeat until no tasks remain -> document-release -> requesting-code-review -> workflow operator
If workflow operator routes QA, run qa-only -> advance-late-stage for qa_recording_required -> workflow operator
When workflow operator reports branch completion ready -> finishing-a-development-branch
```

## Deterministic Finding Examples

Use field-specific findings instead of broad advice. Keep each `Required Fix` independently executable so repair does not require reinterpretation.

```text
### Finding TASK2_DONE_WHEN_2_PROGRESS_REPORTING

**Finding ID:** TASK2_DONE_WHEN_2_PROGRESS_REPORTING
**Severity:** critical
**Task:** Task 2
**Violated Field or Obligation:** DONE_WHEN_2
**Evidence:** The task packet requires progress indicators at the approved interval, but the diff only updates initialization.
**Required Fix:** Add progress reporting at the packet-required interval.
**Hard Fail:** yes

### Finding TASK2_SCOPE_EXTRA_JSON_FLAG

**Finding ID:** TASK2_SCOPE_EXTRA_JSON_FLAG
**Severity:** critical
**Task:** Task 2
**Violated Field or Obligation:** PLAN_DEVIATION_FOUND
**Evidence:** The task packet did not approve a new JSON output mode, but the diff adds an unrequested `--json` flag.
**Required Fix:** Remove the unrequested `--json` flag from the Task 2 diff or route the scope expansion back through plan approval.
**Hard Fail:** yes

### Finding TASK2_PROGRESS_INTERVAL_CONSTANT

**Finding ID:** TASK2_PROGRESS_INTERVAL_CONSTANT
**Severity:** important
**Task:** Task 2
**Violated Field or Obligation:** CONSTRAINT_3
**Evidence:** The approved packet requires the progress interval to come from shared configuration, but the diff hard-codes `100`.
**Required Fix:** Replace the hard-coded interval with the shared configuration value required by `CONSTRAINT_3`.
**Hard Fail:** yes

### Finding FINAL_REVIEW_PROGRESS_INDICATORS

**Finding ID:** FINAL_REVIEW_PROGRESS_INDICATORS
**Severity:** important
**Task:** Task 4
**Violated Field or Obligation:** DONE_WHEN_4
**Evidence:** Final review shows no user-visible progress indicators for the long-running operation covered by Task 4's `DONE_WHEN_4` obligation.
**Required Fix:** Add the approved progress indicator behavior and verify it through the plan's final validation path.
**Hard Fail:** yes
```

## Branch Finish Option Commands

For local merge, refresh the target branch, merge the feature branch, verify the merged result, then delete the feature branch only after verification passes.

Local merge example:

```bash
git fetch origin <base-branch>
git switch <base-branch>
git pull --ff-only origin <base-branch>
git merge --no-ff <feature-branch>
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-targets --all-features --no-fail-fast
git branch -d <feature-branch>
```

For PR flow, use the base branch chosen earlier in the finish flow:

```bash
git push -u origin <feature-branch>
gh pr create --base "<base-branch>" --title "<title>" --body "$(cat <<'EOF'
## Summary
<2-3 bullets of what changed>

## Test Plan
- [ ] <verification steps>
EOF
)"
```

Keep-as-is means leave the branch and worktree untouched, then report the branch name, base branch, and latest verification state. PR-created branches also keep their worktree unless the user explicitly chooses cleanup after the PR exists.

Discard example after typed confirmation:

```bash
git switch <base-branch>
git branch -D <feature-branch>
```

Only delete a remote branch when the typed confirmation explicitly names remote branch deletion.

Worktree cleanup example after the branch is merged, explicitly discarded, or the user explicitly chooses cleanup:

```bash
git worktree list
git worktree remove <worktree-path>
git worktree prune
```

For discard or worktree cleanup that deletes local work, require typed confirmation before deleting commits or worktrees.

## Document Release Audit Hints

Audit README, architecture, contributing/install docs, workflow docs, release notes, and TODO files against the diff. Treat factual corrections as safe, and ask before large narrative rewrites, security or architecture promise changes, ambiguous versioning, large removals, or new subjective TODO priorities.

Release-readiness summaries should cover documentation freshness, release-history updates, rollout, rollback, known risks, and verification or monitoring expectations when relevant.

## QA Exploration Hints

Use browser automation to orient, exercise affected flows, capture evidence, and score health. Prioritize routes and risks from a current branch test-plan artifact when available, then user scope, then diff-aware file changes. Every reported issue needs severity, category, URL or route, repro steps, and evidence.

Suggested health categories:
- Console
- Links
- Visual
- Functional
- UX
- Performance
- Accessibility
