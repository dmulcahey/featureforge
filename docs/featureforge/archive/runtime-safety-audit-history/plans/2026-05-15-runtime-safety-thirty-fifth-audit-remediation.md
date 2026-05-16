# Runtime Safety Thirty-Fifth Audit Remediation

## Workflow State

Engineering remediation plan for the thirty-fifth runtime-safety audit loop. This plan is active until every task below is implemented, fully verified, independently reviewed, and followed by another deep audit loop with the signal-to-noise auditor included.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task. Do not use FeatureForge runtime/workflow/project skills. Do not allow reviewers or implementation subagents to spawn additional subagents. Before each full test cycle, verify no `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, or `cargo clippy` process is already running.

## Goal

Close the actionable findings from the thirty-fifth audit without adding another layer of workflow law. The implementation must remove remaining split decisioning in mutation and presentation paths, make public output unambiguous, replace brittle scanner/source-shape checks with higher-signal behavioral or ownership checks, and reduce prompt/archive noise.

## Architecture

- Public runtime mutations must not authorize against a read model captured before the same command appended authoritative events. Any command that refreshes dispatch, closure, or route authority must re-read the status/operator projection before later eligibility checks.
- Executable route authority must come from `RouteDecision` and typed public command surfaces. DTO fallback fields may be passive diagnostics, but they must not reconstruct executable route readiness.
- Workflow/operator presentation may summarize the route, but semantic route decisions must come from `RouteDecision`, `state_kind`, or a shared route-plan helper, not a second phase/gate predicate.
- Public-facing text must offer one public next step. Display strings can remain compatibility fields, but text should prefer typed argv/template JSON surfaces and avoid shell-like strings when paths can contain spaces.
- Tests should protect public behavior, shared ownership boundaries, and scanner correctness without pinning incidental private helper names or growing exception taxonomies.
- Skills should stay actionable. Route-owning skills may carry mandatory top-level law, but repeated route-flow prose should collapse into the canonical operator route reference.
- Audit history should support current review without turning the repository into a self-referential archive of every loop.

## Change Surface

- `docs/featureforge/plans/2026-05-15-runtime-safety-thirty-fifth-audit-remediation.md`
- `src/execution/commands/close_current_task.rs`
- `src/execution/review_state.rs`
- `src/workflow/operator.rs`
- `src/execution/mod.rs`
- `src/execution/recording.rs`
- `src/workflow/doctor_resolution.rs`
- `src/workflow/status.rs`
- `README.md`
- `skills/*.tmpl`
- `skills/*/SKILL.md`
- `scripts/gen-skill-docs.mjs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/support/public_flow_scan.rs`
- `tests/liveness_model_checker.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json`
- `tests/codex-runtime/*.test.mjs`
- `docs/featureforge/archive/runtime-safety-audit-history/**`

## Preconditions

- The preceding clean audit iteration ran `cargo clean`.
- Validation before this plan was clean:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `/usr/bin/time -p cargo nextest run --all-targets --all-features --no-fail-fast`
  - `cargo test --test liveness_model_checker`
- Full nextest after clean completed in `184.81s`, below the configured clean/rerun threshold.
- Do not begin a full Rust validation cycle until `pgrep -fl '[c]argo nextest|[c]argo-nextest|[n]extest run|[c]argo test|[c]argo clippy|[n]ode --test tests/codex-runtime'` shows no active validation process.

## Known Footguns / Constraints

- Do not weaken `recommended_public_command_argv` or `recommended_public_command_template` authority.
- Do not make `review_state.rs` reconstruct executable route readiness from compatibility DTO fields.
- Do not solve source-shape brittleness by adding larger allowlists or more string fragments.
- Do not raise prompt budgets to absorb repeated law.
- Do not delete archive material that is still referenced by active docs without updating those references.
- Do not interrupt in-flight validations or subagents.
- A full validation and clean-context review is required after each task before moving to the next task.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| `close-current-task` re-authorizes after dispatch refresh | Task 1 |
| `review_state` consumes finalized route decisions only for executable close-current-task surfaces | Task 1 |
| Operator execution-reentry guidance derives from route authority, not a separate phase/gate predicate | Task 1 |
| Boundary tests cover `review_state` and `workflow/operator` split-decisioning hazards | Task 1 |
| Planning reentry presents one public next step instead of a requery/report loop | Task 2 |
| README external-review-ready wording cannot imply verification-only readiness | Task 2 |
| Shell-like operator requery text is safe for plan paths with spaces or is demoted behind typed JSON surfaces | Task 2 |
| Hidden/retired command tokens do not leak in production diagnostics | Task 2 |
| Liveness production-loop tests execute representative runtime routing for critical stuck states | Task 3 |
| Public-flow scanner tests do not overclaim mixed binaries as shipped-runtime proof | Task 3 |
| Source-shape boundary tests are narrowed to semantic ownership and public hazards | Task 3 |
| Prompt budget pressure is relieved by route-law consolidation, not budget increases | Task 4 |
| Runtime-safety audit history is rolled up or indexed so active repo noise is reduced | Task 4 |
| Full validation and independent reviews pass | Task 5 |

## Task 1: Remove Runtime Route Split Decisioning

**Spec Coverage:** `close-current-task` re-authorizes after dispatch refresh; `review_state` consumes finalized route decisions only; operator execution-reentry guidance derives from route authority; boundary tests cover these hazards.

**Goal:** Ensure mutation and presentation code never makes executable route decisions from stale or independently reconstructed state.

**Context:** Audit G found three split-decisioning paths:

- `close-current-task` captures `status` before `ensure_current_review_dispatch_id_for_command(...)` can append a dispatch checkpoint, then reuses that stale status for later mutation eligibility and already-current decisions.
- `review_state.rs::final_close_current_task_route` prefers `RouteDecision`, but reconstructs a ready `close-current-task` route from `ExecutionRoutingState` DTO fields when `route_decision` is absent.
- `workflow/operator.rs::review_requires_execution_reentry` derives execution reentry guidance from phase/detail/gate fields, while executable fields mostly come from route decisions.

**Constraints:**

- Preserve public JSON output shape unless a compatibility field must become diagnostic-only.
- Do not duplicate route-plan selection logic in command or workflow modules.
- Re-read status/operator after any command-local authority write before later eligibility checks.
- If no `RouteDecision` is available for executable close-current-task output, fail closed into diagnostic recovery rather than reconstructing argv/template from DTO fields.
- Operator text may use route state and state kind, but must not recompute execution reentry from unrelated phase/gate conditions.

**Done when:**

- After dispatch refresh, `close-current-task` reloads `ExecutionContext`, status, and operator projection and uses the refreshed status for all later `require_close_current_task_public_mutation(...)` and already-current checks.
- `review_state.rs::final_close_current_task_route` returns an executable route only when a current `RouteDecision` supplies the route.
- Operator final-review execution-reentry guidance is driven by route decision/state-kind/shared route-plan facts.
- Boundary tests fail if `review_state.rs` reconstructs close-current-task executable surfaces from `ExecutionRoutingState` fallback fields.
- Boundary tests fail if `workflow/operator.rs` defines an independent phase/gate execution-reentry predicate.
- Relevant goldens/tests are updated through real runtime output, not manual JSON edits where a generator exists.

**Files:**

- `src/execution/commands/close_current_task.rs`
- `src/execution/review_state.rs`
- `src/workflow/operator.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json`

**Implementation Steps:**

1. Introduce a small local refreshed-route context in `close_current_task` after dispatch refresh:
   - reload `ExecutionContext`
   - rebuild shared status through `status_with_shared_routing_or_context(...)`
   - rebuild `current_workflow_operator(...)`
   - use the refreshed status in all later eligibility and already-current checks.
2. Ensure the candidate-dispatch path keeps using the original status only until no mutation has happened; once the command writes dispatch state, no later path may use pre-write status.
3. Simplify `final_close_current_task_route` so the only executable route source is `close_current_task_route_from_decision(...)`.
4. Decide the non-route-decision fallback behavior explicitly:
   - diagnostic output with no argv/template; or
   - re-enter a shared route-plan builder to obtain a real `RouteDecision`.
   Prefer diagnostic fail-closed if the caller cannot supply authoritative inputs cheaply.
5. Replace `review_requires_execution_reentry` with a helper that consumes route decision/state kind exposed through `OperatorContext`.
6. Add runtime/module boundary checks for:
   - `review_state.rs` not calling `routing_recommended_command`, `routing_recommended_command_argv`, `routing_recommended_command_template`, or `routing_required_inputs` inside `final_close_current_task_route`.
   - `workflow/operator.rs` not deriving execution reentry from `gate_review.allowed` plus phase/detail.
7. Add or update behavior coverage for a dispatch-refresh close-current-task route so the post-refresh status is the authority used for mutation.
8. Regenerate runtime goldens if public route text/state changes.

**Validation Expectations:**

- Targeted:
  - `cargo test --test runtime_module_boundaries`
  - `cargo test --test workflow_shell_smoke`
  - `cargo test --test runtime_behavior_golden`
- Full task gate:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `/usr/bin/time -p cargo nextest run --all-targets --all-features --no-fail-fast`
  - `cargo test --test liveness_model_checker`
- Clean-context review against Task 1 before Task 2 starts.

## Task 2: Make Public Output One-Step And Hidden-Token-Free

**Spec Coverage:** Planning reentry one public next step; README external-review-ready wording; safe operator requery text; hidden token diagnostic leak.

**Goal:** Public output must not send agents into a requery/report loop, premature external-review hinting, shell copy-paste failure, or hidden command vocabulary.

**Context:** Audit H and A found:

- Planning reentry is split between actionable planning pivot text and `runtime_diagnostic_required`, leaving agents with “query operator JSON and stop/report” even when the next action is plan review/fidelity refresh.
- README says to use `--external-review-result-ready` after “review plus verification are ready,” which can imply verification alone justifies the hint.
- `workflow_operator_requery_command` renders shell-like strings with unquoted plan paths.
- `restore_review_state_projection_overlays` says `reconcile-review-state`, a hidden/retired token, in a production diagnostic reachable through public `repair-review-state`.

**Constraints:**

- Preserve typed JSON route authority.
- Do not introduce shell escaping as a second executable authority if typed argv/template can be shown instead.
- Do not remove useful diagnostic context.
- Do not advertise hidden command names even as noun labels in public failures.

**Done when:**

- Planning reentry surfaces one public next step: return to `featureforge:plan-eng-review` or the exact public planning-review skill route already used by status.
- Doctor resolution no longer classifies actionable planning reentry as `runtime_diagnostic_required` when a planning route is known.
- README external-review-ready language says the hint is only for an external task-review/final-review result already in hand.
- Operator requery guidance either quotes paths safely for display or points to typed JSON argv/template fields without shell-like interpolation.
- Production diagnostics do not include `reconcile-review-state`.
- Tests or scanners catch this hidden-token leak class.

**Files:**

- `src/workflow/doctor_resolution.rs`
- `src/workflow/status.rs`
- `src/execution/mod.rs`
- `src/execution/recording.rs`
- `src/workflow/operator.rs`
- `README.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/runtime_instruction_contracts.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json`

**Implementation Steps:**

1. Inspect planning reentry status/operator cases and centralize the public next step in a shared helper if status and doctor currently diverge.
2. Change doctor planning reentry resolution from diagnostic-only to the same planning route where status has an actionable plan-review/fidelity remediation.
3. Update goldens for planning reentry so blockers do not tell agents to requery and stop when a planning action is known.
4. Update README wording for `--external-review-result-ready`.
5. Replace `workflow_operator_requery_command` display construction with a safe display form:
   - either shell-quote the path with an existing safe helper; or
   - render `featureforge workflow operator --plan <approved-plan-path> --json` and rely on `recommended_public_command_argv/template` for execution.
6. Replace `reconcile-review-state requires authoritative harness state.` with public wording such as `repair-review-state requires authoritative harness state.`
7. Add scanner coverage for exact hidden command tokens in production diagnostics, allowing historical docs/tests only through existing archive exclusions.

**Validation Expectations:**

- Targeted:
  - `cargo test --test runtime_behavior_golden`
  - `cargo test --test runtime_instruction_contracts`
  - `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- Full task gate as in Task 1.
- Clean-context review against Task 2 before Task 3 starts.

## Task 3: Replace Brittle Test Shape With Behavioral Runtime Coverage

**Spec Coverage:** Liveness production-loop tests execute representative routing; public-flow scanner reporting is precise; source-shape boundary tests narrow to semantic ownership.

**Goal:** Keep tests high-signal by proving public/runtime behavior and real ownership boundaries instead of testing scanners, exact private names, or fixture presence.

**Context:** Audits E, B, and I found:

- Some liveness “production-loop” tests only prove a synthetic case exists. Representative execution excludes current/stale overlap, cycle-break, targetless stale, downstream-interrupted, and downstream-stale-plus-interruption variants.
- Public-flow scanner tests classify whole binaries while some files contain legitimate internal semantic tests, making results easy to over-cite as shipped-runtime proof.
- `runtime_module_boundaries.rs` still pins exact private function names/imports/field-read fragments in several ownership checks.
- `public_flow_scan_contracts.rs` tests exception taxonomy and scanner category lists more than behavior.

**Constraints:**

- Do not delete tests that catch public/private command drift.
- Do not move all coverage to shell tests; semantic model tests are still useful when clearly labeled.
- Prefer small runtime fixtures over broader source scans when they prove the same behavior.
- Keep source scanners only for hazards that cannot be exercised cheaply by runtime behavior, such as hidden command literals in public surfaces or forbidden import directions.

**Done when:**

- Liveness representative runtime execution includes at least one case for each critical stuck-state family listed above.
- Public-flow scan output distinguishes `public_cli_flow`, `mixed_public_and_internal_semantic`, and `internal_only` or equivalent categories so reporting cannot overclaim mixed binaries.
- `runtime_module_boundaries.rs` removes or narrows the worst incidental private-name/field-read checks and retains ownership checks around public hazards.
- `public_flow_scan_contracts.rs` keeps one classification/script integration check and synthetic scanner fixtures, but removes exact exception taxonomy tests or moves exceptions into file-local annotations consumed by the scanner.

**Files:**

- `tests/liveness_model_checker.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/support/public_flow_scan.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/public_replay_churn.rs`
- `docs/testing.md`

**Implementation Steps:**

1. Extend `runtime_executed_liveness_case_is_representative` or split it into separate tests that execute bounded routing for:
   - current/stale overlap
   - cycle-break clearing/progression
   - targetless stale diagnostic
   - downstream interruption
   - downstream stale plus interruption
2. Keep synthetic liveness model coverage, but rename or document fixture-only checks so they do not read as production convergence proof.
3. Add a mixed-test category to the public-flow scanner for files that intentionally contain both public CLI proof and internal semantic checks.
4. Update docs/testing.md to explain public CLI proof versus internal semantic/model proof.
5. Replace exact scanner exception taxonomy tests with:
   - one script-vs-classification integration assertion
   - synthetic positive/negative scanner fixtures
   - optional file-local annotations for intentional internal tests inside mixed files.
6. Review `runtime_module_boundaries.rs` for the checks called out by Audit I:
   - closure preemption owner check
   - current-task closure predicate check
   - task-key parser scanner
   - duplicated route-token arrays around line 960
   Replace with behavioral route fixtures or production constants where possible.

**Validation Expectations:**

- Targeted:
  - `cargo test --test liveness_model_checker`
  - `cargo test --test public_flow_scan_contracts`
  - `cargo test --test runtime_module_boundaries`
  - `cargo test --test workflow_shell_smoke`
- Full task gate as in Task 1.
- Clean-context review against Task 3 before Task 4 starts.

## Task 4: Consolidate Prompt Law And Audit-History Noise

**Spec Coverage:** Prompt budget pressure relieved by consolidation; audit history rolled up or indexed.

**Goal:** Reduce conceptual surface area without weakening the mandatory top-level runtime law agents need to act.

**Context:** Audit I found the generated top-level skill docs at `4,955 / 5,015` lines, with only 60 lines of headroom. Full route law is generated into seven route-owning skills, and several templates repeat route flow outside the generated block. The archive now contains dozens of runtime-safety audit/remediation loop files that active tests intentionally exclude.

**Constraints:**

- Do not raise budgets.
- Do not move mandatory stop/typed-route law solely into companion docs.
- Do not break source-archive packaging or companion links.
- Do not delete active plans/specs/evidence.
- If archive files are removed or relocated, update any active references.

**Done when:**

- Repeated route-flow prose in `using-featureforge`, `executing-plans`, and `subagent-driven-development` templates is collapsed into skill-specific entry/exit instructions plus the generated route-law/reference block.
- Generated skill docs remain fresh and budget headroom improves materially.
- The audit-history archive has a retention/index rule and no unnecessary per-loop noise remains in active review surfaces.
- Active docs still point to one current rollup or index where historical context is needed.

**Files:**

- `scripts/gen-skill-docs.mjs`
- `skills/using-featureforge/SKILL.md.tmpl`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- generated `skills/*/SKILL.md`
- `skills/skill-doc-budgets.json`
- `docs/featureforge/archive/runtime-safety-audit-history/**`
- `docs/testing.md`
- `README.md`
- `tests/codex-runtime/skill-doc-budget.test.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `scripts/verify-source-archive.mjs`

**Implementation Steps:**

1. Identify duplicated route-flow paragraphs outside generated route-law blocks in the three high-use templates.
2. Replace duplicated paragraphs with concise skill-specific instructions that point to the generated installed control-plane law or `references/operator-route-authority.md`.
3. Regenerate skill docs.
4. Run the skill budget report and confirm headroom increased without lowering ceilings.
5. Add or update a retention note/index under `docs/featureforge/archive/runtime-safety-audit-history/`.
6. Remove or relocate superseded per-loop files only when no active doc/test references them. If deletion is too risky in this pass, keep a single index and create a follow-up cleanup task with exact referenced/unreferenced file lists.
7. Ensure source archive verification still passes.

**Validation Expectations:**

- Targeted:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node --test tests/codex-runtime/skill-doc-budget.test.mjs`
  - `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `node scripts/verify-source-archive.mjs`
- Full task gate as in Task 1.
- Clean-context review against Task 4 before Task 5 starts.

## Task 5: Final Validation, Review, And Re-Audit

**Spec Coverage:** Full validation and independent reviews pass.

**Goal:** Prove the remediation is complete and run the next audit loop. If actionable findings remain, create the next remediation plan and continue.

**Context:** The user requested an audit -> implementation loop until no actionable audit issues remain. This task is not complete until validation, task-level reviews, whole-plan review, and the next audit loop have no actionable findings.

**Constraints:**

- Do not skip full validation before reviews.
- Do not spawn reviewers until strict Clippy and full nextest are clean.
- Do not let reviewers spawn subagents.
- Do not interrupt in-flight validations or productive reviewers.
- Before starting the next audit loop, run `cargo clean`.

**Done when:**

- Every task above has passed:
  - generated docs checks
  - Node codex-runtime tests
  - strict Clippy
  - full no-fail-fast nextest
  - liveness model checker
  - clean-context task review
- Whole-plan clean-context review has no actionable findings.
- Next audit loop runs subagents A-H plus signal-to-noise subagent.
- If the next audit has no actionable findings, record the executive verdict as ship candidate. If it has findings, create the next concrete remediation plan and keep looping.

**Files:**

- This plan
- Any generated audit report or follow-up remediation plan created by the next loop

**Implementation Steps:**

1. After each task, run the full validation gate.
2. Dispatch a clean-context reviewer for the exact task and fix any findings.
3. After all task reviews pass, run the full validation gate again.
4. Dispatch a clean-context whole-plan review with explicit base/head/worktree metadata.
5. After whole-plan review passes, run `cargo clean`.
6. Dispatch audit subagents A-H plus signal-to-noise.
7. Synthesize the audit. If no actionable findings remain, stop the loop and report. Otherwise create the next remediation plan.

**Validation Expectations:**

- Same full gate as Task 1.
- Full nextest must remain under the configured performance threshold. If it exceeds 4-5 minutes, clean/rerun and remediate repeatable regression. If it exceeds 10 minutes, stop immediately and enter performance remediation.
