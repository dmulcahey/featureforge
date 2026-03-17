#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
STATUS_BIN="$REPO_ROOT/bin/superpowers-workflow-status"
STATE_DIR="$(mktemp -d)"
REPO_DIR="$(mktemp -d)"
trap 'rm -rf "$STATE_DIR" "$REPO_DIR"' EXIT
export SUPERPOWERS_STATE_DIR="$STATE_DIR"

WORKFLOW_FIXTURE_DIR="$REPO_ROOT/tests/codex-runtime/fixtures/workflow-artifacts"
USER_NAME="$(whoami 2>/dev/null || echo user)"

# bootstrap repo with no docs -> brainstorming
# draft spec -> plan-ceo-review
# approved spec with no plan -> writing-plans
# draft plan -> plan-eng-review
# stale approved plan -> writing-plans
# corrupted manifest -> backup + warning + conservative route
# out-of-repo path -> explicit failure
# same repo slug, different branches/worktrees -> independent manifests
# bounded refresh -> prefer newest bounded candidate set
# single retry -> one retry on write conflict, then conservative route
# expect missing path survives refresh
# ambiguous fallback routes conservatively with note
# sync with missing artifact reads actual file state (not expect semantics)
# expect/sync write conflicts route conservatively after single retry

require_helper() {
  if [[ ! -x "$STATUS_BIN" ]]; then
    echo "Expected workflow helper to exist and be executable: $STATUS_BIN"
    exit 1
  fi
}

assert_contains() {
  local output="$1"
  local expected="$2"
  local label="$3"
  if [[ "$output" != *"$expected"* ]]; then
    echo "Expected ${label} output to contain '${expected}'"
    printf '%s\n' "$output"
    exit 1
  fi
}

run_status_refresh() {
  local repo_dir="$1"
  local label="$2"
  local expected_skill="$3"
  local output
  local status=0
  output="$(cd "$repo_dir" && "$STATUS_BIN" status --refresh 2>&1)" || status=$?
  if [[ $status -ne 0 ]]; then
    echo "Expected status refresh to succeed for: $label"
    printf '%s\n' "$output"
    exit 1
  fi
  assert_contains "$output" "$expected_skill" "$label"
  printf '%s\n' "$output"
}

run_status_refresh_with_env() {
  local repo_dir="$1"
  local label="$2"
  local expected_skill="$3"
  local output
  local status=0
  shift 3
  output="$(cd "$repo_dir" && env "$@" "$STATUS_BIN" status --refresh 2>&1)" || status=$?
  if [[ $status -ne 0 ]]; then
    echo "Expected status refresh to succeed for: $label"
    printf '%s\n' "$output"
    exit 1
  fi
  assert_contains "$output" "$expected_skill" "$label"
  printf '%s\n' "$output"
}

run_command_fails() {
  local repo_dir="$1"
  local label="$2"
  local expected_output="$3"
  local output
  local status=0
  shift 3
  output="$(cd "$repo_dir" && "$STATUS_BIN" "$@" 2>&1)" || status=$?
  if [[ $status -eq 0 ]]; then
    echo "Expected command to fail for: $label"
    printf '%s\n' "$output"
    exit 1
  fi
  if [[ -n "$expected_output" && "$output" != *"$expected_output"* && "${output,,}" != *"${expected_output,,}"* ]]; then
    echo "Expected failure output for ${label} to mention '${expected_output}'"
    printf '%s\n' "$output"
    exit 1
  fi
}

run_command_succeeds() {
  local repo_dir="$1"
  local label="$2"
  local output
  local status=0
  shift 2
  output="$(cd "$repo_dir" && "$STATUS_BIN" "$@" 2>&1)" || status=$?
  if [[ $status -ne 0 ]]; then
    echo "Expected command to succeed for: $label"
    printf '%s\n' "$output"
    exit 1
  fi
  printf '%s\n' "$output"
}

init_repo() {
  local repo_dir="$1"
  local remote_url="${2:-}"

  mkdir -p "$repo_dir"
  git -C "$repo_dir" init >/dev/null 2>&1
  git -C "$repo_dir" config user.name "Superpowers Test"
  git -C "$repo_dir" config user.email "superpowers-tests@example.com"
  printf '# workflow status regression fixture\n' > "$repo_dir/README.md"
  git -C "$repo_dir" add README.md
  git -C "$repo_dir" commit -m "init" >/dev/null 2>&1
  if [[ -n "$remote_url" ]]; then
    git -C "$repo_dir" remote add origin "$remote_url"
  fi
}

