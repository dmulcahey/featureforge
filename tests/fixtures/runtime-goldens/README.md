# Runtime Goldens

These fixtures pin normalized public runtime behavior captured after the semantic fixes that
precede modularization.

- `public-runtime-routes.json` captures real public CLI output for `plan execution status`,
  `workflow operator --json`, and `workflow status --json` across the Task 8 representative
  states.
- `public-schema-signatures.json` captures compact signatures for the public schemas whose exact
  checked-in JSON files are already parity-tested against generated output.

The tests normalize volatile values such as temp paths, run IDs, chunk IDs, git SHAs, timestamps,
and generated fingerprints. They intentionally preserve semantic routing fields including phase,
phase detail, review-state status, reason codes, command context, next action, recommended command,
and structured public command payloads when those fields are exposed.

Regenerate with `FEATUREFORGE_UPDATE_RUNTIME_GOLDENS=1` only after deliberately reviewing the
behavior or schema change being blessed.
