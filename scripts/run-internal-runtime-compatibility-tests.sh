#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

cargo nextest run \
  --all-targets \
  --all-features \
  --no-fail-fast \
  internal_only_compatibility
