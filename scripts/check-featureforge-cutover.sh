#!/usr/bin/env bash
set -euo pipefail

DEFAULT_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="${FEATUREFORGE_CUTOVER_REPO_ROOT:-$DEFAULT_REPO_ROOT}"
cd "$REPO_ROOT"

LEGACY_ROOT_REGEX='\.(codex|copilot)/featureforge([/[:space:]`"'"'"']|$)'

fail() {
  printf 'cutover check failed: %s\n' "$1" >&2
  exit 1
}

classify_bucket() {
  case "$1" in
    docs/archive/*)
      printf 'archived\n'
      ;;
    docs/featureforge/specs/*|docs/featureforge/plans/*|docs/featureforge/execution-evidence/*|tests/*)
      printf 'nonsurface\n'
      ;;
    *)
      printf 'active\n'
      ;;
  esac
}

tracked_files=()
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  tracked_files+=("$file")
done < <(git ls-files)

active_path_hits=()
archived_path_hits=()
active_content_hits=()
archived_content_hits=()

for file in "${tracked_files[@]}"; do
  bucket="$(classify_bucket "$file")"

  if [[ "$bucket" == "nonsurface" ]]; then
    continue
  fi

  if printf '%s\n' "$file" | rg -q "$LEGACY_ROOT_REGEX"; then
    if [[ "$bucket" == "active" ]]; then
      active_path_hits+=("$file")
    else
      archived_path_hits+=("$file")
    fi
  fi

  while IFS= read -r hit; do
    [[ -n "$hit" ]] || continue
    formatted_hit="$file:$hit"
    if [[ "$bucket" == "active" ]]; then
      active_content_hits+=("$formatted_hit")
    else
      archived_content_hits+=("$formatted_hit")
    fi
  done < <(rg -n -H -I "$LEGACY_ROOT_REGEX" "$file" || true)
done

if ((${#active_path_hits[@]} > 0)); then
  printf 'Forbidden active path names:\n%s\n' "$(printf '%s\n' "${active_path_hits[@]}")" >&2
  fail 'active tracked paths still contain legacy-root paths'
fi

if ((${#active_content_hits[@]} > 0)); then
  printf 'Forbidden active content references:\n%s\n' "$(printf '%s\n' "${active_content_hits[@]}")" >&2
  fail 'active tracked files still contain legacy-root references'
fi

[[ -x bin/featureforge ]] || fail 'bin/featureforge must exist and be executable'
[[ -f bin/prebuilt/darwin-arm64/featureforge ]] || fail 'darwin prebuilt runtime must exist'
[[ -f bin/prebuilt/darwin-arm64/featureforge.sha256 ]] || fail 'darwin checksum must exist'
[[ -f bin/prebuilt/windows-x64/featureforge.exe ]] || fail 'windows prebuilt runtime must exist'
[[ -f bin/prebuilt/windows-x64/featureforge.exe.sha256 ]] || fail 'windows checksum must exist'
grep -Fq 'bin/prebuilt/darwin-arm64/featureforge' bin/prebuilt/manifest.json || fail 'manifest must reference darwin featureforge binary'
grep -Fq 'bin/prebuilt/windows-x64/featureforge.exe' bin/prebuilt/manifest.json || fail 'manifest must reference windows featureforge binary'
if rg -n "$LEGACY_ROOT_REGEX" bin/prebuilt/manifest.json >/dev/null; then
  fail 'manifest must not reference retired legacy-root paths'
fi

printf 'featureforge cutover checks passed\n'
