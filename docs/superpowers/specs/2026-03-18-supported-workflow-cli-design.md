# Supported Workflow CLI

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming

## Summary

Add a supported user-facing workflow inspection CLI for Superpowers:

1. `bin/superpowers-workflow`
2. `bin/superpowers-workflow.ps1`

This CLI gives humans a stable way to answer three questions without reading skill internals, local manifest files, or raw helper diagnostics:

- where am I in the workflow?
- why did Superpowers resolve that stage?
- what is the next safe action?

V1 is intentionally narrow:

- inspection-only
- human-first output
- product-workflow only, up to `implementation_ready`
- no supported public JSON contract
- no execution-stage inspection
- no public mutation commands

The public CLI must reuse the existing workflow-state derivation logic, but it must do so through a side-effect-free inspection path. A supported public read command must not mutate local runtime state just to discover workflow state.

## Problem

Superpowers now has a strong internal workflow runtime, but the user-facing inspection story is still weak.

Today:

- `bin/superpowers-workflow-status` exists and can derive workflow state
- README and platform docs describe it as an internal helper, not a supported public CLI
- the helper's default outputs are shaped for runtime consumption first, not for humans trying to understand what to do next
- the helper's `status` path can write or repair the local manifest while resolving state

That creates a trust and usability gap:

- users do not have a stable supported way to inspect workflow state directly
- users must interpret raw status codes or skill behavior to understand the current stage
- a discovery command can have local side effects, which is surprising for a supported inspection surface
- documentation cannot confidently tell users "run this command to see where you are" because the current helper is intentionally internal

Superpowers has already invested in making workflow routing conservative and reliable. The next leverage point is exposing that capability through a human-friendly, supportable inspection surface.

## Goals

- Add a supported public CLI for inspecting product-workflow state.
- Let users answer "where am I?", "why?", and "what next?" from the terminal.
- Keep repo-tracked spec and plan documents authoritative for approvals and revision linkage.
- Reuse the existing workflow-state derivation logic instead of building a second parser and routing engine.
- Guarantee that public inspection commands are side-effect-free.
- Keep conservative fallback behavior: ambiguity, malformed headers, stale linkage, and local-state mismatches should route to the earlier safe stage instead of guessing.
- Preserve Bash and PowerShell parity.
- Treat public wording as a deliberate compatibility surface and test it accordingly.

## Not In Scope

- Making the local manifest authoritative over repo-tracked workflow docs.
- Publishing `expect` or `sync` as supported user-facing commands.
- Public execution-stage inspection or mutation.
- A stable public JSON schema in v1.
- A new umbrella `superpowers workflow ...` dispatcher or PATH-oriented command family in this change.
- Replacing `bin/superpowers-workflow-status`; it remains the internal helper.
- Broad workflow changes outside the existing product-workflow pipeline:
  - `brainstorming`
  - `plan-ceo-review`
  - `writing-plans`
  - `plan-eng-review`
  - `implementation_ready`

## Existing Context

Superpowers already has the internal pieces needed for this change:

- `bin/superpowers-workflow-status` derives workflow state conservatively from repo docs plus a branch-scoped manifest.
- Repo docs remain authoritative for workflow truth.
- The local manifest under `~/.superpowers/projects/<repo-slug>/<user>-<safe-branch>-workflow-state.json` is a rebuildable index, not the approval record.
- Existing docs and release notes explicitly label the helper as internal and defer a supported public workflow CLI until the contract is stable.
- The current runtime installation model exposes dedicated binaries under `~/.superpowers/install/bin/` rather than a single top-level `superpowers` dispatcher.

That existing runtime layout matters. The clean v1 surface is another dedicated binary, not a larger command router.

## User Experience

### Supported Commands

The public CLI exposes these commands:

```text
superpowers-workflow status
superpowers-workflow next
superpowers-workflow artifacts
superpowers-workflow explain
superpowers-workflow help
```

PowerShell wrapper parity is required through `bin/superpowers-workflow.ps1`.

### Invocation Model

V1 uses the same installation style as the existing runtime helpers:

```bash
~/.superpowers/install/bin/superpowers-workflow status
```

This spec does not introduce a broader PATH or dispatcher story. The public contract is the supported dedicated binary at the install root.

### Command Roles

`status`

- one-screen workflow summary
- human wording first
- includes current stage, short reason, next safe action, and key artifact pointers

`next`

- emphasizes the next safe action
- explains why no later stage is valid yet
- may mention the relevant Superpowers skill or workflow stage explicitly

`artifacts`

- shows the active or expected spec and plan paths
- indicates whether paths came from authoritative repo docs or local expected-path state
- may include the manifest path for debugging, but that is secondary

`explain`

- expands terse diagnostics into actionable guidance
- used for ambiguity, malformed headers, stale plan linkage, repo identity mismatches, missing expected artifacts, or ignored local manifest state

