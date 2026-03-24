#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
HELPER="$REPO_ROOT/bin/superpowers-pwsh-common.ps1"
PUBLIC_WORKFLOW_WRAPPER="$REPO_ROOT/bin/superpowers-workflow.ps1"
WORKFLOW_WRAPPER="$REPO_ROOT/bin/superpowers-workflow-status.ps1"
PLAN_EXEC_WRAPPER="$REPO_ROOT/bin/superpowers-plan-execution.ps1"
PLAN_CONTRACT_WRAPPER="$REPO_ROOT/bin/superpowers-plan-contract.ps1"
SESSION_ENTRY_WRAPPER="$REPO_ROOT/bin/superpowers-session-entry.ps1"
REPO_SAFETY_WRAPPER="$REPO_ROOT/bin/superpowers-repo-safety.ps1"
UPDATE_CHECK_WRAPPER="$REPO_ROOT/bin/superpowers-update-check.ps1"

pwsh_bin="$(command -v pwsh || command -v powershell || true)"
if [[ -z "$pwsh_bin" ]]; then
  echo "Skipping PowerShell wrapper bash-resolution test: no pwsh or powershell binary found."
  exit 0
fi

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

generic_dir="$tmp_root/generic"
git_cmd_dir="$tmp_root/Git/cmd"
git_bin_dir="$tmp_root/Git/bin"
override_dir="$tmp_root/override"

mkdir -p "$generic_dir" "$git_cmd_dir" "$git_bin_dir" "$override_dir"

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
exit 0
SH

cat > "$override_dir/bash" <<'SH'
#!/bin/bash
exit 0
SH

chmod +x "$generic_dir/bash" "$git_cmd_dir/git" "$git_bin_dir/bash.exe" "$override_dir/bash"

selected="$(
  PATH="$generic_dir:$git_cmd_dir:$PATH" \
    "$pwsh_bin" -NoLogo -NoProfile -Command ". '$HELPER'; Get-SuperpowersBashPath"
)"
if [[ "$selected" != "$git_bin_dir/bash.exe" ]]; then
  echo "Expected PowerShell helper to prefer Git Bash over a generic bash on PATH"
  echo "Actual selection: $selected"
  exit 1
fi

selected="$(
  PATH="$generic_dir:$git_cmd_dir:$PATH" \
    SUPERPOWERS_BASH_PATH="$override_dir/bash" \
    "$pwsh_bin" -NoLogo -NoProfile -Command ". '$HELPER'; Get-SuperpowersBashPath"
)"
if [[ "$selected" != "$override_dir/bash" ]]; then
  echo "Expected SUPERPOWERS_BASH_PATH to override wrapper bash resolution"
  echo "Actual selection: $selected"
  exit 1
fi

