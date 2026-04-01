# First-Class `plan-fidelity-review`

**Workflow State:** CEO Approved
**Spec Revision:** 5
**Last Reviewed By:** plan-ceo-review

## Problem Statement

FeatureForge already enforces a runtime-owned plan-fidelity gate in code, but the workflow still treats that gate like a hidden implementation detail instead of a first-class public stage.

Today the repository has all of the following at the same time:

- runtime and contract code define `featureforge:plan-fidelity-review` as a real stage
- `writing-plans` says a dedicated independent reviewer must run before `plan-eng-review`
- `plan-eng-review` refuses to start without a matching passing plan-fidelity receipt
- public workflow docs still skip the stage entirely
- `using-featureforge` still routes many draft-plan cases back to `writing-plans` instead of to a dedicated fidelity reviewer
- there is no first-class public skill directory for `skills/plan-fidelity-review/`

That leaves the workflow in an awkward half-state: the runtime knows this stage is real, but the human-facing skill layer does not expose it cleanly. The result is avoidable friction, ambiguous routing, and repeated bounce-back into `writing-plans` even when the plan itself is ready and only needs an independent fidelity check.

The user also wants the dedicated reviewer to be an **independent fresh-context subagent**. That requirement should be made explicit in the skill contract rather than left implicit in runtime receipt validation.

## Desired Outcome

After this slice lands:

- `featureforge:plan-fidelity-review` is a first-class public workflow stage with its own checked-in skill directory
- the canonical workflow shown in public docs is:
  - `brainstorming -> plan-ceo-review -> writing-plans -> plan-fidelity-review -> plan-eng-review -> execution`
- `writing-plans` hands off to `plan-fidelity-review` explicitly instead of describing a hidden reviewer that has no first-class surface
- `using-featureforge`, workflow routing, and operator guidance direct draft plans with missing/stale/non-pass fidelity receipts to `plan-fidelity-review` instead of reflexively bouncing back to `writing-plans`
- the skill always uses an **independent fresh-context subagent** to perform the review
- the skill writes the existing plan-fidelity review artifact shape and records the runtime-owned receipt through the existing CLI
- `plan-eng-review` remains blocked unless the receipt is current and passing, but “missing fidelity receipt” becomes a routing problem for `plan-fidelity-review`, not a pseudo-editing problem for `writing-plans`
- the workflow becomes easier to follow because every required stage is visible, named, documented, and directly invokable

## Scope

In scope:

- a new first-class public skill for `featureforge:plan-fidelity-review`
- explicit fresh-context-subagent instructions and prompt material for that skill
- routing changes in docs and runtime so draft plans missing a valid fidelity receipt route to the new stage
- updates to `writing-plans` and `plan-eng-review` so their handoff language matches the new public stage
- public README and install/readme workflow documentation updates
- preservation of direct artifact-state routing while introducing `plan-fidelity-review` as a first-class stage
- deterministic tests that prove the stage exists, is routed correctly, and requires fresh-context reviewer provenance

Out of scope:

- changing the receipt schema itself beyond what is needed for clearer routing and documentation
- redesigning engineering review, final review, or execution topology rules
- broadening this slice into a more general prompt-surface rewrite
- removing low-level runtime support for non-canonical provenance values unless required by a later cleanup slice

## Requirement Index

