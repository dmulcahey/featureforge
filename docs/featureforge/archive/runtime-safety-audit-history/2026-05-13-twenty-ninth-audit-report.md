# Twenty-Ninth Runtime Safety Audit Report

Date: 2026-05-13

## Executive Verdict

**Do not ship yet.** The latest implementation materially improved the public route contract, test realism, and typed command surfaces, but this audit still found actionable authority and signal/noise issues.

The branch is close, not structurally unsafe in the original broad sense. Public CLI reachability, hidden command quarantine, plan-fidelity, stale closure convergence, and reviewer recursion controls now have strong evidence. The remaining blockers are narrower:

- active-contract unit-review receipt markdown can still block gate truth;
- QA recording/finish still treats test-plan markdown materialization as mandatory control-plane state;
- a few public diagnostics still point agents toward retired dispatch/manual repair language;
- route skills and prompt/reference packaging have small actionability gaps;
- modularization tests still contain some low-signal shape constraints, and `status_assembly.rs` remains too large a hub.

## What Is Genuinely Fixed

- Public runtime transitions are reachable through shipped commands. `begin`, `close-current-task`, `repair-review-state`, and `advance-late-stage` own the normal paths inspected by the audit.
- Workflow/operator and status use typed public command authority. `recommended_public_command_argv` and `recommended_public_command_template` are the executable surfaces; `recommended_command` is display-only compatibility text.
- Public-flow tests now strongly prefer compiled CLI/shell boundaries, with internal helpers quarantined and statically scanned.
- Current task closure no longer appears to restale from summary/projection-only drift in the inspected paths.
- Projection materialization is explicit and not the normal routing authority.
- Plan-fidelity uses parseable plan review artifacts and five-surface checks instead of unreachable receipt mechanics.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped.
- Prompt budgets and generated skill/agent freshness checks are active and green.

## What Remains Risky

- Runtime receipts have not fully been demoted to projections. Active-contract serial unit-review receipt files still participate in gate truth.
- QA has a remaining markdown artifact dependency through test-plan selection and source-test-plan fingerprint binding.
- Some diagnostics still use action words such as “dispatching task review” or “Repair the spec/plan” without binding the user to the public operator route.
- Prompt surface is improved but near saturation. The best next changes should delete duplicated law and make existing instructions more actionable, not add more prose.
- Static boundary tests still include shape assertions that can incentivize file-count/line-count churn.

## Concrete Dead Ends Still Possible

- A checked/completed step with active contract overlay can fail final-review gating if `unit-review-<run>-task-<task>-step-<step>.md` is missing, unreadable, malformed, or has stale headers, even when runtime state and completed attempt provenance are current.
- QA recording can return a requery/refresh blocker if the current test-plan markdown artifact has been pruned or is HEAD-stale, even though the current branch closure, release-readiness, and final-review records are authoritative and current.
- An equivalent QA rerun can fail to return `already_current` when the current QA record lacks a source test-plan fingerprint and no current test-plan artifact can be found.
- Public diagnostics that say “before dispatching task review” can send an agent searching for retired dispatch commands.

## Concrete Churn Sources Still Possible

- `status_assembly.rs` remains a 2,880-line hub covering hydration, defaults, blocking-record projection, overlay parsing, review-state facts, branch gate bindings, and route-neutral fact assembly.
- `runtime_module_boundaries.rs` still has some low-signal source-shape checks, including child-module count and facade line-count assertions.
- Generated skill tests now avoid duplicating most route-law prose, but `requesting-code-review` currently delegates final-review command materialization to prose instead of executing the typed route.

## Public/Private Test Mismatch Assessment

No current blocker found. Public-flow tests use the compiled CLI boundary, public helper quarantine is scanned, and replay fixtures cover historical stuck paths. Internal model/liveness tests remain internal, but they are labeled and not presented as shipped-runtime proof.

Residual caveat: some boundary tests are source-shape tests rather than behavioral tests. This is a test-signal problem, not a demonstrated public/private mismatch.

## Receipt/Evidence/Projection Control-Plane Assessment

Partially fixed. Task closures, projection materialization, stale summaries, and release/final-review projections are mostly runtime-state-first. However:

