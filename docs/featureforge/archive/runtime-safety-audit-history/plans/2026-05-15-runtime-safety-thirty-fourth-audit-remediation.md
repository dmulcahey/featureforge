# Runtime Safety Thirty-Fourth Audit Remediation

## Workflow State

Engineering remediation plan for the thirty-fourth runtime-safety audit loop. This plan is active until every task below is implemented, fully verified, independently reviewed, and followed by another deep audit loop with the signal-to-noise auditor included.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task. Do not use FeatureForge runtime/workflow commands as workflow participation. Do not use FeatureForge/project skills. Do not allow reviewers or implementation subagents to spawn additional subagents.

## Goal

Close the remaining actionable audit findings without adding new conceptual surface area. The implementation must make public failure guidance public-route-first, make already-current branch-closure repair idempotent, move `repair-review-state` write behavior into the command boundary, and reduce scanner/source-shape churn where it now preserves implementation trivia instead of public behavior or true ownership boundaries.

## Architecture

- Public operator JSON is the first remediation step for gate failures. Artifact/state conditions may explain why a gate failed, but they must not read as direct manual repair instructions.
- Mutation execution belongs under `src/execution/commands/**`. Read/reconcile modules may analyze state and produce decisions, but they must not own public command bodies or persist mutation state directly.
- Authoritative transition setters should be idempotent. Re-running an already-current public aggregate command must not dirty authoritative state when the target fields already match.
- Tests should enforce public behavior, import direction, typed ownership, and cross-module API boundaries. They should avoid private helper-name pins, source snippets, and scanner exception taxonomies unless those symbols are deliberate architectural APIs.
- Schema descriptions should describe field semantics tersely. Full route execution law belongs in the canonical operator route reference and generated top-level skill law.

## Change Surface

