# Runtime Safety Audit Remediation Plan

**Workflow State:** Draft
**Plan Revision:** 1
**Execution Mode:** featureforge:executing-plans
**Source Spec:** `docs/featureforge/reference/2026-05-06-deep-runtime-safety-audit.md`
**Source Spec Revision:** 1
**Source Report:** `docs/featureforge/reference/2026-05-06-deep-runtime-safety-audit.md`
**Last Reviewed By:** writing-plans

## Goal

Eliminate the remaining public/private runtime traps found in the 2026-05-06 deep audit so FeatureForge's public workflow surface is safe for agents to follow without rediscovering hidden command identities, reconstructing deprecated proof/provenance state, parsing text-mode display output, or relying on internal test-only setup as public proof.

## Architecture

The remediation has four coordinated surfaces:

1. **Event authority identity:** Public aggregate commands must own the command identity recorded into authoritative event-log envelopes. Typed event payloads may still be granular, but envelope command names must not resurrect hidden primitive commands for normal public flows.
2. **Public route serialization and text UX:** JSON route output must expose enough structured command authority for input-required routes, and text outputs/skills must either use JSON or point to one concrete JSON rerun path.
3. **Post-closure control-plane cleanup:** Closure history should be authoritative when it can reconstruct current closure truth. Worktree lease provenance and closure overlay repair should not force post-closure workflow detours unless safety is actually blocked.
4. **Decision/test realism guardrails:** Workflow plan routing and late-stage command mode selection should have single shared resolvers; public-flow tests should distinguish synthetic historical fixture setup from public end-to-end proof.

## Change Surface

- Modify: `src/execution/event_log.rs`
- Modify: `src/execution/recording.rs`
- Modify: `src/execution/closure_dispatch_mutation/recording.rs`
- Modify: `src/execution/commands/advance_late_stage.rs`
- Modify: `src/execution/commands/close_current_task.rs`
- Modify: `src/execution/command_eligibility.rs`
- Modify: `src/execution/command_eligibility/late_stage.rs`
- Modify: `src/execution/late_stage_route_selection.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/commands/common/operator_outputs.rs`
- Modify: `src/execution/read_model.rs`
- Modify: `src/execution/read_model_support.rs`
- Modify: `src/execution/follow_up.rs`
- Modify: `src/execution/transitions.rs`
- Modify: `src/execution/invariants.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/workflow/doctor_dashboard.rs`
- Modify: `src/workflow/doctor_resolution.rs`
- Modify: `src/workflow/status.rs`
- Modify: `skills/*.md.tmpl`
- generated `skills/*/SKILL.md`
- Modify: `tests/public_cli_flow_contracts.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_runtime_final_review.rs`
- Modify: `tests/public_replay_churn.rs`
- Modify: `tests/runtime_behavior_golden.rs`
- Modify: `tests/runtime_authority_contracts.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/codex-runtime/*.test.mjs`

## Preconditions

- Work from a clean git worktree.
- Do not edit generated `skills/*/SKILL.md` directly when a corresponding `.tmpl` exists.
- Run targeted tests after each task group and full verification at the end.
- Preserve public CLI compatibility unless a test or schema explicitly changes with this plan.
- Do not reintroduce public hidden/debug commands.

## Known Footguns / Constraints

- Public CLI reachability is mostly fixed; do not regress it while changing event-log command identities.
- Event payload kind and event envelope command are different concerns. Keep payloads typed and granular; change envelope command authority for normal public aggregate commands.
- Keep `recommended_command` display-only compatibility text; do not make it executable authority again.
- If adding placeholders for input-required commands, mark them clearly as input templates, not shell-ready argv.
- Do not make worktree lease safety permissive. Only downgrade already-resolved provenance cleanup from control-plane routing.
- If closure overlay restoration remains a mutation, it must be automatic or diagnostic-only when history reconstructs the same current closure.
- Public replay tests can keep synthetic historical setup, but names and static guards must not imply pure public end-to-end proof where none exists.

## Requirement Coverage Matrix

- H1 -> Task 1
- M1 -> Task 1
- H2 -> Task 2
- M2 -> Task 2
- M8 -> Task 2
- M3 -> Task 3
- M4 -> Task 3
- L1 -> Task 3
- M5 -> Task 4
- M6 -> Task 5
- M7 -> Task 5
- L2 -> Task 5

## Tasks

## Task 1: Make Public Aggregate Commands Own Event-Log Command Identity

**Spec Coverage:** H1, M1

**Goal:** Normal public `advance-late-stage` and `close-current-task` flows must not persist hidden primitive command names as authoritative event envelope `command` values.

