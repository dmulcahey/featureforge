#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
MIGRATE_BIN="$REPO_ROOT/bin/superpowers-migrate-install"
RUST_SUPERPOWERS_BIN="$REPO_ROOT/target/debug/superpowers"
REPO_SAFETY_BIN="$REPO_ROOT/bin/superpowers-repo-safety"

make_source_repo() {
  local dir="$1"
  git init "$dir" >/dev/null 2>&1
  git -C "$dir" config user.name "Superpowers Test"
  git -C "$dir" config user.email "superpowers-tests@example.com"
  mkdir -p "$dir/bin" "$dir/agents" "$dir/.codex/agents"
  : > "$dir/bin/superpowers-update-check"
  chmod +x "$dir/bin/superpowers-update-check"
  : > "$dir/bin/superpowers-config"
  chmod +x "$dir/bin/superpowers-config"
  printf '# reviewer\n' > "$dir/agents/code-reviewer.md"
  printf 'name = "code-reviewer"\ndescription = "reviewer"\ndeveloper_instructions = """review"""' > "$dir/.codex/agents/code-reviewer.toml"
  printf '1.0.0\n' > "$dir/VERSION"
  git -C "$dir" add VERSION bin/superpowers-update-check bin/superpowers-config agents/code-reviewer.md .codex/agents/code-reviewer.toml
  git -C "$dir" commit -m "init" >/dev/null 2>&1
}

add_prebuilt_runtime_fixture() {
  local dir="$1"
  local target="$2"
  local binary_name="$3"
  local revision="$4"
  local contents="$5"
  local binary_path="$dir/bin/prebuilt/$target/$binary_name"
  local checksum

  mkdir -p "$(dirname "$binary_path")"
  printf '%s' "$contents" > "$binary_path"
  chmod +x "$binary_path"
  checksum="$(shasum -a 256 "$binary_path" | awk '{print $1}')"
  printf '%s  %s\n' "$checksum" "$binary_name" > "$dir/bin/prebuilt/$target/$binary_name.sha256"
  cat > "$dir/bin/prebuilt/manifest.json" <<EOF
{
  "runtime_revision": "$revision",
  "targets": {
    "$target": {
      "binary_path": "bin/prebuilt/$target/$binary_name",
      "checksum_path": "bin/prebuilt/$target/$binary_name.sha256"
    }
  }
}
EOF
}

make_state_repo() {
  local dir="$1"
  local remote_url="$2"
  local branch="$3"

  git init "$dir" >/dev/null 2>&1
  git -C "$dir" config user.name "Superpowers Test"
  git -C "$dir" config user.email "superpowers-tests@example.com"
  printf '# state repo\n' > "$dir/README.md"
  git -C "$dir" add README.md
  git -C "$dir" commit -m "init" >/dev/null 2>&1
  git -C "$dir" checkout -B "$branch" >/dev/null 2>&1
  git -C "$dir" remote add origin "$remote_url"
}

make_install_repo() {
  local dir="$1"
  local version="$2"
  local commit_ts="${3:-}"
  git init "$dir" >/dev/null 2>&1
  git -C "$dir" config user.name "Superpowers Test"
  git -C "$dir" config user.email "superpowers-tests@example.com"
  mkdir -p "$dir/bin" "$dir/agents" "$dir/.codex/agents"
  : > "$dir/bin/superpowers-update-check"
  chmod +x "$dir/bin/superpowers-update-check"
  : > "$dir/bin/superpowers-config"
  chmod +x "$dir/bin/superpowers-config"
  printf '# reviewer\n' > "$dir/agents/code-reviewer.md"
  printf 'name = "code-reviewer"\ndescription = "reviewer"\ndeveloper_instructions = """review"""' > "$dir/.codex/agents/code-reviewer.toml"
  printf '%s\n' "$version" > "$dir/VERSION"
  git -C "$dir" add VERSION bin/superpowers-update-check bin/superpowers-config agents/code-reviewer.md .codex/agents/code-reviewer.toml
  if [[ -n "$commit_ts" ]]; then
    GIT_AUTHOR_DATE="@$commit_ts" GIT_COMMITTER_DATE="@$commit_ts" \
      git -C "$dir" commit -m "init-$version" >/dev/null 2>&1
  else
    git -C "$dir" commit -m "init-$version" >/dev/null 2>&1
  fi
}

