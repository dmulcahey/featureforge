# Thirty-Fourth Runtime Safety Audit Report

## Executive Verdict

Close but not done.

The implementation has materially improved runtime safety. Public CLI reachability, typed public route authority, plan-fidelity artifact parsing, prompt packaging, reviewer-recursion scoping, and receipt/projection control-plane separation are now in good shape. The remaining issues are not the old catastrophic dead ends, but they are actionable because they preserve churn vectors: public gate text can still begin with artifact-repair imperatives, `repair-review-state` still crosses the command/read-analysis boundary, already-current branch-closure repair can dirty state on repeat, and several tests still preserve private implementation shape or scanner policy rather than public behavior.

Recommendation: do not ship until the targeted fixes in `docs/featureforge/plans/2026-05-15-runtime-safety-thirty-fourth-audit-remediation.md` are complete, fully verified, independently reviewed, and followed by another audit loop.

## What Is Genuinely Fixed

- Public CLI reachability is coherent. Public mutations are exposed in `src/cli/plan_execution.rs`, dispatched from `src/lib.rs`, and backed by typed public command construction in `src/execution/command_eligibility.rs`.
- `begin` owns preflight bootstrap setup; normal flow does not need `plan execution preflight`.
- `close-current-task` refreshes dispatch/closure state internally.
- `advance-late-stage` owns branch closure, release readiness, final-review dispatch/outcome, QA, and finish progression.
- `recommended_public_command_argv` and operator-materialized templates are authoritative; display strings are marked non-executable.
- Plan-fidelity uses parseable review artifacts under `.featureforge/reviews`, not hidden runtime receipts.
- Engineering-review edits stay in engineering review until the reviewed draft is ready for plan-fidelity.
- Current task closure is the task-boundary authority; receipt/provenance/projection diagnostics do not appear to force reentry after authoritative closure.
- Prompt budgets are enforced and generated skills/agents are fresh.
- Reviewer recursion prevention is prompt-text scoped and reviewer-prompt scoped, not runtime/env enforcement.

## What Remains Risky

- Public gate remediation strings still start with caller-provided artifact-repair imperatives before the appended workflow/operator instruction. This can push an agent toward manual artifact/state repair before it sees the typed public route contract.
- `repair-review-state` still has a shim command module while the write-capable public command body, mutation guard, and follow-up persistence live in `src/execution/review_state.rs`.
- Re-running already-current branch-closure repair can dirty authoritative overlay/follow-up fields even when values are already equal.
- Some architecture tests still pin private helper names, exact snippets, or scanner exception taxonomies. The signal-to-noise trend is mixed but acceptable only if the next loop deletes or consolidates these checks.
- `runtime_authority_contracts` has a raw source-word ban for `receipt`, which caused production code to obfuscate a warning term with `["rec", "eipt"].concat()`. That is scanner-driven code smell.

## Concrete Dead Ends Still Possible

- Gate failure text can still begin with “Repair…”, “Republish…”, “Update evidence_refs…”, or “Refresh criterion_results…”. An agent may manually edit artifacts rather than query workflow/operator JSON first.
- A normal rerun of an already-current branch-closure repair can append/persist no-op state, creating unnecessary dirty authoritative state. The audit did not prove this causes closure-loop corruption, but it is a churn source.

## Concrete Churn Sources Still Possible

- `tests/runtime_module_boundaries.rs` still contains private helper-name/source-shape assertions outside the sections already remediated.
- `tests/support/public_flow_scan.rs` has a large per-file/per-function exception registry, and `tests/public_flow_scan_contracts.rs` tests the exception taxonomy itself.
- Schema descriptions duplicate route-law prose that is already centralized in `references/operator-route-authority.md`.
- Prompt contract tests still include broad forbidden-vocabulary and route-command trap scanners; they catch real failures but also make wording changes expensive.

## Public/Private Test Mismatch Assessment

Public/private separation is mostly sound. Public-flow proof uses compiled CLI helpers such as `tests/support/public_featureforge_cli.rs`, `tests/public_replay_churn.rs`, `tests/workflow_shell_smoke.rs`, `tests/workflow_entry_shell_smoke.rs`, `tests/workflow_runtime.rs`, `tests/workflow_runtime_final_review.rs`, and `tests/plan_execution.rs`.

Internal helper coverage is now visibly quarantined with `internal_only_` helpers and public-flow scan contracts. The remaining mismatch is classification complexity: `scripts/run-public-runtime-flow-tests.sh` is correctly documented as a classified gate, but it combines executable public proof and focused static/internal contract proof. That is acceptable if kept explicit and not treated as pure shipped-runtime proof.

## Receipt/Evidence/Projection Assessment

No actionable control-plane issue was found. Runtime authority is rooted in event/reducer/current-truth state. Projection materialization is explicit and not required for progress. Plain unit-review receipts are diagnostic-only. Current task closure suppresses stale projection-only targets as intended.

## Prompt Surface And Packaging Assessment

Prompt packaging is in good shape. `skills/skill-doc-budgets.json` is enforced, generated docs are fresh, companion references are packaged, and top-level route-owning skills retain mandatory control-plane law. The remaining prompt risk is lower-level: public gate output and one review-state reference row still use direct repair/record wording.