assert_wrapper_behavior() {
  local wrapper_path="$1"
  local command_name="$2"
  local output_payload="$3"
  local expected_output_fragment="$4"
  local expected_args_spec="$5"
  local bash_log="$tmp_root/${command_name}-wrapper-bash.log"
  local wrapper_output
  local wrapper_exit
  local expected_args=()
  local actual_arg
  local index

  if [[ ! -f "$wrapper_path" ]]; then
    echo "Expected ${command_name} PowerShell wrapper to exist: $wrapper_path"
    exit 1
  fi

  IFS='|' read -r -a expected_args <<< "$expected_args_spec"

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
set -euo pipefail

log_file="${SUPERPOWERS_TEST_BASH_LOG:?}"
: > "$log_file"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "$log_file"
done

printf '%s\n' "${SUPERPOWERS_TEST_OUTPUT:?}"
SH
  chmod +x "$git_bin_dir/bash.exe"

  wrapper_output="$(
    PATH="$generic_dir:$git_cmd_dir:$PATH" \
      SUPERPOWERS_TEST_BASH_LOG="$bash_log" \
      SUPERPOWERS_TEST_OUTPUT="$output_payload" \
      "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' status --plan docs/superpowers/plans/example.md"
  )"
  if [[ "$wrapper_output" != *"$expected_output_fragment"* ]]; then
    echo "Expected ${command_name} wrapper to preserve raw transport output"
    echo "Actual output: $wrapper_output"
    exit 1
  fi
  if [[ "$wrapper_output" == *'C:\\tmp\\workspace'* ]]; then
    echo "Expected ${command_name} wrapper to avoid rewriting JSON paths"
    echo "Actual output: $wrapper_output"
    exit 1
  fi

  for index in "${!expected_args[@]}"; do
    actual_arg="$(sed -n "$((index + 1))p" "$bash_log")"
    if [[ $index -eq 0 ]]; then
      if [[ "$actual_arg" != *"${expected_args[$index]}" ]]; then
        echo "Expected ${command_name} wrapper to invoke the canonical compat launcher"
        echo "Actual first arg: $actual_arg"
        exit 1
      fi
    elif [[ "$actual_arg" != "${expected_args[$index]}" ]]; then
      echo "Expected ${command_name} wrapper to forward canonical subcommands unchanged"
      echo "Actual args:"
      cat "$bash_log"
      exit 1
    fi
  done

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
exit 7
SH
  chmod +x "$git_bin_dir/bash.exe"

  set +e
  PATH="$generic_dir:$git_cmd_dir:$PATH" \
    "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' status --plan docs/superpowers/plans/example.md"
  wrapper_exit=$?
  set -e

  if [[ $wrapper_exit -ne 7 ]]; then
    echo "Expected ${command_name} wrapper to preserve nonzero bash exit code"
    echo "Expected: 7"
    echo "Actual:   $wrapper_exit"
    exit 1
  fi
}

assert_public_workflow_wrapper_behavior() {
  local wrapper_path="$1"
  local bash_log="$tmp_root/public-workflow-wrapper-bash.log"
  local wrapper_output
  local first_arg
  local second_arg
  local third_arg
  local wrapper_exit

  if [[ ! -f "$wrapper_path" ]]; then
    echo "Expected public workflow PowerShell wrapper to exist: $wrapper_path"
    exit 1
  fi

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
set -euo pipefail

log_file="${SUPERPOWERS_TEST_BASH_LOG:?}"
: > "$log_file"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "$log_file"
done

printf 'Workflow status: Brainstorming needed\n'
printf 'Why: No current workflow artifacts are available yet.\n'
printf 'Next: Use superpowers:brainstorming\n'
SH
  chmod +x "$git_bin_dir/bash.exe"

  wrapper_output="$(
    PATH="$generic_dir:$git_cmd_dir:$PATH" \
      SUPERPOWERS_TEST_BASH_LOG="$bash_log" \
      "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' status"
  )"
  if [[ "$wrapper_output" != *"Workflow status: Brainstorming needed"* ]]; then
    echo "Expected public workflow wrapper to preserve human workflow output"
    echo "Actual output: $wrapper_output"
    exit 1
  fi
  if [[ "$wrapper_output" == *'"root":"'* ]]; then
    echo "Expected public workflow wrapper to avoid JSON path conversion for human output"
    echo "Actual output: $wrapper_output"
    exit 1
  fi

  first_arg="$(sed -n '1p' "$bash_log")"
  second_arg="$(sed -n '2p' "$bash_log")"
  third_arg="$(sed -n '3p' "$bash_log")"
  if [[ "$first_arg" != *"/compat/bash/superpowers" ]]; then
    echo "Expected public workflow wrapper to invoke Git Bash with the canonical compat launcher"
    echo "Actual first arg: $first_arg"
    exit 1
  fi
  if [[ "$second_arg" != "workflow" || "$third_arg" != "status" ]]; then
    echo "Expected public workflow wrapper to forward the canonical workflow status command"
    echo "Actual args:"
    cat "$bash_log"
    exit 1
  fi

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
printf 'Workflow inspection failed: Read-only workflow resolution requires a git repo.\n'
printf 'Debug:\n- failure_class=RepoContextUnavailable\n'
exit 9
SH
  chmod +x "$git_bin_dir/bash.exe"

  set +e
  wrapper_output="$(
    PATH="$generic_dir:$git_cmd_dir:$PATH" \
      "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' status --debug"
  )"
  wrapper_exit=$?
  set -e

  if [[ $wrapper_exit -ne 9 ]]; then
    echo "Expected public workflow wrapper to preserve nonzero bash exit code"
    echo "Expected: 9"
    echo "Actual:   $wrapper_exit"
    exit 1
  fi
  if [[ "$wrapper_output" != *"Workflow inspection failed: Read-only workflow resolution requires a git repo."* ]]; then
    echo "Expected public workflow wrapper to preserve failure output"
    echo "Actual output: $wrapper_output"
    exit 1
  fi
  if [[ "$wrapper_output" != *"failure_class=RepoContextUnavailable"* ]]; then
    echo "Expected public workflow wrapper to preserve debug diagnostics on failure"
    echo "Actual output: $wrapper_output"
    exit 1
  fi
}

