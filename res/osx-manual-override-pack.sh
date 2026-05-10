#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="${HDESK_MACOS_OUTPUT_DIR:-$WORKSPACE_DIR/tmp_test/macos-manual-dist}"
DERIVED_DATA_ROOT="${HDESK_MACOS_DERIVED_DATA_ROOT:-$WORKSPACE_DIR/tmp_test/macos-manual-derived-data}"
DMG_STAGE_ROOT="${HDESK_MACOS_DMG_STAGE_ROOT:-$WORKSPACE_DIR/tmp_test/macos-manual-dmg-stage}"
MACOS_ARCHES="${HDESK_MACOS_ARCHES:-x86_64 arm64}"
RUST_FEATURES="${HDESK_MACOS_RUST_FEATURES:-flutter,hwcodec}"
RELEASE_ENTITLEMENTS="${HDESK_MACOS_RELEASE_ENTITLEMENTS:-$WORKSPACE_DIR/flutter/macos/Runner/Release.entitlements}"
MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-10.15}"
VCPKG_INSTALLED_ROOT="${VCPKG_INSTALLED_ROOT:-$WORKSPACE_DIR/vcpkg_installed}"
DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
SDKROOT="${SDKROOT:-$(xcrun --sdk macosx --show-sdk-path)}"
LIBCLANG_PATH="${LIBCLANG_PATH:-$DEVELOPER_DIR/Toolchains/XcodeDefault.xctoolchain/usr/lib}"
CPLUS_INCLUDE_PATH="${CPLUS_INCLUDE_PATH:-$DEVELOPER_DIR/Toolchains/XcodeDefault.xctoolchain/usr/include/c++/v1}"
SIGN_IDENTITY="${HDESK_MACOS_CODESIGN_IDENTITY:--}"

export DEVELOPER_DIR
export SDKROOT
export LIBCLANG_PATH
export CPLUS_INCLUDE_PATH
export VCPKG_INSTALLED_ROOT
export MACOSX_DEPLOYMENT_TARGET

deployment_target_for_arch() {
  local arch="$1"

  if [[ "$arch" == "arm64" ]]; then
    case "$MACOSX_DEPLOYMENT_TARGET" in
      10.*|[0-9].*)
        echo "11.0"
        return 0
        ;;
    esac
  fi

  echo "$MACOSX_DEPLOYMENT_TARGET"
}

normalize_arch() {
  case "$1" in
    x86_64|amd64)
      echo "x86_64"
      ;;
    arm64|aarch64)
      echo "arm64"
      ;;
    *)
      echo "Unsupported macOS arch: $1" >&2
      return 1
      ;;
  esac
}

rust_target_for_arch() {
  case "$1" in
    x86_64)
      echo "x86_64-apple-darwin"
      ;;
    arm64)
      echo "aarch64-apple-darwin"
      ;;
  esac
}

vcpkg_triplet_for_arch() {
  case "$1" in
    x86_64)
      echo "x64-osx"
      ;;
    arm64)
      echo "arm64-osx"
      ;;
  esac
}

ensure_rust_target() {
  local rust_target="$1"

  if rustup target list --installed | grep -qx "$rust_target"; then
    return 0
  fi

  echo "Missing Rust target $rust_target. Install it with:" >&2
  echo "  rustup target add $rust_target" >&2
  return 1
}

ensure_vcpkg_triplet() {
  local triplet="$1"

  if [[ -d "$VCPKG_INSTALLED_ROOT/$triplet/lib" ]]; then
    return 0
  fi

  echo "Missing vcpkg triplet $triplet under $VCPKG_INSTALLED_ROOT." >&2
  echo "Install it with your local vcpkg, for example:" >&2
  echo "  vcpkg install --triplet $triplet" >&2
  return 1
}

stage_flutter_rust_dylib() {
  local rust_target="$1"
  local dylib_src="$WORKSPACE_DIR/target/$rust_target/release/liblibrustdesk.dylib"
  local dylib_dest="$WORKSPACE_DIR/target/release/liblibrustdesk.dylib"

  if [[ ! -f "$dylib_src" ]]; then
    echo "Rust dylib not found: $dylib_src" >&2
    return 1
  fi

  mkdir -p "$(dirname "$dylib_dest")"
  cp -f "$dylib_src" "$dylib_dest"
}

detect_built_app() {
  local release_dir="$1"
  local app_path

  app_path="$(find "$release_dir" -maxdepth 1 -type d -name '*.app' | sort | head -n 1)"
  if [[ -z "$app_path" ]]; then
    echo "No app bundle found under $release_dir" >&2
    return 1
  fi

  echo "$app_path"
}

sign_app() {
  local app_path="$1"
  local sign_args=(--force --deep --sign "$SIGN_IDENTITY")

  if [[ ! -f "$RELEASE_ENTITLEMENTS" ]]; then
    echo "Release entitlements not found: $RELEASE_ENTITLEMENTS" >&2
    return 1
  fi

  if [[ "$SIGN_IDENTITY" != "-" ]]; then
    sign_args+=(--options runtime)
  fi

  sign_args+=(--entitlements "$RELEASE_ENTITLEMENTS" "$app_path")

  codesign "${sign_args[@]}"
  codesign --verify --deep --strict "$app_path"
}

create_manual_override_dmg() {
  local app_path="$1"
  local arch="$2"
  local version="$3"
  local product_name="${4:-$(basename "$app_path" .app)}"
  local stage_dir="$DMG_STAGE_ROOT/$arch"
  local dmg_path="$OUTPUT_DIR/${product_name}-${version}-manual-override-${arch}.dmg"

  rm -rf "$stage_dir"
  mkdir -p "$stage_dir"

  ditto "$app_path" "$stage_dir/$(basename "$app_path")"
  ln -s /Applications "$stage_dir/Applications"

  rm -f "$dmg_path"
  hdiutil create \
    -volname "$product_name $arch" \
    -srcfolder "$stage_dir" \
    -ov \
    -format UDZO \
    "$dmg_path" >/dev/null

  echo "$dmg_path"
}

build_arch_package() {
  local raw_arch="$1"
  local arch rust_target vcpkg_triplet derived_data_path release_dir app_src app_name product_name out_dir out_app version dmg_path deployment_target

  arch="$(normalize_arch "$raw_arch")"
  rust_target="$(rust_target_for_arch "$arch")"
  vcpkg_triplet="$(vcpkg_triplet_for_arch "$arch")"
  deployment_target="$(deployment_target_for_arch "$arch")"
  derived_data_path="$DERIVED_DATA_ROOT/$arch"
  release_dir="$derived_data_path/Build/Products/Release"
  version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$WORKSPACE_DIR/Cargo.toml" | head -n 1)"

  echo "==> Preparing $arch build"
  echo "==> Using MACOSX_DEPLOYMENT_TARGET=$deployment_target"
  ensure_rust_target "$rust_target"
  ensure_vcpkg_triplet "$vcpkg_triplet"

  echo "==> Building Rust artifacts for $rust_target"
  MACOSX_DEPLOYMENT_TARGET="$deployment_target" cargo build \
    --target "$rust_target" \
    --features "$RUST_FEATURES" \
    --release

  echo "==> Staging Rust dylib for Flutter"
  stage_flutter_rust_dylib "$rust_target"

  echo "==> Building macOS app bundle for $arch"
  rm -rf "$derived_data_path"
  (
    cd "$WORKSPACE_DIR/flutter/macos"
    MACOSX_DEPLOYMENT_TARGET="$deployment_target" xcodebuild \
      -workspace Runner.xcworkspace \
      -scheme Runner \
      -configuration Release \
      -derivedDataPath "$derived_data_path" \
      -destination 'generic/platform=macOS' \
      ARCHS="$arch" \
      ONLY_ACTIVE_ARCH=YES \
      build
  )

  app_src="$(detect_built_app "$release_dir")"
  app_name="$(basename "$app_src")"
  product_name="${app_name%.app}"

  echo "==> Injecting service binary into $app_name"
  cp -f "$WORKSPACE_DIR/target/$rust_target/release/service" "$app_src/Contents/MacOS/"

  out_dir="$OUTPUT_DIR/$arch"
  out_app="$out_dir/$app_name"
  rm -rf "$out_dir"
  mkdir -p "$out_dir"
  ditto "$app_src" "$out_app"

  echo "==> Ad-hoc signing $app_name"
  sign_app "$out_app"

  echo "==> Packaging DMG for $arch"
  dmg_path="$(create_manual_override_dmg "$out_app" "$arch" "$version" "$product_name")"

  echo "Built app: $out_app"
  echo "Built dmg: $dmg_path"
}

main() {
  local raw_arch

  mkdir -p "$OUTPUT_DIR"
  mkdir -p "$DERIVED_DATA_ROOT"
  mkdir -p "$DMG_STAGE_ROOT"

  cd "$WORKSPACE_DIR"

  for raw_arch in $MACOS_ARCHES; do
    build_arch_package "$raw_arch"
  done
}

main "$@"