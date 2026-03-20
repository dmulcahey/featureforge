#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PWSH_HELPER="$REPO_ROOT/bin/superpowers-install-runtime.ps1"

pwsh_bin="$(command -v pwsh || command -v powershell || true)"
if [[ -z "$pwsh_bin" ]]; then
  echo "Skipping staged runtime install PowerShell test: no pwsh or powershell binary found."
  exit 0
fi

require_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "Expected output to contain: $needle"
    printf '%s\n' "$haystack"
    exit 1
  fi
}

make_runtime_repo() {
  local dir="$1"
  local version="$2"
  local reviewer_suffix="${3:-$version}"

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
  printf '# reviewer %s\n' "$reviewer_suffix" > "$dir/agents/code-reviewer.md"
  printf 'name = "code-reviewer"\ndescription = "reviewer %s"\ndeveloper_instructions = """review %s"""\n' "$reviewer_suffix" "$reviewer_suffix" > "$dir/.codex/agents/code-reviewer.toml"
  printf '%s\n' "$version" > "$dir/VERSION"
  printf 'skill-%s\n' "$reviewer_suffix" > "$dir/skills/runtime-skill.txt"

  for helper in superpowers-config superpowers-workflow-status superpowers-plan-execution; do
    printf '// bundled %s %s\n' "$helper" "$reviewer_suffix" > "$dir/runtime/core-helpers/dist/$helper.cjs"
  done

  git -C "$dir" add VERSION bin agents .codex skills runtime
  git -C "$dir" commit -m "init-$version" >/dev/null 2>&1
}

if [[ ! -f "$PWSH_HELPER" ]]; then
  echo "Expected PowerShell staged runtime install helper to exist: $PWSH_HELPER"
  exit 1
fi

node_bin="$(command -v node || true)"
if [[ -z "$node_bin" ]]; then
  echo "Expected node to be available for staged runtime install PowerShell tests."
  exit 1
fi

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

source_repo="$tmp_root/source-runtime.git"
make_runtime_repo "$source_repo" "3.0.0" "pwsh-new"

home_dir="$tmp_root/home"
shared_root="$home_dir/.superpowers/install"
codex_root="$home_dir/.codex/superpowers"
copilot_root="$home_dir/.copilot/superpowers"
stage_root="$home_dir/.superpowers/stage-install"
mkdir -p "$home_dir/.codex/agents" "$home_dir/.copilot/agents"
make_runtime_repo "$shared_root" "2.0.0" "pwsh-old"
printf 'stale codex reviewer\n' > "$home_dir/.codex/agents/code-reviewer.toml"
printf 'stale copilot reviewer\n' > "$home_dir/.copilot/agents/code-reviewer.agent.md"

set +e
pwsh_output="$(
  HOME="$home_dir" \
  SUPERPOWERS_SHARED_ROOT="$shared_root" \
  SUPERPOWERS_CODEX_ROOT="$codex_root" \
  SUPERPOWERS_COPILOT_ROOT="$copilot_root" \
  SUPERPOWERS_REPO_URL="$source_repo" \
  SUPERPOWERS_INSTALL_STAGE_ROOT="$stage_root" \
  SUPERPOWERS_NODE_BIN="$node_bin" \
  SUPERPOWERS_INSTALL_RUNTIME_TEST_PLATFORM=windows \
  "$pwsh_bin" -NoLogo -NoProfile -Command "& '$PWSH_HELPER'" 2>&1
)"
pwsh_status=$?
set -e
if [[ "$pwsh_status" -ne 0 ]]; then
  echo "Expected PowerShell staged install helper to succeed."
  printf '%s\n' "$pwsh_output"
  exit 1
fi

require_contains "$pwsh_output" "Shared install ready at $shared_root"
require_contains "$pwsh_output" "GitHub Copilot next step:"

if [[ "$(tr -d '[:space:]' < "$shared_root/VERSION")" != "3.0.0" ]]; then
  echo "Expected PowerShell staged install helper to swap in the new shared checkout."
  exit 1
fi
if ! cmp -s "$shared_root/.codex/agents/code-reviewer.toml" "$home_dir/.codex/agents/code-reviewer.toml"; then
  echo "Expected already-present copied Codex agent file to be refreshed by the PowerShell staged install helper."
  exit 1
fi
if ! cmp -s "$shared_root/agents/code-reviewer.md" "$home_dir/.copilot/agents/code-reviewer.agent.md"; then
  echo "Expected already-present copied Copilot agent file to be refreshed by the PowerShell staged install helper."
  exit 1
fi
[[ ! -e "$stage_root" ]] || {
  echo "Expected PowerShell staged install helper to clean up $stage_root"
  exit 1
}

cat > "$tmp_root/failure-check.ps1" <<PS1
\$env:SUPERPOWERS_NODE_BIN = '$tmp_root/does-not-exist/node'
\$env:SUPERPOWERS_SHARED_ROOT = '$shared_root'
\$env:SUPERPOWERS_CODEX_ROOT = '$codex_root'
\$env:SUPERPOWERS_COPILOT_ROOT = '$copilot_root'
\$env:SUPERPOWERS_REPO_URL = '$source_repo'
\$env:SUPERPOWERS_INSTALL_STAGE_ROOT = '$stage_root'
& '$PWSH_HELPER'
Write-Output ('LASTEXIT=' + \$LASTEXITCODE)
if (\$LASTEXITCODE -ne 0) {
  Write-Output 'GATED_EXIT'
  exit \$LASTEXITCODE
}
Write-Output 'AFTER_HELPER'
PS1

set +e
failure_output="$("$pwsh_bin" -NoLogo -NoProfile -File "$tmp_root/failure-check.ps1" 2>&1)"
failure_status=$?
set -e
if [[ "$failure_status" -eq 0 ]]; then
  echo "Expected gated PowerShell staged install caller to fail closed when the helper fails."
  printf '%s\n' "$failure_output"
  exit 1
fi
require_contains "$failure_output" "RuntimeDependencyMissing"
require_contains "$failure_output" "LASTEXIT=1"
require_contains "$failure_output" "GATED_EXIT"
if [[ "$failure_output" == *"AFTER_HELPER"* ]]; then
  echo "Expected gated PowerShell staged install caller to stop before post-install steps."
  printf '%s\n' "$failure_output"
  exit 1
fi

echo "superpowers-install-runtime PowerShell regression test passed."
