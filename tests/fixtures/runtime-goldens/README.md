# Runtime Goldens

These fixtures pin normalized public runtime behavior captured after the semantic fixes that
precede modularization.

- `public-runtime-routes.json` captures public CLI route behavior for `plan execution status`,
  `workflow operator --json`, and selected `workflow status --json` surfaces across representative
  runtime states. Rows marked `full_surface_route` keep compact per-surface route DTO captures for
  schema compatibility sentinels. Rows marked `semantic_route` store one shared
  `route_semantics` object for fields that status/operator must keep in parity, plus
  `surface_specific` deltas for intentional one-surface fields such as `execution_started`,
  `blocking_records`, `reason_codes`, and `base_branch`.
- Some rows use synthetic fixture setup to reach long-lived or historical route states; those rows
  pin public output contracts, not end-to-end public transition proof. The fixture now includes
  explicit active-route evidence for public `reopen` argv and public `transfer` template binding.
  Late-stage final-review dispatch and outcome reachability are covered separately by
  `tests/public_replay_churn.rs`, which replays the relevant `advance-late-stage` transitions
  through the compiled public CLI after any synthetic historical drift setup.

The tests normalize volatile values such as temp paths, run IDs, chunk IDs, git SHAs, timestamps,
and generated fingerprints. The harness asserts status/operator semantic parity before writing
compact rows for phase, phase detail, review-state status, state kind, next action, typed
argv/template, required inputs, command/recording context, blocking scope/task, public repair
targets, blockers, and blocking reason codes. It intentionally preserves surface-specific reason
records and diagnostic fields only as deltas to avoid duplicating entire payloads. It intentionally excludes the
display-only `recommended_command` compatibility field from route goldens; executable command
authority is pinned through typed public argv/template fields instead.

Regenerate route goldens with `FEATUREFORGE_UPDATE_RUNTIME_GOLDENS=1` only after deliberately
reviewing the behavior change being blessed. Public schema JSON files are parity-tested against
generated output separately, while targeted schema tests pin the route DTO fields that are part of
the agent-facing execution contract.
