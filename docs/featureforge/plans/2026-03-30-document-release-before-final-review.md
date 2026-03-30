# Document Release Before Final Review Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use the execution skill recommended by `featureforge plan execution recommend --plan <approved-plan-path>` after engineering approval; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-03-30-document-release-before-final-review-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

**Goal:** Make late-stage workflow routing enforce document-release before terminal final review, while preserving strict freshness and fail-closed provenance boundaries.

**Architecture:** Introduce a runtime-owned canonical late-stage precedence contract and route both authoritative harness-phase producers and operator outputs through it. Align skills/docs to a shared canonical precedence reference so guidance cannot drift from runtime truth, then pin behavior with matrix-style runtime and contract tests.

**Tech Stack:** Rust (`src/workflow`, `src/execution`), Markdown skill templates/docs, generated skill docs, Rust integration tests, Node skill-doc contract tests.

---

## Change Surface
- Runtime precedence and phase-selection logic:
  - `src/workflow/operator.rs`
  - `src/execution/state.rs`
  - `src/execution/harness.rs`
- Workflow-facing late-stage skill guidance:
  - `skills/finishing-a-development-branch/SKILL.md.tmpl`
  - `skills/document-release/SKILL.md.tmpl`
  - `skills/requesting-code-review/SKILL.md.tmpl`
  - `skills/using-featureforge/SKILL.md.tmpl`
  - generated `skills/*/SKILL.md` outputs
- Public workflow docs:
  - `README.md`
  - `docs/README.codex.md`
  - `docs/README.copilot.md`
- Runtime-derived precedence reference artifact for skill/doc reuse:
  - `review/late-stage-precedence-reference.md` (generated from runtime-owned canonical precedence rows)
- Regression/contract tests:
  - `tests/workflow_runtime.rs`
  - `tests/workflow_runtime_final_review.rs`
  - `tests/execution_harness_state.rs`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `tests/codex-runtime/eval-observability.test.mjs`
  - `tests/runtime_instruction_contracts.rs`

## Preconditions
- Approved spec headers remain:
  - `**Workflow State:** CEO Approved`
  - `**Spec Revision:** 1`
  - `**Last Reviewed By:** plan-ceo-review`
- Approved spec `## Requirement Index` remains parseable with REQ-001 through REQ-023.
- Work continues in a writable branch/worktree that passes repo-safety checks for plan and implementation artifacts.

## Existing Capabilities / Built-ins to Reuse
- Existing workflow phase and handoff output surfaces in `src/workflow/operator.rs`.
- Existing harness phase enum/state mapping in `src/execution/harness.rs` and `src/execution/state.rs`.
- Existing structured observability payload seam in `src/execution/observability.rs` plus `tests/codex-runtime/eval-observability.test.mjs`.
- Existing late-stage fixtures already asserting `document_release_pending`, `final_review_pending`, `qa_pending`, and `ready_for_branch_completion` in `tests/workflow_runtime.rs`.
- Existing skill-doc generation pipeline: `node scripts/gen-skill-docs.mjs`.
- Existing skill-doc contract coverage harness in `tests/codex-runtime/skill-doc-contracts.test.mjs`.
- Existing runtime instruction contract checks in `tests/runtime_instruction_contracts.rs`.

## Known Footguns / Constraints
- Do not relax final-review freshness or finish-gate strictness while reordering precedence.
- Do not make checkpoint/ad-hoc `requesting-code-review` invocations universally depend on `document-release`.
- Keep release-artifact provenance authoritative; decoy/malformed artifacts must fail closed.
- Avoid duplicated precedence logic between runtime code and skill/docs; use one canonical contract source.
- Any `.md.tmpl` edits require regeneration of checked-in `SKILL.md` outputs.

## Optional Project Memory Notes
- `docs/project_notes/decisions.md` confirms supportive-vs-authoritative boundaries (`PM-001`, `PM-002`), reinforcing that precedence truth must remain runtime-owned.
- `docs/project_notes/key_facts.md` confirms skill-doc regeneration via `node scripts/gen-skill-docs.mjs`.

