#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'USAGE'
Usage:
  scripts/run-rust-tests-sharded.sh [shard_count]
  scripts/run-rust-tests-sharded.sh [shard_count] -- [nextest filters...]

Environment:
  FEATUREFORGE_SHARD_THREADS    per-shard test threads (default: 1)
  FEATUREFORGE_SHARD_PROFILE    nextest profile (default: default)
  FEATUREFORGE_SHARD_RETRIES    flaky retries per shard (default: 0)
USAGE
}

detect_cpu_count() {
  if command -v sysctl >/dev/null 2>&1; then
    local sysctl_cores
    sysctl_cores="$(sysctl -n hw.ncpu 2>/dev/null || true)"
    if [[ -n "$sysctl_cores" ]]; then
      printf '%s\n' "$sysctl_cores"
      return
    fi
  fi
  if command -v nproc >/dev/null 2>&1; then
    nproc
    return
  fi
  printf '4\n'
}

DEFAULT_MAX_SHARDS=8
CPU_COUNT="$(detect_cpu_count)"
if (( CPU_COUNT > DEFAULT_MAX_SHARDS )); then
  DEFAULT_SHARDS="$DEFAULT_MAX_SHARDS"
else
  DEFAULT_SHARDS="$CPU_COUNT"
fi

SHARDS="$DEFAULT_SHARDS"
if (( $# > 0 )) && [[ "$1" != "--" ]]; then
  if [[ "$1" == "-h" || "$1" == "--help" ]]; then
    usage
    exit 0
  fi
  SHARDS="$1"
  shift
fi

if (( $# > 0 )) && [[ "$1" == "--" ]]; then
  shift
fi
NEXTTEST_FILTERS=()
if (( $# > 0 )); then
  NEXTTEST_FILTERS=("$@")
fi

if (( ${#NEXTTEST_FILTERS[@]} > 0 )); then
  for arg in "${NEXTTEST_FILTERS[@]}"; do
    case "$arg" in
      --package|--workspace|--exclude|--all|--lib|--bin|--bins|--example|--examples|--test|--tests|--bench|--benches|--all-targets|--features|--all-features|--no-default-features|--build-jobs|--release|--cargo-profile|--target|--target-dir|--unit-graph|--timings|--frozen|--locked|--offline|--cargo-message-format|--cargo-quiet|--cargo-verbose|--ignore-rust-version|--future-incompat-report|-Z)
        echo "unsupported filter argument in archive mode: '$arg'" >&2
        echo "use nextest test-name filters or -E expressions after '--'." >&2
        exit 2
        ;;
    esac
  done
fi

THREADS_PER_SHARD="${FEATUREFORGE_SHARD_THREADS:-1}"
NEXTEST_PROFILE="${FEATUREFORGE_SHARD_PROFILE:-default}"
RETRIES="${FEATUREFORGE_SHARD_RETRIES:-0}"

if ! [[ "$SHARDS" =~ ^[0-9]+$ ]] || (( SHARDS < 1 )); then
  echo "invalid shard count: '$SHARDS' (expected integer >= 1)" >&2
  exit 2
fi
if ! [[ "$THREADS_PER_SHARD" =~ ^[0-9]+$ ]] || (( THREADS_PER_SHARD < 1 )); then
  echo "invalid FEATUREFORGE_SHARD_THREADS: '$THREADS_PER_SHARD' (expected integer >= 1)" >&2
  exit 2
fi
if ! [[ "$RETRIES" =~ ^[0-9]+$ ]] || (( RETRIES < 0 )); then
  echo "invalid FEATUREFORGE_SHARD_RETRIES: '$RETRIES' (expected integer >= 0)" >&2
  exit 2
fi

RUN_STAMP="$(date +%Y%m%d-%H%M%S)"
ARTIFACT_ROOT="${TMPDIR:-/tmp}/featureforge-nextest-sharded"
ARCHIVE_FILE="$ARTIFACT_ROOT/archive-$RUN_STAMP.tar.zst"
LOG_DIR="$ARTIFACT_ROOT/logs-$RUN_STAMP"
SHARD_TMP_ROOT="$ARTIFACT_ROOT/tmp-$RUN_STAMP"

mkdir -p "$ARTIFACT_ROOT" "$LOG_DIR" "$SHARD_TMP_ROOT"

echo "[1/3] build once: nextest archive (avoids parallel cargo lock contention)"
cargo nextest archive \
  --workspace \
  --all-targets \
  --all-features \
  --archive-file "$ARCHIVE_FILE"

if (( ${#NEXTTEST_FILTERS[@]} > 0 )); then
  echo "[2/3] run $SHARDS shard(s) with filters: ${NEXTTEST_FILTERS[*]}"
else
  echo "[2/3] run $SHARDS shard(s) over full archive"
fi
echo "      profile=$NEXTEST_PROFILE threads_per_shard=$THREADS_PER_SHARD retries=$RETRIES"

pids=()
cleanup_pids=()
cleanup_children() {
  if (( ${#cleanup_pids[@]} == 0 )); then
    return
  fi
  for pid in "${cleanup_pids[@]}"; do
    kill "$pid" >/dev/null 2>&1 || true
  done
}
trap cleanup_children INT TERM

for shard in $(seq 1 "$SHARDS"); do
  log_file="$LOG_DIR/shard-$shard.log"
  shard_tmp="$SHARD_TMP_ROOT/shard-$shard"
  mkdir -p "$shard_tmp"
  (
    set -euo pipefail
    nextest_args=(
      nextest run
      --archive-file "$ARCHIVE_FILE"
      --workspace-remap "$ROOT_DIR"
      --profile "$NEXTEST_PROFILE"
      --partition "count:${shard}/${SHARDS}"
      --retries "$RETRIES"
      --test-threads "$THREADS_PER_SHARD"
      --no-tests pass
      --no-fail-fast
      --status-level fail
      --final-status-level fail
      --show-progress none
    )
    if (( ${#NEXTTEST_FILTERS[@]} > 0 )); then
      nextest_args+=("${NEXTTEST_FILTERS[@]}")
    fi
    TMPDIR="$shard_tmp" TEMP="$shard_tmp" TMP="$shard_tmp" \
      cargo "${nextest_args[@]}"
  ) >"$log_file" 2>&1 &
  pid="$!"
  pids+=("$pid")
  cleanup_pids+=("$pid")
done

failed=0
for idx in "${!pids[@]}"; do
  shard=$((idx + 1))
  if ! wait "${pids[$idx]}"; then
    echo "shard ${shard}/${SHARDS} failed: $LOG_DIR/shard-$shard.log" >&2
    failed=1
  fi
done

trap - INT TERM

if (( failed != 0 )); then
  echo "[3/3] FAIL: one or more shards failed" >&2
  echo "logs: $LOG_DIR" >&2
  exit 1
fi

echo "[3/3] PASS: all shards completed successfully"
echo "archive: $ARCHIVE_FILE"
echo "logs: $LOG_DIR"
echo "tmp sandboxes: $SHARD_TMP_ROOT"