- [REQ-001][behavior] FeatureForge must ship a first-class public skill at `skills/plan-fidelity-review/` with a generated `SKILL.md` derived from a checked-in template.
- [REQ-002][behavior] The canonical public workflow sequence must explicitly include `plan-fidelity-review` between `writing-plans` and `plan-eng-review` in root docs, Codex docs, Copilot docs, and workflow-facing skill docs.
- [REQ-003][behavior] The `plan-fidelity-review` skill must require an **independent fresh-context subagent** to perform the review. The canonical emitted review artifact must record `Reviewer Source: fresh-context-subagent`.
- [REQ-004][behavior] The `plan-fidelity-review` skill must be a terminal verification gate for the current draft plan revision. It may not silently rewrite the plan. If the review fails, the workflow must route back to `writing-plans`; if it passes and the receipt records successfully, the workflow must advance to `plan-eng-review`.
- [REQ-005][behavior] `writing-plans` must hand off to `featureforge:plan-fidelity-review` after the draft plan is written, linted, and saved, instead of treating the fidelity review as a hidden sub-step with no first-class stage.
- [REQ-006][behavior] Workflow routing for a current draft plan must distinguish “plan needs editing” from “plan needs fidelity review”. Missing, stale, malformed, non-pass, or non-independent plan-fidelity receipt evidence must route to `featureforge:plan-fidelity-review` when the draft plan itself is otherwise the current candidate for the approved spec.
- [REQ-007][behavior] `plan-eng-review` must fail closed unless a matching pass plan-fidelity receipt exists, but when the only blocker is receipt state or freshness, its user-facing remediation must point to `featureforge:plan-fidelity-review` rather than to `featureforge:writing-plans`.
- [REQ-008][behavior] The `plan-fidelity-review` skill must instruct the controller to record the runtime-owned receipt via `featureforge workflow plan-fidelity record --plan <path> --review-artifact <path>` after the fresh-context reviewer writes the artifact.
- [REQ-009][behavior] The skill must force the reviewer to verify, at minimum, exact Requirement Index coverage and execution-topology fidelity against the current approved spec and current draft plan revision.
- [REQ-010][behavior] Introducing `plan-fidelity-review` must preserve direct artifact-state routing: active docs, skills, runtime surfaces, and CLI guidance must not add unrelated pre-routing gate dependencies.
- [REQ-011][verification] Deterministic tests must prove that the new stage is publicly documented, routed by workflow status for missing/stale receipts, and backed by a first-class skill directory and fresh-context reviewer instructions.

## Decisions

- [DEC-001][decision] The canonical workflow reviewer source for first-class plan-fidelity review is `fresh-context-subagent`.
- [DEC-002][decision] Low-level runtime receipt validation may continue to accept `cross-model` for direct/manual compatibility, but no first-class workflow path, generated skill doc, or packaged reviewer prompt should recommend it in this slice.
- [DEC-003][decision] `plan-fidelity-review` is a verification stage, not a plan-authoring stage. It can require edits, but it does not own rewriting the plan.
- [DEC-004][decision] Route draft plans to `writing-plans` only when the plan itself is absent, stale against the approved spec, malformed, or contract-invalid in a way that requires author edits. Route them to `plan-fidelity-review` when the plan is the current draft candidate and the missing piece is the dedicated fidelity gate.

## Repo Reality Check

The repository already contains most of the low-level machinery this slice needs:

- `src/contracts/plan.rs` already defines `PLAN_FIDELITY_REVIEW_STAGE` as `featureforge:plan-fidelity-review`.
- `src/execution/topology.rs` already parses and validates the plan-fidelity review artifact, including the required review stage, reviewer provenance, verified surfaces, and exact requirement-id coverage.
- `src/workflow/status.rs` already computes the plan-fidelity gate and blocks `plan-eng-review` when the receipt is missing or stale.
- `src/cli/workflow.rs` and `src/lib.rs` already expose `workflow plan-fidelity record`.
- `skills/writing-plans/SKILL.md` already instructs the workflow to dispatch a dedicated independent reviewer and then record the receipt.
- `skills/plan-eng-review/SKILL.md` already fails closed on missing or invalid plan-fidelity receipts.

What is missing is the public stage itself and the corresponding routing cleanup:

- there is no `skills/plan-fidelity-review/` directory
- public workflow docs in `README.md`, `docs/README.codex.md`, and `docs/README.copilot.md` still omit the stage
- `skills/using-featureforge/SKILL.md` still routes draft plans with missing/stale receipt evidence to `featureforge:writing-plans`
- `src/workflow/status.rs` still reports `next_skill: featureforge:writing-plans` when the blocking issue is the plan-fidelity gate
- active runtime/docs/tests already route directly from artifact state; this slice must avoid adding unrelated pre-routing gate dependencies while making `plan-fidelity-review` first-class

That means the repo is already enforcing the gate, but not exposing it coherently.

## Proposed Design

### 1) Add a first-class public skill

Create a new skill directory:

- `skills/plan-fidelity-review/SKILL.md.tmpl`
- `skills/plan-fidelity-review/SKILL.md` (generated)
- `skills/plan-fidelity-review/reviewer-prompt.md`

This skill becomes the canonical human-facing entry point for the stage the runtime already knows about.

The skill should be written in the same style as the existing review-oriented skills:

- clear trigger conditions
- hard gate semantics
- exact artifact paths and header requirements
- explicit “do not freelance” constraints
- terminal-state instructions for both pass and fail outcomes

### 2) Make the skill explicitly subagent-backed

The core contract for the new skill should be:

1. locate the current approved spec and draft plan
2. verify the plan is the active draft candidate for that approved spec
3. dispatch a **fresh-context reviewer subagent** using `skills/plan-fidelity-review/reviewer-prompt.md`
4. provide that reviewer only the materials needed to judge fidelity:
   - current approved spec
   - current draft plan
   - any exact repo-local contract output needed to interpret requirement ids or topology claims
5. require the reviewer to write `.featureforge/reviews/<date>-<slug>-plan-fidelity.md`
6. record the runtime-owned receipt using `featureforge workflow plan-fidelity record`
7. if the artifact or receipt fails validation, stop and return to `writing-plans`
8. if the receipt records successfully, hand off to `plan-eng-review`

The skill must explicitly forbid self-review by the current plan-writing context.

### 3) Canonical reviewer prompt behavior

`skills/plan-fidelity-review/reviewer-prompt.md` should instruct the fresh-context reviewer to:

- review the approved spec from scratch, with special attention to the `Requirement Index`
- review the current draft plan from scratch, with special attention to:
  - requirement coverage
  - execution topology
  - dependency claims
  - write-scope and lane-separation claims, where present
- judge whether the plan is faithful to the approved spec rather than merely “reasonable”
- fail when the plan:
  - omits required work
  - introduces unsupported scope
  - weakens explicit requirements
  - contains execution-topology claims not grounded in the spec/plan contract
  - misstates requirement ids or verified coverage
- produce exactly one artifact in the existing parseable format
- use:
  - `Review Stage: featureforge:plan-fidelity-review`
  - `Review Verdict: pass` or `needs-changes`
  - `Reviewer Source: fresh-context-subagent`
  - a concrete `Reviewer ID`
  - `Distinct From Stages: featureforge:writing-plans, featureforge:plan-eng-review`
  - `Verified Surfaces: requirement_index, execution_topology`
  - `Verified Requirement IDs: ...exact ids...`

The prompt should explicitly say the reviewer is not allowed to rewrite the plan or negotiate scope. Its job is to verify fidelity and surface exact gaps.

### 4) Update workflow routing so the stage is real

#### `skills/using-featureforge/*`

Change the public draft-plan routing from:

- missing/stale/non-pass receipt -> `featureforge:writing-plans`

To:

- no relevant plan -> `featureforge:writing-plans`
- draft plan is stale, malformed, or no longer the right plan for the approved spec -> `featureforge:writing-plans`
- draft plan is current but missing/stale/malformed/non-pass/non-independent plan-fidelity receipt evidence -> `featureforge:plan-fidelity-review`
- draft plan has a matching pass plan-fidelity receipt -> `featureforge:plan-eng-review`

That change is critical. Without it, the new public skill exists but the workflow still pushes users into the wrong stage.

#### `src/workflow/status.rs`

Change the `plan.workflow_state == "Draft"` branch so the returned `next_skill` is context-sensitive:

- if the draft plan is the right candidate but `evaluate_plan_fidelity_gate(...)` is not pass, return `featureforge:plan-fidelity-review`
- reserve `featureforge:writing-plans` for cases where the plan itself needs author edits because of stale spec linkage, invalid contract state, packet/buildability issues that the planner owns, or absence of a usable draft plan

The current behavior always sends failed plan-fidelity gate cases to `featureforge:writing-plans`. That is the main routing bug this slice should remove.

#### `src/workflow/operator.rs` (conditional)

`src/workflow/operator.rs` already follows `route.next_skill` for primary public recommendation text. Treat operator edits as conditional:

- verify operator-facing recommendation/remediation text after `status.rs` routing is fixed
- only patch `operator.rs` if any residual diagnostics/remediation wording still points receipt-state blockers to `featureforge:writing-plans`

#### Direct routing contract

The active runtime routes directly from artifact state on the public path. This slice must keep that contract intact:

- do not add unrelated pre-routing gate dependencies on active surfaces
- keep generated and checked-in docs aligned with direct routing via `featureforge workflow status --refresh`
- guard these requirements with active contract tests

