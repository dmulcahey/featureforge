#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

cargo fmt --check
cargo test --test runtime_module_boundaries -- --nocapture
cargo test --test runtime_instruction_contracts -- --nocapture
cargo test --test workflow_runtime -- --nocapture
cargo test --test workflow_shell_smoke -- --nocapture
cargo test --test workflow_entry_shell_smoke -- --nocapture
node scripts/gen-skill-docs.mjs --check
node --test tests/codex-runtime/skill-doc-contracts.test.mjs
node scripts/lint-workspace-runtime-evidence.mjs
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail
