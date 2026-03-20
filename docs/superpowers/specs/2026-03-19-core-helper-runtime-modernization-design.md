# Core Helper Runtime Modernization

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Summary

Modernize the core Superpowers helper runtime by moving the workflow, execution, and config helpers from shell-first implementations to a TypeScript/Node core while preserving the existing command names and external CLI behavior.

The goal is not a CLI redesign. The goal is to keep the public helper surface stable while making the implementation easier to test, easier to maintain, and easier to run consistently across macOS and Windows.

The migration starts with the highest-leverage core helpers:

1. `bin/superpowers-config`
2. `bin/superpowers-workflow-status`
3. `bin/superpowers-plan-execution`

Shell and PowerShell entrypoints remain in place initially as compatibility shims. Over time, the Windows path should stop depending on Git Bash for these core helpers and invoke Node-backed implementations directly.

## Problem

Superpowers is currently shell-heavy in the places where correctness and cross-platform behavior matter most:

- workflow-state derivation
- execution-state mutation
- config read/write behavior
- path normalization and temporary-file handling

That has three costs:

- tests are slower and more integration-heavy than they need to be
- behavior is harder to express and verify with precise fixtures and assertions
- Windows support depends on PowerShell wrappers delegating into Bash, which adds avoidable complexity

The repository already gets value from deterministic Node-based tests and generators, but the most stateful helper behavior is still implemented in Bash. That leaves too much logic in a language that is awkward for structured data, typed invariants, atomic file updates, and rich test tooling.

## Goals

- Keep the existing helper command names and wrapper entrypoints stable.
- Move core helper logic into a more expressive implementation language with stronger testing tools.
- Make most helper behavior testable as deterministic module logic instead of long shell transcripts.
- Reduce the size and importance of the shell integration suite.
- Improve Windows support for core helpers and make removal of the Git Bash dependency the long-term direction.
- Keep the migration incremental so the runtime can be verified command-by-command.
- Standardize the core helper runtime on Node 20 LTS as the minimum supported runtime version.

## Not In Scope

- Redesigning the public helper CLI surface.
- Renaming existing commands or changing the basic wrapper layout in `bin/`.
- Migrating optional UX helpers such as the brainstorming visual companion in this first phase.
- Rewriting every shell script in the repository immediately.
- Committing to a compiled-language rewrite such as Go or Rust for this effort.

## Chosen Direction

Three options were considered:

1. Keep shell as the primary implementation language and improve only the test harness.
2. Move the core helper logic to TypeScript/Node while keeping the existing CLI surface.
3. Rebuild the helper runtime as compiled binaries.

The chosen direction is option 2.

This is the best balance for the current repository:

- it materially improves testability and maintainability
- it preserves the user-visible command surface
- it fits the repository's existing use of Node for deterministic tests and generators
- it avoids the rollout cost of an immediate compiled-runtime rewrite

## Runtime Contract

The modernized core helper runtime should require Node 20 LTS at minimum.

This requirement applies to the migrated core helper execution path, not just to development tooling. Install docs, wrapper behavior, CI coverage, and runtime diagnostics should all treat Node 20 LTS as the supported baseline once the migrated helpers ship.

The spec does not require raising the minimum beyond Node 20 for this effort.

The migrated core helper implementation should be authored in full TypeScript and shipped as compiled JavaScript for runtime execution.

This is a deliberate part of the modernization scope, not an implementation detail left to later judgment. The migration should therefore include the necessary build, typecheck, and packaging conventions for a maintained compiled runtime layer.

Compiled runtime artifacts for migrated core helpers should be checked into the repository and shipped as part of the shared `~/.superpowers/install` checkout.

This effort does not introduce a local build requirement into normal install or update flows. Users should continue to receive a runnable runtime directly from the checked-out repository contents, with generation and freshness validation handled in development and release workflows.

The new TypeScript source, build configuration, and compiled runtime artifacts should live in a dedicated runtime subdirectory rather than redefining the repository root as a Node-first toolchain.

This keeps the migration boundary explicit, limits incidental repo churn, and allows the helper modernization to proceed incrementally without silently broadening scope to unrelated parts of the repository.