## Requirement Coverage Matrix
- REQ-001 -> Task 1, Task 3
- REQ-002 -> Task 1, Task 2
- REQ-003 -> Task 6
- REQ-004 -> Task 3, Task 6
- REQ-005 -> Task 3
- REQ-006 -> Task 1, Task 5
- REQ-007 -> Task 6
- REQ-008 -> Task 6, Task 7
- REQ-009 -> Task 5, Task 8
- REQ-010 -> Task 6, Task 7, Task 8
- REQ-011 -> Task 4, Task 5
- REQ-012 -> Task 5, Task 8
- REQ-013 -> Task 3, Task 5
- REQ-014 -> Task 1, Task 2, Task 6
- REQ-015 -> Task 3, Task 5
- REQ-016 -> Task 1, Task 6
- REQ-017 -> Task 7
- REQ-018 -> Task 2, Task 5
- REQ-019 -> Task 2, Task 5
- REQ-020 -> Task 7
- REQ-021 -> Task 1, Task 3, Task 4
- REQ-022 -> Task 5, Task 8
- REQ-023 -> Task 3, Task 5

## Execution Strategy
- Execute Tasks 1, 2, and 3 serially. They define and enforce canonical precedence and terminal-final-review guard semantics across runtime gate surfaces.
- After Task 3, create two worktrees and run Tasks 4 and 6 in parallel:
  - Task 4 owns runtime observability and reason-family diagnostics.
  - Task 6 owns skill/template alignment plus canonical precedence reference grounding.
- Execute Task 5 serially after Task 4. It finalizes runtime matrix and fail-closed regression coverage on stabilized runtime behavior.
- Execute Task 7 serially after Task 6. It reconciles public docs and skill-doc contract assertions against the canonical precedence contract.
- Execute Task 8 serially after Tasks 5 and 7. It is the final verification and evidence seam.

## Dependency Diagram
```text
Task 1 -> Task 2
Task 2 -> Task 3
Task 3 -> Task 4
Task 3 -> Task 6
Task 4 -> Task 5
Task 6 -> Task 7
Task 5 -> Task 8
Task 7 -> Task 8
```

## Task 1: Introduce Runtime-Owned Canonical Late-Stage Precedence Contract

**Spec Coverage:** REQ-001, REQ-002, REQ-006, REQ-014, REQ-016, REQ-021
**Task Outcome:** Runtime has one canonical late-stage precedence contract (`artifact/failure -> phase -> next_action -> recommended_skill -> reason-family`) that operator routing can consume deterministically.
**Plan Constraints:**
- Canonical contract must be code-owned, not prose-owned.
- Keep fallback behavior fail closed when contract evaluation fails.
**Open Questions:** none

**Files:**
- Modify: `src/workflow/operator.rs`
- Create: `src/workflow/late_stage_precedence.rs`
- Test: `tests/workflow_runtime.rs`

- [ ] **Step 1: Add failing test for mixed stale release/review state precedence**
Run: `cargo test --test workflow_runtime -- canonical_workflow_phase_routes_review_resolved_to_document_release_pending --exact`
Expected: FAIL if old precedence still prefers final review.

- [ ] **Step 2: Implement canonical precedence row type and resolver helper**
Define explicit mapping entries for release/review/qa readiness outcomes, including reason-family text binding.

- [ ] **Step 3: Route operator phase/action/skill/reason through resolver**
Replace ad-hoc branching where needed so outputs are contract-aligned.

- [ ] **Step 4: Add fail-closed fallback behavior for malformed/unknown precedence inputs**
Ensure unknown/malformed states cannot route optimistically to review-ready.

- [ ] **Step 5: Run targeted runtime routing tests**
Run: `cargo test --test workflow_runtime -- workflow_phase_routes_ --nocapture`
Expected: PASS for existing and new precedence scenarios.

