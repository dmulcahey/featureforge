# FeatureForge Runtime Backlog

**Date:** 2026-04-01  
**Priority basis:** stabilize current reviewed closure truth first, then make operators able to record it, then make repair and routing understandable, then harden skills/tests, then refactor

## Priority Queue

### P0

#### U1. Supersession-aware review identity core model

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-supersession-aware-review-identity.md`
- **Reason:** every other fix is downstream of this decision; without a current/superseded/stale closure model, the runtime will keep preserving obsolete proof as if it were still authoritative
- **Depends on:** none
- **Blocks:** `U2` through `U12`

#### U2. Task closure recording on current reviewed closures

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-execution-task-closure-command-surface.md`
- **Reason:** removes the most immediate execution dead end and redefines task closure around current reviewed closure state instead of stale receipt permanence
- **Depends on:** `U1`
- **Blocks:** `U7`, `U10`, `U11`

#### U3. Branch-closure recording on current reviewed branch state

- **Spec:** `docs/archive/featureforge/specs/2026-04-02-branch-closure-recording-and-binding.md`
- **Reason:** late-stage progression cannot be implemented cleanly while the branch-closure producer contract remains implicit
- **Depends on:** `U1`
- **Blocks:** `U4`, `U5`, `U6`, `U7`, `U10`, `U11`

### P1

#### U8. Gate diagnostics and runtime semantics

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-gate-diagnostics-and-runtime-semantics.md`
- **Reason:** makes current reviewed state, current branch closure, dispatch readiness, superseded state, and stale-unreviewed state explicit and actionable
- **Depends on:** `U1` through `U3`
- **Blocks:** `U4`, `U5`, `U6`, `U7`, `U9`, `U10`

#### U9. Workflow public phase and routing contract

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-workflow-public-phase-contract.md`
- **Reason:** workflow outputs need to expose exact next actions and exact next command families without phase/command ambiguity
- **Depends on:** `U8`
- **Blocks:** `U4`, `U5`, `U6`, `U7`, `U10`

#### U7. Supersession-aware reconcile and stale-closure repair

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-execution-repair-and-state-reconcile.md`
- **Reason:** turns repair into append-only supersession and reconcile instead of proof rewriting
- **Depends on:** `U1`, `U2`, `U3`, `U8`, `U9`
- **Blocks:** `U4`, `U5`, `U6`, `U10`, `U11`

#### U4. Release-readiness recording on current reviewed branch closures

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-release-readiness-recording-and-binding.md`
- **Reason:** removes a major manual late-stage loop and binds release-readiness to current reviewed branch state instead of hand-authored markdown
- **Depends on:** `U1`, `U3`, `U7`, `U8`, `U9`
- **Blocks:** `U5`, `U6`, `U10`, `U11`

#### U5. Final-review recording on current reviewed branch closures

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-final-review-recording-and-binding.md`
- **Reason:** moves final review onto the same current reviewed branch closure model and stops treating paired markdown artifacts as the primary truth surface
- **Depends on:** `U1`, `U2`, `U3`, `U4`, `U7`, `U8`, `U9`
- **Blocks:** `U6`, `U10`, `U11`

#### U6. QA recording on current reviewed branch closures

- **Spec:** `docs/archive/featureforge/specs/2026-04-02-qa-recording-and-routing.md`
- **Reason:** `qa_pending` is part of the active workflow contract and cannot remain a prose placeholder or implicit side effect
- **Depends on:** `U1`, `U3`, `U4`, `U5`, `U7`, `U8`, `U9`
- **Blocks:** `U10`, `U11`

### P2

#### U10. Skill and reference hardening

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-execution-review-skill-contract-hardening.md`
- **Reason:** agents must be taught current versus superseded versus stale closure semantics and the exact runtime-owned commands that manage them
- **Depends on:** `U1` through `U9`
- **Blocks:** `U11`

#### U11. Runtime-path coverage and doc-contract rehab

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-runtime-path-coverage-and-doc-contract-rehab.md`
- **Reason:** the new model needs behavioral proofs for supersession, stale-unreviewed state, branch closure, QA, and current-closure gating, not more phrase-lock theater
- **Depends on:** `U1` through `U10`
- **Blocks:** `U12`

### P3

#### U12. Runtime boundary separation

- **Spec:** `docs/archive/featureforge/specs/2026-04-01-execution-runtime-boundary-separation.md`
- **Reason:** once behavior is stable, ownership must be reorganized around closure records, supersession, milestones, repair, routing, and rendering so the churn does not come back
- **Depends on:** `U11`
- **Blocks:** none

## Recommended Execution Order

1. `U1`
2. `U2`
3. `U3`
4. `U8`
5. `U9`
6. `U7`
7. `U4`
8. `U5`
9. `U6`
10. `U10`
11. `U11`
12. `U12`

## Readiness Notes

- `U1` is the real pivot. Without it, the rest of the backlog just makes the old fingerprint-heavy model less painful rather than structurally better.
- `U2` should no longer be framed as “write the missing receipts.” It should frame task closure as recording the current reviewed task closure and treating receipts as optional derivatives.
- `U2` should replace predicate-style `gate-review-dispatch` naming with a clearly mutating review-dispatch recording surface and a compatibility alias boundary.
- `U3` is no longer implicit. The branch-closure producer path needs its own slice because release-readiness, final review, QA, and workflow routing all depend on it.
- `U4`, `U5`, and `U6` should all be implemented as branch-closure milestone recording, not more hand-authored markdown with CLI wrappers.
- `U5` must land after `U4` because final review is only valid once a current release-readiness result `ready` already exists for the same still-current branch closure.
- `U6` must land after both `U4` and `U5` because QA is only valid once a current release-readiness result `ready` and a current final-review result `pass` already exist for the same still-current branch closure.
- `U7` must land before `U4`, `U5`, and `U6` because stale-late-stage reroute and recovery are already normative parts of those milestone contracts.
- `U4` and `U5` should converge on `advance-late-stage` as the preferred agent-facing terminal-stage surface over stage-specific primitive orchestration.
- `U6` should keep QA explicitly outside `advance-late-stage`; `record-qa` is its own public recording surface.
- `U7` should remove in-place proof rewriting as a default path. Older reviewed state must become `superseded` or `stale-unreviewed`, not silently refreshed into fake currency.
- `U8` and `U9` must freeze a deterministic singular `recommended_command` contract and stop letting phase tables encode multi-command bundles.
- `U9` must publish canonical stale late-stage reentry mappings for stale release-readiness and stale QA after repo-tracked edits.
- `U10` should not ship before the command/status model stabilizes or it will immediately drift.
- `U11` must cover the scenarios the session-history audit showed were missing: supersession, stale-unreviewed changes, aggregate-command orchestration, branch-closure recording, QA recording, real-path packaged entrypoints, and layer-specific policy/query/service tests.
- `U12` must stay last. Refactor before behavioral coverage would be irresponsible.
- `U12` should enforce real component seams: domain, resolvers, append-only stores, projections, policy, recording/reconcile services, query interfaces, workflow adapters, and renderer compatibility boundaries.
- `U1` is also the right umbrella superspec. The delivery shape should be one architectural superspec plus phased implementation plans, not one undifferentiated super plan that tries to cross model, commands, skills, tests, and refactor in one pass.
- If a single master plan is desired, it should be a phased program plan that references the slice specs and phase gates. It should not be treated as one linear execution plan.
