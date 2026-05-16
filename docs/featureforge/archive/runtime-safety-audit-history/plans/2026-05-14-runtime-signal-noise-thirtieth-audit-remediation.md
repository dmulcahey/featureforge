# Runtime Signal/Noise Thirtieth-Audit Remediation

**Workflow State:** Draft
**Plan Revision:** 1
**Execution Mode:** implementation
**Source Spec:** `docs/featureforge/archive/runtime-safety-audit-history/2026-05-14-thirtieth-audit-report.md`
**Source Spec Revision:** 1
**Last Reviewed By:** audit-loop

## Goal

Close the actionable thirtieth-audit findings without adding another layer of workflow law. The implementation must reduce split decisioning, fix one public route-reference bug, centralize reason-code ownership, and lower prompt/doc noise while preserving the runtime safety gains from the previous remediation.

## Architecture

Runtime route truth remains structured and shared:

- Public executable authority stays in typed `recommended_public_command_argv` and `recommended_public_command_template`.
- Workflow/operator template binding must include the required plan path and must materialize executable argv through Rust-owned template validation.
- Current task closure versus branch-closure routing must be decided by one shared predicate consumed by route selection and next-action projection.
- Stale/provenance and QA policy reason codes must have named runtime owners and predicates, not repeated string literals.
- Skills keep only compact mandatory law at top level and delegate details to canonical references.
- Historical remediation artifacts are archived so active plan discovery has a single current audit-remediation authority.

## Change Surface

- `references/operator-route-authority.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `src/execution/status_support.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/next_action_choice/execution_routes.rs`
- `src/execution/gate_reason_codes.rs`
- `src/execution/closure_graph.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/read_model.rs`
- `src/execution/state/rebuild_evidence.rs`
- `src/execution/state/review_gate.rs`
- `src/execution/state/finish_gate.rs`
- `src/execution/current_truth.rs`
- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/late_stage.rs`
- `src/workflow/pivot.rs`
- `tests/runtime_module_boundaries.rs`
- `skills/using-featureforge/SKILL.md.tmpl`
- generated `skills/using-featureforge/SKILL.md`
- selected high-use skill templates and generated docs only if repeated route-law prose can be safely collapsed
- `docs/featureforge/plans/**`
- `docs/featureforge/archive/runtime-safety-audit-history/plans/**`
- `RELEASE-NOTES.md` if historical receipt wording can be made clearer without rewriting release history

## Preconditions

- Do not use FeatureForge skills or project skills.
- Do not run FeatureForge workflow/runtime commands as workflow participation.
- Use public CLI only inside normal tests that already exercise public CLI behavior.
- Before every full test cycle, verify no `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, or `cargo clippy` process is active.
- If a full suite exceeds 4-5 minutes, run `cargo clean`, rerun, and remediate if repeatable. If it exceeds 10 minutes, stop immediately and apply the clean/rerun/remediation rule.
- After each task, run strict clippy and a full no-fail-fast nextest suite before dispatching independent review.

## Known Footguns / Constraints

- Do not weaken typed public route authority.
- Do not delete mandatory route law from top-level route-owning skills.
- Do not replace duplicated prompt law with a broken or undiscoverable companion reference.
- Do not add new broad static scanners unless they replace an older noisier check.
- Do not archive the current remediation plan or non-audit product plans.
- Preserve historical audit artifacts by moving them under `docs/featureforge/archive/runtime-safety-audit-history`, not deleting them.
- Do not suppress clippy or weaken lint policy.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| Operator template binding docs include required plan path | Task 1 |
| Canonical route-law contract test pins correct binding shape | Task 1 |
| Close-current-task versus branch-closure routing has one predicate owner | Task 2 |
| `files_proven_drifted` reason code has one owner and predicate | Task 3 |
| `qa_requirement_missing_or_invalid` reason code has one owner and predicate | Task 3 |
| Boundary tests catch reason-code literal drift | Task 3 |
| High-use skills stay actionable and less repetitive | Task 4 |
| `using-featureforge` honors explicit user constraints without high-pressure contradiction | Task 4 |
| Superseded audit-remediation plans leave active plan discovery | Task 5 |

## Task 1: Correct Operator Template Binding Documentation

**Spec Coverage:** Operator template binding docs include required plan path; canonical route-law contract test pins correct binding shape.

**Goal:** Prevent agents from following an incomplete operator template-binding command.

**Context:**

`references/operator-route-authority.md` previously described template rebinding without the required `--plan <approved-plan-path>` argument. The active reference and contract tests must pin the plan-bound form.

**Constraints:**

- Keep the canonical reference as the detailed route-law owner.
- Do not tell agents to hand-write `advance-late-stage`.
- Keep the typed argv/template rule intact.

**Done when:**

- The reference says to rerun the same operator query with `--plan <approved-plan-path> --input NAME=VALUE --json`.
- Late-stage aggregate text says the same.
- The Node contract test rejects the old no-plan wording and accepts the corrected wording.

**Files:**

- `references/operator-route-authority.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