- [ ] **Step 6: Commit Task 1**
Run:
```bash
git add src/workflow/operator.rs src/workflow/late_stage_precedence.rs tests/workflow_runtime.rs
git commit -m "feat: add canonical late-stage precedence contract"
```

## Task 2: Bind Authoritative Harness Phase and Operator Routing to the Same Contract

**Spec Coverage:** REQ-002, REQ-014, REQ-018, REQ-019
**Task Outcome:** `harness_phase` and operator fallback routing cannot diverge on stale-artifact precedence for the same execution/gate evidence.
**Plan Constraints:**
- Preserve existing harness-phase enum wire format.
- Divergence detection must fail closed with deterministic diagnostics.
**Open Questions:** none

**Files:**
- Modify: `src/execution/state.rs`
- Modify: `src/execution/harness.rs`
- Modify: `src/workflow/operator.rs`
- Test: `tests/execution_harness_state.rs`
- Test: `tests/workflow_runtime.rs`

- [ ] **Step 1: Add failing parity test for authoritative harness-phase vs operator phase outputs**
Run: `cargo test --test workflow_runtime -- canonical_workflow_operator_surfaces_ --nocapture`
Expected: FAIL if surfaces can diverge on precedence.

- [ ] **Step 2: Wire authoritative phase emission through canonical precedence helper**
Ensure state/harness producers consume the same mapping as operator routing.

- [ ] **Step 3: Add explicit divergence diagnostic path**
Return reason-coded fail-closed state when parity assumptions are violated.

- [ ] **Step 4: Run targeted parity suites**
Run:
```bash
cargo test --test execution_harness_state -- --nocapture
cargo test --test workflow_runtime -- canonical_workflow_phase_routes_ --nocapture
```
Expected: PASS with parity preserved.

- [ ] **Step 5: Commit Task 2**
Run:
```bash
git add src/execution/state.rs src/execution/harness.rs src/workflow/operator.rs tests/execution_harness_state.rs tests/workflow_runtime.rs
git commit -m "feat: enforce harness/operator precedence parity"
```

## Task 3: Implement Terminal Final-Review Guard and Release-Provenance Fail-Closed Law

**Spec Coverage:** REQ-001, REQ-004, REQ-005, REQ-013, REQ-015, REQ-021, REQ-023
**Task Outcome:** Workflow-routed terminal final review fails closed to document-release when release readiness is stale/missing or non-authoritative, while preserving intentional non-terminal review checkpoints.
**Plan Constraints:**
- Guard applies only to terminal workflow-routed final-review mode.
- Do not block ad-hoc/early checkpoint review workflows.
**Open Questions:** none

**Files:**
- Modify: `src/workflow/operator.rs`
- Modify: `src/execution/state.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/workflow_runtime_final_review.rs`

- [ ] **Step 1: Add failing tests for terminal-guard behavior and checkpoint allowance**
Run:
```bash
cargo test --test workflow_runtime_final_review -- --nocapture
cargo test --test workflow_runtime -- canonical_workflow_phase_routes_review_resolved_to_document_release_pending --exact
```
Expected: FAIL until terminal guard semantics are implemented.

- [ ] **Step 2: Implement terminal final-review mode check for release readiness**
Require authoritative release-readiness freshness before terminal final-review eligibility.

- [ ] **Step 3: Enforce release-artifact provenance validation in the same decision path**
Reject decoy/malformed/non-authoritative artifacts with named reason codes.

- [ ] **Step 4: Preserve non-terminal review path behavior**
Ensure intentional checkpoint review invocations remain valid.

- [ ] **Step 5: Run focused final-review + release-precedence tests**
Run:
```bash
cargo test --test workflow_runtime_final_review -- --nocapture
cargo test --test workflow_runtime -- canonical_workflow_phase_routes_ --nocapture
```
Expected: PASS, including new guard and provenance cases.