run_migrate() {
  local home_dir="$1"
  local shared_root="$2"
  local codex_root="$3"
  local copilot_root="$4"
  local repo_url="$5"
  HOME="$home_dir" \
    SUPERPOWERS_SHARED_ROOT="$shared_root" \
    SUPERPOWERS_CODEX_ROOT="$codex_root" \
    SUPERPOWERS_COPILOT_ROOT="$copilot_root" \
    SUPERPOWERS_REPO_URL="$repo_url" \
    "$MIGRATE_BIN"
}

ensure_rust_superpowers_bin() {
  source "$HOME/.cargo/env"
  (cd "$REPO_ROOT" && cargo build --quiet --bin superpowers >/dev/null)
}

run_rust_migrate() {
  local home_dir="$1"
  local shared_root="$2"
  local codex_root="$3"
  local copilot_root="$4"
  local repo_url="$5"
  local host_target="${6:-}"

  ensure_rust_superpowers_bin
  HOME="$home_dir" \
    SUPERPOWERS_STATE_DIR="$home_dir/.superpowers" \
    SUPERPOWERS_SHARED_ROOT="$shared_root" \
    SUPERPOWERS_CODEX_ROOT="$codex_root" \
    SUPERPOWERS_COPILOT_ROOT="$copilot_root" \
    SUPERPOWERS_REPO_URL="$repo_url" \
    SUPERPOWERS_MIGRATE_STAMP="20260323-140000" \
    SUPERPOWERS_HOST_TARGET="$host_target" \
    "$RUST_SUPERPOWERS_BIN" install migrate
}

require_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "Expected output to contain: $needle"
    printf '%s\n' "$haystack"
    exit 1
  fi
}

require_valid_install() {
  local dir="$1"
  [[ -x "$dir/bin/superpowers-update-check" ]] || {
    echo "Expected $dir to contain bin/superpowers-update-check"
    exit 1
  }
  [[ -x "$dir/bin/superpowers-config" ]] || {
    echo "Expected $dir to contain bin/superpowers-config"
    exit 1
  }
  [[ -f "$dir/VERSION" ]] || {
    echo "Expected $dir to contain VERSION"
    exit 1
  }
  [[ -f "$dir/agents/code-reviewer.md" ]] || {
    echo "Expected $dir to contain agents/code-reviewer.md"
    exit 1
  }
  [[ -f "$dir/.codex/agents/code-reviewer.toml" ]] || {
    echo "Expected $dir to contain .codex/agents/code-reviewer.toml"
    exit 1
  }
}

make_legacy_install_without_config() {
  local dir="$1"
  local version="$2"
  git init "$dir" >/dev/null 2>&1
  git -C "$dir" config user.name "Superpowers Test"
  git -C "$dir" config user.email "superpowers-tests@example.com"
  mkdir -p "$dir/bin"
  : > "$dir/bin/superpowers-update-check"
  chmod +x "$dir/bin/superpowers-update-check"
  printf '%s\n' "$version" > "$dir/VERSION"
  git -C "$dir" add VERSION bin/superpowers-update-check
  git -C "$dir" commit -m "legacy-$version" >/dev/null 2>&1
}

make_legacy_install_without_reviewers() {
  local dir="$1"
  local version="$2"
  git init "$dir" >/dev/null 2>&1
  git -C "$dir" config user.name "Superpowers Test"
  git -C "$dir" config user.email "superpowers-tests@example.com"
  mkdir -p "$dir/bin"
  : > "$dir/bin/superpowers-update-check"
  chmod +x "$dir/bin/superpowers-update-check"
  : > "$dir/bin/superpowers-config"
  chmod +x "$dir/bin/superpowers-config"
  printf '%s\n' "$version" > "$dir/VERSION"
  git -C "$dir" add VERSION bin/superpowers-update-check bin/superpowers-config
  git -C "$dir" commit -m "legacy-no-reviewers-$version" >/dev/null 2>&1
}

