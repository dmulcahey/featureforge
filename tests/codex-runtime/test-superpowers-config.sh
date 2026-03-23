#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG_BIN="$REPO_ROOT/bin/superpowers-config"
RUST_SUPERPOWERS_BIN="$REPO_ROOT/target/debug/superpowers"

STATE_DIR="$(mktemp -d)"
trap 'rm -rf "$STATE_DIR"' EXIT
export SUPERPOWERS_STATE_DIR="$STATE_DIR"

ensure_rust_superpowers_bin() {
  if [[ -x "$RUST_SUPERPOWERS_BIN" ]]; then
    return 0
  fi

  source "$HOME/.cargo/env"
  (cd "$REPO_ROOT" && cargo build --quiet --bin superpowers >/dev/null)
}

run_rust_config() {
  ensure_rust_superpowers_bin
  "$RUST_SUPERPOWERS_BIN" config "$@"
}

run_rust_config_fails() {
  local expected="$1"
  shift
  local output
  local status=0

  ensure_rust_superpowers_bin
  output="$("$RUST_SUPERPOWERS_BIN" config "$@" 2>&1)" || status=$?
  if [[ $status -eq 0 ]]; then
    echo "Expected canonical Rust config command to fail: $*"
    printf '%s\n' "$output"
    exit 1
  fi
  if [[ "$output" != *"$expected"* ]]; then
    echo "Expected canonical Rust config failure to contain '$expected'"
    printf '%s\n' "$output"
    exit 1
  fi
}

missing="$("$CONFIG_BIN" get update_check)"
if [[ -n "$missing" ]]; then
  echo "Expected missing config key to return empty output"
  exit 1
fi

"$CONFIG_BIN" set update_check false
value="$("$CONFIG_BIN" get update_check)"
if [[ "$value" != "false" ]]; then
  echo "Expected update_check=false, got: $value"
  exit 1
fi

"$CONFIG_BIN" set update_check true
value="$("$CONFIG_BIN" get update_check)"
if [[ "$value" != "true" ]]; then
  echo "Expected update_check=true after overwrite, got: $value"
  exit 1
fi

"$CONFIG_BIN" set superpowers_contributor true
listing="$("$CONFIG_BIN" list)"
if ! printf '%s\n' "$listing" | rg -q '^update_check: true$'; then
  echo "Expected config listing to include update_check: true"
  exit 1
fi
if ! printf '%s\n' "$listing" | rg -q '^superpowers_contributor: true$'; then
  echo "Expected config listing to include superpowers_contributor: true"
  exit 1
fi

echo "superpowers-config smoke test passed."

legacy_config="$STATE_DIR/config.yaml"
mkdir -p "$(dirname "$legacy_config")"
cat > "$legacy_config" <<'EOF'
update_check: false
superpowers_contributor: true
EOF

value="$(run_rust_config get update_check)"
if [[ "$value" != "false" ]]; then
  echo "Expected canonical config migration to preserve update_check=false, got: $value"
  exit 1
fi

canonical_config="$STATE_DIR/config/config.yaml"
if [[ ! -f "$canonical_config" ]]; then
  echo "Expected canonical config migration to create $canonical_config"
  exit 1
fi

listing="$(run_rust_config list)"
if ! printf '%s\n' "$listing" | rg -q '^superpowers_contributor: true$'; then
  echo "Expected canonical config listing to preserve migrated superpowers_contributor: true"
  exit 1
fi

cat > "$legacy_config" <<'EOF'
update_check:
  nested: true
EOF
rm -f "$canonical_config"
run_rust_config_fails InvalidConfigFormat list

echo "superpowers-config canonical Rust contract passed."