- [ ] **Step 6: Commit Task 3**
Run:
```bash
git add src/workflow/operator.rs src/execution/state.rs tests/workflow_runtime.rs tests/workflow_runtime_final_review.rs
git commit -m "feat: gate terminal final review on release readiness"
```

## Task 4: Add Precedence Observability and Reason-Family Diagnostics (Parallel Lane A)

**Spec Coverage:** REQ-011, REQ-021
**Task Outcome:** Runtime surfaces deterministic observability signals and reason-family diagnostics for precedence outcomes and stale-review invalidations.
**Plan Constraints:**
- Keep observability additive; do not alter gate semantics.
- Diagnostic vocabulary must match spec failure/rescue registry.
**Open Questions:** none

**Files:**
- Modify: `src/workflow/operator.rs`
- Modify: `src/execution/state.rs`
- Modify: `src/execution/observability.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/codex-runtime/eval-observability.test.mjs`

- [ ] **Step 1: Add failing diagnostics assertions for precedence reason-family coverage**
Run: `cargo test --test workflow_runtime -- canonical_workflow_phase_routes_authoritative_release_provenance_invalid_to_document_release_pending --exact`
Expected: FAIL if reason-family telemetry/diagnostics are absent.

- [ ] **Step 2: Emit precedence observability counters/diagnostics**
Add deterministic event/reason-code signals for document-release-first routing, post-review freshness invalidation, and authoritative provenance failures.

- [ ] **Step 3: Validate reason-family text parity across phase/handoff surfaces**
Ensure operator outputs remain aligned.

- [ ] **Step 4: Run targeted diagnostics tests**
Run:
```bash
cargo test --test workflow_runtime -- --nocapture
node --test tests/codex-runtime/eval-observability.test.mjs
```
Expected: PASS for new diagnostics assertions.

- [ ] **Step 5: Commit Task 4**
Run:
```bash
git add src/workflow/operator.rs src/execution/state.rs tests/workflow_runtime.rs
git add src/execution/observability.rs tests/codex-runtime/eval-observability.test.mjs
git commit -m "feat: add late-stage precedence observability diagnostics"
```

## Task 5: Build Runtime Matrix and Fail-Closed Regression Coverage

**Spec Coverage:** REQ-006, REQ-009, REQ-012, REQ-015, REQ-018, REQ-019, REQ-022, REQ-023
**Task Outcome:** Runtime tests pin phase/action/skill/reason parity across mixed stale states, parity between authoritative and fallback routing, and fail-closed error classes.
**Plan Constraints:**
- Matrix tests must assert all four user-visible outputs together.
- Keep fixtures deterministic and independent of network/external state.
**Open Questions:** none

**Files:**
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_runtime_final_review.rs`
- Modify: `tests/execution_harness_state.rs`

- [ ] **Step 1: Add matrix test table for mixed release/review/qa stale combinations**
Run: `cargo test --test workflow_runtime -- canonical_workflow_phase_routes_ --nocapture`
Expected: FAIL until matrix parity is fully implemented.

- [ ] **Step 2: Add fail-closed tests for malformed precedence inputs**
Cover named failure class/reason-code behavior from spec registry.

- [ ] **Step 3: Add parity regression tests for harness-phase vs operator fallback**
Assert identical precedence outcomes for the same gate evidence.

- [ ] **Step 4: Add terminal-vs-checkpoint review mode coverage**
Prove terminal mode is guarded and checkpoint mode stays available.

- [ ] **Step 5: Run runtime test suite slice**
Run:
```bash
cargo test --test workflow_runtime -- --nocapture
cargo test --test workflow_runtime_final_review -- --nocapture
cargo test --test execution_harness_state -- --nocapture
```
Expected: PASS.

- [ ] **Step 6: Commit Task 5**
Run:
```bash
git add tests/workflow_runtime.rs tests/workflow_runtime_final_review.rs tests/execution_harness_state.rs
git commit -m "test: add late-stage precedence matrix regressions"
```

## Task 6: Align Skill Templates and Generated Skill Docs to Canonical Precedence (Parallel Lane B)

**Spec Coverage:** REQ-003, REQ-007, REQ-008, REQ-010, REQ-014, REQ-016
**Task Outcome:** Late-stage skills describe the same precedence contract as runtime and reference a canonical precedence artifact rather than carrying divergent logic.
**Plan Constraints:**
- Preserve checkpoint/ad-hoc review allowance in `requesting-code-review`.
- Edit `.tmpl` sources first; regenerate `SKILL.md` outputs.
**Open Questions:** none

**Files:**
- Modify: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- Modify: `skills/document-release/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Create: `review/late-stage-precedence-reference.md`
- Modify: `skills/finishing-a-development-branch/SKILL.md`
- Modify: `skills/document-release/SKILL.md`
- Modify: `skills/requesting-code-review/SKILL.md`
- Modify: `skills/using-featureforge/SKILL.md`

