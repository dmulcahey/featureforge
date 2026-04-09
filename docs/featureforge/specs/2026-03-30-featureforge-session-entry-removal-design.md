# FeatureForge Session Entry Removal

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review
**Implementation Target:** Historical

> **Implementation-target notice:** This active source-spec copy exists so historical approved plans continue to resolve to a valid approved spec. The fuller historical design narrative remains archived at [2026-03-30-featureforge-session-entry-removal-design.md](../../archive/featureforge/specs/2026-03-30-featureforge-session-entry-removal-design.md). The active implementation target is still the April supersession-aware corpus rooted at [ACTIVE_IMPLEMENTATION_TARGET.md](ACTIVE_IMPLEMENTATION_TARGET.md).

## Summary

Remove session-entry gating as an active FeatureForge workflow surface so routing starts directly from artifact and runtime state without a separate approval checkpoint.

## Requirement Index

- [REQ-001][behavior] FeatureForge must remove the public `featureforge session-entry` command family from active CLI surfaces.
- [REQ-002][behavior] Workflow resolution must never fail closed on session-entry state and must not require `FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY`.
- [REQ-003][behavior] Workflow/operator phase modeling must remove session-entry gate states and gate-only actions (`needs_user_choice`, `bypassed`, `session_entry_gate`, `continue_outside_featureforge`) from active routing logic.
- [REQ-004][behavior] Generated `using-featureforge` docs and generation logic must remove bypass-gate semantics, session-entry command guidance, and session-entry env exports.
- [REQ-005][behavior] Session-entry schema generation and checked-in `schemas/session-entry-resolve.schema.json` must be removed from active contract surfaces.
- [REQ-006][behavior] Active docs must no longer describe session-entry as a required entry point or strict gate.
- [REQ-007][behavior] Session-entry-only tests/evals must be removed or rewritten to validate direct non-gated routing behavior.
- [REQ-008][verification] Contract suites must fail closed if session-entry gate language or strict-session-entry runtime checks are reintroduced on active surfaces.
- [REQ-009][behavior] Breaking workflow-output changes in this slice must use explicit schema/version signaling per command output family and release-note callouts so automation breakage is visible, not silent.
- [REQ-010][verification] Active contract tests must fail closed if session-entry gate semantics are reintroduced in runtime, templates, generated docs, or runtime instruction docs, including `FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY`, `FEATUREFORGE_SPAWNED_SUBAGENT`, and `FEATUREFORGE_SPAWNED_SUBAGENT_OPT_IN`.
- [REQ-011][verification] Active runtime surfaces (outside `docs/archive/**`) must fail contract checks if legacy session-entry gate module wiring, strict gate checks, or `~/.featureforge/session-entry/...` gate-path references are reintroduced.