- `src/execution/state/unit_review_truth.rs` still classifies active-contract serial unit-review receipts as authoritative and fails gate truth on receipt file absence or malformed content.
- `src/execution/commands/advance_late_stage.rs` and `src/execution/state/artifact_finish_truth.rs` still require current test-plan artifact/fingerprint binding for QA recording and finish readiness.

## Prompt Surface And Packaging Assessment

Mostly fixed, with targeted cleanup required:

- Skill budgets pass.
- Generated docs are fresh.
- Companion references are mostly rooted and packaged.
- `skills/test-driven-development/SKILL.md.tmpl` still uses unrooted `@testing-anti-patterns.md`.
- `.codex/INSTALL.md` and `.copilot/INSTALL.md` preserve stale generated-preamble language.
- `qa/references/issue-taxonomy.md` still says authoritative operations “stay helper-owned.”
- `skills/requesting-code-review/SKILL.md.tmpl` stops after printing final-review route metadata instead of executing or explicitly failing on the typed route.

## Modularization And Split-Decisioning Assessment

Improved but not done. The hot routing path is much more centralized, and no evidence was found of workflow/operator importing mutation helpers, read-model appending events, command modules writing projection read models directly, or public command typing falling back to display strings.

Remaining architectural debt:

- `RouteDecision` can still be reconstructed from `ExecutionRoutingState` DTO fields in `src/execution/route_plan/decision_support.rs`.
- `plan_runtime_route` still uses a two-pass status projection/finalization loop. This appears owned by the route-plan module, but it should be documented as an intentional fixed point or refactored.
- `status_assembly.rs` remains too broad.
- Boundary tests still lean on source strings and shape checks.

## Reviewer Recursion Assessment

Fixed. Reviewer recursion prevention is prompt text only, reviewer-prompt scoped, and no runtime/env recursion enforcement was found. Reviewer prompts prohibit launching additional subagents.

## Validation Results

Passed:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs` - 136/136 passing, approximately 510 seconds
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --no-fail-fast --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query` - 332/332 passing
- `cargo test --test liveness_model_checker` - 32/32 passing

Not run during this audit:

- Full all-target nextest. The audit validation request listed specific Rust test binaries, and those targeted binaries were run successfully after a clean build state.

Performance note:

- The full Node codex-runtime test glob passed but took approximately 8.5 minutes. This is below the 10-minute immediate-stop threshold but high enough to watch. The runtime full nextest suite from the preceding implementation pass remained below the 4-5 minute threshold after a clean build.

## Prioritized Findings

### Blocker

1. **Unit-review receipt files still control gate truth.**
   - Type: user-facing dead end / control-plane leakage
   - Files:
     - `src/execution/state/unit_review_truth.rs`
     - `src/execution/state/review_gate.rs`
     - `src/execution/state/worktree_lease_truth.rs`
   - Functions:
     - `classify_unit_review_proof_authority`
     - `enforce_serial_unit_review_truth`
     - `validate_authoritative_unit_review_receipt`
     - `enforce_worktree_lease_binding_truth`
   - Evidence:
     - Active contract state classifies serial unit-review proof as authoritative.
     - Missing/unreadable/malformed receipt file failures include `serial_unit_review_receipt_missing`, `serial_unit_review_receipt_unreadable`, and receipt header mismatch failures.
   - Required remediation:
     - Active-contract serial unit-review gate truth must be derived from runtime-owned contract, run identity, completed attempt provenance, and repository commit proof. Receipt markdown presence/content must become diagnostic-only.

### High

2. **QA progress depends on test-plan markdown materialization.**
   - Type: user-facing dead end / projection control-plane leakage
   - Files:
     - `src/execution/commands/advance_late_stage.rs`
     - `src/execution/commands/common/late_stage_reruns.rs`
     - `src/execution/state/artifact_finish_truth.rs`
   - Functions:
     - `record_qa`
     - `already_current_qa_rerun_if_equivalent`
     - `current_test_plan_artifact_path_for_qa_recording`
     - `require_authoritative_test_plan_binding_for_current_qa`
   - Evidence:
     - QA recording blocks when `current_test_plan_artifact_path` returns missing/stale.
     - Equivalent current QA rerun refuses `already_current` when the record lacks source test-plan fingerprint and no current artifact exists.
     - Finish readiness fails `qa_source_test_plan_mismatch` for a current QA record missing source-test-plan binding.
   - Required remediation:
     - Make source test-plan artifact/fingerprint a diagnostic provenance field. Current branch closure, final-review, QA record identity, result, summary hash, branch/repo/base/reviewed state, and generated-by identity should own QA readiness.

