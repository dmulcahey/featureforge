# FeatureForge Runtime Safety Audit - Twenty-Third Pass

Date: 2026-05-12

## Executive Verdict

**Recommendation: ship only after targeted fixes.**

The updated codebase is no longer showing the original high-risk public dead ends: normal transitions are reachable from shipped CLI commands, public route JSON carries typed executable argv/template authority, current task closure is the task-boundary authority, and receipt/projection/evidence freshness is diagnostic rather than control-plane truth.

The branch is still not clean enough to ship because the remaining findings are concentrated in split decisioning and signal/noise risk:

- route-plan finalization and router projection still both build status projections from a route decision, and they handle status-blocker errors differently
- read-surface invariant application can rebuild `routing.route_decision` from mutated status after route planning already finalized a route
- `status_assembly/exact_route.rs` still derives exact execution-route necessity from raw status fields instead of finalized route-plan authority
- public operator/handoff guidance lacks the explicit fail-closed branch that doctor and the canonical route reference already contain
- active generated skills still use "helper" vocabulary in normal workflow guidance
- several tests now pin private implementation/prose instead of durable public/runtime contracts

## What Is Genuinely Fixed

- Public CLI reachability is materially improved. `src/cli/plan_execution.rs`, `src/cli/workflow.rs`, and `src/lib.rs` expose normal mutation and read-only workflow surfaces.
- Public `begin` owns preflight/run identity setup through `src/execution/commands/begin.rs`.
- Public `close-current-task` owns current dispatch refresh and closure recording through `src/execution/commands/close_current_task.rs`.
- Public `advance-late-stage` owns branch closure, release readiness, final review, QA, and finish progression through `src/execution/commands/advance_late_stage.rs`.
- `recommended_public_command_argv` and `recommended_public_command_template` are typed public authority. `recommended_command` is display-only compatibility text.
- Public-flow tests are largely realistic: shipped-runtime proofs use the compiled CLI, and internal helper usage is quarantined by scanner contracts.
- Current task closure is task-boundary authority. Stale/missing receipt, dispatch, summary, and projection artifacts do not force reentry after authoritative pass/pass closure exists.
- Evidence and projection materialization are derived/audit surfaces, not normal progress controls.
- Plan fidelity uses parseable plan/review artifacts rather than hidden runtime receipt recording.
- Reviewer recursion prevention is prompt text only and reviewer-prompt scoped.
- Prompt budget enforcement is active and passed in this audit.

## What Remains Risky

1. `src/execution/route_plan/status_projection.rs::status_for_route_plan_finalization` and `src/execution/router.rs::project_final_runtime_routing_projection` duplicate route-to-status projection. The route-plan copy swallows `compute_status_blocking_records` errors with `if let Ok(...)`; the router copy propagates failures with `?`.
2. `src/execution/query.rs::sync_routing_surface_from_status` rebuilds `routing.route_decision` via `route_decision_from_routing` after read-surface invariants mutate status, creating a second decision surface after route planning.
3. `src/execution/status_assembly/exact_route.rs::public_execution_command_route_required` derives exact execution command requirements from raw status/context fields. That predicate should be route-plan-owned or derived from finalized route decision/state kind.
4. `src/workflow/operator.rs::operator_json_command_guidance` tells agents how to execute argv/template, but does not say to stop when neither executable surface exists. `src/workflow/doctor_dashboard.rs` and `references/operator-route-authority.md` already contain the correct stop rule.
5. Active generated skills use "helper" vocabulary for normal workflow routing and execution state, including `skills/using-featureforge/SKILL.md.tmpl`, `skills/executing-plans/SKILL.md.tmpl`, and `skills/subagent-driven-development/SKILL.md.tmpl`.
6. `tests/packet_and_schema.rs::public_runtime_route_golden_diagnostic_routes_are_diagnostic_only` skips diagnostic routes that expose argv or inputs instead of failing them, and it does not assert template absence.
7. `tests/runtime_module_boundaries.rs::route_plan_owns_runtime_route_ordering` pins private module/helper names and implementation shape across a large string-scanner test.
8. `tests/codex-runtime/skill-doc-contracts.test.mjs` pins release-note and README prose that is not itself a stable runtime contract.
9. `skills/requesting-code-review/SKILL.md.tmpl` keeps too much terminal route-binding law in a high-use top-level skill instead of delegating detailed mechanics to `references/operator-route-authority.md`.
10. `tests/fixtures/runtime-remediation/README.md` is verbose and duplicative enough to become a second audit narrative rather than a compact coverage inventory.
11. `tests/support/public_flow_scan.rs` mixes AST-backed checks with line-oriented string state machines. It is useful, but should not be expanded further unless parser-backed.

## Concrete Dead Ends Still Possible

No concrete public runtime dead end was reproduced through normal CLI routes.

The following dead-end classes were checked and were not observed as normal-path requirements:

- `plan execution preflight`
- `record-review-dispatch`
- `gate-review`
- `gate-finish`
- `rebuild-evidence`
- low-level late-stage recorders
- hidden/debug/compatibility helpers

Remaining traps are indirect:

- a diagnostic route can still be under-tested if it accidentally exposes argv/template/input surfaces
- operator/handoff text can send an agent back to re-query JSON without an explicit stop rule when no typed executable surface exists
- exact-route validation can diverge from finalized route-plan authority if status/context predicates drift

## Concrete Churn Sources Still Possible

- duplicated route-to-status projection between route-plan finalization and router projection
- post-invariant route decision rebuilding from status fields
- private implementation pins in `tests/runtime_module_boundaries.rs`
- release-note prose pins in `tests/codex-runtime/skill-doc-contracts.test.mjs`
- repeated "helper" vocabulary in generated skill templates
- over-large terminal-route mechanics in high-use skills
- verbose runtime-remediation inventory narrative

## Public/Private Test Mismatch Assessment

**Mostly fixed.**

Public-flow tests now use compiled CLI helpers for shipped-runtime proof. Internal direct helpers are quarantined under internal-only support files and scanner contracts reject hidden helper calls, hidden command strings, display-command execution, and unregistered synthetic event APIs in public-flow tests.

Remaining realism issue: some static tests have become implementation-shape contracts rather than public behavior contracts. This is not a public/private helper bypass, but it is noise that will make future structural cleanup expensive.

## Receipt/Evidence/Projection Control-Plane Assessment

**Fixed for normal workflow.**

No audit subagent found receipt/provenance/evidence/projection artifacts acting as authoritative workflow truth after current task closure exists. Projection materialization remains explicit and derived. Missing/stale receipts, review summaries, dispatch records, task-verification artifacts, and projection paths are diagnostic under the audited normal paths.

## Prompt-Surface And Packaging Assessment

**Partially fixed.**

Generated skills are fresh, budgets pass, companion references resolve, and reviewer recursion prevention is prompt scoped.

The audit found one packaging issue during the run: `references/reviewer-recursion-rule.md` was not covered by source archive / companion-reference tests. That was remediated in the working tree by updating `scripts/verify-source-archive.mjs` and `tests/codex-runtime/skill-doc-contracts.test.mjs`; targeted and full Node checks passed after the fix.

Remaining prompt issues are signal/noise:

- replace "helper" workflow vocabulary with `runtime`, `workflow/operator`, or `public route`
- add the fail-closed no-argv/no-template stop rule to operator/handoff guidance
- move detailed route-binding mechanics out of high-use skills and into canonical references
- reduce release-note/prose pins in Node tests

## Modularization And Split-Decisioning Assessment

**Close but not done.**

Good progress:

- route planning has a named owner under `src/execution/route_plan/`
- public route command authority is typed
- stale-target and repair-follow-up decisions have more cohesive modules than earlier iterations
- `state.rs` and mutator surfaces are no longer the main route-decision monoliths

Remaining issues:

- status projection is duplicated between route-plan and router
- query invariant application can replace a finalized route decision
- exact-route validation recomputes route necessity from raw status fields
- `PublicCommandKind::as_str` and `PublicMutationKind::public_command_name` duplicate command token vocabulary
- boundary tests enforce too much private shape and not enough behavior