`help`

- prints supported commands and short descriptions
- must clearly distinguish supported public inspection commands from internal helper surfaces

## Public Output Contract

The public CLI is human-first by default.

It must not primarily expose raw internal status codes or raw reason codes. Internals may still power the output, but the wording users read should be stable human language.

### Example `status`

```text
Workflow status: Spec review needed
Why: The current spec is still in Draft.
Next: Use superpowers:plan-ceo-review
Spec: docs/superpowers/specs/2026-03-18-example-design.md
Plan: none
```

### Example `next`

```text
Next safe step: Review and approve the current spec with superpowers:plan-ceo-review.

Reason:
- A spec exists and is authoritative.
- Its Workflow State is Draft.
- No later stage is safe until the spec review resolves.
```

### Example `artifacts`

```text
Workflow artifacts
- Spec: docs/superpowers/specs/2026-03-18-example-design.md (from repo docs)
- Plan: docs/superpowers/plans/2026-03-18-example.md (expected, missing)
- Manifest: ~/.superpowers/projects/<slug>/<user>-<branch>-workflow-state.json
```

### Example `explain`

```text
Superpowers could not identify one unambiguous current spec.

Safe fallback:
- Treat the workflow as spec review stage.

Why this happened:
- Multiple candidate spec documents matched the current repo state.
- Superpowers will not guess which one should drive routing.

What to do:
1. Decide which spec is current.
2. Remove or supersede stale competing specs.
3. Re-run: ~/.superpowers/install/bin/superpowers-workflow status
```

### Public Vocabulary Mapping

The public CLI maps internal workflow codes to human-oriented wording.

Recommended v1 wording:

- `needs_brainstorming` -> `Brainstorming needed`
- `spec_draft` -> `Spec review needed`
- `spec_approved_needs_plan` -> `Plan writing needed`
- `plan_draft` -> `Engineering plan review needed`
- `stale_plan` -> `Plan update needed`
- `implementation_ready` -> `Ready for implementation handoff`

Internal reason codes such as `fallback_ambiguity_spec` or `malformed_plan_headers` remain implementation details. The public CLI translates them into stable explanation text rather than surfacing them as the primary contract.

### Exit Code Policy

- Exit `0` for successful inspection, including conservative fallback outcomes.
- Exit nonzero only for true usage or runtime failures:
  - invalid arguments
  - unsupported command shape
  - unreadable runtime environment
  - wrapper execution failure

Normal workflow outcomes like "go back to plan review" or "state is ambiguous, fall back conservatively" are not errors.

## Architecture

### High-Level Shape

Add two public runtime surfaces:

1. `bin/superpowers-workflow`
2. `bin/superpowers-workflow.ps1`

These public binaries sit above the existing internal helper:

```text
user
  |
  v
superpowers-workflow
  |
  v
side-effect-free internal resolution path
  |
  v
superpowers-workflow-status
  |
  +--> repo docs remain authoritative
  |
  +--> existing manifest may be consulted as a hint
```

### Read-Only Resolution Contract

The public CLI must only use a side-effect-free internal resolution path.

The exact internal entrypoint name is an implementation detail. It may be:

- a dedicated subcommand such as `resolve`
- or a new flag such as `status --read-only`

What matters is behavior, not the internal spelling.

Required read-only guarantees:

- must not create a manifest when none exists
- must not rewrite an existing manifest
- must not back up, repair, or rename a corrupt manifest
- must not invoke behavior owned by `expect` or `sync`
- must not edit repo-tracked spec or plan docs under any circumstance
- must not change the effective workflow state merely because the public CLI was run

Read-only inspection may still:

- read authoritative repo docs
- read an existing manifest as a hint
- ignore an invalid or mismatched manifest
- report that local manifest state appears corrupt, stale, mismatched, or ambiguous

### Authority Model

The existing authority split remains unchanged:

- spec approval truth lives in the spec document headers
- plan approval truth lives in the plan document headers and source-spec linkage
- the manifest may provide expected artifact paths, but it is not the approval authority

If repo docs and local manifest state disagree:

- the public CLI must treat repo docs as authoritative when available
- the public CLI must report the earlier safe stage
- the public CLI must explain the reason in human language

### Why Not A Separate Public Parser

This spec rejects a separate public artifact parser and workflow engine.

That approach would:

- duplicate routing logic the repo just spent multiple releases hardening
- create drift risk between the public CLI and internal runtime behavior
- force every future workflow change to update two sources of derivation logic

The public CLI should be a presentation layer over one routing brain, not a second workflow brain.

## State Handling Rules

The public CLI covers the same product-workflow states as the internal helper up to `implementation_ready`.

It must support and explain at least these conditions:

- no workflow docs present
- one draft spec
- one approved spec without a plan
- one draft plan
- one stale approved plan
- one implementation-ready plan
- malformed spec headers
- malformed plan headers
- ambiguous spec discovery
- ambiguous plan discovery
- missing expected spec
- missing expected plan
- manifest repo-root mismatch
- manifest branch mismatch
- prior-manifest recovery opportunities
- corrupt local manifest

For corrupt or mismatched local state, the public CLI must prefer transparency over repair. If repair is useful later, that remains the job of internal helper behavior or skill-driven flows, not the supported public inspection surface.

## Testing Requirements

This enhancement must ship with a thorough public CLI test surface. The non-mutation guarantee is as important as the feature itself.

### 1. Public Command Matrix

Add regression coverage for all supported public commands:

- `status`
- `next`
- `artifacts`
- `explain`
- `help`

Cover every user-visible workflow condition the public surface can report:

- bootstrap with no docs
- draft spec
- approved spec with no plan
- draft plan
- stale approved plan
- implementation ready
- malformed spec
- malformed plan
- ambiguous spec discovery
- ambiguous plan discovery
- missing expected spec
- missing expected plan
- repo-root mismatch
- branch mismatch
- prior-manifest recovery opportunity
- corrupt manifest present

### 2. Non-Mutation Guarantees

Add explicit tests that every public inspection command:

- leaves repo-tracked docs byte-identical
- leaves an existing manifest byte-identical
- does not create a manifest when none exists
- does not back up, repair, rename, or rewrite corrupt manifests
- does not invoke or emulate `expect` or `sync`

If the implementation ever needs ephemeral computation state, it must not persist that state as a side effect of a public inspection command in v1.

### 3. Public/Private Semantic Parity

For each supported repo-state fixture:

- compare the public CLI meaning against the internal helper's resolved state
- require semantic alignment on:
  - selected workflow stage
  - next safe action
  - chosen spec path
  - chosen plan path when applicable

The text can differ. The workflow meaning cannot.

### 4. Output Contract Coverage

Treat public wording as a real compatibility surface.

Add golden or fixture-based tests for:

- `status`
- `next`
- `artifacts`
- `explain`
- `help`

Normalize dynamic values in test output where needed:

- temp paths
- usernames
- repo roots
- timestamps

Copy changes to the public CLI should be intentional and reviewed, not accidental side effects of internal helper changes.

### 5. Failure-Mode Coverage

Add explicit coverage for:

- invalid command names
- unknown flags
- missing or unreadable repo context
- wrapper execution failures
- runtime failures that should exit nonzero

Also verify that conservative fallback results still exit `0`.

### 6. PowerShell Parity

Add parity coverage for `bin/superpowers-workflow.ps1`:

- same command set
- same stage semantics
- same non-mutation guarantees
- same human-facing meaning

Wrapper parity should be tested semantically, not just by asserting that the wrapper launches.

### 7. Documentation Contract Coverage

Update and test documentation so it clearly states:

- `superpowers-workflow` is the supported public inspection surface
- `superpowers-workflow-status` remains internal
- v1 does not promise public JSON
- v1 does not promise execution-stage inspection

### 8. Regression Policy

Any future change to workflow-state derivation should run both:

- the internal helper suite
- the public CLI semantic-parity suite

Superpowers should not be able to change internal routing meaning without either:

- preserving public CLI behavior
- or intentionally updating the public surface and its golden fixtures

## Documentation And Rollout

Update these docs in this change:

- `README.md`
- `docs/README.codex.md`
- `docs/README.copilot.md`
- any runtime-helper references that currently imply the internal helper is the user-facing inspection entrypoint

Documentation changes should:

- promote `superpowers-workflow` as the supported way to inspect workflow state
- continue to describe `superpowers-workflow-status` as an internal runtime helper
- show example invocations using the install-root binary path
- explain that public inspection commands are read-only

This change should not deprecate `superpowers-workflow-status`. It remains a valid internal helper and skill-facing implementation surface.

## Success Criteria

This enhancement is successful when:

- a user can answer "where am I?", "why?", and "what next?" without reading local manifests, skill docs, or raw reason codes
- the supported public inspection commands are demonstrably non-mutating
- the public CLI stays semantically aligned with the internal routing logic
- README and platform docs can confidently point users at one supported workflow inspection command

## Alternatives Considered

### Thin Wrapper Over Current `status --refresh`

Rejected as-is.

Although the internal helper already derives the right workflow meaning, its current `status` behavior can mutate local manifest state. A supported public inspection surface should not inherit that side effect model directly.

### Separate Public Parser

Rejected.

This would create a second workflow engine and increase drift risk between supported user output and internal routing behavior.

### Broader Public CLI Covering Execution In V1

Rejected for now.

The execution helper is a separate runtime surface with its own state machine. Product-workflow inspection should stabilize first before expanding the public scope to execution-stage inspection.