require_link_target() {
  local path="$1"
  local target="$2"
  if [[ ! -L "$path" ]]; then
    echo "Expected $path to be a symlink"
    exit 1
  fi
  local resolved
  resolved="$(cd "$path/.." && cd "$(dirname "$(readlink "$path")")" && pwd -P)/$(basename "$(readlink "$path")")"
  local expected
  expected="$(cd "$target" && pwd -P)"
  if [[ "$resolved" != "$expected" ]]; then
    echo "Expected $path to point to $expected, got $resolved"
    exit 1
  fi
}

current_user_name() {
  if [[ -n "${USER:-}" ]]; then
    printf '%s\n' "$USER"
  elif [[ -n "${USERNAME:-}" ]]; then
    printf '%s\n' "$USERNAME"
  else
    printf '%s\n' "user"
  fi
}

repo_slug_from_remote() {
  local remote_url="$1"
  remote_url="${remote_url%.git}"
  printf '%s\n' "$remote_url" | awk -F/ '{print $(NF-1) "-" $NF}'
}

task_hash() {
  local stage="$1"
  local task_id="$2"
  printf '%s\n%s' "$stage" "$task_id" | shasum -a 256 | awk '{print substr($1,1,16)}'
}

canonical_approval_path() {
  local state_dir="$1"
  local remote_url="$2"
  local branch="$3"
  local stage="$4"
  local task_id="$5"
  local slug user hash

  slug="$(repo_slug_from_remote "$remote_url")"
  user="$(current_user_name)"
  hash="$(task_hash "$stage" "$task_id")"
  printf '%s\n' "$state_dir/repo-safety/approvals/$slug/$user-$branch/$hash.json"
}

legacy_approval_path() {
  local state_dir="$1"
  local remote_url="$2"
  local branch="$3"
  local stage="$4"
  local task_id="$5"
  local slug user hash

  slug="$(repo_slug_from_remote "$remote_url")"
  user="$(current_user_name)"
  hash="$(task_hash "$stage" "$task_id")"
  printf '%s\n' "$state_dir/projects/$slug/$user-$branch-repo-safety/$hash.json"
}

seed_legacy_approval() {
  local repo_dir="$1"
  local state_dir="$2"
  local stage="$3"
  local task_id="$4"
  local branch
  local remote_url
  local canonical_approval
  local legacy_approval

  branch="$(git -C "$repo_dir" rev-parse --abbrev-ref HEAD)"
  remote_url="$(git -C "$repo_dir" remote get-url origin)"
  (
    cd "$repo_dir"
    SUPERPOWERS_STATE_DIR="$state_dir" \
      "$REPO_SAFETY_BIN" approve \
        --stage "$stage" \
        --task-id "$task_id" \
        --reason "User explicitly approved this write." \
        --path docs/superpowers/specs/example.md \
        --write-target execution-task-slice \
        >/dev/null
  )

  canonical_approval="$(canonical_approval_path "$state_dir" "$remote_url" "$branch" "$stage" "$task_id")"
  legacy_approval="$(legacy_approval_path "$state_dir" "$remote_url" "$branch" "$stage" "$task_id")"
  mkdir -p "$(dirname "$legacy_approval")"
  cp "$canonical_approval" "$legacy_approval"
  rm -f "$canonical_approval"
}

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

source_repo="$tmp_root/source.git"
make_source_repo "$source_repo"
add_prebuilt_runtime_fixture "$source_repo" "darwin-arm64" "superpowers" "1.0.0-test" $'#!/bin/sh\necho darwin-runtime\n'

home_dir="$tmp_root/fresh-home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
mkdir -p "$home_dir"
fresh_output="$(run_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo")"
require_valid_install "$shared_root"
[[ ! -e "$codex_root" ]] || {
  echo "Expected no legacy Codex root for fresh install"
  exit 1
}
[[ ! -e "$copilot_root" ]] || {
  echo "Expected no legacy Copilot root for fresh install"
  exit 1
}
require_contains "$fresh_output" "Codex next step:"
require_contains "$fresh_output" "~/.agents/skills/superpowers"
require_contains "$fresh_output" "~/.codex/agents/code-reviewer.toml"
require_contains "$fresh_output" "GitHub Copilot next step:"
require_contains "$fresh_output" "~/.copilot/skills/superpowers"
require_contains "$fresh_output" "code-reviewer.agent.md"

home_dir="$tmp_root/codex-only-home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
mkdir -p "$(dirname "$codex_root")"
make_install_repo "$codex_root" "2.0.0"
run_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo" >/dev/null
require_valid_install "$shared_root"
require_link_target "$codex_root" "$shared_root"
[[ ! -e "$copilot_root" ]] || {
  echo "Expected untouched missing Copilot root when only Codex existed"
  exit 1
}

home_dir="$tmp_root/copilot-only-home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
mkdir -p "$(dirname "$copilot_root")"
make_install_repo "$copilot_root" "3.0.0"
copilot_output="$(run_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo")"
require_valid_install "$shared_root"
require_link_target "$copilot_root" "$shared_root"
[[ ! -e "$codex_root" ]] || {
  echo "Expected untouched missing Codex root when only Copilot existed"
  exit 1
}
require_contains "$copilot_output" "GitHub Copilot next step:"
require_contains "$copilot_output" "~/.copilot/agents/code-reviewer.agent.md"
require_contains "$copilot_output" "copy on Windows; symlink on Unix-like installs"

home_dir="$tmp_root/legacy-missing-config-home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
mkdir -p "$(dirname "$codex_root")"
make_legacy_install_without_config "$codex_root" "4.9.0"
legacy_missing_output="$(run_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo")"
require_valid_install "$shared_root"
if [[ "$(tr -d '[:space:]' < "$shared_root/VERSION")" != "1.0.0" ]]; then
  echo "Expected invalid legacy installs without superpowers-config to be replaced by a fresh shared clone"
  exit 1
fi
require_link_target "$codex_root" "$shared_root"
legacy_backup_count="$(find "$(dirname "$codex_root")" -maxdepth 1 -name 'superpowers.backup-*' | wc -l | tr -d ' ')"
if [[ "$legacy_backup_count" -lt 1 ]]; then
  echo "Expected invalid legacy install without superpowers-config to be backed up"
  exit 1
fi
require_contains "$legacy_missing_output" "Cloned shared install to $shared_root"
require_contains "$legacy_missing_output" "Backed up legacy install at $codex_root"

home_dir="$tmp_root/legacy-missing-reviewers-home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
mkdir -p "$(dirname "$codex_root")"
make_legacy_install_without_reviewers "$codex_root" "4.9.1"
legacy_missing_reviewers_output="$(run_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo")"
require_valid_install "$shared_root"
if [[ "$(tr -d '[:space:]' < "$shared_root/VERSION")" != "1.0.0" ]]; then
  echo "Expected legacy installs missing reviewer artifacts to be replaced by a fresh shared clone"
  exit 1
fi
require_link_target "$codex_root" "$shared_root"
legacy_reviewers_backup_count="$(find "$(dirname "$codex_root")" -maxdepth 1 -name 'superpowers.backup-*' | wc -l | tr -d ' ')"
if [[ "$legacy_reviewers_backup_count" -lt 1 ]]; then
  echo "Expected invalid legacy install missing reviewer artifacts to be backed up"
  exit 1
fi
require_contains "$legacy_missing_reviewers_output" "Cloned shared install to $shared_root"
require_contains "$legacy_missing_reviewers_output" "Backed up legacy install at $codex_root"

home_dir="$tmp_root/dual-home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
mkdir -p "$(dirname "$codex_root")" "$(dirname "$copilot_root")"
make_install_repo "$codex_root" "4.0.0" "1700000000"
make_install_repo "$copilot_root" "5.0.0" "1700000100"
run_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo" >/dev/null
require_valid_install "$shared_root"
if [[ "$(tr -d '[:space:]' < "$shared_root/VERSION")" != "5.0.0" ]]; then
  echo "Expected newer Copilot checkout to win dual-root migration"
  exit 1
fi
require_link_target "$codex_root" "$shared_root"
require_link_target "$copilot_root" "$shared_root"
backup_count="$(find "$(dirname "$codex_root")" -maxdepth 1 -name 'superpowers.backup-*' | wc -l | tr -d ' ')"
if [[ "$backup_count" -lt 1 ]]; then
  echo "Expected non-selected legacy checkout to be backed up"
  exit 1
fi

home_dir="$tmp_root/ambiguous-home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
mkdir -p "$(dirname "$codex_root")" "$(dirname "$copilot_root")"
make_install_repo "$codex_root" "6.0.0" "1700000200"
make_install_repo "$copilot_root" "7.0.0" "1700000200"
set +e
ambiguous_output="$(run_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo" 2>&1)"
ambiguous_status=$?
set -e
if [[ "$ambiguous_status" -eq 0 ]]; then
  echo "Expected ambiguous dual-root migration to fail"
  exit 1
fi
if [[ "$ambiguous_output" != *"manual reconciliation"* ]]; then
  echo "Expected ambiguous migration failure to mention manual reconciliation"
  printf '%s\n' "$ambiguous_output"
  exit 1
fi

echo "superpowers-migrate-install regression test passed."

home_dir="$tmp_root/canonical-rust-home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
state_dir="$home_dir/.superpowers"
mkdir -p "$home_dir" "$state_dir" "$(dirname "$codex_root")"
make_install_repo "$codex_root" "4.9.0"
cat > "$state_dir/config.yaml" <<'EOF'
update_check: false
EOF
state_repo="$tmp_root/install-state-repo"
remote_url="https://example.com/acme/install-migrate.git"
make_state_repo "$state_repo" "$remote_url" "main"
seed_legacy_approval "$state_repo" "$state_dir" "superpowers:executing-plans" "task-7"
canonical_output="$(run_rust_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo" "darwin-arm64")"
require_valid_install "$shared_root"
require_link_target "$codex_root" "$shared_root"
if [[ ! -f "$shared_root/bin/superpowers" ]]; then
  echo "Expected canonical Rust install migrate to provision $shared_root/bin/superpowers from the checked-in manifest"
  exit 1
fi
if [[ "$(cat "$shared_root/bin/superpowers")" != $'#!/bin/sh\necho darwin-runtime' ]]; then
  echo "Expected canonical Rust install migrate to copy the manifest-selected runtime contents into $shared_root/bin/superpowers"
  exit 1
fi
if [[ ! -f "$state_dir/config/config.yaml" ]]; then
  echo "Expected canonical Rust install migrate to create $state_dir/config/config.yaml"
  exit 1
fi
if [[ ! -f "$state_dir/config.yaml.bak" ]]; then
  echo "Expected canonical Rust install migrate to back up the legacy config"
  exit 1
fi
canonical_approval="$(canonical_approval_path "$state_dir" "$remote_url" "main" "superpowers:executing-plans" "task-7")"
if [[ ! -f "$canonical_approval" ]]; then
  echo "Expected canonical Rust install migrate to rewrite the legacy approval into $canonical_approval"
  exit 1
fi
require_contains "$canonical_output" "Migrated config"
require_contains "$canonical_output" "Migrated repo-safety approval"
require_contains "$canonical_output" "Shared install ready"
require_contains "$canonical_output" "Provisioned checked-in runtime"

echo "superpowers-migrate-install canonical Rust contract passed."