## Reviewer Recursion Assessment

**Fixed with minor packaging/prose cleanup.**

Reviewer recursion prevention is prompt-only, scoped to reviewer prompts, and no runtime/env recursion guard was found. The new canonical reference is packaged by working-tree changes made during this audit. Keep this law centralized; do not repeat it in more places.

## Validation Results

Validation already completed before the audit loop:

- `node scripts/gen-skill-docs.mjs --check`: passed
- `node scripts/gen-agent-docs.mjs --check`: passed
- `node --test tests/codex-runtime/*.test.mjs`: passed, 133/133
- `cargo fmt --check`: passed
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- full `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`: passed, 1663/1663, real 127.23s
- `cargo test --test liveness_model_checker`: passed, 32/32
- `git diff --check`: passed

The twenty-third audit iteration then started with a required `cargo clean` after a process check; the clean removed 86,395 files and 13.8 GiB.

Audit validation after the clean:

- `node scripts/gen-skill-docs.mjs --check`: passed
- `node scripts/gen-agent-docs.mjs --check`: passed
- `node --test tests/codex-runtime/*.test.mjs`: passed, 133/133
- `cargo fmt --check`: passed
- `cargo clippy --all-targets --all-features -- -D warnings`: passed, real 34.97s
- `cargo nextest run --test runtime_authority_contracts`: passed, 7/7
- `cargo nextest run --test workflow_runtime`: passed, 89/89
- `cargo nextest run --test workflow_shell_smoke`: passed, 106/106
- `cargo nextest run --test workflow_entry_shell_smoke`: passed, 13/13
- `cargo nextest run --test plan_execution`: passed, 44/44
- `cargo nextest run --test plan_execution_final_review`: passed, 29/29
- `cargo nextest run --test workflow_runtime_final_review`: passed, 2/2
- `cargo nextest run --test contracts_execution_runtime_boundaries`: passed, 29/29
- `cargo nextest run --test execution_query`: passed, 12/12
- `cargo test --test liveness_model_checker`: passed, 32/32
- `node scripts/verify-source-archive.mjs`: passed after reviewer-recursion reference packaging fix
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`: passed, 62/62 after packaging fix
- `node --test tests/codex-runtime/skill-doc-budget.test.mjs`: passed, 3/3 after packaging fix
- `node --test tests/codex-runtime/*.test.mjs`: passed, 133/133 after packaging fix
- `git diff --check`: passed

No full test cycle was started without checking for active nextest processes. No audit validation run crossed the 10-minute remediation threshold.

## Prioritized Findings

### Blocker

None.

### High

1. **Duplicate route-to-status projection can diverge.**
   - Classification: architecture issue / split-decisioning issue.
   - References:
     - `src/execution/route_plan/status_projection.rs::status_for_route_plan_finalization`
     - `src/execution/router.rs::project_final_runtime_routing_projection`
     - `src/execution/status_assembly::compute_status_blocking_records`
   - Impact: route finalization can ignore blocker-computation failure while router presentation propagates it, allowing route/status surfaces to differ under error conditions.

2. **Read-surface invariants can replace finalized route decisions.**
   - Classification: architecture issue / split-decisioning issue.
   - References:
     - `src/execution/query.rs::apply_read_surface_invariants_to_routing_with_targetless_authority`
     - `src/execution/query.rs::sync_routing_surface_from_status`
     - `src/execution/route_plan::route_decision_from_routing`
     - `src/execution/invariants.rs::convert_status_to_runtime_reconcile_or_bug`
   - Impact: after route planning, invariant mutation can rewrite status and then reconstruct a route decision from routing/status DTOs instead of preserving route-plan authority.

3. **Exact execution-route requirement is derived outside route-plan authority.**
   - Classification: architecture issue / split-decisioning issue.
   - References:
     - `src/execution/status_assembly/exact_route.rs::public_execution_command_route_required`
     - `src/execution/read_model/public_route_projection.rs::apply_public_route_projection`
     - `src/execution/read_model.rs`
   - Impact: status assembly can decide route validity from raw fields instead of finalized route decision/state kind, creating a drift path.

### Medium

4. **Operator/handoff public guidance lacks fail-closed no-executable-surface instruction.**
   - Classification: public-output / agent-UX issue.
   - References:
     - `src/workflow/operator.rs::operator_json_command_guidance`
     - `src/workflow/doctor_dashboard.rs`
     - `references/operator-route-authority.md`
   - Impact: agents may re-query or infer from `next_action` when no typed argv/template exists.

5. **Active skills retain normal-path "helper" vocabulary.**
   - Classification: documentation / prompt-surface issue.
   - References:
     - `skills/using-featureforge/SKILL.md.tmpl`
     - `skills/executing-plans/SKILL.md.tmpl`
     - `skills/subagent-driven-development/SKILL.md.tmpl`
   - Impact: wording can send agents back toward hidden-helper mental models even when commands are now public/runtime-owned.

6. **Diagnostic-only golden test skips bad executable diagnostic routes.**
   - Classification: test realism / public-output issue.
   - References:
     - `tests/packet_and_schema.rs::public_runtime_route_golden_diagnostic_routes_are_diagnostic_only`
   - Impact: a diagnostic route with argv, template, or inputs could avoid the diagnostic-only assertions.

7. **Boundary tests overfit private route-plan implementation shape.**
   - Classification: test signal/noise issue.
   - References:
     - `tests/runtime_module_boundaries.rs::route_plan_owns_runtime_route_ordering`
   - Impact: tests create refactor friction and may encourage adding wrappers around duplication instead of deleting it.

8. **Release/prose checks are heavier than the behavior they protect.**
   - Classification: test signal/noise issue.
   - References:
     - `tests/codex-runtime/skill-doc-contracts.test.mjs`
   - Impact: intentional wording changes can fail tests unrelated to shipped runtime behavior.

9. **High-use code-review skill carries too much route-binding detail.**
   - Classification: prompt-surface / signal-noise issue.
   - References:
     - `skills/requesting-code-review/SKILL.md.tmpl`
     - `skills/requesting-code-review/SKILL.md`
     - `references/operator-route-authority.md`
   - Impact: agents get a runtime spec in a task skill, increasing the chance they miss the one action they need.

### Low

10. **Public command token vocabulary has two owners.**
    - Classification: cleanup / architecture issue.
    - References:
      - `src/execution/command_eligibility.rs::PublicCommandKind::as_str`
      - `src/execution/command_eligibility/mutation_request.rs::PublicMutationKind::public_command_name`
    - Impact: low current drift risk, but the same command names should be derived from one typed owner.

11. **Runtime-remediation inventory is too narrative and duplicative.**
    - Classification: documentation / test fixture signal-noise issue.
    - References:
      - `tests/fixtures/runtime-remediation/README.md`
    - Impact: future audits may update narrative instead of adding public replay proof.

12. **Public-flow scanner should not grow more line-oriented state machines.**
    - Classification: test maintainability issue.
    - References:
      - `tests/support/public_flow_scan.rs`
      - `tests/support/rust_source_scan.rs`
    - Impact: useful scanner, but additional non-AST checks would increase brittleness.

## Specific Failure-Class Checklist

| Failure class | Status | Notes |
|---|---|---|
| Public `begin` can seed preflight | fixed | Public begin owns preflight setup. |
| No normal flow needs `plan execution preflight` | fixed | No public path required it. |
| No normal flow needs `record-review-dispatch` | fixed | `close-current-task` / late-stage own refresh/recording. |
| No normal flow needs `gate-review` | fixed | Hidden command rejected by public-flow scanners. |
| No normal flow needs `gate-finish` | fixed | Late-stage public progression owns terminal gates. |
| No normal flow needs `rebuild-evidence` | fixed | Projection materialization is explicit/derived. |
| No normal flow needs low-level late-stage recorders | fixed | `advance-late-stage` owns aggregate intent. |
| Operator never recommends hidden/debug commands | fixed | Typed command surfaces are public. |
| Status never exposes hidden/debug commands as next actions | fixed | No normal-path hidden recommendations found. |
| Public recommended argv is executable by shipped CLI | fixed | Public CLI auditor found no P0-P2 reachability defect. |
| Plan-fidelity no longer uses hidden runtime receipt recording | fixed | Parseable artifact review path. |
| Plan-fidelity artifact is parseable and not overly hand-format-sensitive | fixed | Five-surface checks are parseable. |
| Engineering-review edits do not bounce back to fidelity early | fixed | No bounce loop found. |
| Final engineering-approved handoff requires current five-surface fidelity | fixed | Contract path still enforced. |
| Active docs do not teach plan-fidelity receipt recording | fixed | No active receipt guidance found. |
| Old `plan_fidelity_receipt` fields are gone or historical only | fixed | No active schema/output dependency found. |
| Current task closure is begin-time authority | fixed | Current closure owns task boundary. |
| Current closure cannot appear in stale closures | fixed | Stale/current overlap not found. |
| Close-current-task can refresh current dispatch internally | fixed | Public command owns dispatch refresh. |
| Stale dispatch does not block public close | fixed | No stale dispatch public-close dead end found. |
| Receipt/projection diagnostics do not trigger reentry | fixed | Diagnostics remain passive under authoritative closure. |
| Summary hash drift does not trigger reentry when pass/pass closure is current | fixed | No drift-driven reentry found. |
| Cycle-break clears after current closure | fixed | No cycle-break persistence issue found. |
| `resume_task` is not authoritative unless exact legal command is same begin | fixed | Treated as diagnostic unless bound to exact route. |
| Repair-review-state cannot loop on same route | fixed | No repeated repair loop found. |
| Runtime reconcile handles targetless stale states | fixed | Targetless reconcile is diagnostic/convergent. |
| Normal commands do not dirty tracked approved plan/evidence markdown | fixed | No normal-progress markdown dirtying found. |
| Projection materialization is explicit and not part of progress | fixed | Projection command is explicit. |
| Runtime-owned projection paths do not stale task/branch closures | fixed | No projection path closure staling found. |
| Supersession is append-only and does not rewrite proof | fixed | No proof rewrite path found. |
| Evidence is audit/projection, not control plane | fixed | No control-plane leakage found. |
| Public-flow tests do not call internal helpers | fixed | Scanner and public CLI helper coverage pass. |
| Internal helpers are quarantined in internal-unit-only tests | fixed | Internal support files are quarantined. |
| Static tests catch hidden helper use in public-flow tests | fixed | Public-flow scanners exist and pass. |
| Replay tests cover historical dead ends | fixed | Coverage exists, inventory wording needs cleanup. |
| Liveness model catches repeated route signatures | fixed | Liveness tests passed. |
| Node/doc contracts pass | fixed | Passed after packaging fix. |
| Prompt budget test passes | fixed | Passed, 5008/5050. |
| Skill docs are within budget | fixed | Budget test passed. |
| Mandatory law remains top-level | fixed | No missing mandatory top-level law found. |
| Companion references exist and are packaged | partially fixed | Reviewer recursion reference packaging was fixed during audit; keep it staged/committed. |
| Generated docs are fresh | fixed | Generation checks passed. |
| Reviewer recursion prevention is prompt-only and reviewer-prompt scoped | fixed | No runtime/env guard found. |
| No runtime/env recursion enforcement is introduced | fixed | Guard scanners pass. |
| Reviewer prompts prohibit launching additional subagents | fixed | Canonical rule present. |
| `state.rs` and `mutate.rs` are not monoliths | fixed | Route law has moved out. |
| New modules have cohesive responsibilities | partially fixed | Route-plan improved, but status projection and exact-route predicates remain split. |
| No new catch-all module replaces old monoliths | partially fixed | `next_action_choice.rs` is still large but not the key actionable issue this pass. |
| Phase/reason strings are centralized | fixed | No new active drift found. |
| Public command authority is typed, not string-parsed | fixed | Display parsing not executable authority. |
| Router/read-model/mutation guards share decision objects | partially fixed | Query invariants can still rebuild route decisions from status. |
| Import-boundary tests exist | fixed | They exist, but some are too brittle. |
