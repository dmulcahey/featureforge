# Branch-Closure Recording On Current Reviewed Branch State

**Workflow State:** Implementation Target  
**Spec Revision:** 1  
**Last Reviewed By:** clean-context review loop  
**Implementation Target:** Current

## Problem Statement

`record-branch-closure` is already a required prerequisite throughout the active April corpus, but it still lacks a dedicated command contract.

That leaves implementers guessing about:

- accepted CLI input shape
- idempotent re-run behavior
- blocked-path semantics
- what the authoritative branch-closure record contains
- how branch closure becomes stale or historical
- what downstream commands are allowed to assume after branch closure succeeds

That is too much implicit contract surface for a command that gates all late-stage progression.

## Desired Outcome

Branch closure should be recordable through one explicit public runtime command that records the authoritative current reviewed branch closure and returns deterministic `recorded`/`already_current`/`blocked` results.

After this work:

- `record-branch-closure` is a first-class documented runtime command
- the authoritative truth is a runtime-owned branch closure record
- release-readiness and final review can depend on that record without guessing
- re-running the command against the same still-current reviewed state is idempotent
- later reviewed work or unreviewed branch edits first move older branch closures to `superseded` or `stale_unreviewed`; older non-current closures may later be retained as `historical` through the normal reviewed-state model

## Decision

Selected approach: add a first-class runtime-owned branch-closure recording contract centered on the current reviewed branch state.

## Dependency

This spec depends on:

- `2026-04-01-supersession-aware-review-identity.md`
- `2026-04-01-gate-diagnostics-and-runtime-semantics.md`
- `2026-04-01-workflow-public-phase-contract.md`

## Requirement Index

- [REQ-001][behavior] FeatureForge must provide a public `featureforge plan execution record-branch-closure --plan <path>` command.
- [REQ-002][behavior] The authoritative result must be a runtime-owned branch closure record, not a hand-authored markdown artifact.
- [REQ-003][behavior] The branch closure record must bind at least plan path/revision, repo slug, branch identity, base branch where applicable, current reviewed state id, reviewed surface, source task-closure lineage, closure status, and closure lineage.
- [REQ-004][behavior] `record-branch-closure` must fail closed unless task-level execution blockers are resolved and no active task prevents terminal-stage progression.
- [REQ-005][behavior] Re-running `record-branch-closure` for the same still-current reviewed branch state must be safe and idempotent.
- [REQ-006][behavior] Command output must clearly report whether the result was `recorded`, `already_current`, or `blocked`.
- [REQ-007][behavior] If the workspace later moves without a new reviewed branch state, the branch closure must become `stale_unreviewed` through normal review-state evaluation; the runtime must not silently refresh the old closure in place.
- [REQ-008][behavior] Later reviewed branch work may supersede older branch closures through lineage, but supersession must not be represented as silent overwrite.
- [REQ-009][behavior] `record-branch-closure` must be owned by a dedicated branch-closure recording service and must not embed second-copy workflow routing or milestone policy.
- [REQ-010][verification] Integration tests must prove normal branch-closure recording, idempotent re-run, blocked validation before mutation, and stale/superseded branch-closure behavior.
- [REQ-011][behavior] `record-branch-closure` may recreate a current reviewed branch state after repo-tracked late-stage edits only when those edits are confined to the trusted late-stage declared surface. In the first slice, that trusted source is normalized approved-plan metadata field `Late-Stage Surface`; omission means the trusted late-stage declared surface is empty.
- [REQ-012][behavior] `record-branch-closure` must use the shared `Late-Stage Surface` normalization and matching contract defined by `2026-04-01-supersession-aware-review-identity.md`; command-local path matching heuristics are not allowed.

## Scope

In scope:

- public branch-closure recording command
- runtime-owned branch-closure records
- idempotent re-run semantics
- blocked-path semantics
- stale and historical branch-closure behavior

Out of scope:

- release-readiness, final-review, or QA milestone recording semantics themselves
- weakening late-stage fail-closed behavior

## Selected Approach

Add:

- `featureforge plan execution record-branch-closure --plan <path>`

`record-branch-closure` must:

1. query authoritative review-state truth
2. fail closed unless the branch is terminal-clean enough for branch closure recording
3. resolve current reviewed state id and effective reviewed branch surface
4. delegate to a dedicated `BranchClosureService`
5. append one authoritative branch closure record when needed
6. return a structured result naming the current branch closure id and any superseded branch closure ids

Chosen branch-closure recreation rule:

- if repo-tracked edits after the authoritative branch-closure baseline for this attempt are confined to the trusted late-stage declared surface, `record-branch-closure` is allowed to create the new current reviewed branch state for late-stage progression without reopening task closure
- if those edits escape the trusted late-stage declared surface or task-level closure truth is otherwise stale, `record-branch-closure` must fail closed and return `required_follow_up=repair_review_state` instead
- this first-slice recreation path is an explicit policy exemption from new branch-scope review; provenance for the recreated current branch closure comes from `source_task_closure_ids[]` plus the approved `Late-Stage Surface`, not from a synthetic branch-level review milestone
- recreated late-stage branch closures must encode `provenance_basis=task_closure_lineage_plus_late_stage_surface_exemption`
- `source_task_closure_ids[]` must include every still-current task closure whose effective reviewed surface overlaps the recreated branch surface outside late-stage-only paths; it may be empty only when the recreated branch surface is covered solely by the approved `Late-Stage Surface`

Chosen branch-closure baseline rule:

1. on first entry into late stages, the authoritative branch-closure baseline is the still-current task-closure set the runtime trusts for the branch
2. after a branch closure already exists, the authoritative branch-closure baseline is that still-current branch closure’s reviewed state
3. if no still-current branch closure exists because the prior branch closure is already `stale_unreviewed` or purely `historical`, that stale or historical branch closure is not a valid baseline; the authoritative baseline falls back to the still-current task-closure set
4. the late-stage-only exemption compares repo-tracked drift against that authoritative baseline for the current attempt; the runtime must not switch between baselines heuristically

Chosen branch-closure readiness rule:

1. `no active task prevents terminal-stage progression` means no task on the plan remains in ordinary execution, pending task-review dispatch, waiting on task-review result, or waiting on task-closure recording
2. `task-level closure blockers are resolved` means there is no outstanding task-scope `stale_unreviewed`, failed-review follow-up, handoff follow-up, or pivot follow-up preventing the branch from leaving task execution
3. `current reviewed state is usable for branch-scope closure recording` means authoritative query state can derive one unambiguous reviewed branch state and reviewed surface from the current task-closure set plus any permitted late-stage declared surface
4. if any of those predicates are not true, branch-closure recording must fail closed before mutation

## Public Contract

`record-branch-closure` must accept at least:

- `--plan <path>`

It must fail closed unless all of these are true:

1. `phase=document_release_pending`
2. either `phase_detail=branch_closure_recording_required_for_release_readiness`, or a current branch closure already exists for the same still-current reviewed state and the command can return `action=already_current` without mutation
3. no active task prevents terminal-stage progression
4. task-level closure blockers are resolved for the current reviewed state
5. the current reviewed state is usable for branch-scope closure recording
6. any repo-tracked drift since the authoritative branch-closure baseline for this attempt is either absent or confined to the trusted late-stage declared surface

The authoritative branch-closure output contains at least:

- `branch_closure_id`
- plan path and revision
- `contract_identity`
- repo slug
- branch identity
- base branch where applicable
- current reviewed state id
- effective reviewed branch surface
- `source_task_closure_ids[]`
- `provenance_basis`
- closure status
- superseded branch closure ids
- required follow-up if blocked

Here, `contract_identity` means the branch-scope contract identity defined by `2026-04-01-supersession-aware-review-identity.md`.

`provenance_basis` selection rules:

- `task_closure_lineage` when the current reviewed branch state is fully justified by still-current task closures and no trusted late-stage-only repo-tracked edits had to be absorbed
- `task_closure_lineage_plus_late_stage_surface_exemption` when the runtime recreated the current reviewed branch state by absorbing repo-tracked edits confined to approved `Late-Stage Surface`

