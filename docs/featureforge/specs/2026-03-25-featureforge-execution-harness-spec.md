# FeatureForge Execution Harness Orchestration

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Summary

Add a Rust-owned execution harness inside `featureforge plan execution` that governs the post-approval implementation loop with explicit macro-phases, contract/evaluate/repair/handoff artifacts, adaptive evaluator and reset policies, and fail-closed intermediate gates. Preserve the current outer workflow and current final completion gates. Do **not** replace the existing planner, spec/plan approval flow, task packets, final code review, QA, or release docs. Instead, make the Rust runtime own the inner loop that sits between `implementation_ready` and branch completion.

The intended result is:

- `featureforge workflow` still routes work from brainstorming through `implementation_ready`
- `featureforge plan execution` becomes a real long-running harness rather than a thin step tracker
- the runtime enforces chunk-level contract, execution, evaluation, repair/pivot, and handoff transitions
- the existing skills become generator/evaluator implementations selected and constrained by runtime policy
- final review, browser QA, release documentation, and branch completion remain fail-closed downstream gates

## Document Contract

This spec is intentionally high-detail. Unless a section explicitly marks a name, field list, state name, command shape, or example as `representative`, concrete names in this document are normative for v1. Representative examples may be renamed during implementation only when the behavior contract remains unchanged.

## Problem

FeatureForge already has strong outer-workflow and provenance discipline, but the control plane becomes thin after plan approval. The README defines a six-layer system where workflow routing ends at `implementation_ready`, execution begins from an engineering-approved plan, `recommend` chooses between `featureforge:subagent-driven-development` and `featureforge:executing-plans`, and completion then flows through final code review, release docs, optional QA, and branch completion. In the current Rust runtime, the workflow operator mostly reduces post-approval execution to `implementation_handoff`, `execution_preflight`, `executing`, `review_blocked`, `qa_pending`, `document_release_pending`, and `ready_for_branch_completion`. Current `PlanExecutionStatus` exposes only plan/execution fingerprints, evidence path, and active/blocking/resume step pointers, while `RecommendOutput` only returns `recommended_skill`, `reason`, and simple decision flags. Existing task packets already include plan/spec fingerprints and requirement IDs, and existing skills already provide per-task spec-compliance and code-quality review loops plus a fail-closed final review and structured QA artifacts. That means the project already has the right building blocks, but the runtime does not yet own the inner long-running loop. 

Anthropic’s March 24, 2026 article adds the missing shape. It describes a planner/generator/evaluator harness that uses negotiated chunk-level contracts before implementation, a separate evaluator with explicit criteria and hard thresholds, file-based handoffs between agents, and the ability to repair or pivot based on evaluator findings. The article also draws a sharp distinction between compaction and context resets, argues that a skeptical standalone evaluator is easier to tune than self-evaluation, and concludes that evaluator and reset overhead should be applied selectively because they are not equally load-bearing on every run. FeatureForge should adopt those principles at the execution layer while keeping its stronger existing planning and provenance model.

## Desired Outcome

FeatureForge executes approved plans through a Rust-enforced harness with two layers of control:

1. **Outer workflow control** remains unchanged through spec approval, plan approval, and `implementation_ready`.
2. **Inner execution control** adds explicit macro-phases that keep long-running implementation on track:
   - prepare the workspace
   - draft and runtime-approve a chunk contract
   - execute only the scoped work for that contract
   - evaluate against explicit criteria
   - repair, pivot, or hand off when needed
   - pass through final review, browser QA, release docs, and finish gates

The end state is:

- execution policy is explicit and machine-readable
- contract/evaluation/handoff artifacts are first-class inputs to the runtime
- evaluator findings are granular enough to reopen or transfer work automatically
- chunking, evaluator frequency, and reset/handoff policy are adaptive rather than hardcoded
- the runtime, not skill prose, keeps the process on track

## Control Model Principles

The execution harness adds runtime-owned control without weakening the existing workflow. The governing principles are:

- the outer workflow remains authoritative through `implementation_ready`
- the Rust runtime is the execution law; skills are implementations operating inside that law
- chunk contracts are drafted by the generator, but approval is runtime-owned through `gate-contract` and `record-contract`
- step-level execution primitives remain available only inside runtime-approved scope and legal harness phases
- authoritative harness state has one runtime-owned writer per active branch/plan execution scope; subagents may produce candidate artifacts but do not advance authoritative state directly
- human approval remains at spec approval, plan approval, downstream review and QA gates, finish readiness, and explicit escalations

## Goals

- Preserve the current outer workflow model up to `implementation_ready`.
- Add a persisted Rust-owned harness state machine under `featureforge plan execution`.
- Keep existing step-level execution primitives (`begin`, `note`, `complete`, `reopen`, `transfer`) as the micro-state layer.
- Add a chunk-scoped `ExecutionContract` artifact that bridges task packets and implementation.
- Add a normalized `EvaluationReport` artifact for all evaluator outputs.
- Add a normalized `ExecutionHandoff` artifact for resets, session changes, and blocked recovery.
- Extend execution status and recommend output to expose harness policy and harness progress.
- Normalize the existing skills as generator/evaluator implementations selected by runtime policy.
- Preserve fail-closed final review, QA, release docs, and finish readiness behavior.
- Add verification and fixture coverage for the new loop.
- Adopt the harness as the only supported active execution path once this design lands.

## Not In Scope

- Replacing the existing planning chain or changing the approval model before `implementation_ready`
- Replacing task packets as the core plan-to-task derivation artifact
- Making an evaluator mandatory on every run regardless of task difficulty
- Moving local execution-contract, evaluation, or handoff artifacts into repo-visible active docs
- Rewriting the existing skills from scratch when normalization is sufficient
- Implementing general-purpose parallel task execution beyond the current subagent strategy
- Adding model-specific token-window instrumentation as a hard requirement for resets
- Supporting mixed legacy/harness execution or fallback continuation paths inside `featureforge plan execution`
- Weakening or bypassing existing final review, QA, release doc, or finish gates

## Current-System Findings

The current repository is already architected around repo-visible spec and plan artifacts, task packets with requirement traceability, and a branch-scoped local artifact root under `~/.featureforge/projects/`. Execution starts only after an engineering-approved plan, and the current recommendation step only chooses between serial execution and same-session isolated subagent execution. That is a strong outer control plane, but it leaves the execution-layer loop under-modeled in Rust.

The existing runtime and skills already contain many of the necessary pieces:

- `featureforge plan execution` exposes `status`, `recommend`, `preflight`, `gate-review`, `gate-finish`, `begin`, `note`, `complete`, `reopen`, and `transfer`
- `TaskPacket` already carries `plan_path`, `plan_revision`, `plan_fingerprint`, source spec metadata, `task_number`, `task_title`, `requirement_ids`, and `packet_fingerprint`
- `PlanExecutionStatus` currently tracks `execution_mode`, `execution_fingerprint`, `evidence_path`, and the current active/blocking/resume task-step pointers
- `RecommendOutput` currently exposes only `recommended_skill`, `reason`, and `decision_flags`
- subagent-driven execution already runs a fresh implementer, then spec-compliance review, then code-quality review before allowing task completion
- final whole-diff review already fails closed against execution state and evidence provenance
- workflow-routed QA already writes structured pass/fail/blocked artifacts

Anthropic’s article shows why those pieces need a stronger runtime connection. Its coding harness used a planner, a generator, and an evaluator; it had the generator and evaluator negotiate a sprint contract before implementation; it ran the evaluator against explicit criteria and hard thresholds; and it used files for handoffs. The same article later removed sprint decomposition for stronger models, kept the planner and evaluator, and concluded that evaluator and reset overhead should be applied where the task exceeds what the current model can reliably do solo, not as an all-or-nothing rule. That matches FeatureForge’s need for an adaptive execution policy rather than a fixed template.

## Requirement Index

- [REQ-001][behavior] The current outer workflow remains authoritative through `implementation_ready`; the new harness is implemented under `featureforge plan execution`, not by replacing pre-implementation workflow routing.
- [REQ-002][behavior] The execution runtime adds a persisted `HarnessPhase` macro-state with legal transitions enforced in Rust.
- [REQ-003][behavior] Existing step-level execution primitives remain the micro-state layer inside the new harness; the runtime must validate them against the current macro-phase.
- [REQ-004][behavior] A new `ExecutionContract` artifact exists for each active chunk and includes source plan/spec/task-packet provenance, scoped steps, scoped requirement IDs, explicit criteria, non-goals, verifiers, evidence requirements, retry budget, pivot threshold, and reset policy.
- [REQ-005][behavior] Every active contract maps to a deterministic chunk of plan work; the runtime rejects `begin`, `complete`, `reopen`, or `transfer` operations that fall outside the currently active contract scope.
- [REQ-006][behavior] A new `EvaluationReport` artifact exists for evaluator outputs and includes evaluator kind, verdict, per-criterion results, requirement/step mappings, evidence references, affected tasks/steps, and recommended next action.
- [REQ-007][behavior] A new `ExecutionHandoff` artifact exists for session changes, adaptive resets, and blocked recovery, and includes active contract provenance, current phase, satisfied criteria, unresolved criteria, files touched, next action, risks, and workspace notes.
- [REQ-008][behavior] `PlanExecutionStatus` is extended to surface harness phase, chunk identity, chunking strategy, evaluator policy, reset policy, active contract fingerprint/path, last evaluation verdict, retry counters, pivot threshold, and handoff requirement state.
- [REQ-009][behavior] Execution evidence is extended so completed attempts can reference the active contract fingerprint, evaluation report fingerprint, evaluator verdict, failing criterion IDs when applicable, and handoff fingerprint when work resumed from a handoff.
- [REQ-010][behavior] The runtime adds fail-closed intermediate gates at least for contract validity, evaluator validity, and handoff validity, while preserving the existing fail-closed review and finish gates.
- [REQ-011][behavior] Evaluator outcomes drive runtime transitions: `pass` advances to the next chunk or final review; `fail` triggers repair inside budget; repeated `fail` beyond threshold triggers pivot or plan-update flow; `blocked` requires explicit unblock or handoff.
- [REQ-012][behavior] `featureforge plan execution recommend` is extended to return harness policy in addition to skill choice: chunking strategy, evaluator policy, reset policy, and required review stack.
- [REQ-013][behavior] The runtime treats existing execution skills as generator/evaluator implementations selected by policy rather than as free-standing execution laws.
- [REQ-014][behavior] Contract criteria and evaluator findings use stable criterion IDs with explicit mappings back to spec requirement IDs and covered plan steps.
- [REQ-015][behavior] `featureforge workflow operator` becomes harness-aware and exposes execution phases detailed enough to distinguish contracting, executing, evaluating, repairing, pivot-required, and handoff-required states.
- [REQ-016][behavior] Reset and handoff policy supports `none`, `chunk-boundary`, and `adaptive` modes; the runtime requires a valid handoff artifact whenever the active policy or state demands a reset/resume boundary.
- [REQ-017][behavior] Final code review, browser QA, release documentation, and finish readiness remain downstream authoritative gates and continue to fail closed.
- [REQ-018][behavior] Once the harness is enabled, active execution under `featureforge plan execution` uses only harness-governed artifacts; pre-harness execution evidence is not a supported continuation source.
- [REQ-019][verification] Verification covers state transitions, artifact parsing and provenance checks, invalidation cascades, operator routing, policy recommendation, hard-cutover behavior, and failure cases.
- [REQ-020][verification] Tests and fixtures prove that skills cannot advance execution past a failing contract, failing evaluation, or missing/invalid handoff.
- [REQ-021][verification] Tests prove that final review and finish readiness fail when unresolved harness failures, stale contract/evaluation provenance, or mismatched artifacts remain.
- [REQ-022][behavior] Runtime storage for harness artifacts stays under the existing `~/.featureforge/projects/` project artifact root and remains branch-scoped and reproducible.
- [REQ-023][behavior] Harness commands and gates expose a stable minimum machine-readable failure-class taxonomy covering at least illegal phase, stale provenance, contract mismatch, evaluation mismatch, missing required handoff, non-harness provenance, and blocked-on-plan-pivot execution.
- [REQ-024][behavior] The harness emits a minimum observability contract for phase transitions, gate outcomes, blocked states, and downstream gate rejections using structured events keyed by stable run, chunk, phase, contract, evaluation, and handoff identifiers.
- [REQ-025][behavior] The runtime enforces a single-writer authority for authoritative harness state per active branch/plan execution scope; concurrent mutation attempts fail closed, and subagents may generate candidate artifacts but may not advance authoritative state directly.
- [REQ-026][behavior] Artifact parsers reject unknown or unsupported `contract_version`, `report_version`, `handoff_version`, and `evidence_artifact_version` values with a stable failure class; the runtime never best-effort parses unsupported artifact versions.
- [REQ-027][behavior] Candidate artifacts are marked or stored separately from authoritative artifacts; only runtime-recorded authoritative artifacts may satisfy gates, appear as active artifacts in status/state, or advance harness state.
- [REQ-028][behavior] Authoritative `record-contract`, `record-evaluation`, and `record-handoff` mutations are idempotent for identical replay against the same expected state; mismatched replay attempts fail closed with a stable failure class and must not duplicate state transitions or side effects.
- [REQ-029][behavior] The runtime captures repo-state provenance for authoritative artifacts and fails closed on out-of-band HEAD or worktree drift when later authoritative mutations or downstream gates depend on that provenance, until the run is reconciled, reopened, or re-evaluated.
- [REQ-030][behavior] The runtime computes authoritative artifact fingerprints from deterministic canonical content, verifies them on every later read that matters to state/gates/review/finish, and fails closed with a stable failure class if recorded fingerprints or on-disk authoritative artifact content no longer match.
- [REQ-031][behavior] Authoritative harness mutations commit atomically: each authoritative mutation either leaves the previously authoritative state intact or fully publishes its new authoritative artifact and state transition; partial or crash-interrupted mutations fail closed and require runtime-owned recovery before execution may proceed.
- [REQ-032][behavior] Provenance invalidation is deterministic: `reopen` stales the active chunk's dependent evaluation, handoff, and downstream gate artifacts; contract pivot supersedes the active contract and all artifacts derived from it; plan pivot blocks the run and stales all execution-derived downstream provenance for the superseded approved plan revision.
- [REQ-033][behavior] Multi-evaluator aggregation is deterministic and fail-closed: every evaluator kind required by the active contract must produce an authoritative report for the active contract; chunk pass requires all required evaluator kinds to pass; any required `fail` prevents aggregate pass and drives repair/pivot logic; any required `blocked` prevents advancement until resolved.
- [REQ-034][behavior] Contract-level `verifiers[]` are inner-loop evaluator kinds only. Downstream gate modes such as `final_code_review` and `browser_qa` may emit normalized artifacts for provenance, but they remain downstream authoritative gates and do not participate in chunk pass aggregation.
- [REQ-035][behavior] `recommended_action` is bounded evaluator guidance, not execution law. `verdict`, phase legality, and runtime policy remain authoritative; the runtime may use `recommended_action` only as a secondary hint within the set of transitions already legal for that verdict and state.
- [REQ-036][behavior] Run and chunk identity rollover is deterministic: `execution_run_id` remains stable across normal execution, repair, handoff, reopen, and contract pivot within the same approved plan revision and policy snapshot; plan-pivot re-entry through `execution_preflight` on a newly approved plan revision or an explicit runtime-owned policy reset boundary that adopts a different policy snapshot creates a new `execution_run_id`; `chunk_id` changes only when the active contract definition changes.
- [REQ-037][behavior] Evaluation-related observability is explicit: structured events and relevant status/operator outputs expose `evaluator_kind` whenever an evaluation artifact, evaluator result, or evaluator-driven transition/block is involved, so operators do not need artifact lookups to identify which evaluator failed, blocked, or most recently reported.
- [REQ-038][behavior] Authoritative ordering is runtime-owned and monotonic: authoritative contracts, evaluations, handoffs, and state transitions carry a monotonic authoritative sequence used for supersession, audit ordering, and replay safety; timestamps and filenames are never the source of authoritative ordering truth.
- [REQ-039][behavior] `authoritative_sequence` is scoped to `execution_run_id`: it starts fresh for each new run, increases monotonically only within that run, and authoritative order is determined by the pair `(execution_run_id, authoritative_sequence)` rather than by a branch-global counter.
- [REQ-040][behavior] `reason_codes[]` use a stable minimum taxonomy for blocked states and evaluator/runtime transitions, covering at least `waiting_on_required_evaluator`, `required_evaluator_failed`, `required_evaluator_blocked`, `handoff_required`, `repair_within_budget`, `pivot_threshold_exceeded`, `blocked_on_plan_revision`, `write_authority_conflict`, `repo_state_drift`, `stale_provenance`, and `recovering_incomplete_authoritative_mutation`.
- [REQ-041][behavior] Contract-declared `evidence_requirements[]` are fail-closed. Required evidence for the active contract must be satisfied by authoritative evidence refs traceable to the relevant criteria and covered steps before `gate-evaluator` or aggregate pass may succeed.
- [REQ-042][behavior] `evidence_requirements[].satisfaction_rule` uses a stable minimum vocabulary with deterministic runtime semantics. At minimum, the runtime must support `all_of`, `any_of`, and `per_step`, and must reject unknown rule values fail closed unless a later supported artifact version explicitly extends the vocabulary.
- [REQ-043][behavior] `EvaluationReport.evidence_refs[]` uses a minimum machine-readable schema. At minimum, each evidence ref must declare `evidence_ref_id`, `kind`, `source`, `requirement_ids[]`, `covered_steps[]`, `evidence_requirement_ids[]`, and `summary`, and the runtime must reject refs that cannot be validated against the active contract and the referenced evidence source.
- [REQ-044][behavior] `EvaluationReport.evidence_refs[].kind` uses a stable minimum vocabulary with deterministic runtime meaning. At minimum, the runtime must support `code_location`, `command_output`, `test_result`, `artifact_ref`, and `browser_capture`, and must reject unknown kind values fail closed unless a later supported artifact version explicitly extends the vocabulary.
- [REQ-045][behavior] `EvaluationReport.evidence_refs[].source` uses a stable minimum locator contract with kind-compatible shapes and canonical validation rules. At minimum, the runtime must support `repo:<relative_path>[#L<line>]` for `code_location`, `command_artifact:<artifact_ref>` for `command_output`, `test_artifact:<artifact_ref>` for `test_result`, `artifact:<artifact_ref>` for `artifact_ref`, and `browser_artifact:<artifact_ref>` for `browser_capture`, and must reject unknown, malformed, non-canonical, or kind-incompatible locators fail closed unless a later supported artifact version explicitly extends the contract.
- [REQ-046][behavior] Artifact-backed evidence locators use a stable artifact-target contract. `<artifact_ref>` resolves by canonical artifact fingerprint; optional canonical path is supporting metadata only and must not determine artifact identity, supersession, or gate truth. Unresolved or ambiguous artifact targets fail closed.
- [REQ-047][behavior] Repo-backed evidence locators use authoritative repo-state provenance. `repo:` locators resolve only against the authoritative repo-state baseline recorded for the relevant evaluation/run; if that baseline is unavailable, stale, or drifted for the requested action, repo-backed evidence fails closed rather than resolving against the live worktree opportunistically.
- [REQ-048][behavior] Repo-backed evidence may resolve against an authoritative worktree snapshot, not only clean committed `HEAD`, when the runtime can prove the exact baseline using authoritative repo-state provenance. For the minimum contract, the effective repo-backed evidence baseline is identified by `repo_state_baseline_head_sha` plus `repo_state_baseline_worktree_fingerprint`; committed-only `HEAD` is a special case of that baseline, not the only valid one.
- [REQ-049][behavior] Dirty-worktree `repo:` evidence is durable. When repo-backed evidence depends on content not recoverable from clean committed `HEAD` alone, the runtime must preserve or materialize the exact provenance-bound content needed for later validation and downstream use; fingerprint-only proof is insufficient when later reread of the cited content is required.
- [REQ-050][behavior] Dirty-worktree `repo:` evidence uses whole-file durable snapshots when later reread is required. Even for line-qualified `repo:` locators, the runtime preserves or materializes the whole provenance-bound file content rather than only the cited span, so later validation can re-read the exact file context deterministically.
- [REQ-051][behavior] Durable runtime-materialized evidence is first-class. When the runtime preserves dirty-worktree `repo:` evidence for later reread, it must materialize a first-class local `EvidenceArtifact` with stable fingerprinted identity and local reference semantics rather than relying on opaque internal storage.
- [REQ-052][behavior] Local harness artifact retention is bounded and fail-closed. Active authoritative artifacts, candidate artifacts still needed for in-flight controller work, and any artifacts still required by current state, review, QA, release-doc, finish, or durable evidence reread dependencies must be retained. Superseded or stale artifacts may be pruned only when no active dependency remains and a runtime-owned retention window has elapsed.
- [REQ-053][behavior] Dependency truth is runtime-owned. The runtime maintains a dependency index/reference graph for authoritative artifacts, downstream gate inputs, and any active candidate-retention claims that matter to pruning or stale-cascade decisions. Invalidation, gate truth, and pruning eligibility must use this runtime-owned dependency model rather than re-infer dependencies ad hoc at each call site.
- [REQ-054][behavior] Downstream gate outputs keep their existing artifact shapes in v1. When final review, browser QA, or release-doc outputs participate in stale-cascade, retention, or downstream gate truth, the runtime must fingerprint and index those existing outputs as authoritative dependency inputs rather than requiring a new harness-owned downstream artifact family.
- [REQ-055][behavior] `PlanExecutionStatus` and operator output expose downstream gate freshness explicitly. At minimum, final review, browser QA, and release-doc status must distinguish `not_required`, `missing`, `fresh`, and `stale`, and must expose the last indexed authoritative downstream artifact fingerprint when one exists.
- [REQ-056][behavior] The emitted execution policy tuple is run-scoped and frozen by default. `chunking_strategy`, `evaluator_policy`, `reset_policy`, and `review_stack[]` remain fixed for the life of an `execution_run_id`; they may change only through `execution_preflight` on a newly approved plan revision or an explicit runtime-owned policy reset boundary that mints a new `execution_run_id`.
- [REQ-057][behavior] `recommend` is advisory only. It may propose a candidate policy snapshot, but only `execution_preflight` may accept, persist, and activate the authoritative policy snapshot for an `execution_run_id`.
- [REQ-058][behavior] Accepted policy snapshots do not require a separate artifact family in v1. The authoritative accepted snapshot for an `execution_run_id` lives in authoritative state and structured policy-acceptance events; implementation must not require a standalone preflight or policy-snapshot artifact to reconstruct active run policy.
- [REQ-059][behavior] `execution_preflight` is idempotent for exact replay. Replaying `execution_preflight` against the same accepted policy snapshot, same approved plan revision, and same authoritative baseline returns the existing accepted result for that run and must not mint a second `execution_run_id`, duplicate policy-acceptance events, or reset run-scoped ordering. Replays with materially different accepted inputs are not exact replay and must either follow the legal new-run boundary rules or fail closed.
- [REQ-060][behavior] Authoritative execution scope remains branch-scoped in v1, not worktree-scoped. Multiple local worktrees on the same branch share one authoritative harness state, dependency index, and run-identity space for that branch; same-branch worktrees are competing controllers under the single-writer rules rather than independent authoritative scopes. Worktree identity may be recorded for diagnostics only.
- [REQ-061][behavior] Write-authority conflict does not create a new public operator phase in v1. The operator keeps the current public phase and surfaces authority blockage through `next_action`, stable `reason_codes[]`, `write_authority_state`, `write_authority_holder`, and `write_authority_worktree` when known.