- [ ] **Step 1: Update `.tmpl` guidance for document-release-before-terminal-review sequencing**
Keep language explicit about terminal guard scope.

- [ ] **Step 2: Add canonical precedence reference artifact**
Generate or refresh table rows in one shared reference consumed by skills/docs, deriving rows directly from the runtime-owned canonical precedence contract.

- [ ] **Step 3: Regenerate skill docs**
Run: `node scripts/gen-skill-docs.mjs`
Expected: SKILL.md outputs update with template changes.

- [ ] **Step 4: Run skill-doc generation tests**
Run: `node --test tests/codex-runtime/skill-doc-generation.test.mjs`
Expected: PASS.

- [ ] **Step 5: Commit Task 6**
Run:
```bash
git add skills/finishing-a-development-branch/SKILL.md.tmpl skills/document-release/SKILL.md.tmpl skills/requesting-code-review/SKILL.md.tmpl skills/using-featureforge/SKILL.md.tmpl review/late-stage-precedence-reference.md skills/finishing-a-development-branch/SKILL.md skills/document-release/SKILL.md skills/requesting-code-review/SKILL.md skills/using-featureforge/SKILL.md
git commit -m "docs: align late-stage skills to canonical precedence"
```

## Task 7: Align Public Docs and Enforce Skill/Doc Divergence Contract Tests

**Spec Coverage:** REQ-008, REQ-010, REQ-017, REQ-020
**Task Outcome:** Public docs reflect the same normative late-stage order and contract tests fail on precedence drift from runtime-owned truth.
**Plan Constraints:**
- Keep public docs concise; do not create a second policy surface.
- Contract tests should compare against canonical reference/phrasing, not brittle full-document snapshots.
**Open Questions:** none

