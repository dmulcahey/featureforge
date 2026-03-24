#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
UPDATE_BIN="$REPO_ROOT/bin/superpowers-update-check"
CONFIG_BIN="$REPO_ROOT/bin/superpowers-config"
RUST_SUPERPOWERS_BIN="$REPO_ROOT/target/debug/superpowers"

canonical_update_state_path() {
  printf '%s\n' "$STATE_DIR/update-check/$1"
}

write_state_file() {
  local path="$1"
  local contents="$2"
  mkdir -p "$(dirname "$path")"
  printf '%s\n' "$contents" > "$path"
}

make_install_dir() {
  local dir
  dir="$(mktemp -d)"
  mkdir -p "$dir/bin"
  ln -s "$CONFIG_BIN" "$dir/bin/superpowers-config"
  printf '%s\n' "$1" > "$dir/VERSION"
  echo "$dir"
}

make_remote_file() {
  local file
  file="$(mktemp)"
  printf '%s\n' "$1" > "$file"
  echo "$file"
}

ensure_rust_superpowers_bin() {
  source "$HOME/.cargo/env"
  (cd "$REPO_ROOT" && cargo build --quiet --bin superpowers >/dev/null)
}

run_rust_update_check() {
  ensure_rust_superpowers_bin
  "$RUST_SUPERPOWERS_BIN" update-check "$@"
}

reset_state() {
  rm -f \
    "$STATE_DIR/last-update-check" \
    "$STATE_DIR/update-snoozed" \
    "$STATE_DIR/just-upgraded-from" \
    "$(canonical_update_state_path last-update-check)" \
    "$(canonical_update_state_path update-snoozed)" \
    "$(canonical_update_state_path just-upgraded-from)" \
    "$STATE_DIR/config.yaml" \
    "$STATE_DIR/config/config.yaml"
}

set_mtime_minutes_ago() {
  local path="$1"
  local minutes="$2"
  local now target_epoch

  now="$(date +%s)"
  target_epoch=$(( now - (minutes * 60) ))
  perl -e 'my ($path, $epoch) = @ARGV; utime($epoch, $epoch, $path) or die "utime($path): $!";' "$path" "$target_epoch"
}

assert_output() {
  local expected="$1"
  local actual="$2"
  local label="$3"
  if [[ "$actual" != "$expected" ]]; then
    echo "Unexpected output for $label"
    echo "Expected: $expected"
    echo "Actual:   $actual"
    exit 1
  fi
}

assert_cache() {
  local expected="$1"
  local actual
  actual="$(cat "$(canonical_update_state_path last-update-check)" 2>/dev/null || true)"
  if [[ "$actual" != "$expected" ]]; then
    echo "Unexpected cache contents"
    echo "Expected: $expected"
    echo "Actual:   $actual"
    exit 1
  fi
}

assert_no_cache() {
  if [[ -e "$(canonical_update_state_path last-update-check)" || -e "$STATE_DIR/last-update-check" ]]; then
    echo "Expected no update-check cache file to be written"
    cat "$(canonical_update_state_path last-update-check)" 2>/dev/null || true
    cat "$STATE_DIR/last-update-check" 2>/dev/null || true
    exit 1
  fi
}

STATE_DIR="$(mktemp -d)"
trap 'rm -rf "$STATE_DIR"' EXIT
export SUPERPOWERS_STATE_DIR="$STATE_DIR"