## Modularization And Split-Decisioning Assessment

Route/status modularization is much stronger than earlier audits. Route decision and status projection are centralized through `src/execution/route_plan.rs`, `src/execution/router.rs`, and `src/execution/route_plan/status_application.rs`. The main remaining split-boundary risk is `repair-review-state`, where analysis and write-capable command execution still live together in `src/execution/review_state.rs`.

## Reviewer Recursion Assessment

No actionable issue. Reviewer recursion prevention remains prompt-text only and scoped to reviewer prompts/agents.

## Validation Results

- `cargo clean`: passed; removed prior build artifacts before this audit iteration.
- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 141/141.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed, real 47.56s after clean build.
- `cargo nextest run --all-targets --all-features --no-fail-fast --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query`: passed, 1786/1786, real 222.00s.
- `cargo test --test liveness_model_checker`: passed, 32/32, real 23.03s.

The nextest invocation ran the full 1786-test suite despite the requested `--test` filters, which provides stronger validation than the minimum targeted list.

## Prioritized Findings

### Medium

1. Public gate remediation can lead with artifact-repair imperatives.
   - Type: agent-UX dead-end risk.
   - Evidence: `src/execution/gates.rs:39`, `src/execution/gates.rs:861`, `src/execution/gates.rs:931`, `src/execution/gates.rs:1296`, `src/execution/gates.rs:1569`, `src/execution/authority.rs:946`.
   - Required fix: make `public_gate_remediation_for_plan` start with the public operator JSON query and place the caller-provided condition/action after it as diagnostic context. Add tests for first-action wording.

2. `repair-review-state` command execution still lives outside the command boundary.
   - Type: architecture/split-decisioning issue.
   - Evidence: `src/execution/commands/repair_review_state.rs:5`, `src/execution/review_state.rs:38`, `src/execution/review_state.rs:62`, `src/execution/review_state.rs:1204`, `src/execution/review_state.rs:1349`, `src/execution/review_state.rs:1387`.
   - Required fix: move public mutation guard, write helpers, and command body into `src/execution/commands/repair_review_state.rs` or child modules; leave `review_state.rs` as analysis/read-only support.

3. Boundary/source-shape tests and public-flow scanner taxonomy still preserve implementation shape.
   - Type: test realism/signal-to-noise issue.
   - Evidence: `tests/runtime_module_boundaries.rs:223`, `tests/runtime_module_boundaries.rs:276`, `tests/runtime_module_boundaries.rs:504`, `tests/support/public_flow_scan.rs:731`, `tests/public_flow_scan_contracts.rs:268`.
   - Required fix: convert private helper/source-shape pins to import/public DTO/behavior checks where possible, and replace expanding exception registries with explicit quarantined fixture modules or marker helpers.

4. Receipt source-word ban caused production obfuscation.
   - Type: test realism/signal-to-noise issue.
   - Evidence: `tests/runtime_authority_contracts.rs:159`, `src/execution/status_assembly/public_warnings.rs:1`, `src/execution/status_assembly.rs:720`, `src/workflow/operator.rs:526`.
   - Required fix: replace raw source substring ban with behavioral/public-output assertions, then remove `["rec", "eipt"].concat()` from production code.

### Low

5. Review-state reference row uses direct “record” wording.
   - Type: documentation/agent-UX issue.
   - Evidence: `docs/featureforge/reference/2026-04-01-review-state-reference.md:49`.
   - Required fix: say to follow the operator-returned typed public route instead of directly “record”ing closure.

6. Already-current branch-closure repair is not idempotent on rerun.
   - Type: churn source.
   - Evidence: `src/execution/commands/common/branch_closure_truth.rs:270`, `src/execution/transitions.rs:1991`, `src/execution/transitions.rs:2233`, first-run coverage in `tests/workflow_shell_smoke.rs:6838`.
   - Required fix: make overlay/follow-up setters dirty only on value change and add rerun/no-op coverage.

7. Schema descriptions duplicate canonical route-law prose.
   - Type: documentation/signal-to-noise issue.
   - Evidence: `src/execution/status.rs:37`, `schemas/workflow-operator.schema.json:20`, `schemas/plan-execution-status.schema.json:29`, canonical rule in `references/operator-route-authority.md:8`.
   - Required fix: keep schema descriptions terse and field-semantic; keep operational route-binding law in the canonical reference.

8. Prompt contract tests still act partly like a prose grammar.
   - Type: test signal-to-noise issue.
   - Evidence: `tests/codex-runtime/skill-doc-contracts.test.mjs:606`, `tests/codex-runtime/skill-doc-contracts.test.mjs:1030`.
   - Required fix: retain mandatory-law/reference/command-trap checks; reduce broad phrase policing where prompt budget and canonical references already enforce shape.

## Recommendation

Ship only after targeted fixes. The branch is no longer structurally unsafe in the old receipt/public-private/runtime-loop sense, but it still has enough actionable UX, idempotency, and signal/noise issues to justify one more remediation loop.