repo_slug_for_manifest() {
  local repo_dir="$1"
  local remote_url
  local slug

  remote_url="$(git -C "$repo_dir" remote get-url origin 2>/dev/null || true)"
  slug="$(printf '%s' "$remote_url" | sed -E 's|.*[:/]+([^/]+/[^/]+)\.git$|\1|; s|.*[:/]+([^/]+/[^/]+)$|\1|')"
  if [[ -z "$slug" || "$slug" == "$remote_url" ]]; then
    slug="$(basename "$repo_dir")"
  fi
  printf '%s' "$slug" | tr '/' '-'
}

manifest_path_for_branch() {
  local repo_dir="$1"
  local branch
  local safe_branch
  local slug

  branch="$(git -C "$repo_dir" rev-parse --abbrev-ref HEAD 2>/dev/null || echo main)"
  if [[ "$branch" == "HEAD" || -z "$branch" ]]; then
    branch="main"
  fi
  safe_branch="$(printf '%s' "$branch" | sed 's#[^A-Za-z0-9._-]#-#g')"
  slug="$(repo_slug_for_manifest "$repo_dir")"
  printf '%s\n' "$STATE_DIR/projects/$slug/${USER_NAME}-${safe_branch}-workflow-state.json"
}

write_file() {
  local path="$1"
  mkdir -p "$(dirname "$path")"
  cat > "$path"
}

copy_fixture() {
  local src="$1"
  local dst="$2"
  mkdir -p "$(dirname "$dst")"
  cp "$src" "$dst"
}

run_bootstrap_no_docs() {
  local repo="$REPO_DIR/bootstrap-no-docs"
  init_repo "$repo"
  run_status_refresh "$repo" "bootstrap without docs" "superpowers:brainstorming"
}

run_draft_spec() {
  local repo="$REPO_DIR/draft-spec"
  local spec_path="$repo/docs/superpowers/specs/2026-03-17-draft-spec-design.md"
  init_repo "$repo"

  write_file "$spec_path" <<'EOF'
# Draft Spec

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming

## Notes
EOF
  run_status_refresh "$repo" "draft spec" "superpowers:plan-ceo-review"
}

run_approved_spec_no_plan() {
  local repo="$REPO_DIR/approved-spec-no-plan"
  init_repo "$repo"
  copy_fixture \
    "$WORKFLOW_FIXTURE_DIR/specs/2026-01-22-document-review-system-design.md" \
    "$repo/docs/superpowers/specs/2026-01-22-document-review-system-design.md"
  run_status_refresh "$repo" "approved spec with no plan" "superpowers:writing-plans"
}

run_draft_plan() {
  local repo="$REPO_DIR/draft-plan"
  local spec_path="$repo/docs/superpowers/specs/2026-01-22-document-review-system-design.md"
  local plan_path="$repo/docs/superpowers/plans/2026-01-22-document-review-system.md"

  init_repo "$repo"
  copy_fixture \
    "$WORKFLOW_FIXTURE_DIR/specs/2026-01-22-document-review-system-design.md" \
    "$spec_path"
  write_file "$plan_path" <<'EOF'
# Draft Plan

**Workflow State:** Draft
**Source Spec:** `docs/superpowers/specs/2026-01-22-document-review-system-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** writing-plans
EOF
  run_status_refresh "$repo" "draft plan" "superpowers:plan-eng-review"
}

run_stale_approved_plan() {
  local repo="$REPO_DIR/stale-approved-plan"
  local spec_path="$repo/docs/superpowers/specs/2026-01-22-document-review-system-design-v2.md"
  local plan_path="$repo/docs/superpowers/plans/2026-01-22-document-review-system.md"
  init_repo "$repo"

  write_file "$spec_path" <<'EOF'
# Approved Spec, Stale Revision

**Workflow State:** CEO Approved
**Spec Revision:** 2
**Last Reviewed By:** plan-ceo-review

## Notes
EOF
  write_file "$plan_path" <<'EOF'
# Approved Plan, Stale Source Revision

**Workflow State:** Engineering Approved
**Source Spec:** `docs/superpowers/specs/2026-01-22-document-review-system-design-v2.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review
EOF
  run_status_refresh "$repo" "stale approved plan" "superpowers:writing-plans"
}

