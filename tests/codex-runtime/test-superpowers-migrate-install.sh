#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
MIGRATE_BIN="$REPO_ROOT/bin/superpowers-migrate-install"
INSTALL_BIN="$REPO_ROOT/bin/superpowers-install-runtime"

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
  [[ -f "$dir/runtime/core-helpers/dist/superpowers-config.cjs" ]] || {
    echo "Expected $dir to contain runtime/core-helpers/dist/superpowers-config.cjs"
    exit 1
  }
}

make_runtime_repo() {
  local dir="$1"
  local version="$2"

  git init "$dir" >/dev/null 2>&1
  git -C "$dir" config user.name "Superpowers Test"
  git -C "$dir" config user.email "superpowers-tests@example.com"

  mkdir -p \
    "$dir/bin" \
    "$dir/agents" \
    "$dir/.codex/agents" \
    "$dir/skills" \
    "$dir/runtime/core-helpers/dist"

  : > "$dir/bin/superpowers-update-check"
  chmod +x "$dir/bin/superpowers-update-check"
  : > "$dir/bin/superpowers-config"
  chmod +x "$dir/bin/superpowers-config"
  printf '# reviewer\n' > "$dir/agents/code-reviewer.md"
  printf 'name = "code-reviewer"\ndescription = "reviewer"\ndeveloper_instructions = """review"""\n' > "$dir/.codex/agents/code-reviewer.toml"
  printf '%s\n' "$version" > "$dir/VERSION"
  printf 'skill-%s\n' "$version" > "$dir/skills/runtime-skill.txt"

  for helper in superpowers-config superpowers-workflow-status superpowers-plan-execution; do
    printf '// bundled %s %s\n' "$helper" "$version" > "$dir/runtime/core-helpers/dist/$helper.cjs"
  done

  git -C "$dir" add VERSION bin agents .codex skills runtime
  git -C "$dir" commit -m "init-$version" >/dev/null 2>&1
}

run_migrate() {
  local home_dir="$1"
  local shared_root="$2"
  local codex_root="$3"
  local copilot_root="$4"
  local repo_url="$5"
  local log_path="$6"

  HOME="$home_dir" \
    SUPERPOWERS_SHARED_ROOT="$shared_root" \
    SUPERPOWERS_CODEX_ROOT="$codex_root" \
    SUPERPOWERS_COPILOT_ROOT="$copilot_root" \
    SUPERPOWERS_REPO_URL="$repo_url" \
    SUPERPOWERS_INSTALL_RUNTIME_TEST_LOG="$log_path" \
    "$MIGRATE_BIN"
}

[[ -x "$INSTALL_BIN" ]] || {
  echo "Expected staged install helper to exist: $INSTALL_BIN"
  exit 1
}

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

source_repo="$tmp_root/source-runtime.git"
make_runtime_repo "$source_repo" "2.0.0"

home_dir="$tmp_root/home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
delegate_log="$home_dir/delegate.log"
mkdir -p "$home_dir"

output="$(run_migrate "$home_dir" "$shared_root" "$codex_root" "$copilot_root" "$source_repo" "$delegate_log")"

[[ -f "$delegate_log" ]] || {
  echo "Expected migrate-install to delegate through superpowers-install-runtime"
  exit 1
}
require_valid_install "$shared_root"
require_contains "$output" "Shared install ready at $shared_root"
require_contains "$output" "Codex next step:"
require_contains "$output" "GitHub Copilot next step:"

echo "superpowers-migrate-install regression test passed."
