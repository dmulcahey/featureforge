#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# This gate is the classified public runtime-flow surface:
# - executable public-flow proof: compiled-CLI/operator smoke and replay suites
# - focused public contracts: query/topology/final-review/golden contract suites
# - static public guards: scanner/schema/runtime-boundary contract suites
#
# public_flow_scan_contracts is the scanner self-test for this gate and runs as
# focused validation; liveness_model_checker and internal_* suites remain
# internal semantic/model coverage, not shipped-runtime public-flow proof.
cargo nextest run \
  --all-features \
  --no-fail-fast \
  --test contracts_execution_runtime_boundaries \
  --test execution_harness_state \
  --test execution_query \
  --test plan_execution \
  --test plan_execution_final_review \
  --test plan_execution_topology \
  --test public_cli_flow_contracts \
  --test public_replay_churn \
  --test runtime_behavior_golden \
  --test workflow_entry_shell_smoke \
  --test workflow_runtime \
  --test workflow_runtime_final_review \
  --test workflow_shell_smoke