**Context:**

- `advance-late-stage` currently routes to helper writers that persist `"record_branch_closure"`, `"record_release_readiness"`, `"record_final_review"`, and `"record_qa"`.
- `close-current-task` can refresh missing dispatch lineage but defaults to `"record_review_dispatch"` when appending a `DispatchRecorded` event.
- Event payloads such as `BranchClosureRecorded`, `ReleaseReadinessRecorded`, `FinalReviewRecorded`, `QaRecorded`, and `DispatchRecorded` should remain typed.

**Constraints:**

- Do not expose the old primitive names in public CLI, docs, or status/operator outputs.
- Do not collapse distinct typed event payloads into a generic event.
- Do not break migration of existing event logs that contain historical primitive command names.

**Done when:**

- New events produced by public `advance-late-stage` use envelope command `advance_late_stage`.
- New dispatch refresh events produced by public `close-current-task` use envelope command `close_current_task`.
- `event_from_command_authoritative_delta` still accepts historical primitive command names for existing event logs or explicit compatibility paths if required, but normal public paths do not emit them.
- Tests fail if normal public aggregate flows append hidden primitive command identities.

**Files:**

- Modify: `src/execution/recording.rs`
- Modify: `src/execution/event_log.rs`
- Modify: `src/execution/closure_dispatch_mutation/recording.rs`
- Modify: `src/execution/commands/advance_late_stage.rs`
- Modify: `src/execution/commands/close_current_task.rs`
- Modify: `tests/runtime_authority_contracts.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/public_cli_flow_contracts.rs`


- [ ] **Step 1: Add an explicit command-owner parameter to branch/release/final/QA recording helpers, or add aggregate-specific wrapper functions used by `advance-late-stage`.**
- [ ] **Step 2: Change public `advance-late-stage` paths to pass `advance_late_stage` into persistence.**
- [ ] **Step 3: Keep compatibility/internal callers on old command identities only if they are intentionally internal, and document the boundary in code.**
- [ ] **Step 4: Change `ensure_current_review_dispatch_id` call sites so public `close-current-task` passes `close_current_task` as the event owner.**
- [ ] **Step 5: Keep lower-level compatibility helper paths isolated and explicitly named.**
- [ ] **Step 6: Add event-log tests that execute public aggregate flows and inspect event envelopes for forbidden normal-path command names.**
- [ ] **Step 7: Extend static public-flow tests to scan normal-path recording helpers for primitive command persistence from aggregate command modules.**

- [ ] **Step 99: Run the task validation commands listed in this plan and confirm they pass.**
## Task 2: Make Public Route Fields Machine-Usable in JSON and Safe in Text/Skills

**Spec Coverage:** H2, M2, M8

**Goal:** Agents must never be told to consume JSON-only route fields from text-mode output, and input-required routes must expose a structured command shape that does not require phase/detail inference.

**Context:**

- Skills currently run text-mode `workflow operator` and then reference `recommended_public_command_argv` and `required_inputs`.
- Text operator/handoff/doctor output warns that JSON is authoritative but does not show exact JSON rerun command or required input names.
- `PublicCommand::to_invocation` returns `None` for input-required commands.

**Constraints:**

- Keep `recommended_command` display-only compatibility text; do not make it executable authority.
- Do not serialize fake shell-ready argv with placeholders that an agent might execute directly.
- Preserve `recommended_public_command_argv` as exact machine-invocation authority for fully bound routes.

**Done when:**

- Any skill/template sentence that consumes `phase`, `phase_detail`, `recommended_public_command_argv`, `required_inputs`, `recording_context`, or `base_branch` uses `workflow operator --json`.
- Contract tests reject instructions that name JSON fields after text-mode operator calls.
- Input-required routes expose a structured public command shape, for example `recommended_public_command_template` or `recommended_public_command_kind`, alongside `required_inputs`.
- Text output prints one concrete safe rerun instruction such as `Rerun with --json and follow recommended_public_command_argv or required_inputs`.
- Doctor dashboard renders required input names for actionable routes with `command_available=false`.

**Files:**