## Design Decisions

- `DEC-001` Keep the current outer workflow contract intact and add the new harness only after `implementation_ready`.
- `DEC-002` Model the execution harness as a macro-state machine layered on top of the current step-level micro-state.
- `DEC-003` Keep task packets as the base task contract; add chunk-level execution contracts rather than replacing packets.
- `DEC-004` Normalize the existing skill ecosystem instead of introducing a separate parallel evaluator stack.
- `DEC-005` Treat evaluator frequency and reset behavior as policy decisions that can be adaptive, not fixed global rules.
- `DEC-006` Use spec requirement IDs as the backbone for criterion traceability across contracts, evaluator findings, and final review.
- `DEC-007` Keep local harness artifacts under `~/.featureforge/projects/` rather than adding more repo-visible authoritative files.
- `DEC-008` Preserve and reuse current fail-closed review and finish behavior instead of replacing it with a new completion model.
- `DEC-009` Treat harness rollout as a hard cutover for active execution; do not preserve a legacy continuation path inside `featureforge plan execution`.
- `DEC-010` Keep authoritative harness-state mutation controller-owned and runtime-mediated rather than allowing parallel workers or subagents to race authoritative state transitions.
- `DEC-011` Make authoritative harness mutations atomic and crash-recoverable rather than reconstructing authoritative truth from partial local writes.
- `DEC-012` Make stale-provenance cascades explicit and deterministic rather than inferring invalidation boundaries ad hoc during implementation.
- `DEC-013` Treat the active contract's required evaluator set as an all-required runtime contract, not as an advisory collection of independent reviews.
- `DEC-014` Preserve final review and QA as downstream authoritative gates even when their outputs can be normalized into evaluation-shaped provenance artifacts.
- `DEC-015` Keep evaluator `recommended_action` advisory under runtime-owned verdict and policy rather than allowing evaluator output to become a second state machine.
- `DEC-016` Keep run identity stable within one approved-plan execution and one frozen policy snapshot; mint a new run identity only when execution re-enters on a newly approved plan revision after a plan pivot or when an explicit runtime-owned policy reset boundary adopts a different policy snapshot.
- `DEC-017` Make evaluator identity first-class in observability and operator surfaces rather than requiring report-fingerprint dereferencing for routine diagnosis.
- `DEC-018` Make supersession and audit order derive from a runtime-owned monotonic sequence rather than timestamps, file paths, or arrival order.
- `DEC-019` Scope monotonic authoritative ordering to a single run identity so run rollover and sequence rollover stay aligned.
- `DEC-020` Treat `reason_codes[]` as a stable machine-readable vocabulary, not as implementation-local free-form labels.
- `DEC-021` Treat contract-declared evidence requirements as runtime-enforced pass criteria rather than evaluator-only narrative guidance.
- `DEC-022` Treat evidence satisfaction rules as stable runtime semantics rather than free-form evaluator interpretation.
- `DEC-023` Treat evidence references as machine-validated runtime inputs rather than evaluator-local annotations.
- `DEC-024` Treat evidence-reference kinds as stable runtime semantics rather than evaluator-local labels.
- `DEC-025` Treat evidence-source locators as stable runtime contracts rather than free-form evaluator references.
- `DEC-026` Treat artifact-backed evidence targets as fingerprint-addressed authoritative references rather than path-addressed pointers.
- `DEC-027` Treat repo-backed evidence as provenance-bound to authoritative repo state rather than live-worktree lookups.
- `DEC-028` Treat authoritative repo-backed evidence baseline as `HEAD` plus worktree snapshot provenance rather than clean-commit-only content.
- `DEC-029` Treat dirty-worktree repo evidence as a durable snapshot obligation, not just an identity proof obligation.
- `DEC-030` Treat durable dirty-worktree repo evidence snapshots as whole-file preservation rather than span-only fragments.
- `DEC-031` Treat durable runtime-materialized evidence as a first-class local artifact family rather than an internal implementation detail.
- `DEC-032` Treat local harness artifact growth as bounded by runtime-owned retention rules rather than unbounded append-only accumulation.
- `DEC-033` Treat artifact dependency truth as a runtime-owned indexed graph rather than ad hoc inference from whichever files happen to be loaded at a call site.
- `DEC-034` Preserve existing downstream review/QA/release-doc artifact shapes and make the runtime fingerprint/index them when they become authoritative dependency inputs, rather than introducing a second downstream artifact family in v1.
- `DEC-035` Make downstream gate freshness first-class in status and operator surfaces instead of forcing operators to infer review/QA/release-doc truth from phase plus gate errors.
- `DEC-036` Treat the emitted execution policy tuple as a frozen run-scoped snapshot rather than something the runtime may recompute underneath an active run.
- `DEC-037` Keep `recommend` side-effect free and make `execution_preflight` the sole policy-acceptance boundary rather than splitting authority across two commands.
- `DEC-038` Keep accepted policy snapshots in authoritative state plus structured events rather than introducing a separate local policy-artifact family in v1.
- `DEC-039` Treat `execution_preflight` like an idempotent control-plane commit point rather than a one-shot edge that mints a new run identity on every retry.
- `DEC-040` Keep authoritative harness scope branch-scoped across same-branch worktrees and use worktree identity only as diagnostic metadata rather than part of authoritative storage or run scope.
- `DEC-041` Keep write-authority conflict inside the existing public phase model and surface it through next-action, reason-code, and holder metadata instead of adding a separate operator phase.

## Affected Surfaces

The implementation directly affects at least these areas:

- `src/execution/state.rs`
- `src/cli/plan_execution.rs`
- `src/workflow/operator.rs`
- `src/workflow/status.rs`
- `src/contracts/packet.rs` and adjacent execution/provenance models
- plan-execution artifact parsing and fingerprinting helpers
- `skills/subagent-driven-development/SKILL.md`
- `skills/executing-plans/*`
- `skills/requesting-code-review/SKILL.md`
- `skills/qa-only/SKILL.md`
- shared review/QA references and exemplars
- tests for workflow runtime, operator routing, and plan execution
- `tests/codex-runtime/fixtures/workflow-artifacts/` and related fixture contracts

## Architecture

### 1. Boundary between workflow routing and execution orchestration

The current workflow contract should remain:

```text
brainstorming
  -> plan-ceo-review
  -> writing-plans
  -> plan-eng-review
  -> implementation_ready
```

The new harness begins only after the exact approved plan reaches `implementation_ready`.

```text
implementation_ready
  -> execution_preflight
  -> execution_harness
       -> contract
       -> execute
       -> evaluate
       -> repair | pivot | handoff
  -> final_review
  -> browser_qa (when required)
  -> release_docs
  -> branch_completion
```

This boundary keeps all planning semantics where FeatureForge is already strong while making the execution loop explicit and enforceable.

### 2. Macro-state machine

Add a new persisted `HarnessPhase` enum owned by the Rust runtime. The minimum public phases are:

- `implementation_handoff`
- `execution_preflight`
- `contract_drafting`
- `contract_pending_approval`
- `contract_approved`
- `executing`
- `evaluating`
- `repairing`
- `pivot_required`
- `handoff_required`
- `final_review_pending`
- `qa_pending`
- `document_release_pending`
- `ready_for_branch_completion`

Here, `contract_pending_approval` means the drafted contract is waiting for runtime validation and recording through `gate-contract` and `record-contract`. It is not a human approval stop unless some other escalation path has already blocked the run.

The phase machine must enforce these invariants:

- exactly one active phase exists per plan revision
- only `contract_approved`, `executing`, or `repairing` may have work actively begun
- `evaluating` must not allow new step execution until the evaluation result is recorded
- `handoff_required` blocks normal execution until a valid handoff is recorded and accepted
- `final_review_pending` is reachable only when all plan steps are resolved and all contract/evaluation obligations for the active policy are satisfied

Recommended transition model:

```text
implementation_handoff
  -> execution_preflight
  -> contract_drafting
  -> contract_pending_approval
  -> contract_approved
  -> executing
  -> evaluating
       pass -> contract_drafting (next chunk) | final_review_pending
       fail -> repairing
       fail over threshold -> pivot_required
       blocked -> handoff_required
  -> repairing -> executing
  -> pivot_required -> contract_drafting (contract pivot) | blocked pending approved plan revision (plan pivot)
  -> handoff_required -> execution_preflight | contract_drafting | contract_approved
  -> final_review_pending -> qa_pending | document_release_pending | ready_for_branch_completion
```

The implementation may store additional internal sub-states, but the above public phase model is the contract.

### 3. Micro-state model

Do **not** discard the existing step-level mechanics. Current `begin`, `note`, `complete`, `reopen`, and `transfer` already map naturally to task-step micro-state. The new harness must keep them, but validate them against the macro-state.

#### Step-level rules

- `begin` is allowed only when the macro-state is `contract_approved`, `executing`, or `repairing`
- `complete` is allowed only for a step inside the active contract scope
- `reopen` invalidates downstream execution/evaluation provenance that depends on the reopened step
- `transfer` is allowed only when a repair path is active and must remain inside the active chunk or next repair chunk selected by runtime
- `note --state Blocked|Interrupted` remains valid for step-level problems, but the runtime decides whether that also forces `handoff_required` or `pivot_required`

### 4. Chunking model

The harness must support these chunking strategies:

- `task`: one plan task per contract
- `task-group`: an ordered subset of consecutive related tasks per contract
- `whole-run`: one contract for all remaining work

Each contract must declare:

- `chunk_id`
- `chunking_strategy`
- the exact covered tasks and steps
- the exact task-packet fingerprints or source packet set that back the chunk
- the exact requirement IDs in scope

The runtime must reject any execution command that targets a task/step outside the active chunk.

Identity rules:

- `chunk_id` remains stable across step execution, repair, handoff, and reopen while the active contract definition is unchanged
- a new `chunk_id` is minted when the runtime activates a different contract definition, including next-chunk advancement or contract pivot
- `chunk_id` must not change merely because a new evaluation report, handoff artifact, or repair attempt is recorded against the same active contract definition

### 5. ExecutionContract artifact

Add a first-class artifact named `ExecutionContract`. This is a **local project artifact**, not a repo-visible authoritative spec/plan file.

#### Purpose

`ExecutionContract` bridges:

```text
approved spec + approved plan + task packet(s)
    -> testable chunk contract
    -> implementation
```

#### Minimum schema

```text
contract_version
authoritative_sequence
source_plan_path
source_plan_revision
source_plan_fingerprint
source_spec_path
source_spec_revision
source_spec_fingerprint
source_task_packet_fingerprints[]
chunk_id
chunking_strategy
covered_steps[]
requirement_ids[]
criteria[]
non_goals[]
verifiers[]
evidence_requirements[]
retry_budget
pivot_threshold
reset_policy
generated_by
generated_at
contract_fingerprint
```

#### Criterion schema

Each criterion must include at least:

```text
criterion_id
title
description
requirement_ids[]
covered_steps[]
verifier_types[]
threshold
notes
```

#### Evidence requirement schema