**Detailed Implementation Steps:**

1. Replace incomplete operator-template rebinding references with the plan-bound form.
2. Update the contract regex in `canonical operator route authority reference owns detailed typed route law`.
3. Add a negative assertion that the canonical reference does not contain the exact incomplete no-plan command phrase.
4. Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` as a focused check before the full gate.

**Validation Expectations:**

- Focused Node contract test passes.
- Strict clippy passes.
- Full no-fail-fast nextest passes before review.
- Independent task review finds no public-route doc mismatch.

## Task 2: Centralize Current-Task Closure Versus Branch-Closure Routing

**Spec Coverage:** Close-current-task versus branch-closure routing has one predicate owner.

**Goal:** Remove duplicated route predicates so route decision and next-action projection cannot drift.

**Context:**

The same rule appears in `route_plan.rs::close_current_task_or_branch_closure_route_decision` and `route_plan/next_action_choice/execution_routes.rs::task_closure_recording_ready_decision`: if the task is already current, no current branch closure exists, and the current task closure set contributes to the branch surface, route to branch closure recording instead of close-current-task.

**Constraints:**

- Preserve existing public output shape.
- Preserve non-branch-contributing task-closure behavior.
- Do not move next-action rendering into mutators.

**Done when:**

- One shared helper owns the predicate.
- `route_plan.rs` and `execution_routes.rs` both call the helper.
- A boundary test or unit assertion protects the helper from being re-expanded into duplicate predicates.

**Files:**

- `src/execution/status_support.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/next_action_choice/execution_routes.rs`
- `tests/runtime_module_boundaries.rs`

**Detailed Implementation Steps:**

1. Add `current_task_closure_should_route_to_branch_closure(context, status, task_number)` to `status_support.rs`.
2. Implement it using the existing current task closure, missing current branch closure, and non-branch-contributing checks.
3. Replace the duplicated inline predicates in `route_plan.rs` and `execution_routes.rs`.
4. Add a static boundary test that checks both consumers call the shared helper and do not locally expand the `current_task_closures`/`current_branch_closure_id`/non-branch predicate.
5. Run focused `cargo test --test runtime_module_boundaries` for the new/updated boundary test before the full gate.

**Validation Expectations:**

- Focused boundary test passes.
- Strict clippy passes.
- Full no-fail-fast nextest passes before review.
- Independent task review finds no duplicate route predicate.

## Task 3: Centralize Stale/QA Reason-Code Ownership

**Spec Coverage:** `files_proven_drifted` reason code has one owner and predicate; `qa_requirement_missing_or_invalid` reason code has one owner and predicate; boundary tests catch reason-code literal drift.

**Goal:** Keep stale/provenance and QA policy reason-code producers and consumers aligned through named constants and classifiers.

**Context:**

The audit found `files_proven_drifted` and `qa_requirement_missing_or_invalid` repeated across producers, classifiers, status assembly, route selection, follow-up/pivot logic, and tests.

**Constraints:**

- Preserve the public reason-code strings.
- Do not broaden or narrow routing semantics.
- Boundary tests should assert ownership without blocking golden/public-output tests from asserting literal compatibility at the boundary.

**Done when:**

- `gate_reason_codes.rs` owns both constants and predicates.
- Runtime code consumes constants/predicates instead of repeating string literals.
- Boundary tests fail if active Rust production code duplicates those literals outside the owner.
- Tests that intentionally assert boundary JSON may still spell literals.

**Files:**

- `src/execution/gate_reason_codes.rs`
- `src/execution/closure_graph.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/read_model.rs`
- `src/execution/state/rebuild_evidence.rs`
- `src/execution/state/review_gate.rs`
- `src/execution/state/finish_gate.rs`
- `src/execution/current_truth.rs`
- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/late_stage.rs`
- `src/execution/route_plan/next_action_choice/execution_routes.rs`
- `src/workflow/pivot.rs`
- `tests/runtime_module_boundaries.rs`

**Detailed Implementation Steps:**

1. Add `FILES_PROVEN_DRIFTED`, `QA_REQUIREMENT_MISSING_OR_INVALID`, `files_proven_drifted_reason_code(...)`, `qa_requirement_missing_or_invalid_reason_code(...)`, and any tiny helper needed for push/check call sites to `gate_reason_codes.rs`.
2. Replace production string literal producers with `String::from(CONST)` or `.to_owned()`.
3. Replace consumers with predicate functions.
4. Update existing finish-gate reason-code boundary test into a broader gate/stale reason-code ownership test or add a new focused test.
5. Keep public golden/test fixture literals where they are boundary assertions or serialized fixture content.
6. Run focused `cargo test --test runtime_module_boundaries reason_code` before the full gate.

**Validation Expectations:**

- Focused boundary tests pass.
- Existing runtime behavior tests still pass.
- Strict clippy passes.
- Full no-fail-fast nextest passes before review.
- Independent task review finds no remaining production duplicate ownership.

## Task 4: Reduce Prompt Rule Pressure Without Losing Mandatory Law

