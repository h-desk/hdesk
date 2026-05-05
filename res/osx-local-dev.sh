#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_DEST="${HDESK_MACOS_APP_DEST:-$HOME/Applications/HDesk.app}"
DERIVED_DATA_PATH="${HDESK_MACOS_DERIVED_DATA_PATH:-$WORKSPACE_DIR/tmp_test/macos-local-derived-data}"
MACOS_ARCH="${HDESK_MACOS_ARCH:-x86_64}"
RUST_FEATURES="${HDESK_MACOS_RUST_FEATURES:-flutter,hwcodec}"

detect_codesign_identity() {
  security find-identity -v -p codesigning 2>/dev/null |
    awk -F '"' '/Developer ID Application:/ && $0 !~ /CSSMERR_TP_CERT_REVOKED/ { print $2; exit }'
}

sign_app_if_possible() {
  local app_path="$1"
  local identity="${HDESK_MACOS_CODESIGN_IDENTITY:-}"

  if [[ -z "$identity" ]]; then
    identity="$(detect_codesign_identity || true)"
  fi

  if [[ -z "$identity" ]]; then
    echo "No usable Developer ID Application identity found; leaving the existing signature in place." >&2
    return 0
  fi

  echo "Using codesign identity: $identity"
  codesign --force --deep --sign "$identity" "$app_path"
}

export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
export SDKROOT="${SDKROOT:-$(xcrun --sdk macosx --show-sdk-path)}"
export LIBCLANG_PATH="${LIBCLANG_PATH:-$DEVELOPER_DIR/Toolchains/XcodeDefault.xctoolchain/usr/lib}"
export CPLUS_INCLUDE_PATH="${CPLUS_INCLUDE_PATH:-$DEVELOPER_DIR/Toolchains/XcodeDefault.xctoolchain/usr/include/c++/v1}"
export VCPKG_INSTALLED_ROOT="${VCPKG_INSTALLED_ROOT:-$WORKSPACE_DIR/vcpkg_installed}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-10.15}"

cd "$WORKSPACE_DIR"

echo "==> Building Rust release artifacts"
echo "Using Rust features: $RUST_FEATURES"
cargo build --features "$RUST_FEATURES" --release

echo "==> Building macOS app bundle"
cd "$WORKSPACE_DIR/flutter/macos"
xcodebuild \
  -workspace Runner.xcworkspace \
  -scheme Runner \
  -configuration Release \
  -derivedDataPath "$DERIVED_DATA_PATH" \
  -destination "platform=macOS,arch=$MACOS_ARCH" \
  ARCHS="$MACOS_ARCH" \
  ONLY_ACTIVE_ARCH=YES \
  build

APP_SRC="$DERIVED_DATA_PATH/Build/Products/Release/HDesk.app"
if [[ ! -d "$APP_SRC" ]]; then
  echo "Built app not found: $APP_SRC" >&2
  exit 1
fi

echo "==> Injecting service binary"
cp -f "$WORKSPACE_DIR/target/release/service" "$APP_SRC/Contents/MacOS/"

echo "==> Signing build output"
sign_app_if_possible "$APP_SRC"

echo "==> Deploying to $APP_DEST"
rm -rf "$APP_DEST"
mkdir -p "$(dirname "$APP_DEST")"
ditto "$APP_SRC" "$APP_DEST"

echo "==> Signing deployed app"
sign_app_if_possible "$APP_DEST"

echo "==> Restarting HDesk"
pkill -f 'HDesk.app/Contents/MacOS/HDesk' || true
if [[ "${HDESK_SKIP_LAUNCH:-0}" != "1" ]]; then
  open -n "$APP_DEST"
fi

echo "==> Signature summary"
codesign -dvv "$APP_DEST" 2>&1 | sed -n '1,20p'