### 5) Update `writing-plans` to hand off cleanly

`skills/writing-plans/SKILL.md.tmpl` should stop treating the stage as a hidden reviewer step and instead hand off directly to the public stage.

The terminal wording should become something like:

- after the draft plan is saved and linted, invoke `featureforge:plan-fidelity-review`
- do not proceed directly to `featureforge:plan-eng-review`
- the dedicated fresh-context subagent review stage owns the fidelity gate and receipt recording

Keep the exact artifact and receipt requirements already present, but move ownership language to the new skill by reducing `writing-plans` to a pure handoff after plan write/lint and moving the numbered post-save review/receipt ownership block into `plan-fidelity-review`.

### 6) Update `plan-eng-review` remediation language

`skills/plan-eng-review/SKILL.md.tmpl` should keep the hard fail-closed rule, but sharpen the remediation path:

- if receipt missing/stale/malformed/non-pass/non-independent -> route to `featureforge:plan-fidelity-review`
- if the engineering review itself requires substantive plan edits -> route to `featureforge:writing-plans`, then back through `plan-fidelity-review`

That distinction matters. Missing a receipt is not the same as needing to rewrite the plan.

### 7) Update public workflow docs everywhere

At minimum update:

- `README.md`
- `docs/README.codex.md`
- `docs/README.copilot.md`
- `skills/using-featureforge/SKILL.md.tmpl`
- any generated contract tests that assert the sequence text

Every public workflow sequence should show:

`featureforge:brainstorming -> featureforge:plan-ceo-review -> featureforge:writing-plans -> featureforge:plan-fidelity-review -> featureforge:plan-eng-review -> execution`

or the non-prefixed equivalent where appropriate.

Normalize workflow-facing skill-doc examples to the same hyphenated stage spelling (`plan-fidelity-review`) and update matching contract tests that currently lock alternate spellings.

### 8) Keep the existing artifact contract; do not invent a second one

Do **not** create a new artifact schema for this slice.

Reuse the existing plan-fidelity review artifact and runtime-owned receipt flow already implemented in:

- `src/execution/topology.rs`
- `src/workflow/status.rs`
- `src/contracts/plan.rs`
- `src/cli/workflow.rs`

This slice is about making the stage first-class and routable, not about redesigning the artifact shape.

## File-Level Changes

### New files

- `skills/plan-fidelity-review/SKILL.md.tmpl`
- `skills/plan-fidelity-review/reviewer-prompt.md`
- `skills/plan-fidelity-review/SKILL.md` (generated)

### Changed docs and templates

- `README.md`
- `docs/README.codex.md`
- `docs/README.copilot.md`
- `skills/using-featureforge/SKILL.md.tmpl`
- `skills/writing-plans/SKILL.md.tmpl`
- `skills/plan-eng-review/SKILL.md.tmpl`
- regenerate checked-in `SKILL.md` files

### Changed runtime / routing code

- `src/workflow/status.rs`
- `src/workflow/operator.rs` (only if residual remediation text mismatch remains after `status.rs` routing fix)

### Changed tests

