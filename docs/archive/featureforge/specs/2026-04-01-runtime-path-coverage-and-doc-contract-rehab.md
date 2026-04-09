# Runtime-Path Coverage And Doc-Contract Rehab

**Workflow State:** Implementation Target  
**Spec Revision:** 4  
**Last Reviewed By:** clean-context review loop
**Implementation Target:** Current

## Problem Statement

The session-history audit showed that false-confidence tests are the single biggest churn source:

- self-comparison suites
- prose-regex contract tests
- meta-tests that prove files or test names exist
- fake packaged-entrypoint or wrapper coverage

The supersession-aware model raises the bar further. If the suite does not prove current/superseded/stale closure behavior end to end, the new architecture will still drift into theater.

## Desired Outcome

The test suite must prove:

- current reviewed closure recording
- supersession by later reviewed work
- stale-unreviewed transitions after later edits
- release-readiness and final-review milestones bound to current reviewed branch state
- real operator flows through public runtime commands

## Decision

Selected approach: move critical workflow coverage onto CLI-first behavioral tests and demote synthetic receipt/artifact tests to narrow compatibility scope.

## Dependency

This spec depends on:

- `2026-04-01-supersession-aware-review-identity.md`
- downstream command and phase specs

## Requirement Index

- [REQ-001][verification] Core workflow guarantees for task closure, supersession, stale-unreviewed repair, release-readiness, final review, and finish readiness must be covered by public-command end-to-end tests.
- [REQ-002][verification] Self-comparison, prose-regex, and meta-existence tests must not remain the primary oracle for workflow behavior.
- [REQ-003][verification] Pure domain and supersession policy must have focused unit coverage that does not require filesystem, git, markdown, or CLI setup.
- [REQ-004][verification] Store and projection logic must have focused integration coverage over append-only records and derived read models.
- [REQ-005][verification] Direct receipt/artifact-writing helpers may remain only for narrow derived-artifact compatibility tests and must not remain the primary evidence for supported operator flows.
- [REQ-006][verification] High-value workflow tests must fail closed on stale-binary use.
- [REQ-007][verification] Where the product claims wrapper, browser, or packaged-entrypoint support, at least one real-path test must exercise that path.
- [REQ-008][verification] Test names and directories must clearly distinguish public-path behavior tests from domain/policy tests and from derived-artifact compatibility tests.
- [REQ-009][verification] Workflow routing tests must consume the public review-state contract rather than constructing oversized internal runtime state where a smaller fixture would prove the behavior.
- [REQ-010][verification] The preferred aggregate command paths for task closure, review-state repair, and late-stage progression must have explicit CLI end-to-end coverage and must be treated as primary operator-path oracles.

## Scope

In scope:

- CLI-first workflow tests
- supersession and stale-unreviewed behavior tests
- doc-contract cleanup
- oracle cleanup for self-comparison and stale-binary cases
- real-path platform smoke where the product claims support

Out of scope:

- removing every narrow low-level parser/compatibility test
- broad refactoring beyond what behavioral coverage requires

## Selected Approach

Use five testing layers:

1. pure domain and supersession-policy tests
2. store and projection tests
3. service-level integration tests for closure and milestone recording
4. public CLI end-to-end tests for reviewed-closure behavior
5. narrow compatibility tests for derived artifacts only where still needed

The public CLI layer must treat these as first-class behavioral oracles:

- `close-current-task`
- `repair-review-state`
- `record-branch-closure`
- `advance-late-stage`
- `record-qa`

Recommended canonical homes:

- pure policy tests under `tests/review_state_domain/`
- store/projection tests under `tests/review_state_projection/`
- service-level integration tests under `tests/review_state_services/`
- public CLI behavior tests under `tests/review_state_e2e/`
- derived-artifact compatibility tests under `tests/review_state_compat/`

## Acceptance Criteria

1. Task closure is proven by public-command tests against current reviewed closure state.
2. Later reviewed work superseding earlier closure state is proven by public-command tests.
3. Post-review edits making current closure stale-unreviewed are proven by public-command tests.
4. Release-readiness and final-review milestone recording are proven by public-command tests against current reviewed branch state.
5. Branch-closure recording and QA recording are proven by public-command tests against current reviewed branch state.
6. The suite no longer relies on self-comparison or prose-regex checks as the main evidence for workflow behavior.
7. Pure policy, store/projection, service, and CLI layers each have clear ownership in the test suite.
8. Real-path tests exist for supported packaged-entrypoint/wrapper/browser paths where claimed.
9. Aggregate-command happy paths are proven directly instead of being assumed from primitive tests.

## Test Strategy

- add pure unit tests for closure status transitions and supersession policy
- add focused integration tests for store/projection derivation of effective current closure state
- add service-level tests for review-dispatch, task closure, release-readiness, final review, and reconcile orchestration
- add CLI-only happy-path tests for task closure recording
- add CLI-only supersession tests where later reviewed work replaces earlier closure state
- add CLI-only stale-unreviewed tests after post-review edits
- add CLI-only tests for `record-branch-closure`, including idempotent re-run and blocked-path behavior
- add CLI-only tests for `record-qa --result pass|fail`
- add CLI-only release-readiness and final-review milestone tests
- add CLI-only reconcile tests for review-state explain/reconcile flows
- add CLI-only aggregate-command tests for `close-current-task`, `repair-review-state`, and `advance-late-stage`
- add workflow-routing tests against public review-state fixtures
- add metadata-policy tests for `Late-Stage Surface` normalization/matching and `QA Requirement` normalization/fail-closed routing
- add workflow/query tests for `follow_up_override` precedence and clearing behavior
- add status/query tests for `workspace_state_id` normalization over repo-tracked content only
- add freshness/build guards so workflow tests cannot silently run against stale binaries
- replace phrase-lock workflow oracles with behavior-oriented assertions
- keep derived-artifact tests only for compatibility where required

## Risks

- keeping the old theater-heavy oracle mix will undermine the new model immediately
- deleting all low-level compatibility tests at once would remove useful precision