### Medium

3. **`requesting-code-review` final-review shell block is not executable enough.**
   - Type: agent UX / prompt actionability
   - Files:
     - `skills/requesting-code-review/SKILL.md.tmpl`
     - `skills/requesting-code-review/SKILL.md`
     - `tests/codex-runtime/skill-doc-contracts.test.mjs`
   - Required remediation:
     - Materialize and execute `recommended_public_command_argv` or a completed `recommended_public_command_template` from `RECORDING_READY_JSON`, or explicitly stop when neither is present.

4. **Public diagnostics still contain retired/manual action wording.**
   - Type: agent UX / documentation issue
   - Files:
     - `src/execution/state/runtime_methods.rs`
     - `src/execution/state/review_gate.rs`
     - `src/workflow/status.rs`
   - Reason/action terms:
     - “before dispatching task review”
     - “Finish all steps in the task before dispatching task review”
     - “Complete, interrupt, or resolve”
     - “Repair the spec/plan”
   - Required remediation:
     - Preserve domain detail, but make remediation text point to workflow/operator JSON and typed public argv/template or to the named public review/authoring route.

5. **Prompt/reference packaging has small stale references.**
   - Type: documentation / packaging
   - Files:
     - `skills/test-driven-development/SKILL.md.tmpl`
     - `.codex/INSTALL.md`
     - `.copilot/INSTALL.md`
     - `qa/references/issue-taxonomy.md`
     - `tests/codex-runtime/skill-doc-contracts.test.mjs`
   - Required remediation:
     - Replace unrooted `@testing-anti-patterns.md`, remove stale preamble wording, replace “helper-owned,” and add a contract preventing unrooted `@path` references in generated skills.

### Low

6. **Route DTO reconstruction remains as a split-decision escape hatch.**
   - Type: architecture issue
   - Files:
     - `src/execution/route_plan/decision_support.rs`
     - `src/execution/query.rs`
     - `src/execution/review_state.rs`
     - `tests/contracts_execution_runtime_boundaries.rs`
   - Required remediation:
     - Fail closed for runtime callers when `route_decision` is absent, or explicitly restrict reconstruction to compatibility/non-runtime paths.

7. **`status_assembly.rs` remains a broad hub.**
   - Type: architecture issue
   - Files:
     - `src/execution/status_assembly.rs`
     - `docs/featureforge/reference/execution-runtime-module-boundaries.md`
     - `tests/runtime_module_boundaries.rs`
   - Required remediation:
     - Extract named responsibilities into cohesive child modules and keep `status_assembly.rs` as a facade/orchestrator.

8. **Some module-boundary tests enforce source shape instead of semantics.**
   - Type: test realism / signal-to-noise issue
   - Files:
     - `tests/runtime_module_boundaries.rs`
   - Required remediation:
     - Keep import-direction, owner, and forbidden dependency assertions. Remove arbitrary child-module count and line-count shape checks where semantic guards are available.

## Failure Class Checklist

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
- Receipt/projection diagnostics do not trigger reentry: partially fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed based on inspected routes.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed for state projection, partially fixed for QA test-plan artifact dependency.
- Runtime-owned projection paths do not stale task/branch closures: fixed in inspected paths.
- Supersession is append-only and does not rewrite proof: fixed in inspected paths.
- Evidence is audit/projection, not control plane: partially fixed.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: fixed for known routes.
- Liveness model catches repeated route signatures: fixed as internal model coverage.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: partially fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces the old monoliths: partially fixed because `status_assembly.rs` remains too broad.
- Phase/reason strings are centralized: mostly fixed.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: mostly fixed.
- Import-boundary tests exist: fixed.

## Recommendation

**Ship only after targeted fixes.** The next remediation should be narrow and deletion-oriented:

1. remove unit-review receipt markdown from gate authority;
2. remove test-plan markdown materialization from QA control-plane truth;
3. tighten public diagnostic wording and the final-review skill command block;
4. clean stale prompt/install references;
5. reduce modularity/test shape noise by extracting `status_assembly.rs` responsibilities and deleting brittle shape assertions.