Each `evidence_requirements[]` entry must include at least:

```text
evidence_requirement_id
kind
requirement_ids[]
covered_steps[]
satisfaction_rule
notes
```

#### Evidence satisfaction-rule vocabulary

For the minimum supported vocabulary, `satisfaction_rule` means:

- `all_of`: the requirement is satisfied only when authoritative `evidence_refs[]` collectively cover the full declared scope of the requirement entry across its `requirement_ids[]` and `covered_steps[]`
- `any_of`: the requirement is satisfied when at least one authoritative `evidence_ref` traceable to the requirement entry covers some declared scope from that entry; full coverage of all listed requirement IDs or steps is not required
- `per_step`: the requirement is satisfied only when each step listed in `covered_steps[]` has at least one authoritative `evidence_ref` traceable to that step and to the requirement entry

Unknown `satisfaction_rule` values are illegal for the minimum artifact version and must be rejected fail closed during contract validation rather than deferred to evaluator-specific interpretation.

#### Contract rules

- the contract must be derived from the exact approved plan revision and matching task packet provenance
- contracts may be revised before approval, but each approved contract gets a stable fingerprint and monotonic `authoritative_sequence` assigned by the runtime from canonical contract content and record order within the active `execution_run_id`
- unsupported or unknown `contract_version` values are rejected fail closed; the runtime never best-effort parses a contract artifact it does not explicitly support
- candidate contract artifacts may exist, but only authoritative contracts recorded by the runtime may satisfy `gate-contract`, become the active contract, or advance state
- the runtime must reject contracts that reference stale plan/spec/task-packet provenance
- the runtime must reject contracts with empty scope, empty criteria, or empty verifier declarations
- `verifiers[]` is the required inner-loop evaluator-kind set for the active contract; unless a later artifact version adds explicit optionality, every listed evaluator kind is required before the chunk may aggregate to `pass`
- `evidence_requirements[]` declares the required evidence obligations for aggregate chunk pass; when no additional evidence is required, the contract must record an explicit empty list rather than omitting the field
- each `evidence_requirements[]` entry must be traceable to requirement IDs and covered steps inside the active contract and must use a deterministic `satisfaction_rule` from the stable minimum vocabulary unless a later supported artifact version explicitly extends it
- downstream gate modes such as `final_code_review` and `browser_qa` must not appear in the active contract's `verifiers[]`
- a chunk cannot enter `contract_approved` until the contract passes `gate-contract`

### 6. EvaluationReport artifact

Add a first-class artifact named `EvaluationReport`.

#### Purpose

This is the normalized output schema for:

- spec-compliance review
- code-quality review
- normalized provenance derived from downstream final whole-diff code review
- normalized provenance derived from downstream browser QA
- future API or DB evaluators

#### Minimum schema

```text
report_version
authoritative_sequence
source_plan_path
source_plan_revision
source_plan_fingerprint
source_contract_fingerprint
evaluator_kind
verdict                # pass | fail | blocked
criterion_results[]
affected_steps[]
evidence_refs[]
recommended_action     # continue | repair | pivot | escalate | handoff
summary
generated_by
generated_at
report_fingerprint
```

#### Criterion result schema

```text
criterion_id
status                 # pass | fail | blocked
requirement_ids[]
covered_steps[]
finding
evidence_refs[]
severity
```

#### Evidence reference schema

Each `evidence_refs[]` entry must include at least:

```text
evidence_ref_id
kind
source
requirement_ids[]
covered_steps[]
evidence_requirement_ids[]
summary
```

`source` must be a parseable runtime-usable locator to the evidence being cited. `evidence_requirement_ids[]` declares which active contract evidence requirements the ref claims to satisfy; informational refs that satisfy no declared evidence requirement must use an explicit empty list rather than omitting the field.

#### Evidence kind vocabulary

For the minimum supported vocabulary, `kind` means:

- `code_location`: `source` points to repository content or diff-localized code evidence relevant to the cited requirement IDs and covered steps
- `command_output`: `source` points to a recorded command invocation or command-output artifact that the runtime can trace back to the cited scope
- `test_result`: `source` points to a recorded test execution result or structured test artifact for the cited scope
- `artifact_ref`: `source` points to another authoritative or parseable supporting artifact whose fingerprint or path the runtime can resolve deterministically
- `browser_capture`: `source` points to a browser QA artifact such as a screenshot, DOM snapshot, trace, or equivalent browser evidence for the cited scope

Unknown `kind` values are illegal for the minimum artifact version and must be rejected fail closed during evaluation validation rather than interpreted ad hoc by individual evaluators.

#### Evidence source locator contract

For the minimum supported vocabulary, `source` must use one of these kind-compatible locator shapes:

- `repo:<relative_path>[#L<line>]` for `code_location`
- `command_artifact:<artifact_ref>` for `command_output`
- `test_artifact:<artifact_ref>` for `test_result`
- `artifact:<artifact_ref>` for `artifact_ref`
- `browser_artifact:<artifact_ref>` for `browser_capture`

Locator rules:

- `relative_path` must stay inside the repository root; absolute paths, parent-directory traversal, and non-canonical path forms are illegal
- `<artifact_ref>` must resolve deterministically to an authoritative or otherwise parseable local artifact the runtime is allowed to consume for that evidence kind
- the runtime must canonicalize accepted locators before authoritative fingerprinting and later validation so equivalent references do not produce divergent authoritative content
- locator scheme and declared `kind` must agree; a syntactically valid locator for one evidence kind is still invalid when paired with a different kind

#### Repo-backed evidence contract

For the minimum supported vocabulary, `repo:` locators resolve against the authoritative repo-state provenance recorded for the relevant evaluation/run, not against the mutable live worktree by default.

Repo-backed evidence rules:

- a `repo:` locator is valid only when the runtime has the authoritative repo-state baseline needed to interpret that locator for the requested action
- the authoritative baseline for repo-backed evidence is the pair `repo_state_baseline_head_sha` and `repo_state_baseline_worktree_fingerprint`; a clean committed baseline is one special case where that pair proves a clean worktree
- a dirty-worktree snapshot is valid evidence input when that authoritative baseline was captured and can later be proven for the requested action; the runtime must not require an artificial commit only to make local code evidence citeable
- when dirty-worktree content is cited and later validation or downstream gates may need to reread the exact content, the runtime must preserve or materialize the provenance-bound content needed to satisfy that reread rather than relying only on a worktree fingerprint
- when such durable preservation is required, the runtime preserves or materializes the whole provenance-bound file content for each referenced `repo:` file; line-qualified locators still bind to that whole-file snapshot rather than to a span-only fragment
- if the relevant repo-state baseline is unavailable, stale, or drifted relative to the authoritative provenance the gate depends on, the `repo:` locator is invalid until the run is reconciled, reopened, or re-evaluated
- the runtime must not silently reinterpret a provenance-bound `repo:` locator against whatever content currently exists at the same path in the live worktree
- line-qualified `repo:` locators must resolve against the provenance-bound repository content identified by that baseline, not against a later edited file at the same path

#### EvidenceArtifact artifact

The harness defines a first-class local artifact family named `EvidenceArtifact` for runtime-materialized evidence.

This is a local project artifact, not a repo-visible authoritative spec/plan file.

For the minimum contract, the runtime must materialize an `EvidenceArtifact` whenever dirty-worktree `repo:` evidence requires durable reread. Other evidence kinds may also use this artifact family when the runtime chooses to materialize them locally.

#### Minimum schema

```text
evidence_artifact_version
evidence_artifact_fingerprint
evidence_kind
source_locator
repo_state_baseline_head_sha
repo_state_baseline_worktree_fingerprint
relative_path
captured_content_fingerprint
generated_by
generated_at
```

The artifact body preserves the durable evidence payload. For dirty-worktree `repo:` evidence, that payload is the whole provenance-bound file content.

#### EvidenceArtifact rules

- unsupported or unknown `evidence_artifact_version` values are rejected fail closed; the runtime never best-effort parses an evidence artifact it does not explicitly support
- authoritative `EvidenceArtifact` fingerprints are computed from canonical artifact metadata plus preserved payload content and must verify on later read before the artifact may satisfy evidence reread requirements
- candidate evidence artifacts may exist, but only authoritative runtime-materialized `EvidenceArtifact` artifacts may satisfy durable evidence reread requirements or artifact-backed evidence resolution
- dirty-worktree `repo:` evidence requiring durability must resolve through exactly one matching authoritative `EvidenceArtifact` consistent with the evidence ref's `source`, repo-state baseline, and preserved file content
- `artifact:<artifact_ref>` may resolve to an `evidence_artifact_fingerprint`; command, test, or browser artifact locators may also resolve to `EvidenceArtifact` fingerprints when the runtime materializes those evidence captures locally
- append-only provenance rules apply to `EvidenceArtifact` just as they do to the other local harness artifact families
- if required durable evidence cannot be matched to exactly one authoritative `EvidenceArtifact`, the dependent evaluation or downstream gate fails closed

#### Artifact target contract

For the minimum supported vocabulary, `<artifact_ref>` means the canonical fingerprint of the target artifact.

Artifact-target rules:

- artifact-backed locators resolve the target artifact by canonical fingerprint, not by filename, browse order, or path similarity
- when the target is runtime-materialized evidence, `<artifact_ref>` resolves to an `evidence_artifact_fingerprint`
- an optional canonical path may be retained as supporting metadata for operator readability or diagnostics, but it must not determine artifact identity, supersession, or gate truth
- if a fingerprint resolves to no artifact, to multiple plausible artifacts, or to an artifact that is not valid for the declared evidence kind, the locator is invalid
- path-only or name-only artifact targeting is illegal for the minimum artifact version, even when a path would currently resolve uniquely on disk

Unknown schemes, malformed locators, unresolved artifact targets, candidate-only artifact targets, and kind-incompatible locator pairs are illegal for the minimum artifact version and must be rejected fail closed during evaluation validation.

`evidence_refs[]` used to satisfy contract evidence obligations must be able to identify which `evidence_requirement_id` they satisfy, or otherwise provide equivalent deterministic traceability back to the required evidence entry. The runtime evaluates those refs using the active requirement entry's `satisfaction_rule` semantics rather than evaluator-local interpretation.

#### Report rules

- unsupported or unknown `report_version` values are rejected fail closed; the runtime never best-effort parses an evaluation artifact it does not explicitly support
- candidate evaluation artifacts may exist, but only authoritative evaluation reports recorded by the runtime may satisfy `gate-evaluator`, update retry state, or drive phase transitions
- authoritative evaluation fingerprints are computed by the runtime from canonical report content and must verify on later read before the report may drive state or downstream review/finish decisions
- every evaluation report must cite the exact contract fingerprint it evaluated
- every failing or blocked criterion must identify the affected requirement IDs and steps
- every evaluation report must use a stable evaluator kind
- every `evidence_refs[]` entry must conform to the minimum schema, use a supported stable `kind`, and resolve to a supported canonical source locator compatible with that kind
- artifact-backed evidence locators must resolve by canonical artifact fingerprint; supporting path metadata must not substitute for or override fingerprint-based target identity
- repo-backed evidence locators must resolve against the authoritative repo-state provenance for the relevant evaluation/run rather than the mutable live worktree, including authoritative worktree snapshots proven by `repo_state_baseline_head_sha` plus `repo_state_baseline_worktree_fingerprint`
- dirty-worktree repo-backed evidence must remain durably readable for later validation when the runtime or downstream gates depend on rereading the cited content
- durable dirty-worktree repo-backed evidence uses whole-file preservation rather than span-only excerpts, even when the original `repo:` locator cites a specific line
- when durable runtime-materialized evidence is required, later validation resolves it through a first-class authoritative `EvidenceArtifact` rather than opaque internal storage
- `pass` may still include warnings, but warnings cannot conceal failing criteria
- `blocked` means runtime cannot continue normal execution until the block is resolved or a handoff is recorded
- the runtime must reject evaluation reports that do not match the active contract or current branch/HEAD when that provenance matters
- a later authoritative report for the same `source_contract_fingerprint` and `evaluator_kind` supersedes the earlier report only when it has the higher runtime-assigned `authoritative_sequence` within the same `execution_run_id`
- no single evaluation report may mark a chunk passed unless it completes the required evaluator-kind set and the aggregate evaluation state resolves to `pass`
- `evidence_requirement_ids[]`, `requirement_ids[]`, and `covered_steps[]` on each `evidence_ref` must be internally consistent with the active contract, the cited criterion results, the claimed evidence source locator, the declared evidence kind, and any fingerprint-resolved artifact target
- `evidence_refs[]` must satisfy the active contract's `evidence_requirements[]` for any criterion or covered step the report claims is passed; missing, untraceable, or rule-insufficient required evidence makes the report invalid for aggregate pass
- reports for downstream gate modes such as `final_code_review` and `browser_qa` may be normalized into this schema for provenance, but they do not satisfy `gate-evaluator`, do not belong in contract-level `verifiers[]`, and do not participate in chunk pass aggregation
- `recommended_action` must remain legal for the report's `verdict`, but it is advisory; it cannot by itself override verdict semantics, phase legality, retry budget, or runtime policy

### 7. ExecutionHandoff artifact

Add a first-class artifact named `ExecutionHandoff`.

#### Purpose

This artifact distinguishes a real reset/resume boundary from mere in-session summarization. It carries enough state for a fresh execution session to continue cleanly.

#### Minimum schema

```text
handoff_version
authoritative_sequence
source_plan_path
source_plan_revision
source_contract_fingerprint
harness_phase
chunk_id
satisfied_criteria[]
open_criteria[]
open_findings[]
files_touched[]
next_action
workspace_notes
commands_run[]
risks[]
generated_by
generated_at
handoff_fingerprint
```

#### Handoff rules

- unsupported or unknown `handoff_version` values are rejected fail closed; the runtime never best-effort parses a handoff artifact it does not explicitly support
- candidate handoff artifacts may exist, but only authoritative handoffs recorded by the runtime may satisfy `gate-handoff`, clear `handoff_required`, or reopen execution
- authoritative handoff fingerprints are computed by the runtime from canonical handoff content and must verify on later read before the handoff may satisfy resume or unblock execution
- authoritative handoff supersession and audit order are determined by runtime-assigned `authoritative_sequence` within the same `execution_run_id`, not by timestamp or file naming order
- a handoff is required whenever the active policy is `chunk-boundary` and a chunk ends
- a handoff is required whenever the runtime enters `handoff_required`
- a handoff may be required adaptively on repeated evaluation failures, explicit session separation, or other deterministic runtime conditions
- preflight must reject resume when a required handoff is missing or malformed
- a handoff must name one concrete next action; vague “continue from here” summaries are invalid

### 8. Status model

Extend `PlanExecutionStatus` with at least the following additional fields:

```text
execution_run_id
latest_authoritative_sequence
harness_phase
chunk_id
chunking_strategy
evaluator_policy
reset_policy
review_stack[]
active_contract_path
active_contract_fingerprint
required_evaluator_kinds[]
completed_evaluator_kinds[]
pending_evaluator_kinds[]
non_passing_evaluator_kinds[]
aggregate_evaluation_state   # pending | pass | fail | blocked
last_evaluation_report_path
last_evaluation_report_fingerprint
last_evaluation_evaluator_kind
last_evaluation_verdict
current_chunk_retry_count
current_chunk_retry_budget
current_chunk_pivot_threshold
handoff_required        # yes | no
open_failed_criteria[]
write_authority_state   # unclaimed | held | conflict
write_authority_holder  # controller/session identifier when known
write_authority_worktree  # worktree path or stable worktree id when known
repo_state_baseline_head_sha
repo_state_baseline_worktree_fingerprint
repo_state_drift_state  # clean | drifted | unknown
dependency_index_state  # clean | missing | inconsistent
final_review_state      # not_required | missing | fresh | stale
browser_qa_state        # not_required | missing | fresh | stale
release_docs_state      # not_required | missing | fresh | stale
last_final_review_artifact_fingerprint
last_browser_qa_artifact_fingerprint
last_release_docs_artifact_fingerprint
```