run_bounded_refresh() {
  local repo="$REPO_DIR/bounded-refresh"
  local old_spec="$repo/docs/superpowers/specs/2026-03-16-approved-design.md"
  local newest_spec="$repo/docs/superpowers/specs/2026-03-17-newest-draft-design.md"
  init_repo "$repo"

  write_file "$old_spec" <<'EOF'
# Older Approved Spec

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review
EOF
  sleep 1
  write_file "$newest_spec" <<'EOF'
# Newest Draft Spec

**Workflow State:** Draft
**Spec Revision:** 2
**Last Reviewed By:** brainstorming
EOF

  local output
  output="$(run_status_refresh_with_env \
    "$repo" \
    "bounded refresh" \
    "superpowers:plan-ceo-review" \
    "SUPERPOWERS_WORKFLOW_STATUS_FALLBACK_LIMIT=1")"
  assert_contains "$output" "2026-03-17-newest-draft-design.md" "bounded refresh candidate selection"
}

run_expected_path_survives_refresh() {
  local repo="$REPO_DIR/expected-path-survives-refresh"
  local expected_spec_path="docs/superpowers/specs/2026-03-17-missing-spec-design.md"
  local output

  init_repo "$repo"
  run_command_succeeds "$repo" "set expected missing spec path" expect --artifact spec --path "$expected_spec_path" >/dev/null
  output="$(run_status_refresh "$repo" "missing expected spec survives refresh" "superpowers:brainstorming")"
  assert_contains "$output" "$expected_spec_path" "expected missing spec path survives refresh"
}

run_ambiguous_fallback_discovery() {
  local repo="$REPO_DIR/ambiguous-fallback"
  local spec_a="$repo/docs/superpowers/specs/2026-03-17-spec-a.md"
  local spec_b="$repo/docs/superpowers/specs/2026-03-17-spec-b.md"
  local output

  init_repo "$repo"
  write_file "$spec_a" <<'EOF'
# Ambiguous Spec A

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming
EOF
  write_file "$spec_b" <<'EOF'
# Ambiguous Spec B

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review
EOF

  output="$(run_status_refresh_with_env \
    "$repo" \
    "ambiguous fallback" \
    "superpowers:plan-ceo-review" \
    "SUPERPOWERS_WORKFLOW_STATUS_FALLBACK_LIMIT=5")"
  assert_contains "$output" "ambigu" "ambiguous fallback note"
}

run_corrupted_manifest() {
  local repo="$REPO_DIR/corrupted-manifest"
  local manifest_path
  local manifest_dir
  local backups_before
  local backups_after

  init_repo "$repo"
  run_status_refresh "$repo" "manifest bootstrap" "superpowers:brainstorming"

  manifest_path="$(manifest_path_for_branch "$repo")"
  manifest_dir="$(dirname "$manifest_path")"
  backups_before="$(find "$manifest_dir" -maxdepth 1 -name '*.corrupt-*' | wc -l | tr -d ' ')"
  printf '%s\n' '{ "bad": "json"' > "$manifest_path"

  local output
  local status=0
  output="$(cd "$repo" && "$STATUS_BIN" status --refresh 2>&1)" || status=$?
  if [[ $status -ne 0 ]]; then
    echo "Expected corrupted manifest to be rescued"
    printf '%s\n' "$output"
    exit 1
  fi
  assert_contains "$output" "superpowers:brainstorming" "corrupted manifest route"
  if [[ "$output" != *"warning"* && "$output" != *"corrupt"* ]]; then
    echo "Expected corrupted manifest warning in output"
    printf '%s\n' "$output"
    exit 1
  fi

  if [[ ! -e "$manifest_path" ]]; then
    echo "Expected manifest file to be rebuilt after corruption"
    exit 1
  fi
  backups_after="$(find "$manifest_dir" -maxdepth 1 -name '*.corrupt-*' | wc -l | tr -d ' ')"
  if (( backups_after <= backups_before )); then
    echo "Expected a corrupted manifest backup file to be created"
    echo "Backup count before: $backups_before"
    echo "Backup count after:  $backups_after"
    exit 1
  fi
}

