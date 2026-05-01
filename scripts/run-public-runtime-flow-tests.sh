#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

cargo nextest run \
  --all-features \
  --no-fail-fast \
  --test public_cli_flow_contracts \
  --test public_replay_churn \
  --test runtime_behavior_golden