**Files:**
- Modify: `README.md`
- Modify: `docs/README.codex.md`
- Modify: `docs/README.copilot.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
- Modify: `tests/runtime_instruction_contracts.rs`

- [ ] **Step 1: Update public workflow docs to remove review-first wording**
Ensure normative sequence reflects document-release before terminal final review.

- [ ] **Step 2: Add contract assertions for precedence grounding**
Fail when skill/public precedence wording diverges from canonical contract rows and from the runtime-derived precedence reference artifact.

- [ ] **Step 3: Run skill-doc contract suite**
Run:
```bash
node --test tests/codex-runtime/skill-doc-contracts.test.mjs
cargo test --test runtime_instruction_contracts -- --nocapture
```
Expected: PASS.

- [ ] **Step 4: Commit Task 7**
Run:
```bash
git add README.md docs/README.codex.md docs/README.copilot.md tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/runtime_instruction_contracts.rs
git commit -m "test: enforce canonical late-stage precedence wording parity"
```

## Task 8: Final Integration, Verification, and Regression Gate

**Spec Coverage:** REQ-009, REQ-010, REQ-012, REQ-022
**Task Outcome:** Runtime behavior, skills/docs, and tests are integrated with no precedence drift; verification evidence proves reordered late-stage flow without freshness relaxation.
**Plan Constraints:**
- Run targeted suites first, then broad verification.
- Do not claim completion without command-backed evidence.
**Open Questions:** none

**Files:**
- Create: `docs/featureforge/execution-evidence/2026-03-30-document-release-before-final-review-r1-evidence.md`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/workflow_runtime_final_review.rs`
- Test: `tests/execution_harness_state.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Run Rust lint and targeted runtime suites**
Run:
```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo test --test workflow_runtime -- --nocapture
cargo test --test workflow_runtime_final_review -- --nocapture
cargo test --test execution_harness_state -- --nocapture
```
Expected: PASS.

- [ ] **Step 2: Run skill-doc generation and contract suites**
Run:
```bash
node scripts/gen-skill-docs.mjs
node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs
node --test tests/codex-runtime/skill-doc-contracts.test.mjs
node --test tests/codex-runtime/skill-doc-generation.test.mjs
```
Expected: PASS.

- [ ] **Step 3: Re-run plan contract lint for final plan coherence**
Run:
```bash
featureforge plan contract lint --spec docs/featureforge/specs/2026-03-30-document-release-before-final-review-design.md --plan docs/featureforge/plans/2026-03-30-document-release-before-final-review.md
```
Expected: PASS.

- [ ] **Step 4: Commit integration verification updates**
Run:
```bash
git add -A
git commit -m "chore: integrate document-release-before-final-review contract updates"
```

## Validation Strategy
- Runtime routing verification:
  - release stale + review stale -> `document_release_pending`
  - release fresh + review stale -> `final_review_pending`
  - release/review fresh + qa stale -> `qa_pending`
  - all fresh -> `ready_for_branch_completion`
- Guard verification:
  - terminal final review blocked when release readiness stale/missing
  - non-terminal checkpoint review remains valid
- Provenance verification:
  - decoy/non-authoritative release artifacts fail closed
- Parity verification:
  - authoritative harness-phase and operator outputs match for the same gate evidence fixture
- Docs/contracts verification:
  - skill/public wording remains grounded in canonical precedence reference

## Evidence Expectations
- Preserve command outputs for all verification steps in execution evidence artifacts.
- Capture reason-code payloads for new fail-closed cases.
- Include before/after routing fixture evidence for stale-review-loop elimination.

## Documentation Update Expectations
- Keep canonical precedence reference and skill/public docs synchronized with runtime contract.
- Update release-facing docs only where wording previously implied review-first normative flow.

## Rollout Plan
- Land runtime precedence contract and guard semantics first.
- Land skill/template and public-doc alignment with regenerated outputs.
- Ship with matrix regression tests enabled to block drift.

## Rollback Plan
- Revert precedence-contract commits as one slice if routing regressions occur.
- Preserve existing freshness/provenance fail-closed behavior during rollback.
- Re-run runtime and skill-doc contract suites after rollback to confirm stability.

## Risks and Mitigations
- Risk: runtime and skill/docs drift over time.
  - Mitigation: canonical precedence source + contract tests for wording parity.
- Risk: over-scoping guard to all reviews.
  - Mitigation: explicit terminal-final-review-only guard tests.
- Risk: harness/operator parity regressions in edge states.
  - Mitigation: dedicated parity matrix tests and fail-closed divergence diagnostics.

## Engineering Review Summary

**Review Status:** clear
**Reviewed At:** 2026-03-30T12:42:06Z
**Review Mode:** small_change
**Reviewed Plan Revision:** 1
**Critical Gaps:** 0
**Browser QA Required:** no
**Test Plan Artifact:** `~/.featureforge/projects/dmulcahey-featureforge/davidmulcahey-current-test-plan-20260330-123721.md`
**Outside Voice:** fresh-context-subagent