run_single_retry_conflict() {
  local repo="$REPO_DIR/single-retry-conflict"
  local manifest_path
  local manifest_dir
  local output
  local status=0
  local retry_count

  init_repo "$repo"
  run_status_refresh "$repo" "single retry bootstrap" "superpowers:brainstorming" >/dev/null

  manifest_path="$(manifest_path_for_branch "$repo")"
  manifest_dir="$(dirname "$manifest_path")"
  chmod u-w "$manifest_dir"
  output="$(cd "$repo" && "$STATUS_BIN" status --refresh 2>&1)" || status=$?
  chmod u+w "$manifest_dir"

  if [[ $status -ne 0 ]]; then
    echo "Expected write conflict fallback to keep status command successful"
    printf '%s\n' "$output"
    exit 1
  fi

  assert_contains "$output" "retrying once" "single retry warning"
  assert_contains "$output" "manifest_write_conflict" "single retry conservative note"
  assert_contains "$output" "superpowers:brainstorming" "single retry conservative route"

  retry_count="$(printf '%s\n' "$output" | grep -o "retrying once" | wc -l | tr -d ' ')"
  if (( retry_count != 1 )); then
    echo "Expected exactly one retry attempt"
    printf '%s\n' "$output"
    exit 1
  fi
}

run_expect_sync_retry_conflict() {
  local repo="$REPO_DIR/expect-sync-retry-conflict"
  local manifest_path
  local manifest_dir
  local spec_path_rel="docs/superpowers/specs/2026-03-17-sync-spec.md"
  local spec_path_abs="$repo/$spec_path_rel"
  local output_expect
  local output_sync
  local status=0

  init_repo "$repo"
  write_file "$spec_path_abs" <<'EOF'
# Sync Spec

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming
EOF

  run_status_refresh "$repo" "expect/sync conflict bootstrap" "superpowers:plan-ceo-review" >/dev/null
  manifest_path="$(manifest_path_for_branch "$repo")"
  manifest_dir="$(dirname "$manifest_path")"
  chmod u-w "$manifest_dir"

  output_expect="$(cd "$repo" && "$STATUS_BIN" expect --artifact spec --path "$spec_path_rel" 2>&1)" || status=$?
  if [[ $status -ne 0 ]]; then
    chmod u+w "$manifest_dir"
    echo "Expected expect write-conflict fallback to succeed"
    printf '%s\n' "$output_expect"
    exit 1
  fi
  assert_contains "$output_expect" "retrying once" "expect write-conflict retry warning"
  assert_contains "$output_expect" "manifest_write_conflict" "expect write-conflict conservative note"
  assert_contains "$output_expect" "superpowers:brainstorming" "expect write-conflict conservative route"

  status=0
  output_sync="$(cd "$repo" && "$STATUS_BIN" sync --artifact spec --path "$spec_path_rel" 2>&1)" || status=$?
  chmod u+w "$manifest_dir"
  if [[ $status -ne 0 ]]; then
    echo "Expected sync write-conflict fallback to succeed"
    printf '%s\n' "$output_sync"
    exit 1
  fi
  assert_contains "$output_sync" "retrying once" "sync write-conflict retry warning"
  assert_contains "$output_sync" "manifest_write_conflict" "sync write-conflict conservative note"
  assert_contains "$output_sync" "superpowers:brainstorming" "sync write-conflict conservative route"
}

run_sync_missing_artifact_behavior() {
  local repo="$REPO_DIR/sync-missing-artifact"
  local missing_path="docs/superpowers/specs/2026-03-17-sync-missing-spec.md"
  local output

  init_repo "$repo"
  output="$(run_command_succeeds "$repo" "sync missing artifact" sync --artifact spec --path "$missing_path")"
  assert_contains "$output" "missing_artifact" "sync missing artifact note"
  assert_contains "$output" "superpowers:brainstorming" "sync missing artifact conservative route"
}

run_sync_preserves_manifest_missing_expectation() {
  local repo="$REPO_DIR/sync-preserves-manifest-missing"
  local missing_path="docs/superpowers/specs/2026-03-17-manifest-backed-missing-spec.md"
  local manifest_path
  local manifest_json
  local output

  init_repo "$repo"
  run_command_succeeds "$repo" "expect missing path for manifest-backed sync" \
    expect --artifact spec --path "$missing_path" >/dev/null

  output="$(run_command_succeeds "$repo" "sync uses manifest-backed missing path" sync --artifact spec)"
  assert_contains "$output" "missing_artifact" "manifest-backed sync missing artifact note"
  assert_contains "$output" "superpowers:brainstorming" "manifest-backed sync conservative route"
  assert_contains "$output" "$missing_path" "manifest-backed sync output preserves missing path"

  manifest_path="$(manifest_path_for_branch "$repo")"
  manifest_json="$(cat "$manifest_path")"
  assert_contains "$manifest_json" "$missing_path" "manifest-backed sync manifest preserves missing path"
}

