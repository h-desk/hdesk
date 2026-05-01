# Windows Build And Deploy

## Scope

This reference is for the Windows desktop RustDesk repo at `D:\ideas\rustdesk`.

Use it when you need to:

- validate Rust desktop changes quickly
- build a Flutter desktop runner
- package a Windows deliverable
- deploy or test against a reachable remote host

## Preflight

Run these first:

```powershell
git -C D:\ideas\rustdesk status --short
cd D:\ideas\rustdesk
cargo check --lib
```

If `flutter` is missing from `PATH`, prepend the local SDK:

```powershell
$env:Path = "D:\software\flutter\bin;$env:Path"
& "D:\software\flutter\bin\flutter.bat" --version
```

Useful environment checks:

```powershell
python --version
cargo --version
Get-ChildItem Env:VCPKG*
```

## Generate FRB Bindings

If Flutter desktop Rust code fails with errors around `bridge_generated` or
`IntoIntoDart`, generate the Flutter Rust Bridge files first:

```powershell
cd D:\ideas\rustdesk
$env:Path = "D:\software\flutter\bin;$env:Path"
$env:RUST_LOG = "info"
& "C:\Users\keke.qiu\.cargo\bin\flutter_rust_bridge_codegen.exe" `
  --rust-input ".\src\flutter_ffi.rs" `
  --dart-output ".\flutter\lib\generated_bridge.dart" `
  --rust-output ".\src\bridge_generated.rs" `
  --skip-deps-check `
  --llvm-path "D:\software\DevEco Studio\sdk\default\openharmony\native\llvm"
```

Expected outputs:

- `D:\ideas\rustdesk\src\bridge_generated.rs`
- `D:\ideas\rustdesk\flutter\lib\generated_bridge.dart`

Notes:

- On this machine, `flutter_rust_bridge_codegen` 1.80.1 is already installed at
  `C:\Users\keke.qiu\.cargo\bin\flutter_rust_bridge_codegen.exe`.
- If `flutter` is not in `PATH`, the code generator fails during the Dart
  `build_runner` step.
- If `libclang.dll` is missing, point `--llvm-path` at the local OpenHarmony LLVM
  tree shown above. It contains `bin\libclang.dll`.

## Fast Iteration

For Rust-only logic changes, use this first:

```powershell
cd D:\ideas\rustdesk
cargo check --lib
```

If you already have a previous Flutter runner build and only changed Rust code, you can rebuild the DLL only:

```powershell
cd D:\ideas\rustdesk
cargo build --features flutter,hwcodec --lib --release
Copy-Item "target\release\librustdesk.dll" `
  "flutter\build\windows\x64\runner\Release\librustdesk.dll" -Force
Copy-Item "target\release\deps\dylib_virtual_display.dll" `
  "flutter\build\windows\x64\runner\Release\dylib_virtual_display.dll" -Force
```

For Harmony or other mobile-client real-device validation, `hwcodec` is required. If `librustdesk.dll` is around 27MB and the desktop log shows `usable: ... h264=false` followed by `encoder: VP9`, the wrong build was deployed. The expected hwcodec build on this machine is about 31MB.

Only use the DLL hot-swap path if `flutter\build\windows\x64\runner\Release\` already exists and matches the current source tree closely.

## Build Runner Only

This is the safest Windows desktop smoke-test build:

```powershell
cd D:\ideas\rustdesk
$env:Path = "D:\software\flutter\bin;$env:Path"
python build.py --flutter --skip-portable-pack
```

Notes:

- In this repo, Windows Flutter builds now auto-enable `hwcodec` even if the flag is omitted, because non-hwcodec DLLs have repeatedly caused black screens with Harmony clients.
- The Windows runner now blocks startup if the sibling `librustdesk.dll` is clearly below the validated hwcodec size threshold, instead of silently launching into a black-screen-prone state.

Expected outputs:

- Runner folder: `D:\ideas\rustdesk\flutter\build\windows\x64\runner\Release\`
- Main executable: `D:\ideas\rustdesk\flutter\build\windows\x64\runner\Release\hdesk.exe`
- Rust DLL: `D:\ideas\rustdesk\target\release\librustdesk.dll`

What `build.py` does on Windows Flutter builds:

1. Builds `libs/virtual_display/dylib`
2. Builds Rust with `cargo build --features flutter,hwcodec --lib --release`
3. Runs `flutter build windows --release`
4. Copies `target\release\deps\dylib_virtual_display.dll` into the runner output

## Full Windows Package

Use this when the user asks for a deliverable build instead of a local runner folder:

```powershell
cd D:\ideas\rustdesk
$env:Path = "D:\software\flutter\bin;$env:Path"
python build.py --flutter
```

Expected outputs:

- Runner folder: `D:\ideas\rustdesk\flutter\build\windows\x64\runner\Release\`
- Packaged EXE: `D:\ideas\rustdesk\rustdesk-<version>-install.exe`

The pack step uses:

- `D:\ideas\rustdesk\libs\portable\generate.py`
- `D:\ideas\rustdesk\libs\portable\requirements.txt`

If the portable pack step fails but the runner build succeeds, keep the runner output and retry packaging separately after fixing Python dependencies.

## Deployment

Before deploying to a remote target, confirm reachability:

```powershell
Test-NetConnection -ComputerName <ip> -Port <port>
```

Recommended deployment choices:

- If the target machine just needs a runnable client:
  - ship `rustdesk-<version>-install.exe`
- If Flutter Windows runner build is blocked by local symlink restrictions but you already
  have a matching installed client on the build machine:
  - copy the installed folder to a new deploy directory
  - replace `librustdesk.dll` with `target\release\librustdesk.dll`
  - replace `dylib_virtual_display.dll` with `target\release\deps\dylib_virtual_display.dll`
  - validate with `rustdesk.exe --version`
- If the target machine already has a matching runner tree and you only changed Rust logic:
  - use the DLL hot-swap path
- If the target IP/port is unreachable from the current machine:
  - do not claim deployment succeeded
  - keep the built artifacts locally and report the blocker

## Current Project-Specific Notes

- Custom server endpoints:
  - `hdesk.yunjichuangzhi.cn:21116`
  - `hdesk.yunjichuangzhi.cn:21117`
- Desktop direct-connect work recently touched:
  - `src\client.rs`
  - `src\common.rs`
- Avoid touching `src\rendezvous_mediator.rs` unless required by the task.
- The repo may contain unrelated local modifications. Work around them; do not reset them.

## Common Failure Modes

- `flutter` not recognized:
  - prepend `D:\software\flutter\bin` to `PATH`
- Windows Flutter build fails with `Building with plugins requires symlink support`:
  - enable Windows Developer Mode, or build from an elevated environment with symlink privilege
  - if you only changed Rust code, use the DLL hot-swap deployment path instead of blocking on Flutter
- `cargo check --lib` passes but package build fails:
  - usually Flutter environment, portable pack dependencies, or stale runner artifacts
- `build.py --flutter` fails in the pack stage:
  - retry with `--skip-portable-pack` to get a usable runner build first
- Remote deploy host not reachable:
  - keep artifacts local and report that deployment is blocked by network reachability
