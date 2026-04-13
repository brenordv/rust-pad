#!/usr/bin/env bash
# Build a Rust Pad.app bundle for local development/testing on macOS.
#
# Usage:
#   ./packaging/macos/bundle.sh          # uses cargo build --release
#   ./packaging/macos/bundle.sh debug    # uses existing debug binary
#
# The .app bundle is created in the repo root as "Rust Pad.app".

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
APP_DIR="${REPO_ROOT}/Rust Pad.app/Contents"

PROFILE="${1:-release}"

# Build if release
if [ "$PROFILE" = "release" ]; then
  echo "Building release binary..."
  cargo build --release -p rust-pad
  BINARY="${REPO_ROOT}/target/release/rust-pad"
elif [ "$PROFILE" = "debug" ]; then
  BINARY="${REPO_ROOT}/target/debug/rust-pad"
  if [ ! -f "$BINARY" ]; then
    echo "Debug binary not found. Run 'cargo build -p rust-pad' first."
    exit 1
  fi
else
  echo "Usage: $0 [release|debug]"
  exit 1
fi

# Read version from workspace Cargo.toml
VERSION=$(grep '^version' "${REPO_ROOT}/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

echo "Bundling Rust Pad v${VERSION} (${PROFILE})..."

# Create .app structure
rm -rf "${REPO_ROOT}/Rust Pad.app"
mkdir -p "${APP_DIR}/MacOS"
mkdir -p "${APP_DIR}/Resources"

# Copy binary
cp "$BINARY" "${APP_DIR}/MacOS/rust-pad"
chmod 755 "${APP_DIR}/MacOS/rust-pad"

# Create Info.plist from template
sed "s/__VERSION__/${VERSION}/g" "${SCRIPT_DIR}/Info.plist.template" > "${APP_DIR}/Info.plist"

# Create .icns icon if iconutil is available, otherwise copy PNG
if command -v iconutil &>/dev/null && command -v sips &>/dev/null; then
  ICONSET=$(mktemp -d)/rust-pad.iconset
  mkdir -p "$ICONSET"
  LOGO="${REPO_ROOT}/assets/logo.png"
  sips -z 16 16     "$LOGO" --out "$ICONSET/icon_16x16.png"      2>/dev/null
  sips -z 32 32     "$LOGO" --out "$ICONSET/icon_16x16@2x.png"   2>/dev/null
  sips -z 32 32     "$LOGO" --out "$ICONSET/icon_32x32.png"      2>/dev/null
  sips -z 64 64     "$LOGO" --out "$ICONSET/icon_32x32@2x.png"   2>/dev/null
  sips -z 128 128   "$LOGO" --out "$ICONSET/icon_128x128.png"    2>/dev/null
  sips -z 256 256   "$LOGO" --out "$ICONSET/icon_128x128@2x.png" 2>/dev/null
  sips -z 256 256   "$LOGO" --out "$ICONSET/icon_256x256.png"    2>/dev/null
  sips -z 512 512   "$LOGO" --out "$ICONSET/icon_256x256@2x.png" 2>/dev/null
  sips -z 512 512   "$LOGO" --out "$ICONSET/icon_512x512.png"    2>/dev/null
  sips -z 1024 1024 "$LOGO" --out "$ICONSET/icon_512x512@2x.png" 2>/dev/null
  iconutil -c icns "$ICONSET" -o "${APP_DIR}/Resources/rust-pad.icns"
  rm -rf "$(dirname "$ICONSET")"
  echo "Icon: .icns created"
else
  cp "${REPO_ROOT}/assets/logo.png" "${APP_DIR}/Resources/rust-pad.png"
  echo "Icon: copied PNG (iconutil not available)"
fi

echo "Done: ${REPO_ROOT}/Rust Pad.app"
echo "Run with: open '${REPO_ROOT}/Rust Pad.app'"