run_sync_missing_plan_preserves_stage() {
  local repo_no_spec="$REPO_DIR/sync-missing-plan-no-spec"
  local repo_draft_spec="$REPO_DIR/sync-missing-plan-draft-spec"
  local repo_approved_spec="$REPO_DIR/sync-missing-plan-approved-spec"
  local missing_plan_path="docs/superpowers/plans/2026-03-17-missing-sync-plan.md"
  local output

  init_repo "$repo_no_spec"
  output="$(run_command_succeeds "$repo_no_spec" "sync missing plan without spec" sync --artifact plan --path "$missing_plan_path")"
  assert_contains "$output" "missing_artifact" "sync missing plan without spec note"
  assert_contains "$output" "superpowers:brainstorming" "sync missing plan without spec route"

  init_repo "$repo_draft_spec"
  write_file "$repo_draft_spec/docs/superpowers/specs/2026-03-17-sync-draft-spec.md" <<'EOF'
# Draft Spec For Missing Plan Sync

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming
EOF
  output="$(run_command_succeeds "$repo_draft_spec" "sync missing plan with draft spec" sync --artifact plan --path "$missing_plan_path")"
  assert_contains "$output" "missing_artifact" "sync missing plan with draft spec note"
  assert_contains "$output" "superpowers:plan-ceo-review" "sync missing plan with draft spec route"

  init_repo "$repo_approved_spec"
  write_file "$repo_approved_spec/docs/superpowers/specs/2026-03-17-sync-approved-spec.md" <<'EOF'
# Approved Spec For Missing Plan Sync

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review
EOF
  output="$(run_command_succeeds "$repo_approved_spec" "sync missing plan with approved spec" sync --artifact plan --path "$missing_plan_path")"
  assert_contains "$output" "missing_artifact" "sync missing plan with approved spec note"
  assert_contains "$output" "superpowers:writing-plans" "sync missing plan with approved spec route"
}

run_out_of_repo_expect() {
  local repo="$REPO_DIR/out-of-repo-path"
  local outside_path="$REPO_DIR/../../outside.md"
  printf 'outside path\n' > "$outside_path"

  init_repo "$repo"
  run_command_fails "$repo" "out-of-repo artifact" "Invalid" expect --artifact spec --path "$outside_path"
  run_command_fails "$repo" "out-of-repo sync artifact" "Invalid" sync --artifact plan --path "$outside_path"
}

run_branch_isolated_manifests() {
  local base_repo="$REPO_DIR/branch-isolation/base"
  local worktree_root="$REPO_DIR/branch-isolation/worktrees"
  local branch_a="$worktree_root/branch-a"
  local branch_b="$worktree_root/branch-b"
  local manifest_a
  local manifest_b

  mkdir -p "$worktree_root"
  init_repo "$base_repo" "https://example.com/example/workflow-status-repo.git"
  git -C "$base_repo" worktree add "$branch_a" -b user-branch-a >/dev/null 2>&1
  git -C "$base_repo" worktree add "$branch_b" -b user-branch-b >/dev/null 2>&1

  manifest_a="$(manifest_path_for_branch "$branch_a")"
  manifest_b="$(manifest_path_for_branch "$branch_b")"

  run_status_refresh "$branch_a" "branch-a independent manifest" "superpowers:brainstorming"
  run_status_refresh "$branch_b" "branch-b independent manifest" "superpowers:brainstorming"

  if [[ ! -f "$manifest_a" ]]; then
    echo "Expected branch A manifest to be written"
    exit 1
  fi
  if [[ ! -f "$manifest_b" ]]; then
    echo "Expected branch B manifest to be written"
    exit 1
  fi
  if [[ "$manifest_a" == "$manifest_b" ]]; then
    echo "Expected separate manifest paths for different branches"
    echo "branch-a: $manifest_a"
    echo "branch-b: $manifest_b"
    exit 1
  fi
}

require_helper

run_bootstrap_no_docs
run_draft_spec
run_approved_spec_no_plan
run_draft_plan
run_stale_approved_plan
run_bounded_refresh
run_expected_path_survives_refresh
run_ambiguous_fallback_discovery
run_corrupted_manifest
run_single_retry_conflict
run_expect_sync_retry_conflict
run_sync_missing_artifact_behavior
run_sync_preserves_manifest_missing_expectation
run_sync_missing_plan_preserves_stage
run_out_of_repo_expect
run_branch_isolated_manifests

echo "superpowers-workflow-status regression scaffold passed."