assert_update_check_wrapper_behavior() {
  local wrapper_path="$1"
  local bash_log="$tmp_root/update-check-wrapper-bash.log"
  local wrapper_exit
  local first_arg
  local second_arg
  local third_arg

  if [[ ! -f "$wrapper_path" ]]; then
    echo "Expected update-check PowerShell wrapper to exist: $wrapper_path"
    exit 1
  fi

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
set -euo pipefail

log_file="${SUPERPOWERS_TEST_BASH_LOG:?}"
: > "$log_file"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "$log_file"
done

exit 0
SH
  chmod +x "$git_bin_dir/bash.exe"

  PATH="$generic_dir:$git_cmd_dir:$PATH" \
    SUPERPOWERS_TEST_BASH_LOG="$bash_log" \
    "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' --force" >/dev/null
  wrapper_exit=$?

  if [[ $wrapper_exit -ne 0 ]]; then
    echo "Expected update-check wrapper to preserve zero bash exit code"
    echo "Actual: $wrapper_exit"
    exit 1
  fi

  first_arg="$(sed -n '1p' "$bash_log")"
  second_arg="$(sed -n '2p' "$bash_log")"
  third_arg="$(sed -n '3p' "$bash_log")"
  if [[ "$first_arg" != *"/compat/bash/superpowers" ]]; then
    echo "Expected update-check wrapper to invoke Git Bash with the canonical compat launcher"
    echo "Actual first arg: $first_arg"
    exit 1
  fi
  if [[ "$second_arg" != "update-check" || "$third_arg" != "--force" ]]; then
    echo "Expected update-check wrapper to forward the canonical update-check command"
    echo "Actual args:"
    cat "$bash_log"
    exit 1
  fi
}

assert_session_entry_wrapper_behavior() {
  local wrapper_path="$1"
  local bash_log="$tmp_root/session-entry-wrapper-bash.log"
  local wrapper_output
  local first_arg
  local second_arg
  local third_arg
  local fourth_arg
  local wrapper_exit

  if [[ ! -f "$wrapper_path" ]]; then
    echo "Expected session-entry PowerShell wrapper to exist: $wrapper_path"
    exit 1
  fi

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
set -euo pipefail

log_file="${SUPERPOWERS_TEST_BASH_LOG:?}"
: > "$log_file"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "$log_file"
done

printf '{"outcome":"needs_user_choice","decision_path":"/c/tmp/state/session-entry/using-superpowers/session-123"}\n'
SH
  chmod +x "$git_bin_dir/bash.exe"

  wrapper_output="$(
    PATH="$generic_dir:$git_cmd_dir:$PATH" \
      SUPERPOWERS_TEST_BASH_LOG="$bash_log" \
      "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' resolve --message-file transcript.md"
  )"
  if [[ "$wrapper_output" != *'"/c/tmp/state/session-entry/using-superpowers/session-123"'* ]]; then
    echo "Expected session-entry wrapper to preserve raw JSON paths"
    echo "Actual output: $wrapper_output"
    exit 1
  fi

  first_arg="$(sed -n '1p' "$bash_log")"
  second_arg="$(sed -n '2p' "$bash_log")"
  third_arg="$(sed -n '3p' "$bash_log")"
  fourth_arg="$(sed -n '4p' "$bash_log")"
  if [[ "$first_arg" != *"/compat/bash/superpowers" ]]; then
    echo "Expected session-entry wrapper to invoke Git Bash with the canonical compat launcher"
    echo "Actual first arg: $first_arg"
    exit 1
  fi
  if [[ "$second_arg" != "session-entry" || "$third_arg" != "resolve" || "$fourth_arg" != "--message-file" ]]; then
    echo "Expected session-entry wrapper to forward canonical subcommands to the compat launcher"
    echo "Actual args:"
    cat "$bash_log"
    exit 1
  fi

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
exit 6
SH
  chmod +x "$git_bin_dir/bash.exe"

  set +e
  PATH="$generic_dir:$git_cmd_dir:$PATH" \
    "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' resolve --message-file transcript.md"
  wrapper_exit=$?
  set -e

  if [[ $wrapper_exit -ne 6 ]]; then
    echo "Expected session-entry wrapper to preserve nonzero bash exit code"
    echo "Expected: 6"
    echo "Actual:   $wrapper_exit"
    exit 1
  fi
}