Status must remain readable before execution starts and must not require a running skill to infer the current execution law. It must also make write-authority blockage visible enough for an operator to distinguish “I can continue” from “another controller currently owns authoritative state mutation.” When known, `write_authority_worktree` should identify the holder's worktree path or stable worktree identifier for diagnosis, but that metadata is diagnostic only and does not create a separate authoritative scope. Same-branch multi-worktree sessions remain one authoritative execution scope; status must not imply separate run-identity spaces for each worktree. When repo-state provenance is load-bearing for the current phase or latest authoritative artifact, status must expose whether the run is clean or drifted relative to that baseline. When dependency truth is load-bearing for pruning, stale-cascade, or downstream gate behavior, status must expose whether the runtime currently considers the dependency index clean, missing, or inconsistent. When a chunk requires multiple evaluator kinds, status must make it explicit whether the chunk is still waiting on missing evaluator reports, blocked by a required evaluator, or failed by a required evaluator rather than collapsing those states into a generic latest-report verdict. `required_evaluator_kinds[]`, `completed_evaluator_kinds[]`, `pending_evaluator_kinds[]`, `non_passing_evaluator_kinds[]`, and `aggregate_evaluation_state` refer only to the active contract's inner-loop evaluator set, not to downstream review or QA gates in `review_stack[]`. `chunking_strategy`, `evaluator_policy`, `reset_policy`, and `review_stack[]` are the authoritative policy snapshot for the current `execution_run_id`; they must not drift mid-run, and any runtime-owned policy reset that changes one or more of those fields must mint a new `execution_run_id` through `execution_preflight` before further authoritative execution continues. `final_review_state`, `browser_qa_state`, and `release_docs_state` describe downstream-gate freshness for the current run and dependency truth: `not_required` means the gate is not required for the current run or current point in the workflow, `missing` means a required gate has no currently indexed authoritative downstream artifact, `fresh` means the indexed downstream artifact set is current for the active execution provenance, and `stale` means previously indexed downstream output exists but is no longer authoritative for the current run because provenance, repo state, or dependency truth invalidated it. `last_final_review_artifact_fingerprint`, `last_browser_qa_artifact_fingerprint`, and `last_release_docs_artifact_fingerprint` expose the last indexed authoritative downstream artifact for each gate when present. `last_evaluation_evaluator_kind` must expose which evaluator kind produced the latest authoritative evaluation report for the active contract. `latest_authoritative_sequence` must expose the most recent authoritative mutation order known to the runtime for the active run and must reset when a new `execution_run_id` is minted. `execution_run_id` must stay stable for the life of one harness-governed execution against a specific approved plan revision and policy snapshot and must change when a plan pivot re-enters through `execution_preflight` on a newly approved plan revision or when an explicit runtime-owned policy reset boundary adopts a different policy snapshot.

### 9. Evidence and provenance model

Current execution evidence already records per-step attempts, file proofs, verification summary, packet fingerprint, and HEAD/base SHA. Extend that provenance rather than replacing it.

#### New provenance requirements

Each completed attempt written by the new harness must be able to reference:

- active contract fingerprint
- source contract path
- source evaluation report fingerprint when completion follows a repair/evaluation cycle
- evaluator verdict that justified the continuation
- failing criterion IDs being addressed when work is in repair mode
- source handoff fingerprint when the session resumed from a required handoff
- source HEAD SHA
- source worktree fingerprint or equivalent runtime-owned repo-state snapshot identifier when repo-state-sensitive provenance matters

#### Invalidation rules

- the runtime-owned dependency index/reference graph is authoritative for determining which artifacts become stale because of reopen, supersession, downstream-gate derivation, or durable evidence dependencies; command implementations must not re-infer a narrower or different dependency set ad hoc
- when a step is reopened inside the active chunk, any dependent completion evidence for that step, any authoritative evaluation reports for the active chunk that relied on the reopened step, any authoritative handoff derived from that chunk state, and any downstream review, QA, release-doc, or finish-readiness artifacts that relied on that chunk provenance become stale
- when a contract is superseded by contract pivot or replacement, the old contract remains preserved but becomes non-active, and all completion evidence, evaluation reports, handoffs, and downstream gate artifacts derived from that contract become stale or superseded for authoritative use
- when an evaluation report is superseded by a later report for the same contract/evaluator kind, the authoritative ordering is determined by higher `authoritative_sequence` within the same `execution_run_id`, not by timestamp or filename
- when execution enters a plan-pivot blocked state, all execution-derived provenance and downstream gate artifacts tied to the superseded approved plan revision become stale for authoritative use until a newly approved plan revision re-enters through `execution_preflight`
- the runtime must never silently rewrite or discard stale artifacts; it must mark them superseded or stale through explicit provenance rules

#### Repo-state drift rules

- authoritative artifacts whose later use depends on repo-state provenance must capture the relevant HEAD SHA and worktree fingerprint or equivalent runtime-owned repo-state snapshot
- when an authoritative mutation or downstream gate depends on an earlier authoritative artifact, the runtime must compare current repo state against that artifact's repo-state provenance
- out-of-band HEAD or worktree drift marks the dependent artifact stale for that use and fails closed with `RepoStateDrift` until the run is reconciled, reopened, or re-evaluated
- the runtime must never silently refresh or overwrite repo-state provenance to hide drift

#### Artifact integrity rules

- authoritative artifact fingerprints are derived from a deterministic canonicalization of the parseable artifact content for that artifact version
- the runtime must recompute and verify authoritative artifact fingerprints on every later read that is used for state advancement, gate satisfaction, resume, review, or finish decisions
- if canonical content on disk no longer matches the recorded fingerprint, the artifact is invalid for authoritative use and the runtime fails closed with `ArtifactIntegrityMismatch`
- the runtime must never silently rewrite fingerprints, trust cached metadata over mismatched on-disk authoritative content, or downgrade an integrity failure to a warning

### 10. Intermediate gates

Add three new fail-closed gates:

#### `gate-contract`

Validates:

- current phase allows contract approval
- source plan/spec/task-packet fingerprints match
- contract scope matches real plan tasks/steps
- criteria are non-empty and map to requirement IDs and steps
- verifier set is valid for the active policy and excludes downstream gate-only modes from contract-level `verifiers[]`

#### `gate-evaluator`

Validates:

- evaluation report matches the active contract
- evaluator kind is expected by the current policy
- criterion IDs are valid for that contract
- verdict is one of `pass|fail|blocked`
- recommended action is legal for the verdict
- affected steps resolve to real steps in the plan
- `evidence_refs[]` entries conform to the minimum schema and their `kind`, `source`, `requirement_ids[]`, `covered_steps[]`, and `evidence_requirement_ids[]` validate against the active contract, the stable source-locator contract, the authoritative repo-state provenance contract for `repo:` locators, the fingerprint-based artifact-target contract for artifact-backed locators, any required first-class `EvidenceArtifact` backing for durable dirty-worktree evidence, and report content
- required contract evidence obligations are satisfied by authoritative `evidence_refs[]` traceable to the relevant criteria and covered steps under the active requirement entry's `satisfaction_rule`
- same-kind supersession is legal only within the same active contract; reports for missing or unexpected required evaluator kinds must not be treated as satisfying the active evaluator set
- recommended action does not override runtime-owned transition legality; it is accepted only as guidance inside the legal transition set for that verdict and current state

#### `gate-handoff`

Validates:

- a handoff is required or explicitly being recorded
- source contract fingerprint matches the active chunk
- open criteria and next action are present when unresolved work remains
- the handoff phase and resume path are legal

Preserve existing `gate-review` and `gate-finish` behavior, but extend them to account for unresolved harness failures and stale contract/evaluation provenance.
They must also fail closed on `RepoStateDrift` when the current repo state no longer matches the authoritative provenance they rely on.
They must also fail closed on `ArtifactIntegrityMismatch` when referenced authoritative artifacts no longer match their recorded canonical fingerprints.
When final review, browser QA, or release-doc outputs participate in stale-cascade, retention, or downstream gate truth, the runtime must treat their existing artifact files as fingerprinted indexed dependency inputs rather than relying on path-only references or introducing a separate harness-owned downstream artifact family in v1.

#### Multi-evaluator aggregation rules

- the active contract's `verifiers[]` defines the required evaluator-kind set for the active chunk
- downstream gate modes listed in `review_stack[]`, including `final_code_review` and `browser_qa`, do not participate in chunk-level aggregate evaluation
- the runtime remains in `evaluating` while any required evaluator kind lacks an authoritative report for the active contract
- aggregate evaluation precedence is deterministic: `blocked` > `fail` > `pending` > `pass`
- aggregate evaluation resolves to `blocked` when any required evaluator report for the active contract is `blocked`
- aggregate evaluation resolves to `fail` when no required evaluator is `blocked` and at least one required evaluator report for the active contract is `fail`
- aggregate evaluation resolves to `pending` when no required evaluator is `blocked` or `fail` but one or more required evaluator kinds still lack an authoritative report for the active contract
- aggregate evaluation resolves to `pass` only when every required evaluator kind for the active contract has an authoritative report and every one of those reports is `pass`
- downstream gates must not treat a chunk as passed while aggregate evaluation is `pending`, `fail`, or `blocked`
- when multiple runtime transitions are legal after aggregation, `recommended_action` may guide the choice among those legal transitions but must never create a transition that verdict or policy would otherwise forbid

### 11. Runtime transition logic

#### Pass path

- contract approved
- chunk steps execute
- aggregate evaluation resolves to `pass`
- runtime either:
  - opens the next chunk with `contract_drafting`, or
  - moves to `final_review_pending` when all required chunks are complete

#### Fail path

- aggregate evaluation resolves to `fail`
- runtime increments retry counter for the chunk
- runtime either:
  - enters `repairing` when within retry budget, or
  - enters `pivot_required` when the configured threshold is exceeded
- evaluator `recommended_action` may help select repair versus pivot only when both are already legal under runtime policy and retry state

#### Blocked path

- aggregate evaluation resolves to `blocked`
- runtime enters `handoff_required` or an explicit blocked state that must be resolved before further execution
- final review and finish gates must reject while blocked state remains unresolved
- evaluator `recommended_action` may suggest escalation or handoff, but the runtime decides the actual blocked-state transition

#### Pivot path

`pivot_required` means “the current contract or current implementation approach is no longer the correct path.” The runtime must allow at least two recoveries:

- **contract pivot**: stays inside the harness and re-enters `contract_drafting` with the same plan revision but a different chunk definition or verifier strategy
- **plan pivot**: exits active execution into a runtime-owned `stale_plan` or equivalent blocked-execution state when the approved plan itself no longer describes the right work

Plan-pivot rules:

- while blocked on a plan pivot, normal execution commands remain illegal and downstream review/finish gates remain closed
- only a newly approved plan revision may clear the blocked-execution state created by a plan pivot
- re-entry from a plan pivot must occur through `execution_preflight` against the new approved plan revision
- operator/status surfaces must make it explicit that execution is blocked on a new approved plan revision, even if the public phase remains `pivot_required`
- plan pivot stales all execution-derived downstream provenance for the superseded approved plan revision rather than allowing review, QA, release-doc, or finish artifacts from that revision to remain authoritative
- re-entry through `execution_preflight` on the newly approved plan revision must mint a new `execution_run_id` rather than continuing the superseded run identity

The runtime must distinguish contract-pivot and plan-pivot cases in reason codes.

### 12. Recommend / policy engine

Current recommendation only selects `featureforge:subagent-driven-development` or `featureforge:executing-plans`. Extend recommendation so the runtime emits a real harness policy.

#### Extended output

```text
recommended_skill
reason
decision_flags
chunking_strategy      # task | task-group | whole-run
evaluator_policy       # final-only | per-chunk | adaptive
reset_policy           # none | chunk-boundary | adaptive
review_stack[]         # spec_compliance, code_quality, final_code_review, browser_qa, ...
policy_reason_codes[]
```

The emitted policy fields are a run-scoped snapshot, not a live hint stream. `recommend` is proposal-only and side-effect free: it must not mint `execution_run_id`, allocate `authoritative_sequence`, mutate authoritative state, rewrite downstream freshness, or otherwise change active execution truth. Once `execution_preflight` accepts a policy snapshot for an `execution_run_id`, `chunking_strategy`, `evaluator_policy`, `reset_policy`, and `review_stack[]` remain fixed for that run. The runtime may adopt a different policy only through `execution_preflight` on a newly approved plan revision or an explicit runtime-owned policy reset boundary. Sane defaults for v1: `execution_preflight` is the only policy-commit point; it may validate a caller-supplied proposed snapshot or compute one itself when none is supplied, but the accepted authoritative snapshot always belongs to `execution_preflight`; the accepted snapshot is persisted in authoritative state plus structured policy-acceptance events, not as a separate preflight-artifact family; exact replay of `execution_preflight` against the same approved plan revision, same accepted policy snapshot, and same authoritative baseline is idempotent and returns the existing accepted result; materially different accepted inputs are not replay and must either mint a legal new run identity or fail closed; a policy reset boundary is legal only when no active contract is mid-execution, must be recorded explicitly, and must mint a new `execution_run_id` plus reset `authoritative_sequence` before authoritative execution resumes under the new policy snapshot.

#### Policy principles

- default to the simplest policy that still preserves quality
- use chunked execution when work is not comfortably inside the model’s reliable solo boundary
- use evaluator loops where the task still benefits from external judgment
- use handoff/reset only when the active policy or state makes it load-bearing
- do not silently recompute downstream gate requirements or other policy outputs underneath an active run

The exact heuristic thresholds do **not** need to be a public compatibility contract in v1, but the emitted policy fields are public.

### 13. Operator integration

The workflow operator must become harness-aware. The current phase model is too coarse once execution begins.

Recommended mapping:

- `implementation_handoff` when no harness state exists yet
- `execution_preflight` when preflight is the next required step
- `contracting` when the active harness phase is `contract_drafting` or `contract_pending_approval`
- `executing` when the phase is `contract_approved`, `executing`, or step-level work is active
- `evaluating` when an evaluation report is required or being processed
- `repairing` when runtime has accepted evaluation failure within retry budget
- `pivot_required` when further execution requires contract or plan change
- `handoff_required` when a reset/resume artifact is mandatory
- existing downstream `review_blocked`, `qa_pending`, `document_release_pending`, and `ready_for_branch_completion` remain

The public `next_action` mapping must distinguish these cases instead of collapsing them into generic “return to execution.” When execution is blocked pending a new approved plan revision, `next_action` and reason codes must say so explicitly rather than presenting the state as a generic pivot or generic blocked execution.
When evaluation is the reason work is blocked, repairing, or unable to advance, operator-visible output must identify the relevant evaluator kind rather than exposing only an aggregate verdict or report fingerprint.
When downstream review, QA, or release-doc truth is load-bearing, operator-visible output must expose the same per-gate freshness states as status rather than collapsing them into a generic downstream-blocked phase.
When write authority is the blocking condition, the operator must keep the current public phase and surface the blockage through `next_action`, `write_authority_state`, `write_authority_holder`, `write_authority_worktree`, and the stable reason code `write_authority_conflict` rather than inventing a separate public phase.
Operator-visible `reason_codes[]` must come from the stable minimum taxonomy rather than ad hoc per-command strings.

### 14. Skill normalization

FeatureForge already has the right skills; the runtime needs to normalize them.

#### Generator modes

- `featureforge:executing-plans`
- `featureforge:subagent-driven-development`

#### Inner-loop evaluator modes

- `spec_compliance` -> currently inside `subagent-driven-development`
- `code_quality` -> currently inside `subagent-driven-development`

#### Downstream gate modes

- `final_code_review` -> currently `featureforge:requesting-code-review`
- `browser_qa` -> currently `featureforge:qa-only`

#### Runtime contract

Inner-loop evaluator modes required by the active contract must emit a normalized `EvaluationReport` or a directly parseable artifact that the runtime can normalize into the common schema. Downstream gate modes may also emit parseable artifacts that the runtime normalizes into the same schema family for provenance, but those outputs remain downstream-gate evidence rather than chunk-pass evaluators. In v1, downstream review/QA/release-doc flows keep their existing artifact shapes; when those outputs become load-bearing for invalidation, retention, or gate truth, the runtime must fingerprint and index the existing artifacts as authoritative dependency inputs rather than cloning them into a new harness-owned downstream artifact family. Skills remain free to present richer human-readable output, but the machine-readable contract is required.

Add checked-in evaluator references and exemplars under existing review/QA support directories so evaluators can be tuned skeptically without hard-coding all behavior into the generator.

### 15. Relationship to task packets

Task packets remain authoritative task slices derived from the approved spec and approved plan. `ExecutionContract` does **not** replace them.

Relationship model:

```text
approved plan
  -> task packet(s)
  -> execution contract
  -> execution evidence
  -> evidence artifact(s)
  -> evaluation report(s)
  -> final review / QA / release docs
```

Requirements:

- each contract must cite the source packet fingerprint(s)
- packet `requirement_ids` remain the minimum traceability backbone for criteria
- contracts must not introduce requirements or non-goals that silently contradict the packet or approved plan

### 16. Storage model

Store harness artifacts under the existing project-scoped artifact root. The exact directory layout may be refined during implementation, but the contract is:

```text
~/.featureforge/projects/{repo_slug}/
  {user}-{safe_branch}-execution-harness-state.json
  {user}-{safe_branch}-execution-dependency-index.json
  {user}-{safe_branch}-execution-contract-{chunk_id}-{timestamp}.md
  {user}-{safe_branch}-evaluation-{evaluator_kind}-{chunk_id}-{timestamp}.md
  {user}-{safe_branch}-evidence-artifact-{evidence_kind}-{timestamp}.md
  {user}-{safe_branch}-handoff-{chunk_id}-{timestamp}.md
```

Rules:

- artifacts are branch-scoped
- same-branch worktrees share the same authoritative state and artifact namespace; the runtime must not fork authoritative storage by worktree path in v1
- artifacts are append-only from a provenance perspective
- state file points to the currently active contract/report/handoff where applicable
- artifacts remain human-readable markdown with parseable headers/sections
- timestamps or filename order may help operators browse files, but they are never the authoritative ordering primitive for supersession or audit truth
- optional JSON mirrors are allowed for runtime efficiency, but markdown is the primary interoperability contract
- the dependency index may reference existing downstream review/QA/release-doc artifacts by canonical fingerprint plus supporting metadata such as canonical path, but the runtime must not treat path alone as authoritative identity and does not require duplicate harness-owned copies of those downstream artifacts in v1

#### Retention and pruning rules

The harness may prune local artifacts, but only under bounded retention rules.

Rules:

- active authoritative artifacts referenced by current state must never be pruned
- candidate artifacts for an in-flight controller session must not be pruned while that session still depends on them
- stale or superseded artifacts must remain available while any active contract, evaluation, handoff, review, QA, release-doc, finish gate, or durable evidence reread path still depends on them
- pruning eligibility is runtime-owned and dependency-aware; operators must not have to guess whether an old artifact is still safe to delete
- once no active dependency remains, superseded or stale artifacts may be pruned only after a retention window has elapsed
- the exact default retention duration may be implementation-defined in v1, but the runtime must expose or document the chosen window and apply it consistently across artifact families
- pruning must never silently remove the only remaining authoritative artifact needed to explain current blocked state, current review/finish truth, or current evidence-resolution requirements
- pruning eligibility must be derived from the runtime-owned dependency index/reference graph, not from ad hoc scans of whichever artifacts a particular command happened to load
- pruning runs only at runtime-owned maintenance points such as `preflight`, startup recovery, or after a successful authoritative mutation commit; it must not race an in-flight authoritative mutation
- if the dependency index is missing, malformed, or inconsistent with authoritative state, the runtime must skip pruning and fail closed for any command or gate that requires dependency truth rather than guessing

#### Authoritative write ownership

The harness must enforce one authoritative writer for the active execution state per branch/plan execution scope.

Rules:

- `execution_preflight` or an equivalent runtime-owned resume path acquires or reconciles authoritative write ownership for the controlling session
- only the controller holding write authority may invoke authoritative mutation commands: `record-contract`, `record-evaluation`, `record-handoff`, `begin`, `note`, `complete`, `reopen`, and `transfer`
- subagents, reviewers, and parallel helpers may generate candidate contracts, evaluations, handoffs, or notes, but those artifacts remain non-authoritative until the controller records them through the runtime
- concurrent authoritative mutation attempts fail closed with a stable `ConcurrentWriterConflict` failure class
- same-branch worktrees are the same authoritative execution scope for write-authority purposes; a second worktree on the same branch must conflict or wait under the same single-writer rules rather than minting separate authoritative state
- when known, the runtime should record holder worktree identity as diagnostic metadata alongside controller identity so operators can see which same-branch worktree currently owns authority
- the exact lock or lease implementation may vary in v1, but authority release or stale-owner reclaim must be runtime-owned, deterministic, and visible to operators

#### Candidate versus authoritative artifacts

The harness must make candidate artifacts unambiguous.

Rules:

- each artifact family must distinguish `candidate` from `authoritative` either through a dedicated storage location/prefix or through an explicit parseable authority marker
- the active state file may reference only authoritative artifacts
- `record-contract`, `record-evaluation`, and `record-handoff` are the only commands that may promote candidate material into authoritative harness artifacts
- gates, status surfaces, and downstream review/finish gates must reject candidate artifacts when an authoritative artifact is required
- candidate artifacts may remain available for inspection or debugging, but they must not be mistaken for authoritative state by parsers or operators

#### Authoritative mutation atomicity and recovery

The harness must treat authoritative mutation as an atomic publication boundary.

Rules:

- each authoritative mutation command, including `record-contract`, `record-evaluation`, `record-handoff`, `begin`, `note`, `complete`, `reopen`, and `transfer`, must either commit its authoritative artifact and matching state transition together or leave the prior authoritative state unchanged
- the runtime must never expose a newly written authoritative artifact as active state until the corresponding authoritative state transition is durably committed
- crash, process exit, or torn-write scenarios must not silently advance authoritative state, even when candidate or staged files were created before the interruption
- restart, `preflight`, or other runtime-owned recovery entry points must detect incomplete authoritative mutations, fail closed with a stable failure class, and reconcile or discard incomplete staged writes before execution may resume
- recovery must be runtime-owned and deterministic; implementation details may vary, but the runtime must not require operators to guess which partially written artifact is authoritative

### 17. CLI surface

Extend `src/cli/plan_execution.rs` with new commands sufficient for runtime-owned orchestration. The minimum new surface is:

- `gate-contract`
- `record-contract`
- `gate-evaluator`
- `record-evaluation`
- `gate-handoff`
- `record-handoff`

Existing commands remain:

- `status`
- `recommend`
- `preflight`
- `gate-review`
- `gate-finish`
- `begin`
- `note`
- `complete`
- `reopen`
- `transfer`

#### CLI behavior requirements

- `record-contract` validates `gate-contract`, records the approved contract, and advances state from `contract_pending_approval` to `contract_approved`
- `record-evaluation` validates `gate-evaluator`, updates retry counters, records the evaluator result, and advances phase automatically
- `record-handoff` validates `gate-handoff`, records the handoff artifact, and clears `handoff_required` only when the recorded handoff satisfies the active policy
- `recommend` may return a proposed policy snapshot and its reason codes, but it must not mutate authoritative state, active status, downstream freshness, dependency truth, or run identity
- `execution_preflight` is the only command that may accept and persist the authoritative policy snapshot for a run; when it accepts a materially different snapshot, it must mint the corresponding new `execution_run_id` and reset run-scoped ordering
- exact replay of `execution_preflight` against the same approved plan revision, same accepted policy snapshot, and same authoritative baseline returns the existing accepted result without minting a second run identity, duplicating policy-acceptance events, or resetting run-scoped ordering
- replay of `execution_preflight` with mismatched approved plan revision, accepted policy snapshot, or authoritative baseline is not exact replay; it must either follow the legal new-run boundary rules or fail closed with `IdempotencyConflict`
- accepted policy snapshots must be durably represented in authoritative state and correlated policy-acceptance events; the runtime must not require a separate policy-artifact file to determine the active run policy in v1
- current write authority validation applies across all local worktrees on the same branch because branch scope, not worktree path, defines authoritative execution scope in v1
- authoritative mutation commands validate current write authority before changing harness state
- authoritative mutations and downstream gates verify authoritative artifact integrity on read before changing state or approving completion
- authoritative mutations and downstream gates validate repo-state drift against the authoritative provenance they depend on before changing state or approving completion
- authoritative mutations, downstream gates, stale-cascade decisions, and pruning decisions that rely on dependency truth must load the runtime-owned dependency index and fail closed when it is missing, malformed, or inconsistent with authoritative state
- authoritative mutation commands commit atomically; on crash, torn write, or interrupted publication they must leave the previously authoritative state intact and surface a stable failure class until recovery reconciles incomplete writes
- replaying an identical authoritative `record-contract`, `record-evaluation`, or `record-handoff` submission against the same expected state returns the existing authoritative result and must not create duplicate authoritative artifacts or duplicate state transitions
- replaying a mismatched authoritative submission, or replaying after the expected state has changed, fails closed with `IdempotencyConflict`
- `begin`/`complete`/`reopen`/`transfer` fail with stable failure classes when illegal in the current harness phase
- new failure messages must be specific enough to tell the caller which artifact or phase is missing

#### Minimum failure classes

The runtime may add more failure classes, but the following minimum machine-readable taxonomy is part of the harness contract:

- `IllegalHarnessPhase`: the requested command is not legal in the current macro-state
- `StaleProvenance`: the referenced contract, evaluation, handoff, plan, spec, or completion evidence is stale or superseded
- `ContractMismatch`: the referenced step, artifact, or packet provenance does not match the active contract scope
- `EvaluationMismatch`: the evaluation artifact does not match the active contract, evaluator policy, criterion set, or expected evaluator kind
- `MissingRequiredHandoff`: execution cannot resume or advance because the active policy/state requires a valid handoff artifact and none is available
- `NonHarnessProvenance`: active execution, review, or finish attempted to rely on evidence outside the harness-governed provenance contract after cutover
- `BlockedOnPlanPivot`: execution is locked pending a new approved plan revision after a plan-pivot decision
- `ConcurrentWriterConflict`: authoritative state mutation was attempted by a controller that does not currently hold write authority, or by a competing controller while write authority is already held elsewhere
- `UnsupportedArtifactVersion`: the referenced contract, evaluation, or handoff artifact declares a version the runtime does not explicitly support
- `NonAuthoritativeArtifact`: a candidate or otherwise non-authoritative artifact was supplied where an authoritative runtime-recorded artifact is required
- `IdempotencyConflict`: a repeated authoritative mutation request does not match the already-recorded artifact or the expected current state for safe replay
- `RepoStateDrift`: current HEAD or worktree state no longer matches the authoritative repo-state provenance required for the requested mutation, gate, or downstream check
- `ArtifactIntegrityMismatch`: a recorded authoritative fingerprint no longer matches the canonical on-disk content for the authoritative artifact being read
- `PartialAuthoritativeMutation`: the runtime detected a crash-interrupted, torn, or otherwise incomplete authoritative mutation that must be reconciled before execution may proceed
- `AuthoritativeOrderingMismatch`: authoritative sequence data is missing, duplicated, non-monotonic, or otherwise insufficient to determine supersession and audit order fail closed
- `DependencyIndexMismatch`: the runtime-owned dependency index is missing, malformed, stale relative to authoritative state, or otherwise inconsistent with the dependency truth required for invalidation, pruning, or downstream gate decisions

Failure-class rules:

- these class names are normative minimums for v1
- callers must not be forced to parse free-form prose to distinguish these cases
- user-facing error text may be richer, but the machine-readable failure class must remain stable
- a single failure may include additional detail fields, but it must map to one primary failure class

### 18. Cutover model

This harness is a hard cutover for active execution under `featureforge plan execution`.

#### Cutover rules

- once enabled, `featureforge plan execution` reads and writes only harness-governed artifacts for active execution
- pre-harness execution evidence is not a supported continuation source
- status, operator routing, and downstream gates fail closed when required harness artifacts are missing, malformed, non-harness, or stale
- final review, browser QA, release docs, and finish readiness trust only harness provenance for the active execution path

### 19. Observability contract

The harness is runtime-owned, so its control-plane behavior must be observable without reconstructing intent from free-form prose or ad hoc local debugging.

#### Minimum structured event fields

At minimum, harness transition, gate, blocked-state, and downstream-gate events must carry:

```text
event_kind
timestamp
execution_run_id
authoritative_sequence      # when an authoritative mutation or authoritative artifact is involved
source_plan_path
source_plan_revision
harness_phase
chunk_id                  # when a chunk exists
evaluator_kind            # when an evaluation artifact or evaluator-driven transition exists
active_contract_fingerprint
evaluation_report_fingerprint
handoff_fingerprint
command_name
gate_name
failure_class
reason_codes[]
```

Rules:

- `execution_run_id` must remain stable for the active harness-governed run
- `execution_run_id` remains stable across reopen, repair, handoff, and contract pivot within the same approved plan revision and frozen policy snapshot
- a new `execution_run_id` must be minted when execution re-enters through `execution_preflight` on a newly approved plan revision after a plan pivot or when an explicit runtime-owned policy reset boundary adopts a different policy snapshot
- events may omit `chunk_id`, `evaluation_report_fingerprint`, `handoff_fingerprint`, `evaluator_kind`, or `authoritative_sequence` only when those artifacts or authoritative semantics do not exist for that event
- `chunk_id` must remain stable while the active contract definition is unchanged and must change when the runtime activates a different contract definition
- authoritative supersession and event ordering must follow monotonic `authoritative_sequence` within the same `execution_run_id`, not timestamp sort order
- `(execution_run_id, authoritative_sequence)` is the authoritative ordering identity; `authoritative_sequence` alone is not branch-global
- operator-visible blocked and failure states must be traceable back to the structured event stream through these identifiers

#### Minimum reason-code taxonomy

The runtime may add more reason codes, but the following minimum machine-readable vocabulary is part of the harness contract:

- `waiting_on_required_evaluator`: one or more required evaluator kinds have not yet produced authoritative reports for the active contract
- `required_evaluator_failed`: at least one required evaluator kind has produced a failing authoritative report for the active contract
- `required_evaluator_blocked`: at least one required evaluator kind has produced a blocked authoritative report for the active contract
- `handoff_required`: execution cannot continue until a valid handoff artifact is recorded or resolved
- `repair_within_budget`: runtime has accepted evaluator failure and selected repair while retry budget remains
- `pivot_threshold_exceeded`: repeated failure exceeded the configured pivot threshold for the active chunk
- `blocked_on_plan_revision`: execution is blocked pending a newly approved plan revision after a plan pivot
- `write_authority_conflict`: authoritative progress is blocked because another controller currently holds or conflicts for write authority in the same branch-scoped execution scope
- `repo_state_drift`: current repo state no longer matches authoritative provenance needed for the requested action or gate
- `stale_provenance`: required authoritative provenance is stale, superseded, or invalid for the requested action or gate
- `recovering_incomplete_authoritative_mutation`: execution is blocked on runtime-owned recovery from a partial or interrupted authoritative mutation
- `missing_required_evidence`: required contract evidence has not been supplied or cannot be traced to the relevant criteria and covered steps
- `invalid_evidence_satisfaction_rule`: an evidence requirement declared an unknown or unsupported `satisfaction_rule`

Reason-code rules:

- these reason-code names are normative minimums for v1
- callers must not be forced to parse prose to distinguish these common blocked or transition causes
- a single event or status surface may include multiple reason codes when more than one applies
- implementations may add narrower reason codes, but they must not silently replace or rename the minimum vocabulary above

#### Required event types

The minimum observability contract must cover:

- phase-transition events with previous phase, next phase, and triggering command or verdict
- recommendation proposal events must remain distinct from policy-acceptance events; only the latter may be treated as authoritative run-configuration changes
- evaluation-related phase transitions and gate results must include `evaluator_kind` whenever a specific evaluator report or evaluator-driven aggregate change is involved
- gate-result events for `gate-contract`, `gate-evaluator`, `gate-handoff`, `gate-review`, and `gate-finish`
- blocked-state entry and exit events for `handoff_required`, `pivot_required`, and plan-pivot blocked execution
- write-authority conflict and reclaim events for authoritative mutation attempts
- authoritative mutation replay events that distinguish accepted idempotent replay from rejected replay conflict
- preflight replay events that distinguish accepted exact replay from rejected mismatched replay or accepted new-run policy/reset boundaries
- repo-state drift detection and reconciliation events
- artifact-integrity mismatch detection events for authoritative artifact reads
- partial-authoritative-mutation detection and recovery events for interrupted authoritative writes
- downstream gate rejection events when review, QA, release-doc, or finish checks fail because harness provenance is stale, missing, or non-harness

#### Metrics and operator-facing expectations

The implementation may choose the exact metrics backend, but the runtime contract must preserve machine-readable counts or equivalent telemetry for:

- phase transitions by from/to phase
- gate failures by gate name and failure class
- blocked-state entries by blocked reason
- retry and pivot counts by chunk
- evaluation outcomes by evaluator kind and verdict
- authoritative mutation counts and ordering gaps by command family
- write-authority conflicts and reclaim events by result
- authoritative replay accepts and replay conflicts by command
- repo-state drift detections and reconciliations by command or gate
- artifact-integrity mismatches by artifact type and command or gate
- partial-authoritative-mutation detections and recoveries by command family

The spec does not require a specific dashboard product in v1, but it does require that operators can answer:

- which run is blocked
- which chunk and contract caused the block
- which gate or evaluator failed
- which evaluator kind most recently reported or currently blocks advancement
- which authoritative mutation or artifact superseded the previous one and in what order
- which failure class and reason code explain the failure
- whether the run is waiting on a handoff, a contract rewrite, or a new approved plan revision
- whether authoritative write ownership is held, conflicted, or waiting on reclaim
- whether repo-state drift invalidated the currently relied-on evaluation, handoff, or downstream gate input
- whether an authoritative artifact failed fingerprint verification on read
- whether execution is blocked on recovery from an incomplete authoritative mutation
- which stable reason codes explain the current blocked state or next action

### 20. Failure Modes and Edge Cases

The runtime must make each of the following explicit and testable.

Each failure mode below must map to one or more stable failure classes from the minimum taxonomy in Section 17 rather than relying only on free-form error strings.

#### Contract failures

- contract declares an unknown or unsupported `contract_version`
- contract artifact is candidate/non-authoritative when an authoritative contract is required
- contract references the wrong plan revision or fingerprint
- contract references task packets that do not match the approved plan
- contract has no criteria or no covered steps
- contract declares an unknown or unsupported `satisfaction_rule` value in `evidence_requirements[]`
- `begin` targets a step outside the active contract

#### Evaluation failures

