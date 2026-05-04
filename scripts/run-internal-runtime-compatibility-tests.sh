#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

internal_test_args=()
for test_file in tests/internal_*.rs; do
  test_name="$(basename "$test_file" .rs)"
  internal_test_args+=(--test "$test_name")
done

cargo nextest run \
  --all-features \
  --no-fail-fast \
  "${internal_test_args[@]}"