local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.1)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
output="$("$UPDATE_BIN")"
assert_output "" "$output" "normalized equal versions"
assert_cache "UP_TO_DATE 5.1.0"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.2.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
write_state_file "$(canonical_update_state_path last-update-check)" "UP_TO_DATE 5.1.0"
output="$("$UPDATE_BIN" --force)"
assert_output "UPGRADE_AVAILABLE 5.1.0 5.2.0" "$output" "--force bypasses cached up-to-date result"
assert_cache "UPGRADE_AVAILABLE 5.1.0 5.2.0"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.2.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
output="$("$UPDATE_BIN")"
assert_output "UPGRADE_AVAILABLE 5.1.0 5.2.0" "$output" "local behind remote"
assert_cache "UPGRADE_AVAILABLE 5.1.0 5.2.0"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.2.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
write_state_file "$(canonical_update_state_path last-update-check)" "UP_TO_DATE 5.1.0"
set_mtime_minutes_ago "$(canonical_update_state_path last-update-check)" 61
output="$("$UPDATE_BIN")"
assert_output "UPGRADE_AVAILABLE 5.1.0 5.2.0" "$output" "stale up-to-date cache refresh"
assert_cache "UPGRADE_AVAILABLE 5.1.0 5.2.0"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.1.2)"
remote_file="$(make_remote_file 5.1.10)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
output="$("$UPDATE_BIN")"
assert_output "UPGRADE_AVAILABLE 5.1.2 5.1.10" "$output" "multi-digit semver comparison"
assert_cache "UPGRADE_AVAILABLE 5.1.2 5.1.10"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.0.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
write_state_file "$(canonical_update_state_path last-update-check)" "UPGRADE_AVAILABLE 5.1.0 5.2.0"
output="$("$UPDATE_BIN")"
assert_output "UPGRADE_AVAILABLE 5.1.0 5.2.0" "$output" "fresh upgrade cache reuse"
assert_cache "UPGRADE_AVAILABLE 5.1.0 5.2.0"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.0.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
write_state_file "$(canonical_update_state_path last-update-check)" "UPGRADE_AVAILABLE 5.1.0 5.2.0"
output="$("$UPDATE_BIN" --force)"
assert_output "" "$output" "--force bypasses cached upgrade-available result"
assert_cache "UP_TO_DATE 5.1.0"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.2.0)"
remote_file="$(make_remote_file 5.1.9)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
output="$("$UPDATE_BIN")"
assert_output "" "$output" "local ahead of remote"
assert_cache "UP_TO_DATE 5.2.0"
output="$("$UPDATE_BIN")"
assert_output "" "$output" "cached local-ahead result"
assert_cache "UP_TO_DATE 5.2.0"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.1.0)"
export SUPERPOWERS_DIR="$local_dir"
write_state_file "$(canonical_update_state_path just-upgraded-from)" "5.0.0"
output="$("$UPDATE_BIN")"
assert_output "JUST_UPGRADED 5.0.0 5.1.0" "$output" "just-upgraded marker"
assert_cache "UP_TO_DATE 5.1.0"
rm -rf "$local_dir"
reset_state

local_dir="$(make_install_dir 5.1.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file:///does/not/exist"
output="$("$UPDATE_BIN")"
assert_output "" "$output" "remote lookup failure with empty cache"
assert_no_cache
rm -rf "$local_dir"
reset_state

local_dir="$(make_install_dir 5.1.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file:///does/not/exist"
write_state_file "$(canonical_update_state_path last-update-check)" "UPGRADE_AVAILABLE 5.1.0 5.2.0"
output="$("$UPDATE_BIN")"
assert_output "UPGRADE_AVAILABLE 5.1.0 5.2.0" "$output" "remote lookup failure with sticky upgrade cache"
assert_cache "UPGRADE_AVAILABLE 5.1.0 5.2.0"
rm -rf "$local_dir"
reset_state

local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.2.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
"$CONFIG_BIN" set update_check false
output="$("$UPDATE_BIN")"
assert_output "" "$output" "disabled update check"
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.2.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
write_state_file "$(canonical_update_state_path update-snoozed)" "5.2.0 1 $(date +%s)"
output="$("$UPDATE_BIN")"
assert_output "" "$output" "snoozed true upgrade"

echo "superpowers-update-check smoke test passed."

reset_state
local_dir="$(make_install_dir 5.1.0)"
remote_file="$(make_remote_file 5.2.0)"
export SUPERPOWERS_DIR="$local_dir"
export SUPERPOWERS_REMOTE_URL="file://$remote_file"
output="$(run_rust_update_check)"
assert_output "UPGRADE_AVAILABLE 5.1.0 5.2.0" "$output" "canonical rust update-check"
canonical_cache="$STATE_DIR/update-check/last-update-check"
if [[ ! -f "$canonical_cache" ]]; then
  echo "Expected canonical Rust update-check to write $canonical_cache"
  exit 1
fi
if [[ "$(cat "$canonical_cache")" != "UPGRADE_AVAILABLE 5.1.0 5.2.0" ]]; then
  echo "Expected canonical Rust update-check cache to preserve helper status-line format"
  cat "$canonical_cache"
  exit 1
fi
if [[ -e "$STATE_DIR/last-update-check" ]]; then
  echo "Expected canonical Rust update-check to stop writing the legacy root cache path"
  cat "$STATE_DIR/last-update-check"
  exit 1
fi
rm -rf "$local_dir"
rm -f "$remote_file"
reset_state

printf '%s\n' "5.0.0" > "$STATE_DIR/just-upgraded-from"
local_dir="$(make_install_dir 5.1.0)"
export SUPERPOWERS_DIR="$local_dir"
output="$(run_rust_update_check)"
assert_output "JUST_UPGRADED 5.0.0 5.1.0" "$output" "canonical rust just-upgraded marker"
canonical_cache="$STATE_DIR/update-check/last-update-check"
if [[ "$(cat "$canonical_cache")" != "UP_TO_DATE 5.1.0" ]]; then
  echo "Expected canonical Rust update-check to normalize just-upgraded cache under update-check/"
  cat "$canonical_cache"
  exit 1
fi
if [[ -e "$STATE_DIR/just-upgraded-from" ]]; then
  echo "Expected canonical Rust update-check to consume the legacy just-upgraded marker"
  exit 1
fi
rm -rf "$local_dir"
reset_state

echo "superpowers-update-check canonical Rust contract passed."