- evaluation report declares an unknown or unsupported `report_version`
- evaluation artifact is candidate/non-authoritative when an authoritative evaluation report is required
- evaluator report is for a stale or wrong contract
- criterion IDs do not belong to the active contract
- evaluation report contains an `evidence_ref` with an unknown or unsupported `kind`
- evaluation report contains an `evidence_ref` missing required schema fields or using an unsupported, malformed, or unresolved `source` locator
- evaluator reports `pass` while containing unresolved required failing criteria
- evaluator report claims `pass`, but required contract evidence is missing, non-authoritative, or not traceable to the relevant criteria and covered steps
- evaluation report uses a supported `evidence_ref.kind`, but the cited `source` does not match that kind's runtime meaning and the runtime accepts it anyway
- evaluation report uses a `repo:` source locator that escapes the repository root, uses path traversal, or points to a non-canonical repository location and the runtime accepts it anyway
- evaluation report uses a `repo:` source locator and the runtime resolves it against the live worktree even though the authoritative repo-state baseline is unavailable, stale, or drifted
- evaluation report cites uncommitted local code through a `repo:` locator, the authoritative `HEAD` plus worktree baseline is available, but the runtime rejects it solely because the content is not committed
- evaluation report cites dirty-worktree `repo:` evidence, but the runtime records only the baseline fingerprint and later cannot reread the exact cited content needed for validation or downstream gates
- evaluation report cites line-qualified dirty-worktree `repo:` evidence, but the runtime preserves only the cited span and later cannot deterministically re-read the full provenance-bound file context
- evaluation report depends on durable dirty-worktree `repo:` evidence, but no authoritative `EvidenceArtifact` exists or multiple candidate artifacts ambiguously match the same evidence ref and baseline
- evaluation report depends on an `EvidenceArtifact` with an unknown or unsupported `evidence_artifact_version`, or a candidate-only evidence artifact is accepted as if it were authoritative
- evaluation report uses an artifact-backed locator whose `<artifact_ref>` does not resolve by canonical fingerprint, resolves ambiguously, or is accepted on the basis of path/name matching instead
- evaluation report claims evidence for a requirement ID, covered step, or `evidence_requirement_id` that does not belong to the active contract or cited criterion scope
- evaluator report claims `pass`, but supplied evidence only partially satisfies an `all_of` or `per_step` requirement and the runtime accepts it anyway
- repeated fail exceeds threshold but runtime keeps trying to execute instead of pivoting

#### Handoff failures

- handoff declares an unknown or unsupported `handoff_version`
- handoff artifact is candidate/non-authoritative when an authoritative handoff is required
- handoff is required but missing
- handoff lacks a concrete next action
- resume occurs from a different contract or different chunk without explicit pivot
- a blocked run resumes without resolving the block

#### Concurrency failures

- a second controller attempts to mutate authoritative harness state while another controller holds write authority
- a subagent or helper attempts to call an authoritative mutation command directly
- a controller exits or loses liveness while still holding write authority
- stale or orphaned write authority is not reclaimed deterministically before the next controller attempts to proceed
- the same branch is opened in a second worktree and the runtime incorrectly creates a separate authoritative state or run identity instead of treating it as a single-writer conflict on the same scope

#### Mutation replay failures

- a controller retries the same authoritative `record-*` command after a local timeout and the runtime incorrectly duplicates the state transition
- a controller retries with a different artifact or after state drift and the runtime accepts the replay instead of failing closed
- an accepted replay incorrectly increments retry counters, advances phase again, or writes a second authoritative artifact

#### Atomicity and crash-recovery failures

- a crash occurs after writing a new authoritative artifact candidate or staged file, but before the authoritative state pointer is durably updated, and the runtime later treats that partial mutation as committed
- a crash occurs after updating authoritative state, but before the corresponding authoritative artifact is durably published, and later reads accept the broken state/artifact pairing
- restart or `preflight` resumes execution without detecting incomplete authoritative mutation state
- recovery silently guesses the winning artifact after a partial mutation instead of deterministically reconciling or failing closed

#### Repo-state drift failures

- HEAD moves outside the harness after an evaluation report is recorded, but downstream review still accepts that evaluation as current
- worktree contents drift outside the harness after a handoff is recorded, but preflight resumes without requiring reconciliation
- final review or finish readiness accepts stale repo-state provenance after out-of-band local changes
- the runtime detects drift but silently rewrites provenance instead of requiring reopen, reconciliation, or re-evaluation

#### Invalidation-cascade failures

- `reopen` stales step completion evidence but leaves the active chunk's evaluation, handoff, or downstream gate artifacts authoritative
- contract pivot replaces the active chunk, but earlier evaluation, handoff, review, QA, release-doc, or finish artifacts derived from the superseded contract remain accepted
- plan pivot blocks execution, but downstream gates still rely on execution-derived provenance from the superseded approved plan revision
- different runtime entry points disagree about which downstream artifacts became stale after reopen or pivot

#### Artifact integrity failures

- an authoritative contract, evaluation, or handoff artifact is edited on disk after recording, but later reads still trust the recorded fingerprint
- the runtime uses cached or state-file metadata instead of re-verifying canonical artifact content on read
- downstream review or finish consumes an authoritative artifact whose on-disk content no longer matches its recorded fingerprint
- integrity mismatch is detected but treated as a warning instead of a fail-closed blocker

#### Retention failures

- the runtime prunes an active authoritative artifact still referenced by current state
- the runtime prunes a stale or superseded artifact that is still needed by current review, QA, release-doc, finish, or durable evidence reread dependencies
- the runtime keeps artifacts forever because pruning eligibility is undefined or not dependency-aware
- different runtime entry points disagree about whether a superseded artifact is still retention-protected
- pruning proceeds even though the runtime-owned dependency index is missing, malformed, or inconsistent with authoritative state
- different commands, gates, or maintenance points infer different dependency graphs for the same artifacts instead of using the runtime-owned index

#### Dependency-index failures

- the dependency index omits an authoritative artifact, downstream gate input, or durable evidence edge that later matters to invalidation, pruning, or gate truth
- the dependency index references artifacts, fingerprints, or sequences that do not exist in authoritative state
- authoritative mutation commits state and artifact changes without atomically updating the dependency index to the same authoritative truth boundary
- startup, `preflight`, or maintenance loads a missing or malformed dependency index and continues with ad hoc dependency inference anyway
- downstream gates, stale-cascade logic, and pruning eligibility disagree because they read different dependency snapshots or bypass the runtime-owned index

#### Downstream gate failures

- final review requested while unresolved failed criteria remain
- final review requested while one required evaluator kind is still missing or `blocked`, but a passing report from another evaluator kind is incorrectly treated as sufficient
- final review requested against stale completion evidence after reopen
- final review or finish readiness requested against non-harness provenance after the harness cutover
- finish readiness requested when QA or release docs are fresh for HEAD but contract/evaluation provenance is stale for the active execution path
- chunk pass is incorrectly treated as satisfying final review or browser QA merely because downstream gate outputs were normalized into evaluation-shaped artifacts

#### Evaluator aggregation failures

- one required evaluator kind reports `pass`, but the runtime advances even though another required evaluator kind has not yet reported
- one required evaluator kind reports `pass`, while another required evaluator kind reports `fail`, and the runtime incorrectly aggregates to `pass`
- one required evaluator kind reports `blocked`, but the runtime enters repair or next-chunk advancement instead of remaining blocked
- a superseded report for one evaluator kind remains in the aggregate verdict after a later authoritative report of the same kind is recorded
- a downstream gate mode such as `final_code_review` or `browser_qa` is incorrectly accepted as a contract-level evaluator kind or counted toward aggregate chunk pass

#### Evaluator recommendation failures

- an evaluation report with `recommended_action: continue` causes advancement even though the authoritative verdict or aggregate state is `fail` or `blocked`
- an evaluation report with `recommended_action: handoff` or `escalate` bypasses runtime policy or phase legality instead of being treated as bounded guidance
- the runtime treats `recommended_action` as authoritative even when it conflicts with retry budget, pivot threshold, or blocked-state rules

#### Evaluator identity observability failures

- an evaluation-driven block or repair state is visible, but status/operator output does not identify which evaluator kind caused it
- a structured evaluation-related event omits `evaluator_kind`, forcing operators to dereference artifacts just to learn which evaluator reported
- multi-evaluator runs expose aggregate `fail` or `blocked` state without naming the non-passing evaluator kinds responsible

#### Reason-code taxonomy failures

- operator or event surfaces emit ad hoc reason strings instead of the stable minimum reason-code vocabulary
- a blocked-on-plan-revision state omits `blocked_on_plan_revision` and forces clients to infer the cause from prose
- evaluator-driven pending, fail, or blocked states omit the corresponding required-evaluator reason codes
- recovery from incomplete authoritative mutation is visible only as generic blocked execution instead of `recovering_incomplete_authoritative_mutation`
- missing required evidence is exposed only as prose or a generic evaluator failure instead of `missing_required_evidence`

#### Authoritative ordering failures

- two authoritative artifacts of the same family rely on timestamp or filename order instead of a runtime-owned monotonic sequence to decide which one supersedes the other
- an authoritative artifact is missing `authoritative_sequence`, duplicates an existing sequence, or records a non-monotonic sequence relative to prior authoritative state
- event ordering and artifact supersession disagree because one path uses timestamp order while another uses runtime mutation order
- `authoritative_sequence` is treated as branch-global instead of run-scoped, causing cross-run supersession or audit confusion after a new `execution_run_id` is minted

#### Interaction edge cases

- plan update required after repeated evaluator failures
- browser-facing work under whole-run chunking still needs late browser QA
- subagent mode uses per-task evaluator loops while whole-diff review remains required
- a session resumes on a new day with no valid handoff artifact

#### Identity rollover failures

- `reopen` or handoff resume incorrectly mints a new `execution_run_id`, breaking continuity for the same approved plan revision
- contract pivot incorrectly mints a new `execution_run_id` instead of preserving one run identity across the same approved plan revision
- plan-pivot re-entry on a newly approved plan revision incorrectly reuses the superseded `execution_run_id`
- `chunk_id` changes for the same active contract definition, or stays the same after the runtime activates a different contract definition

#### Policy snapshot failures

- `review_stack[]`, `chunking_strategy`, `evaluator_policy`, or `reset_policy` changes underneath an active `execution_run_id` without an explicit runtime-owned policy reset boundary
- `execution_preflight` adopts a materially different policy snapshot but incorrectly reuses the prior `execution_run_id`
- different commands or entry points observe different policy snapshots for the same `execution_run_id`
- downstream gate freshness or dependency truth is evaluated against a recomputed `review_stack[]` instead of the frozen run-scoped policy snapshot
- `recommend` output is treated as authoritative without an `execution_preflight` acceptance boundary
- `recommend` mutates active status, run identity, or downstream freshness even though no policy snapshot has been accepted
- authoritative state and structured policy-acceptance events disagree about the accepted policy snapshot for the same `execution_run_id`
- the runtime requires a missing standalone policy-artifact file to reconstruct active run policy even though authoritative state and events exist
- exact replay of `execution_preflight` incorrectly mints a new `execution_run_id`, duplicates policy-acceptance events, or resets `authoritative_sequence`
- mismatched `execution_preflight` replay is silently collapsed into the prior accepted run instead of failing closed or following legal new-run boundary rules

### 21. Verification Strategy

FeatureForge already has runtime tests and workflow fixtures. The new harness must extend those suites rather than relying on prompt instructions.

#### Required automated coverage

1. **State transition tests**
   - happy path: contract -> execute -> evaluate pass -> next chunk
   - fail path: evaluate fail -> repairing -> pass
   - repeated fail path: evaluate fail beyond threshold -> pivot_required
   - blocked path: evaluate blocked -> handoff_required
   - multi-evaluator pass path waits for every required evaluator kind to report `pass`
   - multi-evaluator aggregation stays `pending` while any required evaluator kind is still missing
   - multi-evaluator aggregation resolves to `blocked` when any required evaluator kind reports `blocked`
   - multi-evaluator aggregation resolves to `fail` when no required evaluator kind is `blocked` and at least one required evaluator kind reports `fail`
   - `execution_run_id` stays stable across repair, handoff resume, reopen, and contract pivot within the same approved plan revision
   - plan-pivot re-entry on a newly approved plan revision mints a new `execution_run_id`
   - explicit runtime-owned policy reset on the same approved plan revision mints a new `execution_run_id` only when the policy snapshot actually changes
   - exact replay of `execution_preflight` with the same accepted policy snapshot and same authoritative baseline returns the same `execution_run_id`
   - `chunk_id` changes only when the active contract definition changes
   - `authoritative_sequence` resets when a new `execution_run_id` is minted and remains monotonic only within that run

2. **Artifact parsing tests**
   - valid/invalid contract parsing
   - valid/invalid evaluation parsing
   - valid/invalid handoff parsing
   - valid/invalid `EvidenceArtifact` parsing and fingerprint verification
   - unsupported artifact-version rejection for contract, evaluation, handoff, and evidence artifacts
   - candidate/non-authoritative artifact rejection when authoritative artifacts are required
   - contract rejection when `verifiers[]` includes downstream gate-only modes such as `final_code_review` or `browser_qa`
   - contract parsing and validation for required `evidence_requirements[]`, including explicit empty-list handling when no additional evidence is required
   - contract parsing rejects unknown `satisfaction_rule` values and accepts the stable minimum vocabulary
   - evaluation parsing and validation for required `evidence_refs[]` fields, including explicit empty-list handling for `evidence_requirement_ids[]` when a ref is informational rather than satisfying a declared evidence requirement
   - evaluation parsing rejects unknown `evidence_ref.kind` values and validates the stable minimum kind vocabulary
   - evaluation parsing accepts only the stable minimum `source` locator shapes and rejects malformed, unresolved, non-canonical, or kind-incompatible locators
   - `repo:` locator validation requires the authoritative repo-state baseline and rejects repo-backed evidence when that baseline is unavailable, stale, or drifted
   - `repo:` locator validation accepts authoritative dirty-worktree baselines when `repo_state_baseline_head_sha` and `repo_state_baseline_worktree_fingerprint` prove the cited snapshot
   - dirty-worktree `repo:` evidence preserves or materializes the exact cited content needed for later reread rather than storing fingerprint-only proof when later validation depends on content access
   - dirty-worktree `repo:` evidence preserves whole-file content for later reread even when the original locator is line-qualified
   - durable dirty-worktree `repo:` evidence materializes a valid `EvidenceArtifact` and later resolution finds exactly one matching authoritative artifact for the cited source and baseline
   - artifact-backed locator parsing resolves `<artifact_ref>` by canonical fingerprint and rejects path-only, unresolved, or ambiguous artifact targets
   - repo-state provenance parsing and comparison for authoritative artifacts that depend on it
   - authoritative-sequence parsing, run-scoped uniqueness, and within-run monotonicity validation for contract, evaluation, and handoff artifacts
   - authoritative fingerprint recomputation and mismatch rejection for contract, evaluation, and handoff artifacts
   - fingerprint mismatch handling
   - partial-authoritative-mutation detection and rejection for interrupted artifact/state publication
   - retention eligibility checks reject pruning for artifacts still referenced by active state or dependency paths
   - accepted policy snapshot parsing/validation from authoritative state and structured policy-acceptance events does not depend on a separate policy artifact family
   - dependency-index parsing and validation rejects missing, malformed, or authoritative-state-inconsistent graph state when dependency truth is load-bearing

3. **Command legality tests**
   - `begin` blocked outside legal phases
   - `complete` blocked outside active contract scope
   - `record-evaluation` blocked when contract mismatch exists
   - `record-handoff` blocked when no handoff is required and no explicit override is allowed
   - failure-class assertions for illegal phase, stale provenance, contract mismatch, evaluation mismatch, missing required handoff, non-harness provenance, blocked-on-plan-pivot, concurrent-writer-conflict, unsupported-artifact-version, non-authoritative-artifact, idempotency-conflict, repo-state-drift, artifact-integrity-mismatch, partial-authoritative-mutation, and authoritative-ordering-mismatch cases
   - `recommend` is side-effect free: repeated calls may change proposed output, but they do not mutate authoritative state, accepted policy snapshot, run identity, or downstream freshness
   - `recommend` or `preflight` may propose a new policy snapshot, but active status and downstream truth do not change until a runtime-owned reset boundary or new approved plan revision mints a new `execution_run_id`
   - `execution_preflight` is the only command that may accept and persist the authoritative policy snapshot for the run
   - exact replay of `execution_preflight` returns the existing accepted result without duplicate policy-acceptance side effects
   - mismatched replay of `execution_preflight` fails with `IdempotencyConflict` unless it qualifies as a legal new-run boundary
   - commands or gates that rely on dependency truth fail with `DependencyIndexMismatch` when the dependency index is missing, malformed, stale relative to authoritative state, or inconsistent with authoritative fingerprints/sequences
   - identical authoritative `record-*` replay returns the existing result without duplicate state mutation
   - mismatched authoritative replay fails with `IdempotencyConflict`
   - a passing report from one required evaluator kind cannot advance the chunk while other required evaluator kinds remain missing, blocked, or failed
   - downstream gate artifacts normalized into evaluation-shaped provenance cannot satisfy `gate-evaluator` or unblock chunk pass aggregation
   - `recommended_action` cannot cause a transition that is illegal for the verdict, current phase, retry state, or runtime policy
   - `gate-evaluator` rejects reports whose `evidence_refs[]` do not satisfy the active contract's `evidence_requirements[]`
   - `gate-evaluator` rejects reports whose `evidence_refs[]` use missing fields, unparseable sources, or requirement/step traceability that does not match the active contract
   - `gate-evaluator` rejects reports whose `evidence_ref.kind` and `source` combination does not match the stable minimum kind semantics
   - `gate-evaluator` rejects reports whose `source` locator does not match the stable minimum locator grammar or canonicalization rules for the declared evidence kind
   - `gate-evaluator` rejects `repo:` locators when the authoritative repo-state baseline they depend on is unavailable or drifted instead of falling back to the live worktree
   - `gate-evaluator` accepts `repo:` locators backed by an authoritative dirty-worktree baseline when the recorded `HEAD` plus worktree fingerprint proves the cited snapshot
   - `gate-evaluator` rejects dirty-worktree `repo:` evidence when the exact provenance-bound content required for later reread was not durably preserved or materialized
   - `gate-evaluator` rejects dirty-worktree `repo:` evidence that preserved only a cited span when whole-file reread is required by the contract
   - `gate-evaluator` rejects durable dirty-worktree `repo:` evidence when it cannot resolve exactly one authoritative `EvidenceArtifact` matching the cited source and baseline
   - `gate-evaluator` rejects artifact-backed locators whose target identity is derived from path or name matching instead of canonical fingerprint resolution
   - `gate-evaluator` applies deterministic `all_of`, `any_of`, and `per_step` semantics rather than evaluator-local interpretation