- `tests/runtime_instruction_plan_review_contracts.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/workflow_runtime.rs`
- `tests/cli_parse_boundary.rs`
- `tests/workflow_entry_shell_smoke.rs`
- `tests/using_featureforge_skill.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

## Verification Strategy

### Red tests first

1. **Skill existence / public surface**
   - fail if `skills/plan-fidelity-review/SKILL.md` is missing
   - fail if public workflow docs omit the stage

2. **Routing tests**
   - fail if `workflow status` returns `featureforge:writing-plans` for a current draft plan whose only blocker is missing/stale/non-pass fidelity receipt evidence
   - pass only when it returns `featureforge:plan-fidelity-review`

3. **Instruction contract tests**
   - fail if `writing-plans` still describes the fidelity gate as a hidden step instead of handing off to the first-class skill
   - fail if `plan-eng-review` remediation still points missing/stale receipt cases back to `writing-plans`
   - fail if `plan-fidelity-review` docs do not require `fresh-context-subagent`

4. **Direct-routing contract tests**
   - fail if active docs/skills/runtime surfaces add unrelated pre-routing gate dependencies
   - fail if active CLI surface regresses by reintroducing removed legacy bootstrap command dependencies

### Green-state validation

Minimum expected validation matrix:

```bash
cargo test --test runtime_instruction_plan_review_contracts
cargo test --test runtime_instruction_contracts
cargo test --test workflow_runtime
cargo test --test cli_parse_boundary
cargo test --test workflow_entry_shell_smoke
node --test tests/codex-runtime/skill-doc-contracts.test.mjs
```

Use the repo’s actual test entrypoints if the exact command names differ.

## Implementation Plan

### Phase 1 — add the skill surface

- create `skills/plan-fidelity-review/SKILL.md.tmpl`
- create `skills/plan-fidelity-review/reviewer-prompt.md`
- regenerate `skills/plan-fidelity-review/SKILL.md`
- update root/public docs to include the stage

### Phase 2 — fix routing and handoffs

- update `skills/using-featureforge/SKILL.md.tmpl`
- update `skills/writing-plans/SKILL.md.tmpl`
- update `skills/plan-eng-review/SKILL.md.tmpl`
- update `src/workflow/status.rs`
- validate `src/workflow/operator.rs` output and patch only if residual remediation text mismatch remains

### Phase 3 — lock the behavior with tests

- update instruction contract tests
- update workflow runtime route expectations
- add or update direct-routing contract regression tests
- update Codex skill-doc contract tests

## Acceptance Criteria

This slice is complete only when all of the following are true:

- `skills/plan-fidelity-review/` exists as a first-class public skill directory
- the new skill explicitly requires an **independent fresh-context subagent** reviewer
- public workflow docs show `plan-fidelity-review` as a real stage
- `writing-plans` hands off to `plan-fidelity-review`
- `workflow status` routes eligible draft plans with missing/stale/non-pass fidelity receipts to `featureforge:plan-fidelity-review`
- `plan-eng-review` still refuses to start without a matching pass receipt, but its remediation distinguishes receipt problems from plan-authoring problems
- no unrelated pre-routing gate dependencies are introduced while landing the first-class stage
- deterministic tests protect the stage’s existence, routing, and fresh-context reviewer requirement

## Non-Goals / Guardrails

- Do not turn `plan-fidelity-review` into another authoring stage.
- Do not allow the plan writer to count as the reviewer.
- Do not introduce a second artifact format for fidelity review.
- Do not leave public docs showing a workflow sequence that differs from runtime routing.
- Do not keep routing missing/stale receipt cases to `writing-plans` once the first-class stage exists.

## System Audit Snapshot

- Repo: `featureforge` in worktree `/Users/davidmulcahey/.codex/worktrees/f24b/featureforge`
- Branch context: `codex/plan-fidelity-review` (tracking `origin/codex/plan-fidelity-review`), base branch `main`
- In-flight local state before this spec import: no local staged/unstaged diff and no stashes
- Recent history relevant to this slice: branch already hardened per-task review gates and write-target naming parity; this spec touches the same trust-boundary/routing surfaces and must keep that rigor
- Existing open TODO pressure linked to this scope:
  - enforce "no new runtime deps in generated skill/runtime flows"
  - keep independent fresh-context reviewer requirements contractual, not optional
- UI scope detection: `UI_SCOPE = no` (no new screens/components/user-visible interaction changes)

## Landscape Snapshot

### Layer 1: Existing repo-native solutions

- Runtime already has a concrete stage constant (`PLAN_FIDELITY_REVIEW_STAGE`) and receipt validators.
- `workflow status` already computes a plan-fidelity gate and reason codes.
- `workflow plan-fidelity record` already persists runtime-owned receipts.

### Layer 2: Current footguns observed in this repo

- Public docs/skills drift from runtime routing truth.
- Stage ownership is implicit in `writing-plans`, creating ambiguous remediation loops.
- Legacy bootstrap assumptions can drift back into docs/specs and pollute direct routing guidance.

### Layer 3: First-principles conclusion

This is a surfacing and routing coherence problem, not a schema problem. The highest-leverage fix is to make `plan-fidelity-review` first-class across docs, skill routing, and status/operator remediation while preserving existing runtime-owned artifact contracts.

### Decision impact

- Keep receipt schema stable.
- Introduce first-class stage surface and route intent to that stage.
- Preserve fail-closed behavior at `plan-eng-review`.

## Step 0 Review Mode (Resolved)

- Selected mode: `selective_expansion`
- Rationale: this is an enhancement slice on an existing runtime, so baseline scope is held while small quality upgrades are surfaced explicitly.
- Expansion cherry-picks accepted in this pass: none (baseline scope retained; optional polish listed under `Delight Opportunities`).

## Architecture Diagram

```text
user intent
   |
   v