`source_task_closure_ids[]` selection rules:

1. include every still-current task closure whose effective reviewed surface overlaps the resulting reviewed branch surface outside late-stage-only paths
2. exclude superseded, stale-unreviewed, or purely historical task closures
3. permit an empty list only when the recreated reviewed branch surface is covered solely by approved `Late-Stage Surface` and no still-current task closure contributes non-exempt reviewed surface to that recreated state
4. fail closed if the runtime cannot determine this lineage set unambiguously from authoritative closure/query state

Repo/branch/base-branch bindings must come from the runtime-owned `RepositoryContextResolver`, not from duplicated per-command parsing or hand-authored input.

Branch closures do not synthesize branch-scope review or verification milestones in the first slice. Their review provenance is inherited through `source_task_closure_ids[]` plus the bound current reviewed branch state.

## Return Contract

`record-branch-closure` must return at least:

- `action`: `recorded` | `already_current` | `blocked`
- `branch_closure_id` when current
- `superseded_branch_closure_ids[]`
- `required_follow_up` when blocked
- `trace[]` or `trace_summary`

For `record-branch-closure`, blocked follow-up values are:

- `repair_review_state`

If the direct command is invoked out of phase and the exact next safe step is not deterministically one of those blocked follow-ups, the runtime must return the shared out-of-phase response contract defined by `2026-04-01-gate-diagnostics-and-runtime-semantics.md`.

`already_current` means:

- the requested plan/scope already has a current branch closure bound to the same still-current reviewed branch state and the same resolved branch `contract_identity`
- no new branch closure record was appended
- downstream late-stage milestone commands may reuse the returned `branch_closure_id`

## Concrete Examples

### Example 1: Normal Branch Closure Recording

Scenario:

- all task closures are current
- no active task remains
- the workflow is entering late-stage branch work

Expected result:

- `record-branch-closure` records one current branch closure
- the command returns `action=recorded`
- release-readiness may now bind to the returned branch closure id
- final review may bind only after a current release-readiness result `ready` is also recorded for that same branch closure

### Example 2: Idempotent Re-Run

Scenario:

- a current branch closure already exists for the same still-current reviewed branch state
- the operator reruns the command

Expected result:

- no duplicate branch closure is appended
- the command returns `action=already_current`
- the same branch closure id remains authoritative

### Example 3: Repo Changes After Branch Closure

Scenario:

- branch closure was current for reviewed state `B1`
- release notes or other repo-tracked content changed, producing workspace state `B2`

Expected result:

- the older branch closure becomes `stale_unreviewed`
- if the changed files are inside the trusted `Late-Stage Surface`, `repair-review-state` may route directly back to `record-branch-closure`
- otherwise later late-stage recording commands fail closed until execution reentry produces new reviewed task or branch state

## Acceptance Criteria

1. Branch closure can be recorded through one public CLI command with no direct artifact editing.
2. Idempotent re-run behavior is explicit and testable.
3. Downstream late-stage specs can depend on this command without inventing its semantics.
4. Blocked validation fails before mutation.
5. Stale and historical branch-closure behavior follows the reviewed-state model instead of silent rewrite.

## Test Strategy

- add a CLI-only happy-path test for `record-branch-closure`
- add an idempotent re-run test that proves `action=already_current`
- add a blocked-path test that proves validation fails before mutation
- add a stale-unreviewed branch test after repo-tracked edits
- add a superseded branch-closure test when later reviewed branch state replaces the earlier current branch closure
- add a provenance test proving branch closures bind `source_task_closure_ids[]` instead of inventing synthetic branch-scope review milestones
- add metadata-normalization tests for `Late-Stage Surface`, including invalid entries, file-versus-directory matching, and case-sensitive path behavior

## Risks

- leaving branch closure implicit would keep late-stage implementation split across guesswork and copy-pasted assumptions
- allowing milestone commands to infer branch closure implicitly would reintroduce hidden mutation and state drift
