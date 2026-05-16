# Thirty-Seventh Runtime Safety Audit Report

Date: 2026-05-16

Scope: updated working tree after the thirty-sixth remediation implementation and whole-plan review. The audit reused the original deep-audit process and added the signal-to-noise auditor requested for this loop.

## Method

Nine clean-context read-only subagents audited independent risk areas:

- Subagent A: public CLI and reachable runtime.
- Subagent B: tests vs shipped-runtime realism.
- Subagent C: receipt, provenance, evidence, and projection control-plane leakage.
- Subagent D: plan-review and engineering-review workflow.
- Subagent E: stale closure, cycle-break, and reentry loops.
- Subagent F: prompt-surface and skill packaging.
- Subagent G: modularization and split decisioning.
- Subagent H: public-output and agent UX.
- Subagent I: signal-to-noise and conceptual-surface audit.

## Executive Verdict

Verdict: ship only after targeted fixes.

The runtime core is much safer than earlier audit loops. The public mutation path, receipt/projection authority boundary, stale-closure convergence, plan-fidelity handoff, and reviewer recursion controls are all now backed by code and tests. The remaining issues are not the old catastrophic dead ends, but they are actionable because they can reintroduce agent confusion or maintenance churn:

- `workflow status` still reads like a route authority surface even though operator JSON is now the normal executable route contract.
- Several static guard tests duplicate command/follow-up taxonomies and source manifests instead of deriving from canonical runtime ownership.
- Workflow presentation still has repeated skill/prose projections.
- Harness phase parsing is duplicated around the canonical `HarnessPhase`.
- `transfer` rechecks presentation phase fields after the public mutation guard already authorized the command.
- The remediation inventory under-reports the current FS-22 public replay proof.

Recommendation: do not ship this exact state. Implement `docs/featureforge/plans/2026-05-16-runtime-safety-thirty-seventh-audit-remediation.md`, verify with strict clippy and full nextest, review with clean context, then repeat the audit loop.

## What Is Genuinely Fixed

- Public CLI reachability: Subagent A found no normal route recommending a non-CLI command and no normal mutation path requiring hidden/debug commands. Typed public command authority is in `src/execution/command_eligibility.rs` and route surfaces are derived in `src/execution/route_plan/decision.rs`.
- Public `begin`: `src/execution/commands/begin.rs` owns preflight identity setup and non-begin commands cannot bootstrap acceptance-only preflight through `src/execution/state/preflight.rs`.
- Public `close-current-task`: `src/execution/commands/close_current_task.rs` refreshes missing dispatch lineage through public aggregate authority and records current closure without hidden dispatch repair.
- Public `advance-late-stage`: aggregate late-stage progression is mode-bound through `src/execution/command_eligibility/late_stage.rs` and `src/execution/commands/advance_late_stage.rs`.
- Receipt/projection control plane: Subagent C found no active receipt/provenance artifact path that can force reentry after authoritative current closure exists. Diagnostics are separated in `src/execution/closure_diagnostics/reason_codes.rs`, `src/execution/status_assembly/task_state.rs`, and `src/execution/route_plan/status_application.rs`.
- Plan review: Subagent D found plan-fidelity now uses parseable review artifacts and five-surface approval checks in `src/contracts/plan.rs`, not hidden runtime receipts.
- Stale closure convergence: Subagent E found current/stale overlap is blocked as runtime bug, current closures are removed from stale target projection, and targetless stale states route to reconcile diagnostics.
- Prompt packaging: Subagent F found budget enforcement, generated docs, reviewer recursion prompt-scope, and companion reference packaging are active and passing.

## What Remains Risky

- Public UX can still send an agent to `workflow status` as if it were route authority. This is a user-facing agent behavior risk, not a mutation corruption risk.
- Static guard tests are becoming their own source of duplicated workflow law. This is a maintenance and drift risk.
- Some compatibility projections still recompute phase-to-skill or phase-to-reason text locally. This is lower risk than runtime route duplication, but it keeps conceptual surface area wider than needed.
- Local harness phase string parsing and transfer handoff checks are small split-decisioning pockets.

## Concrete Dead Ends Still Possible

No old-style hard dead end was proven:

- No normal flow requires `plan execution preflight`, `record-review-dispatch`, `gate-review`, `gate-finish`, `rebuild-evidence`, or low-level late-stage recorders.
- No current receipt/projection freshness drift was found that can force `execution_reentry_required` when current pass/pass task closure is authoritative.
- No hidden repair helper path was found as a normal next action.

