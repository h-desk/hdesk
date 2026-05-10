#!/usr/bin/env bash

set -euo pipefail

DMG_NAME="HDesk-${VERSION}.dmg"

echo $MACOS_CODESIGN_IDENTITY
cargo install flutter_rust_bridge_codegen --version 1.80.1 --features uuid
cd flutter; flutter pub get; cd -
~/.cargo/bin/flutter_rust_bridge_codegen --rust-input ./src/flutter_ffi.rs --dart-output ./flutter/lib/generated_bridge.dart --c-output ./flutter/macos/Runner/bridge_generated.h
./build.py --flutter
rm -f "$DMG_NAME"
# security find-identity -v
codesign --force --options runtime -s $MACOS_CODESIGN_IDENTITY --deep --strict ./flutter/build/macos/Build/Products/Release/HDesk.app -vvv
create-dmg --volname "HDesk Installer" --icon "HDesk.app" 200 190 --hide-extension "HDesk.app" --window-size 800 400 --app-drop-link 600 185 "$DMG_NAME" ./flutter/build/macos/Build/Products/Release/HDesk.app
codesign --force --options runtime -s $MACOS_CODESIGN_IDENTITY --deep --strict "$DMG_NAME" -vvv
# notarize the HDesk-${VERSION}.dmg
rcodesign notary-submit --api-key-path ~/.p12/api-key.json --staple "$DMG_NAME"
