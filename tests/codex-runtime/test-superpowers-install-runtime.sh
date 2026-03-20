#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
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

canonical_path() {
  local dir="$1"
  (
    cd "$dir" >/dev/null 2>&1
    pwd -P
  )
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
  for bundle in \
    "$dir/runtime/core-helpers/dist/superpowers-config.cjs" \
    "$dir/runtime/core-helpers/dist/superpowers-workflow-status.cjs" \
    "$dir/runtime/core-helpers/dist/superpowers-plan-execution.cjs"; do
    [[ -f "$bundle" ]] || {
      echo "Expected $dir to contain bundled runtime artifact $bundle"
      exit 1
    }
  done
}

require_symlink_target() {
  local path="$1"
  local target="$2"

  [[ -L "$path" ]] || {
    echo "Expected $path to be a symlink"
    exit 1
  }

  local resolved
  resolved="$(cd "$path/.." && cd "$(dirname "$(readlink "$path")")" && pwd -P)/$(basename "$(readlink "$path")")"
  local expected
  expected="$(canonical_path "$target")"

  if [[ "$resolved" != "$expected" ]]; then
    echo "Expected $path to point to $expected, got $resolved"
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

run_install() {
  local home_dir="$1"
  local shared_root="$2"
  local codex_root="$3"
  local copilot_root="$4"
  local repo_url="$5"
  local stage_root="$6"
  local node_bin="$7"

  HOME="$home_dir" \
    SUPERPOWERS_SHARED_ROOT="$shared_root" \
    SUPERPOWERS_CODEX_ROOT="$codex_root" \
    SUPERPOWERS_COPILOT_ROOT="$copilot_root" \
    SUPERPOWERS_REPO_URL="$repo_url" \
    SUPERPOWERS_INSTALL_STAGE_ROOT="$stage_root" \
    SUPERPOWERS_NODE_BIN="$node_bin" \
    "$INSTALL_BIN"
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
echo "unexpected args: \$*" >&2
exit 64
EOF
  chmod +x "$path"
}

if [[ ! -x "$INSTALL_BIN" ]]; then
  echo "Expected staged runtime install helper to exist: $INSTALL_BIN"
  exit 1
fi

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

source_repo="$tmp_root/source-runtime.git"
make_runtime_repo "$source_repo" "2.0.0" "fresh"

node_bin="$(command -v node || true)"
if [[ -z "$node_bin" ]]; then
  echo "Expected node to be available for staged runtime install tests."
  exit 1
fi

missing_node_home="$tmp_root/missing-node-home"
missing_node_shared="$missing_node_home/.superpowers/install"
missing_node_codex="$missing_node_home/.codex/superpowers"
missing_node_copilot="$missing_node_home/.copilot/superpowers"
missing_node_stage="$missing_node_home/.superpowers/stage-install"
mkdir -p "$(dirname "$missing_node_shared")"
make_runtime_repo "$missing_node_shared" "1.0.0" "old-shared"
set +e
missing_node_output="$(run_install "$missing_node_home" "$missing_node_shared" "$missing_node_codex" "$missing_node_copilot" "$source_repo" "$missing_node_stage" "$tmp_root/does-not-exist/node" 2>&1)"
missing_node_status=$?
set -e
if [[ "$missing_node_status" -eq 0 ]]; then
  echo "Expected staged install to fail when Node is missing."
  exit 1
fi
require_contains "$missing_node_output" "RuntimeDependencyMissing"
require_contains "$missing_node_output" "Node 20"
[[ ! -e "$missing_node_stage" ]] || {
  echo "Expected failed staged install to clean up $missing_node_stage"
  exit 1
}
if [[ "$(tr -d '[:space:]' < "$missing_node_shared/VERSION")" != "1.0.0" ]]; then
  echo "Expected failed staged install to preserve the current shared install."
  exit 1
fi

unsupported_node_home="$tmp_root/unsupported-node-home"
unsupported_node_shared="$unsupported_node_home/.superpowers/install"
unsupported_node_codex="$unsupported_node_home/.codex/superpowers"
unsupported_node_copilot="$unsupported_node_home/.copilot/superpowers"
unsupported_node_stage="$unsupported_node_home/.superpowers/stage-install"
unsupported_node_bin="$tmp_root/bin/node18"
mkdir -p "$(dirname "$unsupported_node_shared")"
make_runtime_repo "$unsupported_node_shared" "1.1.0" "old-unsupported"
make_node_stub "$unsupported_node_bin" "v18.20.0"
set +e
unsupported_node_output="$(run_install "$unsupported_node_home" "$unsupported_node_shared" "$unsupported_node_codex" "$unsupported_node_copilot" "$source_repo" "$unsupported_node_stage" "$unsupported_node_bin" 2>&1)"
unsupported_node_status=$?
set -e
if [[ "$unsupported_node_status" -eq 0 ]]; then
  echo "Expected staged install to fail when Node is too old."
  exit 1
fi
require_contains "$unsupported_node_output" "RuntimeDependencyVersionUnsupported"
require_contains "$unsupported_node_output" "Node 20"
[[ ! -e "$unsupported_node_stage" ]] || {
  echo "Expected unsupported-node failure to clean up $unsupported_node_stage"
  exit 1
}
if [[ "$(tr -d '[:space:]' < "$unsupported_node_shared/VERSION")" != "1.1.0" ]]; then
  echo "Expected unsupported-node failure to preserve the current shared install."
  exit 1
fi

missing_bundle_repo="$tmp_root/source-missing-bundle.git"
make_runtime_repo "$missing_bundle_repo" "2.1.0" "missing-bundle"
rm "$missing_bundle_repo/runtime/core-helpers/dist/superpowers-plan-execution.cjs"
git -C "$missing_bundle_repo" rm runtime/core-helpers/dist/superpowers-plan-execution.cjs >/dev/null 2>&1
git -C "$missing_bundle_repo" commit -m "remove-plan-execution-bundle" >/dev/null 2>&1

missing_bundle_home="$tmp_root/missing-bundle-home"
missing_bundle_shared="$missing_bundle_home/.superpowers/install"
missing_bundle_codex="$missing_bundle_home/.codex/superpowers"
missing_bundle_copilot="$missing_bundle_home/.copilot/superpowers"
missing_bundle_stage="$missing_bundle_home/.superpowers/stage-install"
mkdir -p "$(dirname "$missing_bundle_shared")"
make_runtime_repo "$missing_bundle_shared" "1.2.0" "old-missing-bundle"
set +e
missing_bundle_output="$(run_install "$missing_bundle_home" "$missing_bundle_shared" "$missing_bundle_codex" "$missing_bundle_copilot" "$missing_bundle_repo" "$missing_bundle_stage" "$node_bin" 2>&1)"
missing_bundle_status=$?
set -e
if [[ "$missing_bundle_status" -eq 0 ]]; then
  echo "Expected staged install to fail when a required bundle is missing."
  exit 1
fi
require_contains "$missing_bundle_output" "RuntimeArtifactMissing"
require_contains "$missing_bundle_output" "superpowers-plan-execution.cjs"
[[ ! -e "$missing_bundle_stage" ]] || {
  echo "Expected missing-bundle failure to clean up $missing_bundle_stage"
  exit 1
}
if [[ "$(tr -d '[:space:]' < "$missing_bundle_shared/VERSION")" != "1.2.0" ]]; then
  echo "Expected missing-bundle failure to preserve the current shared install."
  exit 1
fi

success_home="$tmp_root/success-home"
success_shared="$success_home/.superpowers/install"
success_codex="$success_home/.codex/superpowers"
success_copilot="$success_home/.copilot/superpowers"
success_stage="$success_home/.superpowers/stage-install"
codex_skills_link="$success_home/.agents/skills/superpowers"
copilot_skills_link="$success_home/.copilot/skills/superpowers"
mkdir -p "$(dirname "$success_shared")" "$(dirname "$success_codex")" "$(dirname "$success_copilot")" "$(dirname "$codex_skills_link")" "$(dirname "$copilot_skills_link")"
make_runtime_repo "$success_shared" "1.5.0" "old-success"
ln -s "$success_shared" "$success_codex"
ln -s "$success_shared" "$success_copilot"
ln -s "$success_shared/skills" "$codex_skills_link"
run_install "$success_home" "$success_shared" "$success_codex" "$success_copilot" "$source_repo" "$success_stage" "$node_bin" >"$tmp_root/success-output.txt"
success_output="$(cat "$tmp_root/success-output.txt")"

require_valid_install "$success_shared"
if [[ "$(tr -d '[:space:]' < "$success_shared/VERSION")" != "2.0.0" ]]; then
  echo "Expected successful staged install to swap in the new shared checkout."
  exit 1
fi
require_symlink_target "$success_codex" "$success_shared"
require_symlink_target "$success_copilot" "$success_shared"
require_symlink_target "$codex_skills_link" "$success_shared/skills"
[[ ! -e "$copilot_skills_link" ]] || {
  echo "Expected missing first-time discovery links to remain manual."
  exit 1
}
[[ ! -e "$success_stage" ]] || {
  echo "Expected successful staged install to clean up $success_stage"
  exit 1
}
require_contains "$success_output" "Shared install ready at $success_shared"
require_contains "$success_output" "Codex next step:"
require_contains "$success_output" "GitHub Copilot next step:"
require_contains "$success_output" "~/.copilot/skills/superpowers"

echo "superpowers-install-runtime regression test passed."