The remaining plausible agent dead end is UX-driven: a human or agent can run `workflow status`, see `next_skill` and reason codes, and treat that diagnostic read model as executable route authority instead of using `workflow operator --plan ... --json`.

## Concrete Churn Sources Still Possible

- Hardcoded public mutation token lists in `tests/runtime_module_boundaries.rs` can drift from `PublicCommandKind`.
- Hardcoded follow-up command token lists and per-file allowlists in `tests/runtime_module_boundaries.rs` can drift from `FollowUpKind` and review route-token ownership.
- Hardcoded production authority file manifests in `tests/support/public_flow_scan.rs` can miss new route/status modules.
- Broad runtime goldens still pin sizeable DTO shape. They are useful but should remain limited to externally visible behavior.
- Generated route-owning skills repeat mandatory law. Budgets contain this now, but future additions should replace weaker prose instead of expanding the block.

## Public/Private Test Mismatch Assessment

Assessment: mostly fixed, with one inventory drift item.

Public proof paths use `tests/support/public_featureforge_cli.rs` and the compiled CLI. Direct helper support is quarantined in `tests/support/plan_execution_direct.rs`, `tests/support/workflow_direct.rs`, `tests/support/root_direct.rs`, and `tests/support/internal_runtime_direct.rs`. Static scanners in `tests/support/public_flow_scan.rs` reject internal helper imports from public-flow files.

Finding B-1: `tests/fixtures/runtime-remediation/README.md` still says FS-22 is covered by `workflow_runtime.rs` and `internal_plan_execution.rs`, while the public replay now lives in `tests/public_replay_churn.rs`.

## Receipt/Evidence/Projection Control-Plane Assessment

Assessment: pass.

Authoritative closure state, not markdown evidence or receipts, owns task-boundary truth. Projection writes remain derived read models and explicit materialization returns `runtime_truth_changed: false`. Summary hash drift and missing dispatch artifacts are diagnostic when a current positive closure is present. No actionable control-plane leakage was found.

## Prompt-Surface And Packaging Assessment

Assessment: pass with residual cost.

Skill docs are budgeted and generated. Companion references are packaged. Reviewer recursion prevention is prompt text only and reviewer scoped. Prompt law is clearer where it points agents to operator JSON and typed argv/template fields. Residual risk is repetition: route-owning generated skills still carry repeated route law, so future prompt work should delete and centralize instead of adding more prose.

## Modularization And Split-Decisioning Assessment

Assessment: close but not done.

Core execution route decisioning now runs through `route_plan`, and workflow route DTOs were moved to `src/contracts/workflow.rs`. `state.rs`, `mutate.rs`, and `next_action.rs` are no longer the primary route decision monoliths. Remaining split-decisioning:

- `src/execution/commands/transfer.rs` recomputes handoff eligibility from workflow-operator presentation fields after the public mutation guard already authorized `transfer`.
- `src/workflow/operator.rs` and `src/workflow/status.rs` still have separate phase-to-skill/prose projection maps.
- Several status assembly modules parse harness phase strings locally instead of using `HarnessPhase`.

## Reviewer Recursion Assessment

Assessment: fixed.

Reviewer recursion prevention is prompt text only, scoped to reviewer prompts, and tests reject runtime/env guard markers. No runtime recursion guard was found.

## Validation Results

Commands run after `cargo clean` for this audit iteration:

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 143/143.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed, real 40.58s after clean.
- `cargo nextest run --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query --no-fail-fast`: passed, 345/345, real 71.21s.
- `cargo test --test liveness_model_checker`: passed, 33/33, real 8.45s.

Full nextest was not rerun for the audit-only validation because the original audit validation list names targeted Rust suites. Full nextest had already passed for the completed thirty-sixth implementation review gate: `cargo nextest run --all-targets --all-features --no-fail-fast`, 1809/1809, real 96.53s.

## Prioritized Findings

### Blocker

No blockers found.

### High

H-1. Public command and follow-up taxonomies are duplicated in static scanners.

- Type: architecture and test realism issue.
- Files: `tests/runtime_module_boundaries.rs`.
- References: `public_mutation_tokens()` near `tests/runtime_module_boundaries.rs:6981`; `raw_tokens` and `follow_up_token_literal_allowed_outside_owner()` near `tests/runtime_module_boundaries.rs:7107` and `tests/runtime_module_boundaries.rs:7194`.
- Risk: static scanner policy can drift from `PublicCommandKind`, `FollowUpKind`, or canonical route-token ownership.
- Required fix: derive scanner vocabulary from runtime/test shared helpers and delete raw duplicate lists.