4. **Invalidation-cascade tests**
   - `reopen` stales the active chunk's dependent evaluation, handoff, and downstream gate artifacts rather than only the reopened step evidence
   - contract pivot supersedes the active contract and invalidates all derived execution, evaluation, handoff, and downstream gate artifacts for that contract
   - plan pivot blocks execution and invalidates all execution-derived downstream provenance for the superseded approved plan revision
   - status, operator, review, QA, release-doc, and finish surfaces agree on the same invalidation boundary after reopen and both pivot kinds
   - invalidation-cascade logic derives stale artifacts from the runtime-owned dependency index rather than ad hoc per-command inference

5. **Operator routing tests**
   - `contracting`
   - `evaluating`
   - `repairing`
   - `pivot_required`
   - `handoff_required`
   - downstream review/QA/release/finish transitions
   - evaluator-driven blocked or repairing states expose the relevant evaluator kind in operator/status output
   - operator-visible blocked and next-action states expose the stable minimum reason codes for the underlying cause
   - status/operator output exposes `final_review_state`, `browser_qa_state`, and `release_docs_state` with deterministic `not_required` / `missing` / `fresh` / `stale` semantics for the active run
   - status/operator output exposes one frozen policy snapshot per `execution_run_id` and shows the new snapshot only after a recorded policy reset boundary or new approved plan revision preflight
   - write-authority conflict keeps the current public phase and exposes `write_authority_conflict`, `write_authority_state`, `write_authority_holder`, and `write_authority_worktree` rather than introducing a separate operator phase

6. **Hard-cutover tests**
   - pre-harness execution evidence cannot be resumed under harness-governed execution
   - status/operator/gates fail closed when required harness artifacts are absent, malformed, or stale
   - final review and finish reject non-harness provenance after cutover
   - existing downstream review/QA/release-doc artifact shapes remain valid only when the runtime has fingerprinted and indexed them as authoritative dependency inputs where gate truth depends on them

7. **Repo-state drift tests**
   - authoritative mutations fail closed when prerequisite repo-state provenance has drifted out of band
   - review and finish gates reject stale HEAD/worktree provenance until reconciliation, reopen, or re-evaluation
   - reconciliation or re-evaluation clears `RepoStateDrift` only by producing fresh authoritative provenance

8. **Observability contract tests**
   - structured event output includes stable run/chunk/phase/contract identifiers
   - evaluation-related structured events include `evaluator_kind` whenever an evaluation artifact or evaluator-driven transition is involved
   - authoritative mutation and supersession events expose monotonic `authoritative_sequence` values
   - blocked-state and transition events emit the stable minimum reason-code vocabulary where applicable
   - missing required evidence surfaces `missing_required_evidence` through status/operator/event reason codes when evidence obligations prevent aggregate pass
   - gate failures expose gate name, failure class, and relevant artifact identifiers
   - blocked plan-pivot state is observable as distinct from generic execution failure
   - accepted authoritative replay and replay-conflict outcomes are observable as distinct events
   - recommendation proposal events and policy-acceptance events are observable as distinct event kinds and only policy-acceptance events correlate with run-identity changes
   - policy-acceptance events carry enough machine-readable policy fields to reconstruct the accepted snapshot together with authoritative state, without requiring a standalone policy artifact
   - preflight exact-replay and preflight replay-conflict outcomes are observable as distinct events
   - repo-state drift and reconciliation outcomes are observable as distinct events
   - artifact-integrity mismatch outcomes are observable as distinct events
   - interrupted authoritative mutation detection and recovery outcomes are observable as distinct events
   - downstream gate rejections expose stale, missing, or non-harness provenance through structured identifiers
   - dependency-index mismatch and pruning-skip outcomes are observable as distinct events with stable failure or reason identifiers

9. **Single-writer tests**
   - only the current controller may mutate authoritative harness state
   - concurrent mutation attempts fail with `ConcurrentWriterConflict`
   - subagent-generated candidate artifacts do not advance authoritative state until the controller records them
   - stale-owner release or reclaim is required before a new controller can resume authoritative mutation
   - same-branch multi-worktree sessions share one authoritative execution scope and conflict under the same single-writer rules instead of creating separate branch-local truths

10. **Authority-boundary tests**
   - candidate contract, evaluation, and handoff artifacts are distinguishable from authoritative artifacts on disk or in parseable headers
   - state/status surfaces never point at candidate artifacts as active authoritative state
   - review and finish gates reject candidate artifacts when authoritative provenance is required

11. **Fixture tests**
   - add representative workflow artifacts under the existing fixture roots for authoritative and candidate contract/evaluation/handoff artifacts, stale contract, stale evaluation, repo-state drift rejection, partial-authoritative-mutation recovery, non-harness provenance rejection, and pivot-required cases
   - add retention/pruning fixtures covering active authoritative artifacts, superseded artifacts still needed by downstream gates, and safely prunable stale artifacts after the retention window
   - add dependency-index fixtures covering clean graph state, missing/malformed graph state, stale graph state after authoritative mutation, and graph-backed invalidation/pruning consistency
   - add downstream-gate fixtures showing existing final-review, QA, and release-doc artifact shapes indexed by canonical fingerprint without requiring duplicate harness-owned artifact copies

### 22. Implementation Phasing

#### Phase 1: Data model and status extension

- add `HarnessPhase`
- add extended `PlanExecutionStatus`
- add new request/response structs for contract/evaluation/handoff commands
- add authoritative write-ownership metadata or lease state for the active execution controller
- record optional holder worktree metadata for diagnostics while keeping authoritative state branch-scoped rather than worktree-scoped
- add repo-state provenance fields and drift-state exposure in status and authoritative artifacts
- add canonical artifact fingerprint helpers and read-time verification for authoritative artifacts
- add authoritative-mutation staging/commit markers or equivalent runtime metadata needed for deterministic crash recovery
- add aggregate-evaluation-state fields and required/pending evaluator-kind tracking to status
- add evaluator-kind attribution fields to status and machine-readable event emission
- add runtime-owned monotonic authoritative-sequence fields to artifacts, state, and event emission
- scope the authoritative-sequence allocator to `execution_run_id` and treat `(execution_run_id, authoritative_sequence)` as the ordered identity
- add stable minimum reason-code enums/constants for operator, status, and event surfaces
- add downstream gate freshness fields and last-indexed downstream artifact fingerprints to status and operator output using the stable `not_required` / `missing` / `fresh` / `stale` vocabulary
- persist the accepted policy snapshot per `execution_run_id` and validate that status/operator surfaces never mix fields from different snapshots
- keep proposed recommendation output separate from accepted policy state so `recommend` cannot accidentally mutate authoritative run configuration
- keep accepted policy snapshots in authoritative state plus structured event payloads rather than introducing a new checked local policy-artifact family
- implement exact-replay detection for `execution_preflight` using accepted policy snapshot, approved plan revision, and authoritative baseline identity
- add contract evidence-requirement validation and evidence-ref traceability checks to evaluator gating and aggregate-pass logic
- add stable `satisfaction_rule` parsing and runtime evaluation semantics for `all_of`, `any_of`, and `per_step`
- add `evidence_ref` schema parsing and source/traceability validation for evaluation artifacts
- add stable `evidence_ref.kind` parsing and kind-specific source validation for the minimum vocabulary
- add stable `evidence_ref.source` locator parsing, canonicalization, and kind-compatible resolution for the minimum vocabulary
- bind `repo:` evidence resolution to authoritative repo-state provenance instead of the live worktree when provenance is load-bearing
- allow authoritative dirty-worktree repo baselines when `repo_state_baseline_head_sha` plus `repo_state_baseline_worktree_fingerprint` proves the cited snapshot
- preserve or materialize dirty-worktree repo-backed content needed for later reread, not just its baseline fingerprint
- preserve whole-file dirty-worktree repo-backed snapshots when later reread is required, even for line-qualified locators
- implement `EvidenceArtifact` parsing, fingerprinting, storage, and deterministic resolution for durable runtime-materialized evidence
- add canonical fingerprint-based resolution for artifact-backed evidence targets, with fail-closed rejection for unresolved or ambiguous targets
- add dependency-aware artifact-retention bookkeeping and pruning eligibility checks across harness artifact families
- add a runtime-owned dependency index/reference graph model persisted alongside authoritative state and updated at the same authoritative truth boundary as artifact/state mutations
- add canonical fingerprint capture and dependency-index integration for existing downstream review/QA/release-doc artifacts that become load-bearing gate inputs
- distinguish contract-level inner-loop evaluator kinds from downstream gate modes in policy and status surfaces
- add deterministic run-identity and chunk-identity rollover fields to status and event emission surfaces
- keep old status fields intact

#### Phase 2: Artifact parsing and gates

- implement `ExecutionContract`, `EvaluationReport`, `ExecutionHandoff` parsers
- implement `gate-contract`, `gate-evaluator`, `gate-handoff`
- verify authoritative artifact integrity on parser reads used by mutations, gates, resume, review, and finish
- implement incomplete authoritative mutation detection on startup, `preflight`, or equivalent recovery entry points
- add invalidation logic

#### Phase 3: Runtime transition engine

- implement macro-state transition table
- bind `record-contract`, `record-evaluation`, `record-handoff`
- validate `begin`/`complete`/`reopen`/`transfer` against macro-state and contract scope
- validate authoritative write ownership and deterministic reclaim before every state mutation
- make authoritative artifact publication and state-pointer updates atomic from the runtime contract perspective
- implement idempotent replay handling for authoritative `record-*` commands and fail-closed replay mismatch detection
- detect out-of-band HEAD/worktree drift before authoritative mutations and downstream gates that depend on authoritative repo-state provenance
- implement deterministic all-required evaluator aggregation across authoritative reports for the active contract
- reject downstream gate-only modes in contract-level verifier sets and keep downstream gate artifacts out of chunk-pass aggregation
- keep evaluator `recommended_action` advisory and map it only within the legal transition set for the authoritative verdict and runtime policy
- keep `execution_run_id` stable within one approved plan revision and one frozen policy snapshot; mint a new run identity only on plan-pivot re-entry to a newly approved plan revision or an explicit policy reset boundary that adopts a different policy snapshot
- use runtime-owned monotonic authoritative sequence to resolve supersession, replay ordering, and audit order instead of timestamps or filenames
- implement deterministic invalidation-cascade marking for reopen, contract pivot, and plan pivot
- use the runtime-owned dependency index for stale-cascade, pruning eligibility, and downstream gate truth, with fail-closed `DependencyIndexMismatch` behavior when the graph is missing or inconsistent
- extend `recommend`

#### Phase 4: Operator integration

- update `featureforge workflow operator` phase mapping
- add harness-aware `next_action`
- preserve current downstream behavior for review/QA/release/finish
- surface blocked recovery when execution cannot resume until an incomplete authoritative mutation is reconciled

#### Phase 5: Skill normalization

- update generator/evaluator skills to emit normalized artifacts
- add evaluator references/exemplars
- keep the existing human-readable skill behavior where useful

#### Phase 6: Tests and fixtures

- add Rust and codex-runtime fixtures
- add contract transition, evaluator, handoff, stale-provenance, hard-cutover, observability-contract, and single-writer cases
- update docs and schemas

### 23. Risks and Mitigations

- **Risk:** The new harness adds too much scaffolding for stronger models.  
  **Mitigation:** Make evaluator and reset usage policy-driven and removable rather than mandatory.

- **Risk:** Artifact overhead becomes noisy and slow.  
  **Mitigation:** Keep artifacts local, structured, append-only, and scoped to the smallest useful chunk.

- **Risk:** Evaluator quality is too lenient or too noisy.  
  **Mitigation:** Normalize evaluator outputs, use explicit criteria, and add exemplars/rubrics instead of relying on self-critique.

- **Risk:** Runtime and skill prose diverge.  
  **Mitigation:** Make the Rust state machine authoritative and gate illegal transitions fail-closed.

- **Risk:** Hard cutover lands before the harness covers every active execution dependency.  
  **Mitigation:** Complete contract, evaluator, handoff, downstream-gate, and cutover verification before enabling the harness path; do not ship a partial dual-path rollout.

- **Risk:** The harness fails closed but remains opaque during real incidents.  
  **Mitigation:** Require structured observability for transitions, gate failures, blocked states, and downstream-gate rejections before the harness path is considered complete.

- **Risk:** Multiple controllers or subagents race authoritative harness state and corrupt the execution law.  
  **Mitigation:** Enforce single-writer authority for authoritative state mutation, surface conflicts with stable failure classes, and require deterministic runtime-owned release or reclaim.

- **Risk:** The same branch is opened in multiple local worktrees and the harness silently forks authoritative execution truth by worktree path.  
  **Mitigation:** Keep authoritative state branch-scoped, treat same-branch worktrees as one single-writer scope, and surface holder worktree identity only as diagnostic metadata.

- **Risk:** Retry after timeout or partial client failure duplicates authoritative mutations and corrupts counters or phase advancement.  
  **Mitigation:** Make authoritative `record-*` mutations idempotent for identical replay, reject mismatched replay with a stable failure class, and verify no duplicate side effects occur.

- **Risk:** Out-of-band HEAD or worktree drift makes authoritative evaluation or handoff provenance lie about the current repo state.  
  **Mitigation:** Capture repo-state provenance on authoritative artifacts, surface `RepoStateDrift` fail-closed, and require reconciliation, reopen, or re-evaluation before proceeding.

- **Risk:** Authoritative markdown artifacts are edited on disk after recording, causing fingerprints and state references to lie about actual content.  
  **Mitigation:** Compute fingerprints from canonical content, re-verify on authoritative reads, and fail closed on `ArtifactIntegrityMismatch`.

- **Risk:** Final review/finish truth becomes weaker after adding intermediate artifacts.  
  **Mitigation:** Extend downstream gates to account for stale contract/evaluation provenance rather than bypassing them.

- **Risk:** Local crash or torn write leaves the harness in a half-published authoritative state that later commands treat as truth.  
  **Mitigation:** Make authoritative mutations atomic, detect incomplete mutation state on restart or preflight, and fail closed with deterministic runtime-owned recovery before execution resumes.

- **Risk:** Reopen or pivot invalidates only part of the downstream evidence chain, leaving review or finish surfaces with contradictory views of what is still authoritative.  
  **Mitigation:** Make stale-provenance cascades deterministic for reopen, contract pivot, and plan pivot, and verify that all status and downstream gates apply the same invalidation boundary.

- **Risk:** A passing report from one evaluator kind masks a missing, failing, or blocked report from another required evaluator kind, causing the runtime to advance on incomplete judgment.  
  **Mitigation:** Treat the contract's required evaluator set as all-required, expose aggregate evaluation state in status, and make multi-evaluator aggregation deterministic and fail closed.

- **Risk:** Downstream final review or QA evidence gets double-counted as chunk-pass evaluation, weakening the intended boundary between the inner execution loop and downstream gates.  
  **Mitigation:** Keep contract-level `verifiers[]` limited to inner-loop evaluator kinds, preserve final review and QA as downstream gates, and allow normalized downstream artifacts only as provenance rather than chunk-pass inputs.

- **Risk:** Evaluator `recommended_action` silently becomes a second control plane and overrides the runtime's verdict, retry budget, or phase legality.  
  **Mitigation:** Keep `recommended_action` advisory only, validate it against the verdict, and let the runtime choose transitions strictly within the legal set defined by verdict and policy.

- **Risk:** Observability and provenance lose continuity because reopen or contract pivot incorrectly starts a new run identity, or plan-pivot/policy-reset re-entry incorrectly reuses the old one across incompatible execution snapshots.  
  **Mitigation:** Keep `execution_run_id` stable within one approved plan revision and frozen policy snapshot, mint a new run identity only on plan-pivot re-entry or an explicit policy reset boundary that adopts a different policy snapshot, and tie `chunk_id` changes only to active contract-definition changes.

- **Risk:** Multi-evaluator failures are visible only as aggregate state or report fingerprints, forcing manual artifact lookups to learn which evaluator actually failed or blocked.  
  **Mitigation:** Emit `evaluator_kind` in evaluation-related structured events and expose relevant evaluator kinds directly in status/operator output.

- **Risk:** Supersession, replay, or audit order is inferred from wall-clock timestamps or filenames, producing ambiguous or inconsistent authoritative truth under clock skew, retries, or file-copy artifacts.  
  **Mitigation:** Assign a runtime-owned monotonic authoritative sequence to authoritative artifacts and state transitions, fail closed on missing or non-monotonic ordering, and use that sequence consistently for supersession and observability.