- `docs/featureforge/archive/runtime-safety-audit-history/2026-05-15-thirty-fourth-audit-report.md`
- `docs/featureforge/plans/2026-05-15-runtime-safety-thirty-fourth-audit-remediation.md`
- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `docs/runtime-architecture.md`
- `docs/testing.md`
- `src/execution/gates.rs`
- `src/execution/authority.rs`
- `src/execution/commands/repair_review_state.rs`
- `src/execution/commands/repair_review_state/**` if split into child modules
- `src/execution/review_state.rs`
- `src/execution/transitions.rs`
- `src/execution/commands/common/branch_closure_truth.rs`
- `src/execution/status.rs`
- `src/execution/status_assembly/public_warnings.rs`
- `schemas/plan-execution-status.schema.json`
- `schemas/workflow-operator.schema.json`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_authority_contracts.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/support/public_flow_scan.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/codex-runtime/*.test.mjs` as needed for schema/skill contract updates

## Preconditions

- Do not use FeatureForge skills or project skills.
- Do not run FeatureForge runtime/workflow commands as workflow participation. Test commands that exercise the shipped CLI are allowed as validation.
- Before each full test cycle, verify no `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, or `cargo clippy` process is active.
- Before each audit-loop iteration, run `cargo clean`.
- Run strict clippy and a full no-fail-fast nextest suite before dispatching each clean-context review.
- If a full suite exceeds 4-5 minutes, run `cargo clean`, rerun, and remediate introduced performance issues if repeatable. If it exceeds 10 minutes, stop immediately and apply the clean/rerun/remediation rule.
- Edit generated schema or skill sources, then regenerate/check generated artifacts through the existing scripts.
- Prefer deleting or narrowing duplicate scanners over expanding exception lists.

## Known Footguns / Constraints

- Do not replace direct artifact-repair wording with vaguer text. The public next step must be concrete and executable through workflow/operator JSON.
- `recommended_public_command_argv` remains the authoritative machine invocation; `recommended_command` remains display-only compatibility text.
- Do not weaken gate diagnostics; preserve the failed condition as context after the public next step.
- Do not move repair-review-state analysis into command code wholesale. Move the write-capable public command boundary; keep reusable analysis facts shared.
- Do not hide scanner smells by obfuscating words in production code.
- Do not delete tests that protect real public/private command boundaries. Remove or rework tests that pin private implementation spelling.
- Do not make already-current branch closure repair skip necessary first-run overlay restoration or follow-up clearing.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| Gate remediation starts with one public operator JSON next step | Task 1 |
| Artifact/state repair conditions are diagnostic context, not first imperatives | Task 1 |
| Review-state reference avoids direct record/repair wording | Task 1 |
| Already-current branch-closure repair is idempotent on rerun | Task 2 |
| Transition setters dirty state only when values change | Task 2 |
| `repair-review-state` mutation execution lives under command boundary | Task 3 |
| `review_state.rs` is analysis/read-support, not public command body owner | Task 3 |
| Boundary tests reduce private helper/source-shape pins | Task 4 |
| Public-flow scanner exception taxonomy is narrowed or replaced with explicit quarantine markers | Task 4 |
| Receipt terminology tests assert behavior/output rather than raw source-word bans | Task 4 |
| Production code no longer obfuscates `receipt` to satisfy a scanner | Task 4 |
| Schema descriptions are terse field semantics; canonical route law remains in reference/skills | Task 5 |
| Prompt contract tests retain high-value traps while reducing prose grammar checks | Task 5 |
| Full validation and independent rereview pass | Task 6 |

## Task 1: Make Public Gate Remediation Public-Route-First

**Spec Coverage:** Gate remediation starts with one public operator JSON next step; artifact/state repair conditions are diagnostic context; review-state reference avoids direct record/repair wording.

**Goal:** Ensure gate failures tell agents to query workflow/operator JSON first, then explain the failed artifact/state condition as diagnostic context. No gate failure should begin with “repair”, “republish”, “update”, “refresh”, or equivalent manual artifact imperatives.

**Context:** The audit found that `public_gate_remediation_for_plan` appends the typed route contract after caller-provided text. Many callers pass first-sentence artifact actions such as “Repair the authoritative harness state…”, “Republish authoritative harness state…”, “Update evidence_refs…”, or “Refresh criterion_results…”. The appended guard is correct, but it appears after the risky imperative.

**Constraints:**

- Preserve the caller-provided gate condition as diagnostic context.
- Preserve `PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT`.
- Do not hand-enumerate every caller if the shared helper can solve the ordering globally.
- Do not remove the `--json` operator query instruction.

**Done when:**

- `public_gate_remediation_for_plan` begins with a public workflow/operator JSON query sentence.
- Caller-provided text appears as diagnostic context, not the primary next step.
- Tests assert representative gate remediation strings start with the public query and include the typed route contract.
- `docs/featureforge/reference/2026-04-01-review-state-reference.md` says `missing_current_closure` should follow the operator-returned typed public route, not directly “record” closure.

**Files:**

- `src/execution/gates.rs`
- `src/execution/authority.rs`
- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs` if doc wording is covered there

**Implementation Steps:**

1. Change `public_gate_remediation_for_plan(plan_rel, action)` to render:
   - sentence 1: `Query workflow/operator JSON for the approved plan by running ...`
   - sentence 2: include `PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT`
   - sentence 3: `Diagnostic context: {action}.`
   - final sentence: `Do not hand-edit or reconstruct proof artifacts.`
2. Ensure `action` is trimmed, punctuation-normalized, and optional-safe if future callers pass empty text.
3. Add or update a Rust contract test that samples remediation output from gate failures and asserts:
   - starts with `Query workflow/operator JSON`
   - contains `recommended_public_command_argv`
   - contains `Diagnostic context:`
   - does not start with manual repair verbs.
4. Update the review-state reference table row for `missing_current_closure`.
5. Run targeted tests:
   - `cargo test --test public_cli_flow_contracts`
   - `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` if docs scanner coverage changes.

**Validation Expectations:**

- Gate remediation text is public-route-first.
- No active docs or tests teach manual proof/artifact repair as the primary action.
- Existing public-output diagnostics still include enough context to debug the gate failure.

## Task 2: Make Already-Current Branch Closure Repair Idempotent

**Spec Coverage:** Already-current branch-closure repair is idempotent on rerun; transition setters dirty state only when values change.

**Goal:** Re-running an already-current branch-closure repair must not persist or append state if overlay fields and repair follow-up fields are already correct.

**Context:** The stale-closure auditor found that already-current branch-closure helpers restore overlay fields, clear repair follow-up, and persist whenever called. The first repair is correct. The second identical call should be a no-op.

**Constraints:**

- Do not skip the first repair when overlay fields are missing or stale.
- Do not skip clearing non-null repair follow-up state.
- Do not weaken branch-closure identity or reviewed-state matching.
- Prefer idempotent transition setters over caller-side checks that can drift.

**Done when:**

- `restore_current_branch_closure_overlay_fields` marks dirty only when at least one target field changes.
- `set_review_state_repair_follow_up(None)` and `set_review_state_repair_follow_up_record(None)` mark dirty only when serialized fields change.
- A rerun/no-op regression test proves the second already-current branch-closure repair does not append a new event or bump authoritative sequence.
- Existing first-run already-current repair coverage still passes.

**Files:**

- `src/execution/transitions.rs`
- `src/execution/commands/common/branch_closure_truth.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/internal_plan_execution.rs` or a narrower transition/unit test if more appropriate

**Implementation Steps:**

1. Add a small internal helper in `AuthoritativeTransitionState` to insert a JSON field only if the existing value differs.
2. Use that helper in `restore_current_branch_closure_overlay_fields`.
3. Use that helper for legacy repair follow-up fields in `set_review_state_repair_follow_up`.
4. Use that helper for `review_state_repair_follow_up_record` and derived target fields in `set_review_state_repair_follow_up_record`.
5. Ensure `persist_if_dirty_with_failpoint_and_command` is not called or is harmlessly no-op when dirty is false.
6. Add a regression that performs the same already-current branch-closure repair twice and asserts:
   - first run reports `already_current` and restores/clears expected state
   - second run reports `already_current` without authoritative sequence/event-log growth.

**Validation Expectations:**

- Idempotency is enforced in transition state, not only one caller.
- No existing branch closure, release readiness, final review, or QA progression tests regress.

## Task 3: Move `repair-review-state` Write Behavior Into The Command Boundary

**Spec Coverage:** `repair-review-state` mutation execution lives under command boundary; `review_state.rs` is analysis/read-support, not public command body owner.

**Goal:** Make `src/execution/commands/repair_review_state.rs` own public `repair-review-state` command execution, mutation guard, write helpers, and persistence orchestration. Keep `src/execution/review_state.rs` focused on shared analysis/read facts.

**Context:** The modularization auditor found that `commands/repair_review_state.rs` is a shim into `review_state.rs`, while `review_state.rs` imports recording/write helpers, route-plan decision constructors, performs mutation guards, persists repair follow-up, and owns the public command body.

**Constraints:**

- Do not duplicate repair/reentry route decisions.
- Do not move broad analysis code into command modules if it is shared by status/router/read-model.
- Use shared types for `RepairPhaseBundle`, repair plan analysis, and route decisions.
- Preserve public output JSON shape.
- Add boundary tests to catch write-capable command execution drifting back into `review_state.rs`.

**Done when:**

- `repair_review_state_command` and public command body live in `src/execution/commands/repair_review_state.rs` or child modules.
- `require_repair_review_state_mutation`, cycle-break clearing, worktree-lease release, and repair-follow-up persistence orchestration live under the command boundary.
- `src/execution/review_state.rs` no longer imports command recording/write helpers solely to execute public mutation.
- Boundary tests fail if `review_state.rs` imports mutation guard/persistence helpers or defines the public command body.
- Existing repair-review-state tests and liveness tests pass.

**Files:**

- `src/execution/commands/repair_review_state.rs`
- `src/execution/commands/repair_review_state/**` if split
- `src/execution/review_state.rs`
- `src/execution/commands/common.rs`
- `src/execution/mod.rs` if module declarations change
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/liveness_model_checker.rs`

**Implementation Steps:**

1. Inventory functions in `review_state.rs` used only by public command execution:
   - mutation guard
   - explicit-target check
   - external-wait output construction if command-only
   - cycle-break clearing
   - lease release
   - repair-follow-up persistence and reroute
   - command body.
2. Move command-only functions into `commands/repair_review_state.rs` or cohesive child modules.
3. Expose minimal analysis/read helpers from `review_state.rs` with restricted visibility where command code still needs them.
4. Update imports and ensure command module owns mutation/persistence dependencies.
5. Add boundary tests:
   - `commands/repair_review_state.rs` imports the mutation/persistence helpers it owns
   - `review_state.rs` does not define `repair_review_state` or `repair_review_state_command`
   - `review_state.rs` does not call `require_public_mutation` or `persist_review_state_repair_follow_up`.
6. Run targeted tests:
   - `cargo test --test workflow_runtime repair_review`
   - `cargo test --test workflow_shell_smoke repair_review`
   - `cargo test --test liveness_model_checker`.

**Validation Expectations:**

- Public behavior unchanged.
- Command/read-analysis boundary is clearer.
- No second implementation of repair route decisions appears.

## Task 4: Reduce Scanner And Source-Shape Churn

**Spec Coverage:** Boundary tests reduce private helper/source-shape pins; public-flow scanner exception taxonomy is narrowed; receipt terminology tests assert behavior/output; production code no longer obfuscates `receipt`.

**Goal:** Replace low-signal source-word/private-helper tests with behavior, ownership, and import-boundary checks. Remove scanner-induced production obfuscation.

**Context:** The signal-to-noise auditor found several medium issues: private helper-name/source-shape pins in `runtime_module_boundaries`, a large public-flow scanner exception registry, and `["rec", "eipt"].concat()` in production code caused by a raw source-word ban.

**Constraints:**

- Do not weaken hidden-helper/public-flow leakage protection.
- Do not remove import-direction or single-owner checks.
- Do not allow receipt/provenance concepts back into route authority.
- Prefer a small explicit marker/quarantine model over many per-function exceptions.

**Done when:**

- The cited `runtime_module_boundaries` tests no longer rely on exact private helper names/snippets unless the symbol is a documented cross-module boundary.
- Public-flow scan exceptions are reduced or backed by explicit marker helpers/modules that make internal-only proof obvious.
- `runtime_authority_contracts` no longer scans selected files with a raw lowercase `receipt` source substring ban.
- `src/execution/status_assembly/public_warnings.rs` can spell the legacy term directly or uses a clearer domain name without obfuscation.
- Tests assert public behavior/output: receipt-shaped warning text is projected/renamed and cannot drive route authority.

**Files:**

- `tests/runtime_module_boundaries.rs`
- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/runtime_authority_contracts.rs`
- `src/execution/status_assembly/public_warnings.rs`
- `src/execution/status_assembly.rs`
- `src/workflow/operator.rs`
- `docs/testing.md`
- `docs/runtime-architecture.md`

**Implementation Steps:**

1. For `completed_task_closure_preemption_predicate_has_single_authoritative_definition`, replace exact private helper-name pins with:
   - owner module import/dependency checks
   - absence of duplicate predicate definitions outside owner
   - behavior or DTO checks that prove preemption consumes the owner.
2. For `current_task_closure_branch_route_predicate_has_single_owner`, replace method/snippet pins with:
   - parsed call-path checks to the owner
   - field-read checks only where those reads are the boundary being guarded.
3. For `execution_template_bindability_policy_lives_in_command_eligibility`, keep the file/function check only if it is documented as the boundary owner API; otherwise assert ownership through public dependency/call paths and DTO behavior.
4. Review `tests/support/public_flow_scan.rs` exceptions. Move repeated synthetic/internal setup cases behind explicit `internal_only_` marker helpers or fixture modules, then simplify the exception registry.
5. Replace `production_routing_authority_uses_artifacts_not_receipts` with targeted assertions that:
   - public route/status/operator output does not expose receipt authority fields
   - receipt-shaped diagnostics are rendered as projection/audit warnings
   - receipt terms do not produce public repair targets or recommended commands.
6. Replace `["rec", "eipt"].concat()` with direct, readable code once the raw source-word ban is gone.
7. Update docs to say boundary tests should not preserve private spelling unless the spelling is a public boundary owner API.

**Validation Expectations:**

- Static checks still catch hidden helper leakage.
- Tests become easier to maintain and less tied to private helper spelling.
- Production code no longer contains scanner obfuscation.

## Task 5: Trim Duplicated Schema And Prompt Prose

**Spec Coverage:** Schema descriptions are terse field semantics; prompt contract tests retain high-value traps while reducing prose grammar checks.

**Goal:** Keep agent-facing route law authoritative and discoverable while reducing duplicate prose across schemas and tests.

**Context:** The signal-to-noise auditor found that schema descriptions duplicate route-law prose already owned by `references/operator-route-authority.md`, and prompt contract tests still act partly like a prose grammar.

**Constraints:**

- Do not remove `display-only` / `not executable` semantics from schema fields.
- Do not remove top-level mandatory route law from route-owning skills.
- Do not remove dangerous command-trap tests for hidden helpers or display-command execution.
- Prompt budget must remain enforced.

**Done when:**

- Schema descriptions for route fields are terse, field-semantic, and non-operational.
- `references/operator-route-authority.md` remains the canonical detailed route execution law.
- Prompt tests focus on mandatory law, canonical reference links, generated route section presence, and dangerous executable traps.
- Broad phrase policing is deleted or narrowed where budget/reference checks already enforce the desired shape.
- Generated schema/docs are fresh.

**Files:**

- `src/execution/status.rs`
- `schemas/plan-execution-status.schema.json`
- `schemas/workflow-operator.schema.json`
- `references/operator-route-authority.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/codex-runtime/skill-doc-budget.test.mjs`
- `scripts/gen-skill-docs.mjs` if generated law wording changes
- generated `skills/**/SKILL.md` if templates change

**Implementation Steps:**

1. Shorten schema descriptions for `recommended_command`, `recommended_public_command_argv`, `recommended_public_command_template`, and `next_public_action` to field semantics.
2. Ensure the canonical reference still contains full binding/materialization law.
3. Regenerate or update schema artifacts as required by the existing build path.
4. Review prompt contract tests around broad forbidden-vocabulary lists and route-command traps:
   - keep hidden-helper and display-command execution traps
   - keep canonical reference presence checks
   - remove exact prose/phrase checks that duplicate the generated section and budget constraints.
5. Run Node contract tests and schema-related Rust tests.

**Validation Expectations:**

- Agents still see unambiguous typed route authority.
- Schema prose no longer carries full operational law.
- Prompt tests remain high signal.

## Task 6: Final Verification, Review, And Re-Audit

**Spec Coverage:** Full validation and independent rereview pass.

**Goal:** Prove the remediation is complete before the next audit loop.

**Context:** The user requires strict clippy and full nextest before dispatching each clean-context review, with performance guardrails.

**Constraints:**

- Always check no cargo/nextest process is running before a full Rust test cycle.
- If full nextest exceeds 4-5 minutes, clean/rerun and remediate repeatable regression. If it exceeds 10 minutes, stop immediately and remediate.
- Clean-context reviewers must not use FeatureForge runtime/workflow commands, FeatureForge/project skills, or subagents.

**Done when:**

- `node scripts/gen-skill-docs.mjs --check` passes.
- `node scripts/gen-agent-docs.mjs --check` passes.
- `node --test tests/codex-runtime/*.test.mjs` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `/usr/bin/time -p cargo nextest run --all-targets --all-features --no-fail-fast` passes and remains under performance threshold or repeatable regression is remediated.
- `cargo test --test liveness_model_checker` passes.
- A clean-context whole-plan reviewer finds no actionable issues, or all issues are remediated and re-reviewed clean.
- A new audit loop runs after `cargo clean` with subagents A-H plus the signal-to-noise auditor.

**Files:**

- All files changed by Tasks 1-5.

**Implementation Steps:**

1. Run focused tests while developing each task.
2. Before each review, run:
   - `pgrep -fl 'cargo nextest|cargo-nextest|nextest run|cargo test|cargo clippy|node --test tests/codex-runtime'`
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `/usr/bin/time -p cargo nextest run --all-targets --all-features --no-fail-fast`
   - `cargo test --test liveness_model_checker`
3. Dispatch a clean-context reviewer against this exact plan and current diff.
4. Remediate all findings and repeat full validation/rereview until clean.
5. Start the next audit loop with `cargo clean`, subagents A-H, and signal-to-noise auditor.

**Validation Expectations:**

- Verification evidence is current after the last code change.
- No known actionable audit issue remains before the next audit starts.