H-2. Public-flow production authority scan manifest is hardcoded and incomplete for the modular route/status split.

- Type: test realism and architecture issue.
- Files: `tests/support/public_flow_scan.rs`, `tests/public_cli_flow_contracts.rs`.
- References: `production_command_authority_files()` near `tests/support/public_flow_scan.rs:2198`; consumer in `tests/public_cli_flow_contracts.rs:375`.
- Risk: new route/status modules under `src/execution/route_plan/**`, `src/execution/status_assembly/**`, or public recovery modules can bypass display-command authority scanning.
- Required fix: discover production source roots and apply narrow documented exemptions instead of hand-maintaining a small manifest.

### Medium

M-1. `workflow status` still presents itself as route authority.

- Type: user-facing agent UX issue.
- Files: `src/cli/workflow.rs`, `src/lib.rs`.
- References: status help near `src/cli/workflow.rs:11`; text renderer near `src/lib.rs:348`.
- Risk: agents can route from diagnostic `next_skill` and reason codes instead of operator JSON typed public argv/template authority.
- Required fix: reword as read-only diagnostics and point text mode to one public next step: `workflow operator --plan <plan> --json`.

M-2. Workflow handoff/status repeat phase-to-skill and reason projections.

- Type: architecture and signal-to-noise issue.
- Files: `src/workflow/operator.rs`, `src/workflow/status.rs`.
- References: handoff mapping near `src/workflow/operator.rs:676`; `reason_text()` near `src/workflow/operator.rs:1546`; planning route assignments near `src/workflow/status.rs:876` and `src/workflow/status.rs:899`; compatibility projection schema note near `src/workflow/status.rs:1858`.
- Risk: compatibility projections can diverge from route skill/status decisions.
- Required fix: centralize recommended skill/reason projection behind one helper and reuse it across handoff/status presentation.

M-3. Harness phase normalization is duplicated.

- Type: architecture cleanup.
- Files: `src/execution/harness.rs`, `src/execution/status_assembly/overlay.rs`, `src/execution/status_assembly/late_stage.rs`, `src/execution/route_plan/status_application.rs`.
- References: canonical `HarnessPhase::as_str` and `FromStr` near `src/execution/harness.rs:47` and `src/execution/harness.rs:79`; duplicates near `overlay.rs:387`, `late_stage.rs:637`, and `status_application.rs:81`.
- Risk: phase spelling changes can drift across modules.
- Required fix: use canonical `HarnessPhase` parsing/string helpers.

### Low

L-1. `transfer` rechecks handoff eligibility from presentation fields after shared guard approval.

- Type: split-decisioning issue.
- Files: `src/execution/commands/transfer.rs`.
- References: `require_public_mutation(...)` near `transfer.rs:198`; local `operator_routes_handoff` check near `transfer.rs:223`; canonical route/public command binding in `src/execution/route_plan/public_commands.rs:176` and `src/execution/command_eligibility.rs:1653`.
- Risk: fail-closed transfer blockage if presentation phase strings change independently of public mutation authority.
- Required fix: derive the post-guard handoff classification from the same public mutation decision/route object, not raw operator phase strings.

L-2. Runtime remediation inventory under-reports FS-22 public proof.

- Type: documentation/test inventory issue.
- Files: `tests/fixtures/runtime-remediation/README.md`, `tests/public_replay_churn.rs`.
- References: stale FS-22 coverage lines near `tests/fixtures/runtime-remediation/README.md:32` and `tests/fixtures/runtime-remediation/README.md:49`; current public proof near `tests/public_replay_churn.rs:3233`.
- Risk: future auditors can misread public replay coverage and re-open already fixed public/private mismatch work.
- Required fix: update the inventory to list `public_replay_churn.rs` as FS-22 public replay coverage.

## Checklist

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs `plan execution preflight`: fixed.
- No normal flow needs `record-review-dispatch`: fixed.
- No normal flow needs `gate-review`: fixed.
- No normal flow needs `gate-finish`: fixed.
- No normal flow needs `rebuild-evidence`: fixed.
- No normal flow needs low-level late-stage recorders: fixed.
- Operator never recommends hidden/debug commands: fixed.
- Status never exposes hidden/debug commands as next actions: fixed.
- Public recommended argv is executable by shipped CLI: fixed.

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: fixed.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed.
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: fixed.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: fixed.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.
- Runtime remediation inventory reflects current public replay coverage: partially fixed.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: fixed.
- No new catch-all module replaces the old monoliths: fixed.
- Phase/reason strings are centralized: partially fixed.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: fixed.

