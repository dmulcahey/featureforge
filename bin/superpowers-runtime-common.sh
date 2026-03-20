#!/usr/bin/env bash
set -euo pipefail

superpowers_emit_runtime_failure() {
  local failure_class="$1"
  local message="$2"
  printf '{"failure_class":"%s","message":"%s"}\n' "$failure_class" "$message" >&2
}

superpowers_install_root() {
  cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1
  pwd -P
}

superpowers_node_path() {
  local node_bin="${SUPERPOWERS_NODE_BIN:-}"
  if [[ -z "$node_bin" ]]; then
    node_bin="$(command -v node 2>/dev/null || true)"
  fi

  if [[ -z "$node_bin" || ! -x "$node_bin" ]]; then
    superpowers_emit_runtime_failure "RuntimeDependencyMissing" "Node 20 LTS or newer is required."
    return 1
  fi

  local version
  version="$("$node_bin" --version 2>/dev/null || true)"
  if [[ ! "$version" =~ ^v?([0-9]+) ]]; then
    superpowers_emit_runtime_failure "RuntimeDependencyVersionUnsupported" "Couldn't determine the installed Node version."
    return 1
  fi

  if (( BASH_REMATCH[1] < 20 )); then
    superpowers_emit_runtime_failure "RuntimeDependencyVersionUnsupported" "Node 20 LTS or newer is required. Found $version."
    return 1
  fi

  printf '%s\n' "$node_bin"
}

superpowers_run_runtime() {
  local entry_relative="$1"
  shift

  local install_root
  install_root="$(superpowers_install_root)"
  local node_bin
  node_bin="$(superpowers_node_path)" || return 1
  local entry_path="$install_root/$entry_relative"

  if [[ ! -f "$entry_path" ]]; then
    superpowers_emit_runtime_failure "RuntimeArtifactMissing" "Missing runtime bundle: $entry_relative"
    return 1
  fi

  if ! "$node_bin" --check "$entry_path" >/dev/null 2>&1; then
    superpowers_emit_runtime_failure "RuntimeArtifactInvalid" "Invalid runtime bundle: $entry_relative"
    return 1
  fi

  "$node_bin" "$entry_path" "$@"
}