assert_repo_safety_wrapper_behavior() {
  local wrapper_path="$1"
  local bash_log="$tmp_root/repo-safety-wrapper-bash.log"
  local wrapper_output
  local first_arg
  local second_arg
  local third_arg
  local fourth_arg
  local wrapper_exit

  if [[ ! -f "$wrapper_path" ]]; then
    echo "Expected repo-safety PowerShell wrapper to exist: $wrapper_path"
    exit 1
  fi

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
set -euo pipefail

log_file="${SUPERPOWERS_TEST_BASH_LOG:?}"
: > "$log_file"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "$log_file"
done

printf '{"outcome":"blocked","approval_path":"/c/tmp/state/projects/repo-safety/approval.json"}\n'
SH
  chmod +x "$git_bin_dir/bash.exe"

  wrapper_output="$(
    PATH="$generic_dir:$git_cmd_dir:$PATH" \
      SUPERPOWERS_TEST_BASH_LOG="$bash_log" \
      "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' check --intent write --stage superpowers:brainstorming --task-id spec-task --path docs/spec.md --write-target spec-artifact-write"
  )"
  if [[ "$wrapper_output" != *'"/c/tmp/state/projects/repo-safety/approval.json"'* ]]; then
    echo "Expected repo-safety wrapper to preserve raw JSON paths"
    echo "Actual output: $wrapper_output"
    exit 1
  fi

  first_arg="$(sed -n '1p' "$bash_log")"
  second_arg="$(sed -n '2p' "$bash_log")"
  third_arg="$(sed -n '3p' "$bash_log")"
  fourth_arg="$(sed -n '4p' "$bash_log")"
  if [[ "$first_arg" != *"/compat/bash/superpowers" ]]; then
    echo "Expected repo-safety wrapper to invoke Git Bash with the canonical compat launcher"
    echo "Actual first arg: $first_arg"
    exit 1
  fi
  if [[ "$second_arg" != "repo-safety" || "$third_arg" != "check" || "$fourth_arg" != "--intent" ]]; then
    echo "Expected repo-safety wrapper to forward canonical subcommands to the compat launcher"
    echo "Actual args:"
    cat "$bash_log"
    exit 1
  fi

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
exit 7
SH
  chmod +x "$git_bin_dir/bash.exe"

  set +e
  PATH="$generic_dir:$git_cmd_dir:$PATH" \
    "$pwsh_bin" -NoLogo -NoProfile -Command "& '$wrapper_path' check --intent write --stage superpowers:brainstorming --task-id spec-task --path docs/spec.md --write-target spec-artifact-write"
  wrapper_exit=$?
  set -e

  if [[ $wrapper_exit -ne 7 ]]; then
    echo "Expected repo-safety wrapper to preserve nonzero bash exit code"
    echo "Expected: 7"
    echo "Actual:   $wrapper_exit"
    exit 1
  fi
}