using-featureforge (direct routing)
   |
   v
workflow status/resolve -----------------------------------------------+
   |                                                                   |
   | plan missing/stale/invalid -> featureforge:writing-plans          |
   | plan current + fidelity gate not pass -> featureforge:plan-fidelity-review
   | plan current + fidelity pass -> featureforge:plan-eng-review      |
   v                                                                   |
plan-fidelity-review skill                                              |
   |                                                                    |
   | writes review artifact (.featureforge/reviews/*-plan-fidelity.md)  |
   | records receipt via workflow plan-fidelity record                  |
   v                                                                    |
runtime receipt validator (contracts + topology) -----------------------+
```

## Data Flow and State Machine

```text
HAPPY PATH
approved spec + current draft plan
  -> independent fresh-context review artifact
  -> runtime receipt record
  -> gate=pass
  -> next_skill=featureforge:plan-eng-review

SHADOW PATHS
nil/missing receipt
  -> gate invalid
  -> next_skill=featureforge:plan-fidelity-review

empty/malformed artifact or receipt
  -> gate invalid + explicit reason_code diagnostics
  -> next_skill=featureforge:plan-fidelity-review

upstream parse/status error (plan/spec not parseable)
  -> plan_fidelity_verification_incomplete
  -> fail closed with deterministic remediation
```

```text
STATE MACHINE (draft-plan branch)

[Draft plan exists]
    |
    +-- fidelity gate != pass --> [Needs plan-fidelity-review] --(pass receipt)-> [Plan-eng-review ready]
    |
    +-- fidelity gate == pass --> [Plan-eng-review ready]

[Draft plan invalid/stale/missing] --> [Needs writing-plans]
```

## Error & Rescue Registry

| Method/Codepath | Failure mode | Named class/code | Rescued? | Required rescue action | User sees |
|---|---|---|---|---|---|
| `evaluate_plan_fidelity_receipt_at_path` | receipt missing | `missing_plan_fidelity_receipt` | Yes | route to `featureforge:plan-fidelity-review` and require fresh receipt | explicit gate-block diagnostic |
| `evaluate_plan_fidelity_receipt_at_path` | receipt malformed json/schema | `malformed_plan_fidelity_receipt` | Yes | regenerate artifact + re-record receipt | explicit parse/remediation message |
| `evaluate_plan_fidelity_receipt_at_path` | receipt stale vs spec/plan revision | `stale_plan_fidelity_receipt` | Yes | re-run fidelity review against current approved spec + draft plan | explicit stale receipt diagnostic |
| `evaluate_plan_fidelity_receipt_at_path` | non-pass verdict | `plan_fidelity_receipt_not_pass` | Yes | route back through fidelity review stage after plan edits | explicit not-pass diagnostic |
| `evaluate_plan_fidelity_receipt_at_path` | reviewer not independent | `plan_fidelity_receipt_not_independent` / `plan_fidelity_reviewer_provenance_invalid` | Yes | force fresh-context independent reviewer rerun | explicit provenance failure |
| `evaluate_plan_fidelity_gate` | draft plan/spec cannot parse | `plan_fidelity_verification_incomplete` | Yes | fix plan/spec contract parseability before gate check | explicit "cannot be validated" diagnostic |
| `plan_fidelity_gate_diagnostics` | remediation text points to wrong stage | routing defect (spec-governed) | Yes | standardize remediation to `featureforge:plan-fidelity-review` when blocker is receipt state | consistent next-skill guidance |

## Failure Modes Registry

| Codepath | Failure mode | Rescued? | Test? | User sees? | Logged? |
|---|---|---|---|---|---|
| workflow status draft-plan branch | fidelity gate fails but routes to writing-plans | Yes | Yes (required by REQ-006/REQ-011) | deterministic next-skill + reason codes | yes |
| direct-routing contract surfaces | unrelated pre-routing gate dependencies accidentally introduced | Yes | Yes | deterministic contract failure before release | yes |
| plan-eng-review prerequisite | receipt missing/stale/non-pass | Yes | Yes | fail-closed engineering review with remediation path | yes |
| writing-plans terminal handoff | hidden reviewer semantics persist | Yes | Yes | explicit handoff to first-class stage after update | yes |
| public docs pipeline text | stage omitted from canonical sequence | Yes | Yes | docs now match runtime routing | n/a |

No row is allowed to remain silent (`RESCUED=N` + `TEST=N` + `USER SEES=Silent`). This spec requires deterministic diagnostics for every blocked transition.

## Security and Threat Model

- Threat: forged or stale fidelity artifact/receipt used to bypass review gate.
  - Likelihood: medium
  - Impact: high (unreviewed plans reach engineering review)
  - Mitigation: runtime fingerprint binding, spec/plan revision matching, pass-only receipt acceptance.
- Threat: self-review provenance masquerades as independent reviewer.
  - Likelihood: medium
  - Impact: high (trust-boundary collapse)
  - Mitigation: explicit provenance checks (`Distinct From Stages`, reviewer source validation), fresh-context requirement in public skill.
- Threat: path traversal via artifact path fields.
  - Likelihood: low
  - Impact: medium
  - Mitigation: repo-relative path enforcement in runtime parsing/validation.

## Observability and Debuggability

- Preserve and expand reason-code parity across `workflow status`, operator handoff text, and diagnostics.
- Add/retain explicit reason codes for missing/stale/malformed/non-pass/non-independent receipt states.
- Require test coverage that validates next-skill routing text and reason-code output together.
- Ensure remediation text points to the owning stage (`plan-fidelity-review`) when the blocker is receipt state.

## Deployment Sequence

```text
1) Add new skill surface (tmpl + prompt + generated doc)
2) Update workflow docs to include canonical stage
3) Update routing/runtime surfaces while preserving direct-routing contract
4) Update contract/runtime/doc tests
5) Run targeted suites then broader regressions
6) Merge only when routing + diagnostics + docs are in parity
```

## Rollback Flowchart

```text
regression detected?
  |
  +-- no --> continue rollout
  |
  +-- yes
        |
        +-- docs/skill text only regression --> revert docs/skill changes
        |
        +-- runtime routing regression --> revert routing changes in status/operator and rerun active direct-routing contract tests
        |
        +-- receipt contract regression --> revert to prior validator behavior and rerun gate tests
```

## What Already Exists

- Runtime stage constant and receipt contract machinery already exist and are authoritative.
- Receipt record CLI already exists.
- `writing-plans` and `plan-eng-review` already model the gate conceptually.
- Gap is primarily first-class public stage visibility and route ownership, not missing low-level primitives.

## Dream State Delta

```text
CURRENT STATE
runtime enforces plan-fidelity gate, but public workflow surfaces hide/blur ownership
   ---> THIS SPEC DELTA
first-class stage + explicit routing/remediation + doc parity + deterministic tests
   ---> 12-MONTH IDEAL
all workflow stages are visible and invokable; diagnostics always point to the exact owning stage;
no drift between runtime truth and skill/doc guidance
```

## NOT in Scope

- Receipt schema redesign or new receipt format (deferred to future dedicated schema slice).
- Reworking broader engineering review or execution topology beyond fidelity-stage routing.
- Removing runtime acceptance of compatibility reviewer provenance values in this slice.

## Delight Opportunities (Selective Expansion, Optional)

- Add a one-line `workflow doctor` hint that prints the exact `workflow plan-fidelity record` command with current paths.
- Add a small generated section in `plan-fidelity-review` docs mapping each receipt reason code to remediation.
- Add a compact fixture set that exercises all fidelity reason codes for faster local regression checks.
- Add a docs matrix that maps each workflow stage to its owning skill and terminal handoff.
- Add a `workflow status` smoke test that asserts docs pipeline sequence and runtime next-skill alignment together.

## Stale Diagram Audit

- Existing diagrams in related specs remain accurate for their own slices.
- This spec adds fresh architecture/data/state/deploy/rollback diagrams for the newly first-class stage.

## Unresolved Decisions

- None.

## CEO Review Summary

**Review Status:** clear
**Reviewed At:** 2026-04-01T13:55:04Z
**Review Mode:** selective_expansion
**Reviewed Spec Revision:** 5
**Critical Gaps:** 0
**UI Design Intent Required:** no
**Outside Voice:** skipped