- Modify: `src/execution/command_eligibility.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/status.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/workflow/doctor_dashboard.rs`
- Modify: `src/workflow/doctor_resolution.rs`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-operator.schema.json`
- Modify: `schemas/workflow-handoff.schema.json`
- Modify: `skills/*.md.tmpl`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/public_cli_flow_contracts.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/workflow_runtime_final_review.rs`


- [ ] **Step 1: Design a structured route field for input-required public commands. Prefer a JSON object with command kind, base argv prefix, and required input names over placeholder shell argv.**
- [ ] **Step 2: Populate the new field from typed `PublicCommand` without parsing display strings.**
- [ ] **Step 3: Update JSON schemas and schema contract tests.**
- [ ] **Step 4: Update text renderers to show exact JSON rerun guidance and required input names when available.**
- [ ] **Step 5: Update doctor dashboard to distinguish executable command available, input contract available, and diagnostic-only state.**
- [ ] **Step 6: Update skill templates to use `--json` for all JSON-field consumption.**
- [ ] **Step 7: Regenerate generated skill docs.**
- [ ] **Step 8: Update Node tests so old text-mode wording fails.**

- [ ] **Step 99: Run the task validation commands listed in this plan and confirm they pass.**
## Task 3: Demote Resolved Provenance/Overlay Repair from Post-Closure Control Plane

**Spec Coverage:** M3, M4, L1

**Goal:** Current closure truth must remain sufficient for forward progress when provenance cleanup or overlay restoration is recoverable from authoritative history.

**Context:**

- Worktree lease provenance can route `repair-review-state` after current closures exist.
- Missing/mismatched current task closure overlays can block `begin` even when event history can reconstruct current closure records.
- Targetless-stale invariant backup is not effective if reducer projection is bypassed.

**Constraints:**

- Do not permit unsafe active leases or unresolved worktree ownership to slip through.
- Do not remove repair when the reviewed state is actually stale or closure binding is unsafe.
- Do not silently rewrite proof records.

**Done when:**

- Resolved worktree lease cleanup is automatic/idempotent or diagnostic-only after current closure truth is sufficient.
- Unresolved unsafe lease states still fail closed.
- Recoverable current task closure overlay mismatch no longer blocks next-task begin when reducer history reconstructs the same current closure.
- If overlay restoration writes state, it happens inside the owning public mutation or a diagnostic maintenance path, not as a mandatory manual repair detour.
- Targetless-stale invariant detects missing diagnostic marker instead of returning early.

**Files:**

- Modify: `src/execution/read_model.rs`
- Modify: `src/execution/review_state.rs`
- Modify: `src/execution/recording.rs`
- Modify: `src/execution/read_model_support.rs`
- Modify: `src/execution/transitions.rs`
- Modify: `src/execution/follow_up.rs`
- Modify: `src/execution/invariants.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/plan_execution.rs`
- Modify: `tests/internal_plan_execution.rs`
- Modify: `tests/public_replay_churn.rs`
- Modify: `tests/liveness_model_checker.rs`


- [ ] **Step 1: Classify worktree lease gate failures into unsafe blockers versus resolved cleanup.**
- [ ] **Step 2: Keep unsafe blockers as public `repair-review-state` or runtime diagnostic routes.**
- [ ] **Step 3: Move resolved cleanup to automatic postcondition resolution when a current closure is already authoritative, or expose it as diagnostic-only.**
- [ ] **Step 4: Add tests where pass/pass current task closure plus resolved lease provenance does not force execution reentry or manual repair.**
- [ ] **Step 5: Change `require_prior_task_closure_for_begin` so `TaskCurrentClosureStatus::Current` from reducer history is accepted before overlay restoration blocks.**
- [ ] **Step 6: If overlay restoration is still needed for projection consistency, perform it opportunistically after accepting authoritative history or through an explicit projection/materialization path.**
- [ ] **Step 7: Fix `check_targetless_stale_unreviewed_routes_to_reconcile` so it raises when raw targetless stale state lacks the reconcile marker.**
- [ ] **Step 8: Add regression tests for overlay-loss begin acceptance and invariant detection.**

- [ ] **Step 99: Run the task validation commands listed in this plan and confirm they pass.**
## Task 4: Tighten Public-Test Realism Boundaries

**Spec Coverage:** M5

**Goal:** Public-flow tests should either construct state through public commands or clearly declare synthetic historical fixture setup, with static guards preventing accidental internal APIs in public proof tests.

**Context:**

- Public tests use compiled CLI for recovery assertions.
- Hard states are still seeded through `*_for_tests` event-log APIs and direct state mutation in several public-gate tests.
- Static scanner does not currently forbid these event-log test APIs in protected public-flow files.

**Constraints:**

- Historical replay fixtures may remain synthetic when the goal is reproducing impossible/legacy stuck states.
- Do not remove valuable replay coverage just because setup is synthetic.
- Names and contract comments must prevent synthetic fixtures from being mistaken for end-to-end public-flow proof.

**Done when:**

- Protected public-flow tests cannot use event-log `_for_tests` APIs unless an explicit synthetic-fixture exception is registered.
- Normal-path late-stage/public flow scenarios are set up through public commands where feasible.
- Synthetic historical replay tests are named and documented as synthetic setup plus public recovery.
- Test contracts distinguish public end-to-end proof from synthetic replay recovery proof.

**Files:**

- Modify: `tests/public_cli_flow_contracts.rs`
- Modify: `tests/runtime_behavior_golden.rs`
- Modify: `tests/public_replay_churn.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/liveness_model_checker.rs`
- Modify: `tests/support/**`
- Modify: `scripts/run-public-runtime-flow-tests.sh`


- [ ] **Step 1: Extend `scan_source_for_public_flow_violations` to detect `load_reduced_authoritative_state_for_tests`, `sync_fixture_event_log_for_tests`, and similar `_for_tests` event-log APIs in protected public-flow files.**
- [ ] **Step 2: Add an explicit exception registry for synthetic historical fixtures with reason text.**
- [ ] **Step 3: Rename or annotate existing synthetic replay helpers so the boundary is visible at call sites.**
- [ ] **Step 4: Convert at least one late-stage golden scenario to public command setup to prove a normal shipped path end-to-end.**
- [ ] **Step 5: Keep liveness/model-checker synthetic setup, but document that it is model-state generation plus public CLI edge validation.**
- [ ] **Step 6: Add a test that a new protected public-flow file using `_for_tests` APIs fails without an exception.**

- [ ] **Step 99: Run the task validation commands listed in this plan and confirm they pass.**
## Task 5: Centralize Remaining Workflow and Late-Stage Decisioning

**Spec Coverage:** M6, M7, L2

**Goal:** Remove residual duplicated semantic routing logic in workflow status and late-stage mode selection.

**Context:**

- `src/workflow/status.rs` repeats route decisions for normal discovery and explicit `--plan` override.
- Late-stage public command mode selection has multiple owners.
- Execution module boundary tests exist; workflow boundary guardrails are thinner.

**Constraints:**

- Preserve current status/operator JSON shape unless intentionally changed by Task 2.
- Keep explicit plan override behavior distinct only where the boundary requires it, such as candidate counts and manifest decoration.
- Do not introduce string parsing of display commands.

**Done when:**

- Normal discovery and explicit plan override call one shared route-for-plan-candidate helper for stale linkage, contract analysis, fidelity routing, and implementation readiness.
- Phase-detail to `PublicAdvanceLateStageMode` resolution has one owner.
- Router special-cases and recovery contracts call the shared late-stage resolver instead of reconstructing modes.
- Workflow boundary tests or source scans prevent reintroducing duplicated route candidate logic.

**Files:**

- Modify: `src/workflow/status.rs`
- Modify: `src/execution/command_eligibility/late_stage.rs`
- Modify: `src/execution/late_stage_route_selection.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/commands/common/operator_outputs.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_runtime_final_review.rs`
- Modify: `tests/public_cli_flow_contracts.rs`


- [ ] **Step 1: Extract a `route_for_plan_candidate` helper that takes workflow runtime, approved/spec candidate, plan candidate, scan metadata, and override/discovery decoration context.**
- [ ] **Step 2: Make the normal discovery branch and explicit override branch call that helper.**
- [ ] **Step 3: Keep manifest/candidate-count decoration outside the semantic helper.**
- [ ] **Step 4: Add route parity tests that construct the same plan through discovery and explicit override and compare semantic route fields.**
- [ ] **Step 5: Introduce a single late-stage command resolver that returns the `PublicCommand` or input-required command contract for a phase detail.**
- [ ] **Step 6: Replace router and operator-output special cases with calls to the resolver.**
- [ ] **Step 7: Add a source scan that forbids direct construction of `PublicAdvanceLateStageMode::FinalReview`, `ReleaseReadiness`, or `Qa` outside the resolver and tightly scoped tests.**
- [ ] **Step 8: Add workflow module boundary tests or exception tracking for large workflow files.**

- [ ] **Step 99: Run the task validation commands listed in this plan and confirm they pass.**
## Final Verification Gate

After all tasks are complete:

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test runtime_authority_contracts
cargo nextest run --test workflow_runtime
cargo nextest run --test workflow_shell_smoke
cargo nextest run --test workflow_entry_shell_smoke
cargo nextest run --test plan_execution
cargo nextest run --test plan_execution_final_review
cargo nextest run --test workflow_runtime_final_review
cargo nextest run --test contracts_execution_runtime_boundaries
cargo nextest run --test execution_query
cargo test --test liveness_model_checker
```

The branch is ready to reconsider as a ship candidate only if the final verification gate passes and the audit checklist updates all High and Medium findings to fixed.