- **Risk:** A new run reuses or is compared against prior-run sequence numbers as if `authoritative_sequence` were branch-global, creating false supersession or broken audit continuity across plan revisions.  
  **Mitigation:** Scope `authoritative_sequence` to `execution_run_id`, reset it on new-run creation, and treat `(execution_run_id, authoritative_sequence)` as the authoritative ordering identity.

- **Risk:** Clients and operators cannot reliably diagnose blocked states because `reason_codes[]` drift into ad hoc implementation-local strings.  
  **Mitigation:** Define a stable minimum reason-code taxonomy for common blocked and transition causes and verify that operator, status, and event surfaces emit it consistently.

- **Risk:** `evidence_requirements[]` becomes decorative metadata, allowing a chunk to pass without the concrete evidence the contract said was required.  
  **Mitigation:** Make evaluator gating and aggregate pass fail closed on missing or untraceable required evidence, and surface `missing_required_evidence` as a stable reason code.

- **Risk:** `satisfaction_rule` exists in schema only, allowing different runtime paths or evaluators to interpret the same evidence obligation differently.  
  **Mitigation:** Define and test a stable minimum `satisfaction_rule` vocabulary with fail-closed rejection for unknown values.

- **Risk:** `evidence_refs[]` remains too loose for the runtime to validate evidence deterministically, forcing gate logic back into evaluator-specific inference.  
  **Mitigation:** Define a minimum machine-readable `evidence_ref` schema with parseable source and explicit requirement/step linkage.

- **Risk:** `evidence_ref.kind` becomes an ungoverned label, so identical-looking evidence refs carry different meanings across evaluators or runtime paths.  
  **Mitigation:** Define a stable minimum `kind` vocabulary with kind-compatible source validation and fail-closed rejection for unknown kinds.

- **Risk:** `evidence_ref.source` remains vague enough that different parsers accept different locator shapes or resolve the same evidence differently, breaking deterministic gating.  
  **Mitigation:** Define a stable minimum source-locator grammar, require canonicalization before authoritative fingerprinting, and reject unresolved or kind-incompatible locators fail closed.

- **Risk:** Artifact-backed evidence resolves by path or filename convention instead of canonical fingerprint, making evidence truth drift when files move, names collide, or stale copies remain on disk.  
  **Mitigation:** Make `<artifact_ref>` fingerprint-addressed, treat paths as supporting metadata only, and reject unresolved or ambiguous artifact targets fail closed.

- **Risk:** Repo-backed code evidence silently rebinds to the live worktree after edits, so the same `repo:` locator points at different content over time.  
  **Mitigation:** Bind `repo:` locators to authoritative repo-state provenance and reject them fail closed when the required baseline is unavailable or drifted.

- **Risk:** The harness requires artificial commits just to cite truthful local code evidence, weakening real local execution and encouraging provenance-distorting workflow workarounds.  
  **Mitigation:** Allow authoritative dirty-worktree baselines when `HEAD` plus worktree fingerprint proves the exact cited snapshot.

- **Risk:** Dirty-worktree `repo:` evidence proves identity at record time but cannot be reread later because the exact cited content was never durably preserved.  
  **Mitigation:** Require durable preservation or materialization of provenance-bound dirty-worktree content whenever later validation or downstream gates depend on rereading it.

- **Risk:** Span-only preservation for line-qualified dirty-worktree `repo:` evidence loses surrounding file context and makes later validation or human review depend on guessed context reconstruction.  
  **Mitigation:** Preserve whole-file provenance-bound content whenever durable dirty-worktree repo evidence is required.

- **Risk:** Durable dirty-worktree evidence exists only as opaque internal storage, making later validation, debugging, and fingerprint-based artifact resolution less trustworthy than the rest of the harness artifact model.  
  **Mitigation:** Materialize durable runtime-backed evidence as first-class local `EvidenceArtifact` files with canonical fingerprints and deterministic resolution rules.

- **Risk:** Local harness artifacts accumulate without bound, or worse, cleanup deletes artifacts that still determine current truth.  
  **Mitigation:** Use runtime-owned dependency-aware retention with bounded pruning windows and fail-closed protection for any artifact still needed by active state or downstream gates.

- **Risk:** Dependency truth is re-inferred differently by different commands, so stale-cascade, pruning, and downstream gates disagree about which artifacts still matter.  
  **Mitigation:** Maintain a runtime-owned dependency index updated with authoritative mutations, expose its health in status, and fail closed with `DependencyIndexMismatch` whenever dependency truth cannot be trusted.

- **Risk:** The harness duplicates downstream review/QA/release-doc artifacts into a second artifact family, creating drift between native downstream outputs and what the runtime thinks those gates produced.  
  **Mitigation:** Keep existing downstream artifact shapes as canonical in v1 and fingerprint/index them as authoritative dependency inputs whenever downstream gate truth depends on them.

- **Risk:** Operators can see that execution is blocked downstream, but not whether review, QA, or release docs are missing versus stale, which weakens diagnosis and makes dependency-truth bugs harder to spot.  
  **Mitigation:** Expose per-gate downstream freshness fields and last-indexed downstream artifact fingerprints directly in status and operator output.

- **Risk:** The runtime silently recomputes `review_stack[]` or other policy fields mid-run, so downstream requirements and execution semantics drift underneath already-recorded provenance.  
  **Mitigation:** Freeze the emitted policy tuple per `execution_run_id`, allow changes only through recorded preflight-owned reset boundaries, and mint a new run identity whenever a materially different policy snapshot is adopted.

- **Risk:** `recommend` becomes a soft-authoritative side channel, so callers cannot tell whether they are seeing a proposal or active execution law.  
  **Mitigation:** Keep `recommend` side-effect free, make `execution_preflight` the only policy-acceptance boundary, and observe proposal versus acceptance as distinct event types.

- **Risk:** Policy acceptance grows a redundant local artifact family whose contents can drift from authoritative state and event truth.  
  **Mitigation:** Persist accepted policy snapshots in authoritative state plus structured policy-acceptance events only, and do not add a separate policy-artifact family in v1.

- **Risk:** Retry after timeout at `execution_preflight` mints duplicate run identities or duplicate policy-acceptance events even though the accepted inputs are unchanged.  
  **Mitigation:** Make exact `execution_preflight` replay idempotent on accepted snapshot plus authoritative baseline identity, and surface mismatched replay as `IdempotencyConflict` unless it is a legal new-run boundary.

## Acceptance Criteria

The work is complete only when all of the following are true:

1. The current outer workflow still routes work to `implementation_ready` exactly as before.
2. `featureforge plan execution status --plan ...` exposes harness phase, current policy, active contract, and latest evaluation state.
3. The runtime enforces legal macro-state transitions in Rust rather than relying on skill prose.
4. The runtime rejects step execution outside the active contract scope.
5. Contract approval is runtime-owned and impossible without successful `gate-contract` validation against matching plan/spec/task-packet provenance.
6. Evaluator results are normalized into a common report model with per-criterion findings tied to requirement IDs and steps.
7. Evaluation failure automatically drives repair, pivot, or handoff transitions according to runtime policy.
8. Handoff-required execution cannot resume without a valid handoff artifact.
9. `recommend` returns policy beyond just skill choice.
10. Existing generator and evaluator skills operate through the normalized contract rather than as free-form execution law.
11. Final code review, browser QA, release docs, and finish readiness remain fail-closed and are aware of stale harness provenance.
12. Active execution under `featureforge plan execution` uses only harness-governed artifacts; pre-harness execution evidence is not a supported continuation path.
13. Automated tests and fixtures cover happy path, repair path, repeated-fail pivot, blocked handoff, contract mismatch, evaluation mismatch, stale provenance rejection, and hard-cutover behavior.
14. Harness commands and gates emit the stable minimum failure-class taxonomy rather than relying on free-form error text for control-plane decisions.
15. The runtime emits the minimum structured observability contract for transitions, gate failures, blocked states, and downstream-gate rejections with stable run/chunk/phase/artifact identifiers.
16. Only one controller may mutate authoritative harness state for an active branch/plan execution scope at a time; subagents may generate candidate artifacts but cannot advance authoritative state directly.
17. Unknown or unsupported artifact versions are rejected fail closed with a stable failure class; the runtime never best-effort parses unsupported contract, evaluation, handoff, or evidence artifacts.
18. Candidate artifacts are marked or stored separately from authoritative artifacts, and only authoritative runtime-recorded artifacts may satisfy gates, appear as active artifacts, or advance state.
19. Identical authoritative `record-*` replay is safe and side-effect free, while mismatched replay fails closed with a stable failure class.
20. Out-of-band HEAD or worktree drift against authoritative repo-state provenance fails closed until the run is reconciled, reopened, or re-evaluated.
21. Authoritative artifact fingerprints are computed from canonical content, re-verified on later authoritative reads, and fail closed on mismatch.
22. Authoritative mutations commit atomically; crash-interrupted or torn authoritative mutations fail closed and require runtime-owned recovery before execution may proceed.
23. `reopen`, contract pivot, and plan pivot apply deterministic stale-provenance cascades across dependent evaluation, handoff, and downstream gate artifacts rather than leaving invalidation scope implementation-defined.
24. Required evaluator kinds aggregate deterministically and fail closed: a chunk passes only when every required evaluator kind for the active contract has an authoritative passing report, while missing, failing, or blocked required evaluator kinds prevent advancement according to the aggregation rules.
25. Contract-level `verifiers[]` remain limited to inner-loop evaluator kinds; downstream gate modes such as `final_code_review` and `browser_qa` may emit normalized provenance artifacts but do not participate in chunk pass aggregation or replace downstream gate decisions.
26. Evaluator `recommended_action` remains bounded guidance only; verdict, phase legality, retry state, and runtime policy remain authoritative over actual transitions.
27. `execution_run_id` remains stable across normal execution, repair, handoff, reopen, and contract pivot within one approved plan revision and frozen policy snapshot; plan-pivot re-entry on a newly approved plan revision or an explicit policy reset boundary that adopts a different policy snapshot mints a new run identity; `chunk_id` changes only when the active contract definition changes.
28. Evaluation-related structured events and relevant status/operator outputs expose `evaluator_kind` whenever an evaluation artifact, evaluator result, or evaluator-driven transition/block is involved.
29. Authoritative contracts, evaluations, handoffs, and state transitions carry a runtime-owned monotonic authoritative sequence used for supersession, replay ordering, and audit truth rather than timestamps or filenames.
30. `authoritative_sequence` is scoped to `execution_run_id`, resets when a new run is minted, and is used together with `execution_run_id` as the authoritative ordering identity.
31. `reason_codes[]` emit the stable minimum machine-readable taxonomy for blocked states and evaluator/runtime transitions rather than ad hoc strings.
32. Contract-declared `evidence_requirements[]` are enforced fail closed: aggregate pass and `gate-evaluator` reject reports whose authoritative `evidence_refs[]` do not satisfy the required evidence obligations for the active contract.
33. `evidence_requirements[].satisfaction_rule` uses stable minimum runtime semantics: at least `all_of`, `any_of`, and `per_step` are supported, and unknown rule values are rejected fail closed instead of being interpreted ad hoc.
34. `EvaluationReport.evidence_refs[]` uses a minimum machine-readable schema with stable identity, source, and requirement/step linkage fields so evidence satisfaction can be validated fail closed rather than inferred from evaluator prose.
35. `EvaluationReport.evidence_refs[].kind` uses a stable minimum vocabulary with deterministic runtime meaning, and `gate-evaluator` rejects unknown kinds or kind/source combinations that do not match the supported semantics.
36. `EvaluationReport.evidence_refs[].source` uses a stable minimum locator contract with canonical kind-compatible shapes, and `gate-evaluator` rejects malformed, unresolved, non-canonical, or kind-incompatible evidence locators.
37. Artifact-backed evidence locators resolve `<artifact_ref>` by canonical artifact fingerprint, with path treated only as supporting metadata; unresolved, ambiguous, or path-derived artifact targeting is rejected fail closed.
38. Repo-backed `repo:` evidence locators resolve only against authoritative repo-state provenance for the relevant evaluation/run, and fail closed when that provenance is unavailable or drifted rather than rebinding to the live worktree.
39. Repo-backed `repo:` evidence may cite authoritative dirty-worktree snapshots when `repo_state_baseline_head_sha` plus `repo_state_baseline_worktree_fingerprint` proves the exact baseline; clean committed `HEAD` is a valid special case, not the only allowed source of code evidence.
40. Dirty-worktree `repo:` evidence is durably preserved or materialized when later validation or downstream gates depend on rereading the exact cited content; baseline fingerprint proof alone is not sufficient in those cases.
41. Durable dirty-worktree `repo:` evidence snapshots preserve whole-file provenance-bound content, not span-only excerpts, even when the original locator is line-qualified.
42. Durable dirty-worktree `repo:` evidence is materialized as a first-class local `EvidenceArtifact` with canonical fingerprint and deterministic resolution, not left as opaque internal storage.
43. Local harness artifacts follow bounded runtime-owned retention rules: active or still-dependent artifacts are retained, and superseded or stale artifacts are pruned only after no active dependency remains and the retention window has elapsed.
44. A runtime-owned dependency index/reference graph determines invalidation, pruning eligibility, and downstream gate dependency truth; commands and gates fail closed when that graph is missing, malformed, or inconsistent with authoritative state.
45. Existing downstream review, QA, and release-doc artifact shapes remain canonical in v1, and the runtime fingerprints/indexes them as authoritative dependency inputs whenever stale-cascade, retention, or downstream gate truth depends on them.
46. Status and operator output expose explicit downstream gate freshness for final review, browser QA, and release docs, using deterministic `not_required`, `missing`, `fresh`, and `stale` states plus last-indexed downstream artifact fingerprints when present.
47. The emitted policy tuple is frozen per `execution_run_id`: `chunking_strategy`, `evaluator_policy`, `reset_policy`, and `review_stack[]` do not change mid-run, and any policy reset that materially changes them occurs only through `execution_preflight` and mints a new run identity.
48. `recommend` remains advisory and side-effect free, while `execution_preflight` is the only command that may accept and persist the authoritative policy snapshot for a run.
49. Accepted policy snapshots remain reconstructable from authoritative state plus structured policy-acceptance events; v1 does not require a separate policy-artifact family.
50. Exact replay of `execution_preflight` against the same accepted policy snapshot, approved plan revision, and authoritative baseline returns the existing accepted result without minting a second run identity or duplicate policy-acceptance side effects; mismatched replay fails closed unless it is a legal new-run boundary.
51. Same-branch multi-worktree sessions share one branch-scoped authoritative execution scope and must conflict under the single-writer rules rather than minting separate authoritative state or run identities; worktree identity is diagnostic only.
52. Write-authority conflict does not become a separate public phase; it remains visible through `next_action`, stable `reason_codes[]`, and write-authority holder/worktree metadata within the current public phase.
53. The workflow operator exposes harness-aware public phases and next actions that distinguish contracting, evaluating, repairing, pivot-required, and handoff-required execution states.

## ASCII Diagrams

### Control boundary

```text
repo-visible workflow
  brainstorming
    -> spec review
    -> plan writing
    -> plan review
    -> implementation_ready
                         |
                         v
local execution harness (Rust-owned)
  preflight
    -> contract
    -> execute
    -> evaluate
       -> repair
       -> pivot
       -> handoff
    -> final review
    -> QA
    -> release docs
    -> finish
```

### Provenance chain

```text
approved spec
   + approved plan
        -> task packet(s)
            -> execution contract
                -> execution evidence
                -> evaluation report(s)
                -> handoff (if needed)
                    -> final code review
                    -> QA result
                    -> release docs
                    -> finish gate
```

### Dependency truth and stale-cascade

```text
authoritative state
  -> active contract fingerprint
  -> active evaluation fingerprint(s)
  -> active handoff fingerprint
  -> indexed downstream fingerprints
  -> dependency index / reference graph
       |
       +--> execution artifacts
       +--> evidence artifacts
       +--> downstream review / QA / release-doc inputs
       +--> retention-protected stale artifacts

runtime event
  reopen | contract pivot | plan pivot | downstream refresh | prune check
       |
       v
dependency index is authoritative
  -> compute affected artifact set
  -> mark stale / superseded artifacts
  -> update downstream freshness states
  -> preserve still-dependent artifacts
  -> allow pruning only after no active dependency remains

commands and gates
  gate-review / gate-finish / status / prune / invalidation
       |
       v
  all read the same dependency graph

if dependency index is missing / malformed / inconsistent
  -> fail closed
  -> no ad hoc dependency inference
  -> no pruning
```

### Policy Proposal and Acceptance

```text
recommend
  -> proposed policy snapshot
       chunking_strategy
       evaluator_policy
       reset_policy
       review_stack[]
  -> proposal only
  -> no run identity change
  -> no authoritative state change

execution_preflight
  + approved plan revision
  + authoritative baseline
  + proposed policy snapshot (or runtime-computed equivalent)
       |
       +--> exact replay of already accepted snapshot/baseline
       |      -> return existing execution_run_id
       |      -> no duplicate acceptance side effects
       |
       +--> newly approved plan revision
       |      -> mint new execution_run_id
       |
       +--> explicit policy reset boundary with different snapshot
       |      -> mint new execution_run_id
       |      -> reset run-scoped ordering
       |
       +--> otherwise
              -> accept snapshot
              -> persist in authoritative state
              -> emit policy-acceptance event

status / operator / downstream truth
  -> read only the accepted snapshot
  -> never read proposal-only output as execution law
```