The migrated core runtime may use normal third-party Node dependencies where that meaningfully improves implementation speed, correctness, or maintainability.

This effort does not impose a zero-dependency runtime rule on the modernized helpers. Dependency choice should still be deliberate, but the implementation is not restricted to the Node standard library.

When third-party runtime dependencies are used, the shipped runtime should package them into self-contained bundled artifacts that are checked into the repository alongside the compiled helper runtime.

This spec does not permit clone-time dependency restore as part of normal runtime installation or updates, and it does not rely on checked-in `node_modules` as the shipped runtime surface.

The dedicated runtime subdirectory must keep a checked-in lockfile, and bundled runtime artifacts must be reproducibly generated from that locked dependency state.

Any new third-party runtime dependency must go through explicit dependency review as part of the implementation and release process, not informal incidental approval.

Install and migration instructions must verify Node availability and version compliance before exposing migrated core helpers to the user.

If those install-time checks fail, the install flow must fail closed, tell the user exactly what requirement failed, and clean up any partial install artifacts created by that failed attempt instead of leaving a partially usable migrated runtime in place.

Fresh install and update flows for the migrated runtime generation should run through a dedicated staged install/update helper rather than relying on direct `git pull` as the operational upgrade contract.

That helper should validate Node 20 availability, verify the bundled runtime artifacts, prepare the new checkout in a staging location, and only swap the shared install into place after the staged runtime passes preflight checks. If preflight fails, the helper must preserve the last known-good install and clean up the failed staged attempt.

## Target Architecture

The new runtime should be organized around small modules with clear ownership rather than one-for-one ports of the existing shell scripts.

### CLI adapters

Each helper gets a thin CLI adapter responsible for:

- parsing arguments
- selecting output mode
- mapping typed failures to exit codes and user-facing output
- calling one core module

The CLI layer should own presentation, not business logic.

The CLI adapters and core modules should be authored in TypeScript, then compiled to JavaScript for the shipped runtime path.

The TypeScript toolchain for these adapters and modules should be isolated within the dedicated runtime subdirectory and not treated as the default build system for unrelated repository surfaces.

### Core modules

The initial module split should be:

- `core/config`
  - read, write, and list config state under `~/.superpowers/config.yaml`
- `core/workflow`
  - manifest loading
  - repo artifact discovery
  - workflow header parsing
  - conservative routing
  - supported public inspection rendering inputs
- `core/execution`
  - approved-plan state machine
  - evidence-path derivation
  - step transitions
  - rollback-safe mutation planning
- `core/platform`
  - path normalization
  - temp file and atomic write helpers
  - repo-root detection
  - filesystem and process helpers that differ across platforms

This boundary keeps stateful logic separate from CLI formatting and makes the behavioral core directly testable.

## Command Flow

Each migrated helper should follow the same path:

1. The existing command in `bin/` is invoked.
2. A thin compatibility shim forwards arguments into the Node-backed entrypoint.
3. The Node CLI adapter parses the invocation and calls a typed core module.
4. The core module returns structured success data or a typed failure.
5. The CLI adapter renders the existing output contract: JSON, human-readable text, stderr, and exit status.

This structure preserves compatibility while removing most logic from the wrapper layer.

Once a helper is migrated, its wrapper should hard-cut to the new runtime path rather than silently or conditionally falling back to the legacy shell implementation. Rollback should happen through normal release reversion or checkout rollback, not through hidden runtime bifurcation inside the shipped wrapper.

If the wrapper cannot launch the migrated runtime because Node is missing, the Node version is unsupported, the bundled artifact is missing, or the bundled artifact is invalid, it must fail closed through a stable machine-readable failure contract plus a concise remediation message.

Initial wrapper-level failure classes should include:

- `RuntimeDependencyMissing`
- `RuntimeDependencyVersionUnsupported`
- `RuntimeArtifactMissing`
- `RuntimeArtifactInvalid`

The remediation text should explicitly tell the operator whether they need to install or upgrade Node 20 LTS, refresh the shared checkout, or reinstall Superpowers.

