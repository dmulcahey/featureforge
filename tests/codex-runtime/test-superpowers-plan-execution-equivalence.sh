#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LEGACY_EXEC_BIN="$REPO_ROOT/bin/superpowers-plan-execution"
TMP_ROOT="$(mktemp -d)"
REPO_DIR="$TMP_ROOT/repos"
NODE_BUNDLE="$TMP_ROOT/superpowers-plan-execution.cjs"
PLAN_REL="docs/superpowers/plans/2026-03-17-example-execution-plan.md"
SPEC_REL="docs/superpowers/specs/2026-03-17-example-execution-plan-design.md"
trap 'rm -rf "$TMP_ROOT"' EXIT

mkdir -p "$REPO_DIR"

npm --prefix "$REPO_ROOT/runtime/core-helpers" exec esbuild -- \
  "$REPO_ROOT/runtime/core-helpers/src/cli/superpowers-plan-execution.ts" \
  --bundle \
  --platform=node \
  --format=cjs \
  --log-level=warning \
  --outfile="$NODE_BUNDLE" >/dev/null

assert_same_output() {
  local label="$1"
  local legacy_status="$2"
  local legacy_output="$3"
  local node_status="$4"
  local node_output="$5"

  if [[ "$legacy_status" -ne "$node_status" ]]; then
    echo "Expected legacy helper and bundled CLI to return the same exit code for: $label"
    echo "legacy: $legacy_status"
    echo "node:   $node_status"
    printf '%s\n' "$legacy_output"
    printf '%s\n' "$node_output"
    exit 1
  fi

  if [[ "$legacy_output" != "$node_output" ]]; then
    echo "Expected legacy helper and bundled CLI to match for: $label"
    diff -u <(printf '%s\n' "$legacy_output") <(printf '%s\n' "$node_output") || true
    exit 1
  fi
}

init_repo() {
  local repo_dir="$1"
  mkdir -p "$repo_dir"
  git -C "$repo_dir" init >/dev/null 2>&1
  git -C "$repo_dir" config user.name "Superpowers Test"
  git -C "$repo_dir" config user.email "superpowers-tests@example.com"
  printf '# plan execution equivalence fixture\n' > "$repo_dir/README.md"
  git -C "$repo_dir" add README.md
  git -C "$repo_dir" commit -m "init" >/dev/null 2>&1
}

write_file() {
  local path="$1"
  mkdir -p "$(dirname "$path")"
  cat > "$path"
}

write_approved_spec() {
  local repo_dir="$1"
  write_file "$repo_dir/$SPEC_REL" <<EOF
# Example Execution Plan Design

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review
EOF
}

run_case() {
  local label="$1"
  local repo_dir="$2"
  local legacy_output
  local node_output
  local legacy_status=0
  local node_status=0
  shift 2

  legacy_output="$(cd "$repo_dir" && "$LEGACY_EXEC_BIN" "$@" 2>&1)" || legacy_status=$?
  node_output="$(cd "$repo_dir" && SUPERPOWERS_RUNTIME_ROOT="$REPO_ROOT" node "$NODE_BUNDLE" "$@" 2>&1)" || node_status=$?

  assert_same_output "$label" "$legacy_status" "$legacy_output" "$node_status" "$node_output"
}

run_clean_status_case() {
  local repo="$REPO_DIR/clean-status"
  init_repo "$repo"
  write_approved_spec "$repo"
  write_file "$repo/$PLAN_REL" <<EOF
# Example Execution Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** \`${SPEC_REL}\`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Task 1: Core flow

- [ ] **Step 1: Prepare workspace for execution**
- [ ] **Step 2: Validate the generated output**
EOF

  run_case "clean status" "$repo" status --plan "$PLAN_REL"
}

run_independent_recommend_case() {
  local repo="$REPO_DIR/independent-recommend"
  init_repo "$repo"
  write_approved_spec "$repo"
  write_file "$repo/$PLAN_REL" <<EOF
# Example Execution Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** \`${SPEC_REL}\`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Task 1: Parser slice

**Files:**
- Modify: \`src/parser-slice.sh:10-40\`
- Test: \`bash tests/parser-slice.test.sh\`

- [ ] **Step 1: Build parser slice**

## Task 2: Formatter slice

**Files:**
- Modify: \`src/formatter-slice.sh:12-36\`
- Test: \`bash tests/formatter-slice.test.sh\`

- [ ] **Step 1: Build formatter slice**
EOF

  run_case \
    "independent recommend" \
    "$repo" \
    recommend \
    --plan "$PLAN_REL" \
    --isolated-agents available \
    --session-intent stay \
    --workspace-prepared yes
}

run_missing_execution_mode_case() {
  local repo="$REPO_DIR/missing-execution-mode"
  init_repo "$repo"
  write_approved_spec "$repo"
  write_file "$repo/$PLAN_REL" <<EOF
# Example Execution Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Source Spec:** \`${SPEC_REL}\`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Task 1: Core flow

- [ ] **Step 1: Prepare workspace for execution**
EOF

  run_case "missing execution mode" "$repo" status --plan "$PLAN_REL"
}

run_clean_status_case
run_independent_recommend_case
run_missing_execution_mode_case

echo "plan-execution legacy/bundled equivalence checks passed."