assert_plan_contract_wrapper_behavior() {
  local bash_log="$tmp_root/plan-contract-wrapper-bash.log"
  local wrapper_output
  local first_arg
  local second_arg
  local third_arg
  local fourth_arg
  local fifth_arg
  local sixth_arg
  local wrapper_exit

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
set -euo pipefail

log_file="${SUPERPOWERS_TEST_BASH_LOG:?}"
: > "$log_file"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "$log_file"
done

printf '{"plan_path":"docs/superpowers/plans/example.md","root":"/c/tmp/workspace"}\n'
SH
  chmod +x "$git_bin_dir/bash.exe"

  wrapper_output="$(
    PATH="$generic_dir:$git_cmd_dir:$PATH" \
      SUPERPOWERS_TEST_BASH_LOG="$bash_log" \
      "$pwsh_bin" -NoLogo -NoProfile -Command "& '$PLAN_CONTRACT_WRAPPER' analyze-plan --spec docs/superpowers/specs/example.md --plan docs/superpowers/plans/example.md"
  )"
  if [[ "$wrapper_output" != *'"/c/tmp/workspace"'* ]]; then
    echo "Expected plan-contract wrapper to preserve raw transport JSON"
    echo "Actual output: $wrapper_output"
    exit 1
  fi
  if [[ "$wrapper_output" == *'C:\\tmp\\workspace'* ]]; then
    echo "Expected plan-contract wrapper to avoid rewriting JSON paths"
    echo "Actual output: $wrapper_output"
    exit 1
  fi

  first_arg="$(sed -n '1p' "$bash_log")"
  second_arg="$(sed -n '2p' "$bash_log")"
  third_arg="$(sed -n '3p' "$bash_log")"
  fourth_arg="$(sed -n '4p' "$bash_log")"
  fifth_arg="$(sed -n '5p' "$bash_log")"
  sixth_arg="$(sed -n '6p' "$bash_log")"
  if [[ "$first_arg" != *"/compat/bash/superpowers" ]]; then
    echo "Expected plan-contract wrapper to invoke the canonical compat launcher"
    echo "Actual first arg: $first_arg"
    exit 1
  fi
  if [[ "$second_arg" != "plan" || "$third_arg" != "contract" || "$fourth_arg" != "analyze-plan" || "$fifth_arg" != "--spec" || "$sixth_arg" != "docs/superpowers/specs/example.md" ]]; then
    echo "Expected plan-contract wrapper to forward canonical plan contract subcommands"
    echo "Actual args:"
    cat "$bash_log"
    exit 1
  fi

  cat > "$git_bin_dir/bash.exe" <<'SH'
#!/bin/bash
exit 8
SH
  chmod +x "$git_bin_dir/bash.exe"

  set +e
  PATH="$generic_dir:$git_cmd_dir:$PATH" \
    "$pwsh_bin" -NoLogo -NoProfile -Command "& '$PLAN_CONTRACT_WRAPPER' analyze-plan --spec docs/superpowers/specs/example.md --plan docs/superpowers/plans/example.md"
  wrapper_exit=$?
  set -e

  if [[ $wrapper_exit -ne 8 ]]; then
    echo "Expected plan-contract wrapper to preserve nonzero bash exit code"
    echo "Expected: 8"
    echo "Actual:   $wrapper_exit"
    exit 1
  fi
}

assert_public_workflow_wrapper_behavior "$PUBLIC_WORKFLOW_WRAPPER"
assert_wrapper_behavior "$WORKFLOW_WRAPPER" "workflow-status" '{"status":"needs_brainstorming","next_skill":"superpowers:brainstorming","root":"/c/tmp/workspace"}' '"/c/tmp/workspace"' "/compat/bash/superpowers|workflow|status|--plan"
assert_wrapper_behavior "$PLAN_EXEC_WRAPPER" "plan-execution" '{"execution_mode":"none","execution_started":"no","root":"/c/tmp/workspace"}' '"/c/tmp/workspace"' "/compat/bash/superpowers|plan|execution|status|--plan"
assert_plan_contract_wrapper_behavior
assert_session_entry_wrapper_behavior "$SESSION_ENTRY_WRAPPER"
assert_repo_safety_wrapper_behavior "$REPO_SAFETY_WRAPPER"
assert_update_check_wrapper_behavior "$UPDATE_CHECK_WRAPPER"

echo "PowerShell wrapper bash-resolution regression test passed."