This spec does not require a checked-in runtime fingerprint or manifest file for wrapper-time validation. Wrappers and staged install helpers may check for expected entry artifacts and attempt launchability directly, but bundle-integrity validation remains a release/test responsibility rather than a runtime manifest contract.

## Compatibility Rules

The external contract should remain stable during the migration.

Preserve:

- current command names
- current flag shapes
- current JSON field names where they are already part of the contract
- current exit-code behavior
- current human-facing output that is already documented or asserted in deterministic tests

The compatibility target is behavioral equivalence at the command boundary, not source-level parity with the shell implementations.

This spec does not require byte-for-byte preservation of every incidental stdout or stderr line. Wording may improve where it is not already part of a documented or test-asserted contract, but any deliberate compatibility break to an existing documented or test-backed human-facing output must be reviewed explicitly.

The new implementation may introduce:

- stronger internal typing
- clearer error modeling
- more explicit state transitions
- safer file-write behavior

## Error Handling

The Node core should replace ad hoc shell failures with a small explicit failure taxonomy.

Initial user-facing categories should cover:

- invalid arguments
- missing repo context
- malformed workflow headers
- stale or invalid workflow linkage
- invalid execution state
- file-write conflicts
- missing runtime dependencies at the wrapper boundary

Where the public contract already exposes machine-readable fields such as `failure_class` or `reason`, the new implementation must preserve that behavior.

Wrapper-launch failures must use the stable failure classes defined in the command flow section rather than helper-specific ad hoc wording.

Filesystem mutations, especially for plan and evidence updates, should use atomic write patterns with explicit backup and restore behavior rather than implicit shell pipelines.

## Testing Strategy

The new testing model should invert the current balance:

- most coverage should live in fast deterministic TypeScript/Node tests
- a small shell suite should remain only for wrapper and launch contracts

### Node-heavy coverage

Add deterministic tests for:

- workflow header parsing
- manifest derivation and validation
- routing decisions
- plan-step transitions
- evidence-path handling
- config read/write/list behavior
- platform path conversion and normalization

Add fixture-driven integration tests that invoke the Node CLI entrypoints against temporary repositories and assert on:

- stdout
- stderr
- exit status
- mutated files

Add fail-closed wrapper and install-path tests for:

- Node missing
- Node version below the supported floor
- bundled runtime artifact missing
- bundled runtime artifact invalid
- install-time Node checks failing before migrated helper exposure
- cleanup of partial install state after failed install attempts
- staged update preserving the last known-good install when preflight fails
- staged update swapping in the new install only after runtime preflight succeeds

Add runtime-packaging governance tests or checks for:

- checked-in lockfile presence
- reproducible bundle generation from locked dependencies
- detection of stale bundled artifacts when runtime source or dependency state changes

### Thin shell coverage

Keep a smaller shell-focused contract layer for:

- Unix wrapper argument forwarding
- PowerShell wrapper argument forwarding
- Windows path normalization at the wrapper boundary
- verification that the installed command names still behave as expected

### Migration equivalence tests

For the riskiest helpers, temporarily run old and new implementations against the same fixture inputs and assert on equivalence for representative success and failure cases.

This is the main anti-regression mechanism during the rewrite.

## Migration Plan

The migration should proceed helper-by-helper.

Implementation may still phase helper work internally, but the migrated cutover should ship as one coordinated release once all three targeted helpers are ready and validated together.

### Phase 1: `superpowers-config`

Port the smallest helper first to establish:

- the Node entrypoint pattern
- the TypeScript compile-and-run pattern
- shell and PowerShell shim behavior
- test harness conventions
- config file round-trip expectations

### Phase 2: `superpowers-workflow-status`

Port the workflow-state helper next because it has high logic density and large test payoff.

This phase should introduce:

- typed workflow status derivation
- fixture-driven repo-state tests
- explicit malformed-header and fallback behavior tests

### Phase 3: `superpowers-plan-execution`

Port the execution helper last because it is the most stateful and mutation-heavy surface.

This phase must prioritize:

- state machine correctness
- evidence and plan mutation safety
- rollback and recovery behavior
- exact compatibility for reviewed execution flows

## Release Strategy

The runtime modernization should ship the three targeted migrated helpers in one coordinated release:

1. `superpowers-config`
2. `superpowers-workflow-status`
3. `superpowers-plan-execution`

That release boundary is intentionally broader than the implementation phases. The implementation work may land incrementally behind the scenes, but the supported hard-cut wrapper behavior should switch over for the three targeted helpers together rather than one helper per public release.

This choice raises the verification burden and must be matched by stronger pre-release compatibility evidence across config, routing, and execution flows.

Release verification for the coordinated cutover must also confirm that install and migration flows:

- detect missing or unsupported Node before exposing migrated helpers
- fail with the stable wrapper failure contract
- clean up partial install state on failure
- expose a runnable bundled runtime on success without clone-time dependency restore
- preserve the last known-good install when staged update preflight fails
- swap the validated staged install into place only after preflight succeeds

Release verification must also confirm that the shipped bundles were generated from the checked-in lockfile and that any new runtime dependencies received explicit review.

Release and test workflows, not wrapper-time manifest validation, are responsible for detecting stale or corrupted bundled artifacts before shipment.

## Windows Direction

For the targeted core helpers, the long-term Windows direction is to stop requiring Git Bash.

During migration:

- existing PowerShell wrappers may remain as compatibility shims
- wrappers may continue to call the existing shell path only until the helper is migrated and released

After compatibility is established for each targeted helper:

- PowerShell wrappers should invoke the Node-backed implementation directly
- Git Bash should no longer be a required dependency for those migrated core helpers

This change is a major part of the platform payoff and should not be treated as optional cleanup.

## Legacy Helper Retirement

After the coordinated migrated cutover ships, the legacy shell implementations of:

1. `superpowers-config`
2. `superpowers-workflow-status`
3. `superpowers-plan-execution`

should be removed from the shipped runtime surface rather than retained as dormant fallback or reference code.

Any parity evidence still worth preserving should live in deterministic tests, fixtures, release evidence, or normal git history rather than as live legacy helper bodies inside the runtime package.

## Risks

- A compatibility-focused migration can still accidentally drift in output wording or exit behavior.
- Temporary dual implementations increase maintenance cost while equivalence gates are active.
- `superpowers-plan-execution` is stateful enough that unsafe partial migration could create subtle corruption or evidence drift.
- Introducing Node as a core runtime dependency changes the install/runtime contract and must be documented clearly.
- A hard-cut migration raises release quality requirements because the shipped wrapper is no longer a hidden rollback surface.
- A coordinated cutover for all three targeted helpers increases blast radius and makes release verification quality-critical.
- Allowing normal third-party runtime dependencies increases supply-chain, packaging, and update-management complexity.
- Bundled runtime artifacts become part of the shipped compatibility surface and must be regenerated and validated reliably in release workflows.
- Install and migration flows become part of the fail-closed runtime contract and must handle missing or unsupported Node environments without leaving broken partial installs behind.
- Lockfile and dependency-review discipline become mandatory controls against supply-chain drift in the new runtime subdirectory.
- Direct `git pull` is no longer sufficient as the operational update contract for migrated runtime installs, so the staged helper becomes release-critical infrastructure.
- Because shipped wrappers do not validate bundles against a checked-in fingerprint manifest, release and test workflows must catch stale or corrupted bundles before release.
- Removing the legacy helper bodies in the coordinated cutover increases pressure on pre-release parity evidence, because git history and tests become the primary fallback reference.

## Success Criteria

- Core helper behavior is primarily implemented in TypeScript/Node rather than Bash.
- Most behavioral coverage runs as deterministic parallel Node tests.
- The shell suite becomes a narrow wrapper-contract layer instead of the main behavioral test surface.
- The public helper CLI surface remains stable throughout the migration.
- Windows execution for the migrated core helpers no longer depends on Git Bash.

## Open Questions

None. CEO review resolved the runtime, packaging, compatibility, dependency, install/update, and rollout decisions needed for implementation planning.
