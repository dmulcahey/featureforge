#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMMON_SH="$REPO_ROOT/bin/superpowers-runtime-common.sh"
COMMON_PS1="$REPO_ROOT/bin/superpowers-runtime-common.ps1"
CONFIG_WRAPPER="$REPO_ROOT/bin/superpowers-config"
CONFIG_PWSH_WRAPPER="$REPO_ROOT/bin/superpowers-config.ps1"
DIST_CONFIG="$REPO_ROOT/runtime/core-helpers/dist/superpowers-config.cjs"

pwsh_bin="$(command -v pwsh || command -v powershell || true)"

require_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "Expected output to contain: $needle"
    printf '%s\n' "$haystack"
    exit 1
  fi
}

make_node_stub() {
  local path="$1"
  local version="$2"

  mkdir -p "$(dirname "$path")"
  cat > "$path" <<EOF
#!/usr/bin/env bash
if [[ "\${1:-}" == "--version" || "\${1:-}" == "-v" ]]; then
  printf '%s\n' '$version'
  exit 0
fi
exec "${NODE_BIN:-$(command -v node)}" "\$@"
EOF
  chmod +x "$path"
}

if [[ ! -f "$COMMON_SH" ]]; then
  echo "Expected shared shell runtime launcher to exist: $COMMON_SH"
  exit 1
fi

if [[ ! -f "$COMMON_PS1" ]]; then
  echo "Expected shared PowerShell runtime launcher to exist: $COMMON_PS1"
  exit 1
fi

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

state_dir="$tmp_root/state"
mkdir -p "$state_dir"

set +e
missing_output="$(
  SUPERPOWERS_NODE_BIN="$tmp_root/does-not-exist/node" \
  SUPERPOWERS_STATE_DIR="$state_dir" \
  "$CONFIG_WRAPPER" get update_check 2>&1
)"
missing_status=$?
set -e
if [[ "$missing_status" -eq 0 ]]; then
  echo "Expected config wrapper to fail when Node is missing."
  exit 1
fi
require_contains "$missing_output" "RuntimeDependencyMissing"

node18_stub="$tmp_root/bin/node18"
make_node_stub "$node18_stub" "v18.20.0"
set +e
old_node_output="$(
  SUPERPOWERS_NODE_BIN="$node18_stub" \
  SUPERPOWERS_STATE_DIR="$state_dir" \
  "$CONFIG_WRAPPER" get update_check 2>&1
)"
old_node_status=$?
set -e
if [[ "$old_node_status" -eq 0 ]]; then
  echo "Expected config wrapper to fail when Node is too old."
  exit 1
fi
require_contains "$old_node_output" "RuntimeDependencyVersionUnsupported"

dist_backup="$tmp_root/superpowers-config.cjs.backup"
cp "$DIST_CONFIG" "$dist_backup"
mv "$DIST_CONFIG" "$DIST_CONFIG.missing"
set +e
missing_dist_output="$(
  SUPERPOWERS_STATE_DIR="$state_dir" \
  "$CONFIG_WRAPPER" get update_check 2>&1
)"
missing_dist_status=$?
set -e
mv "$DIST_CONFIG.missing" "$DIST_CONFIG"
if [[ "$missing_dist_status" -eq 0 ]]; then
  echo "Expected config wrapper to fail when the runtime bundle is missing."
  exit 1
fi
require_contains "$missing_dist_output" "RuntimeArtifactMissing"

cp "$DIST_CONFIG" "$tmp_root/superpowers-config.valid"
printf 'not valid javascript {\n' > "$DIST_CONFIG"
set +e
invalid_dist_output="$(
  SUPERPOWERS_STATE_DIR="$state_dir" \
  "$CONFIG_WRAPPER" get update_check 2>&1
)"
invalid_dist_status=$?
set -e
cp "$tmp_root/superpowers-config.valid" "$DIST_CONFIG"
if [[ "$invalid_dist_status" -eq 0 ]]; then
  echo "Expected config wrapper to fail when the runtime bundle is invalid."
  exit 1
fi
require_contains "$invalid_dist_output" "RuntimeArtifactInvalid"

(
  cd "$tmp_root"
  SUPERPOWERS_STATE_DIR="$state_dir" \
    "$CONFIG_WRAPPER" set update_check true >/dev/null
)
config_value="$(
  cd "$tmp_root"
  SUPERPOWERS_STATE_DIR="$state_dir" \
    "$CONFIG_WRAPPER" get update_check
)"
if [[ "$config_value" != "true" ]]; then
  echo "Expected runtime launcher to resolve the bundle relative to the install root instead of cwd."
  exit 1
fi

if [[ -n "$pwsh_bin" ]]; then
  bash_log="$tmp_root/bash.log"
  generic_dir="$tmp_root/generic"
  git_cmd_dir="$tmp_root/Git/cmd"
  git_bin_dir="$tmp_root/Git/bin"
  mkdir -p "$generic_dir" "$git_cmd_dir" "$git_bin_dir"

  cat > "$generic_dir/bash" <<'SH'
#!/bin/bash
exit 0
SH
  cat > "$git_cmd_dir/git" <<'SH'
#!/bin/bash
exit 0
SH
  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
printf 'bash wrapper invoked\n' >> "${SUPERPOWERS_TEST_BASH_LOG:?}"
exit 9
SH
  chmod +x "$generic_dir/bash" "$git_cmd_dir/git" "$git_bin_dir/bash.exe"

  pwsh_output="$(
    PATH="$generic_dir:$git_cmd_dir:$PATH" \
      SUPERPOWERS_TEST_BASH_LOG="$bash_log" \
      SUPERPOWERS_STATE_DIR="$state_dir" \
      "$pwsh_bin" -NoLogo -NoProfile -Command "& '$CONFIG_PWSH_WRAPPER' get update_check" 2>&1
  )"
  if [[ -s "$bash_log" ]]; then
    echo "Expected the migrated config PowerShell wrapper to launch Node directly instead of Git Bash."
    printf '%s\n' "$pwsh_output"
    exit 1
  fi
fi

echo "core helper runtime launch regression test passed."
