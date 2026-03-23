#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
HELPER_BIN="$REPO_ROOT/bin/superpowers-slug"
RUST_SUPERPOWERS_BIN="$REPO_ROOT/target/debug/superpowers"

if [[ ! -x "$HELPER_BIN" ]]; then
  echo "Expected helper to exist and be executable: $HELPER_BIN"
  exit 1
fi

ensure_rust_superpowers_bin() {
  if [[ -x "$RUST_SUPERPOWERS_BIN" ]]; then
    return 0
  fi

  source "$HOME/.cargo/env"
  (cd "$REPO_ROOT" && cargo build --quiet --bin superpowers >/dev/null)
}

repo_hash() {
  local value="$1"
  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$value" | shasum -a 256 | awk '{print substr($1, 1, 12)}'
    return
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$value" | sha256sum | awk '{print substr($1, 1, 12)}'
    return
  fi
  printf '%s' "$value" | cksum | awk '{print $1}'
}

make_repo() {
  local dir="$1"
  git init "$dir" >/dev/null 2>&1
  git -C "$dir" config user.name "Superpowers Test"
  git -C "$dir" config user.email "superpowers-tests@example.com"
  printf '# slug fixture\n' > "$dir/README.md"
  git -C "$dir" add README.md
  git -C "$dir" commit -m "init" >/dev/null 2>&1
}

run_helper() {
  local repo_dir="$1"
  (cd "$repo_dir" && "$HELPER_BIN")
}

run_rust_slug() {
  local repo_dir="$1"
  ensure_rust_superpowers_bin
  (cd "$repo_dir" && "$RUST_SUPERPOWERS_BIN" repo slug)
}

assert_equal() {
  local actual="$1"
  local expected="$2"
  local label="$3"
  if [[ "$actual" != "$expected" ]]; then
    echo "Unexpected $label"
    echo "Expected: $expected"
    echo "Actual:   $actual"
    exit 1
  fi
}

assert_parsed_identity_matches() {
  local output="$1"
  local expected_slug="$2"
  local expected_branch="$3"
  local label="$4"

  unset SLUG BRANCH
  eval "$output"
  assert_equal "$SLUG" "$expected_slug" "$label slug"
  assert_equal "$BRANCH" "$expected_branch" "$label branch"
  if [[ "$output" == *"SAFE_BRANCH"* ]]; then
    echo "Helper should not emit SAFE_BRANCH"
    printf '%s\n' "$output"
    exit 1
  fi
}

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

remote_repo="$tmp_root/remote-repo"
make_repo "$remote_repo"
git -C "$remote_repo" remote add origin "https://example.com/acme/slug-helper.git"
git -C "$remote_repo" checkout -b 'feature/$(shell)$branch' >/dev/null 2>&1
remote_output="$(run_helper "$remote_repo")"
rust_remote_output="$(run_rust_slug "$remote_repo")"
expected_remote_branch="$(printf '%s\n' 'feature/$(shell)$branch' | sed 's/[^[:alnum:]._-]/-/g')"
assert_parsed_identity_matches "$remote_output" "acme-slug-helper" "$expected_remote_branch" "helper remote"
assert_parsed_identity_matches "$rust_remote_output" "acme-slug-helper" "$expected_remote_branch" "canonical remote"

fallback_repo="$tmp_root/slug with 'quotes' and \$dollar and \$(cmd)"
make_repo "$fallback_repo"
git -C "$fallback_repo" checkout -b 'topic/$(weird)$branch' >/dev/null 2>&1
fallback_output="$(run_helper "$fallback_repo")"
rust_fallback_output="$(run_rust_slug "$fallback_repo")"
expected_hash="$(repo_hash "$(git -C "$fallback_repo" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$fallback_repo")")"
expected_slug="$(basename "$(git -C "$fallback_repo" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$fallback_repo")")-$expected_hash"
expected_fallback_branch="$(printf '%s\n' 'topic/$(weird)$branch' | sed 's/[^[:alnum:]._-]/-/g')"
assert_parsed_identity_matches "$fallback_output" "$expected_slug" "$expected_fallback_branch" "helper fallback"
assert_parsed_identity_matches "$rust_fallback_output" "$expected_slug" "$expected_fallback_branch" "canonical fallback"

detached_repo="$tmp_root/detached-repo"
make_repo "$detached_repo"
git -C "$detached_repo" checkout --detach HEAD >/dev/null 2>&1
detached_output="$(run_helper "$detached_repo")"
rust_detached_output="$(run_rust_slug "$detached_repo")"
expected_hash="$(repo_hash "$(git -C "$detached_repo" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$detached_repo")")"
expected_slug="$(basename "$(git -C "$detached_repo" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$detached_repo")")-$expected_hash"
assert_parsed_identity_matches "$detached_output" "$expected_slug" "current" "helper detached"
assert_parsed_identity_matches "$rust_detached_output" "$expected_slug" "current" "canonical detached"

branch_safe_repo="$tmp_root/branch-safe-repo"
make_repo "$branch_safe_repo"
git -C "$branch_safe_repo" checkout -b 'release.v1_2-3/needs-cleanup@now' >/dev/null 2>&1
branch_safe_output="$(run_helper "$branch_safe_repo")"
rust_branch_safe_output="$(run_rust_slug "$branch_safe_repo")"
expected_hash="$(repo_hash "$(git -C "$branch_safe_repo" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$branch_safe_repo")")"
expected_slug="$(basename "$(git -C "$branch_safe_repo" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$branch_safe_repo")")-$expected_hash"
assert_parsed_identity_matches "$branch_safe_output" "$expected_slug" "release.v1_2-3-needs-cleanup-now" "helper branch-safe"
helper_branch_safe_slug="$expected_slug"
assert_parsed_identity_matches "$rust_branch_safe_output" "$helper_branch_safe_slug" "release.v1_2-3-needs-cleanup-now" "canonical branch-safe"

echo "superpowers-slug helper contract passed."