**Spec Coverage:** High-use skills stay actionable and less repetitive; `using-featureforge` honors explicit user constraints without high-pressure contradiction.

**Goal:** Lower prompt noise where the current wording encourages self-referential skill invocation or repeats route law already centralized in the operator reference.

**Context:**

The signal/noise audit found `using-featureforge` uses high-pressure "1% chance" language while also saying user instructions win. It also flagged repeated phase-specific prompt rules in high-use skills.

**Constraints:**

- Do not remove the compact Installed Control Plane law from route-owning skills.
- Do not move mandatory law solely into companion docs.
- Prefer deleting or collapsing repeated prose over adding new tests.
- Regenerate generated skill docs after template edits.

**Done when:**

- `using-featureforge` states one clear rule: check applicable skills unless explicit user instructions forbid them; user instructions always win.
- High-use skill templates retain only compact top-level route law plus the canonical reference unless a phase-specific rule is genuinely unique.
- Contract tests assert reference/link coverage and mandatory law, not excessive exact prose.
- Generated `SKILL.md` files are fresh.

**Files:**

- `skills/using-featureforge/SKILL.md.tmpl`
- `skills/using-featureforge/SKILL.md`
- selected high-use skill templates and generated skills if exact duplication is removed
- `scripts/gen-skill-docs.mjs` only if the generator owns the repeated text
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `skills/skill-doc-budgets.json` if line counts change enough to require budget updates

**Detailed Implementation Steps:**

1. Replace the `using-featureforge` extreme rule block and "The Rule" wording with a concise rule that preserves skill discovery but explicitly respects user prohibitions.
2. Inspect the cited high-use templates and remove duplicated low-level routing prose only where the canonical reference already covers it and the skill keeps a clear action.
3. Update contract tests from exact prose assertions to minimal-law plus reference assertions where needed.
4. Regenerate skill docs with `node scripts/gen-skill-docs.mjs`.
5. Run focused `node --test tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-budget.test.mjs`.

**Validation Expectations:**

- Generated docs are up to date.
- Prompt budget remains in enforce mode and passes.
- Strict clippy passes.
- Full no-fail-fast nextest passes before review.
- Independent task review finds no prompt-law loss or added noise.

## Task 5: Archive Superseded Audit-Loop Plans

**Spec Coverage:** Superseded audit-remediation plans leave active plan discovery.

**Goal:** Reduce active plan directory noise without deleting historical audit/remediation evidence.

**Context:**

`docs/featureforge/plans` contains many superseded runtime-safety audit-remediation plans from prior loop iterations. The archive already has `runtime-safety-audit-history/plans`.

**Constraints:**

- Do not archive the current thirtieth remediation plan.
- Do not archive non-audit product plans such as the workflow doctor plan.
- Preserve file contents and git history through moves.
- Add a short active index or README note only if needed to explain why old plans moved.

**Done when:**

- Superseded `*audit*remediation*.md`, `*signal-noise*.md`, and round-specific runtime-audit-loop plan files are moved under `docs/featureforge/archive/runtime-safety-audit-history/plans`.
- `docs/featureforge/plans` keeps the current remediation plan and non-audit plans.
- Active docs/tests that reference moved files are updated only when they point to current active paths.

**Files:**

- `docs/featureforge/plans/**`
- `docs/featureforge/archive/runtime-safety-audit-history/plans/**`
- docs/tests that reference moved paths, if any

**Detailed Implementation Steps:**

1. List active plans and classify each as current remediation, non-audit product plan, or superseded audit-loop plan.
2. Move superseded audit-loop plans to the runtime-safety archive plans directory.
3. Update any active references that must point to archive paths.
4. Run `rg` for moved filenames in active docs/tests and verify references are historical or corrected.
5. Run `git diff --check`.

**Validation Expectations:**

- Node doc/source archive tests pass.
- Strict clippy passes.
- Full no-fail-fast nextest passes before review.
- Independent task review finds reduced active-plan noise without broken references.

## Task 6: Final Audit Loop

**Spec Coverage:** all requirements.

**Goal:** Prove the remediation closed all actionable findings and did not reintroduce runtime churn.

**Context:**

The user requires audit -> implementation loops until no actionable audit issues remain.

**Constraints:**

- Run `cargo clean` before the next audit iteration.
- Include the additional signal/noise subagent.
- Do not interrupt in-flight audits or tests.

**Done when:**

- Full validation passes.
- A clean-context whole-plan review of this remediation is clean.
- Fresh A-H plus signal/noise audit reports no actionable findings, or any new actionable finding is converted into the next remediation plan and implementation continues.

**Files:**

- audit report and plan artifacts as needed

**Detailed Implementation Steps:**

1. After Task 5 review is clean, run the full validation gate.
2. Dispatch a clean-context whole-remediation review against this plan.
3. Remediate any findings and revalidate/re-review until clean.
4. Run `cargo clean`.
5. Dispatch the original audit subagents A-H plus signal/noise auditor.
6. Synthesize results and either declare no actionable findings or write the next remediation plan.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`
