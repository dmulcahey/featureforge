#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TARGET_KEY="${FEATUREFORGE_PREBUILT_TARGET:-darwin-arm64}"
case "$TARGET_KEY" in
  darwin-arm64)
    DEFAULT_RUST_TARGET="aarch64-apple-darwin"
    BINARY_NAME="featureforge"
    ;;
  windows-x64)
    DEFAULT_RUST_TARGET="x86_64-pc-windows-msvc"
    BINARY_NAME="featureforge.exe"
    ;;
  *)
    echo "unsupported FEATUREFORGE_PREBUILT_TARGET: $TARGET_KEY" >&2
    exit 1
    ;;
esac
RUST_TARGET="${FEATUREFORGE_PREBUILT_RUST_TARGET:-$DEFAULT_RUST_TARGET}"
VERSION="$(tr -d '[:space:]' < "$REPO_ROOT/VERSION")"
OUTPUT_DIR="$REPO_ROOT/bin/prebuilt/$TARGET_KEY"
OUTPUT_PATH="$OUTPUT_DIR/$BINARY_NAME"
CHECKSUM_PATH="$OUTPUT_PATH.sha256"
BUILD_PATH="$REPO_ROOT/target/$RUST_TARGET/release/$BINARY_NAME"

command -v cargo >/dev/null 2>&1 || {
  echo "cargo is required to refresh the checked-in runtime." >&2
  exit 1
}
command -v node >/dev/null 2>&1 || {
  echo "node is required to update bin/prebuilt/manifest.json." >&2
  exit 1
}

cd "$REPO_ROOT"
cargo build --release --target "$RUST_TARGET" --bin featureforge

mkdir -p "$OUTPUT_DIR"
cp "$BUILD_PATH" "$OUTPUT_PATH"
chmod +x "$OUTPUT_PATH"
if [[ "$TARGET_KEY" == "darwin-arm64" ]]; then
  cp "$OUTPUT_PATH" "$REPO_ROOT/bin/featureforge"
  chmod +x "$REPO_ROOT/bin/featureforge"
fi

CHECKSUM="$(shasum -a 256 "$OUTPUT_PATH" | awk '{print $1}')"
printf '%s  %s\n' "$CHECKSUM" "$BINARY_NAME" > "$CHECKSUM_PATH"

node "$SCRIPT_DIR/prebuilt-runtime-provenance.mjs" update \
  --target "$TARGET_KEY" \
  --binary-path "bin/prebuilt/$TARGET_KEY/$BINARY_NAME" \
  --checksum-path "bin/prebuilt/$TARGET_KEY/$BINARY_NAME.sha256" \
  --version "$VERSION" \
  --repo-root "$REPO_ROOT"

node "$SCRIPT_DIR/prebuilt-runtime-provenance.mjs" verify \
  --target "$TARGET_KEY" \
  --repo-root "$REPO_ROOT"

echo "Refreshed checked-in runtime for $TARGET_KEY at $OUTPUT_PATH"
