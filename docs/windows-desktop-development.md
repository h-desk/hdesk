# HDesk Windows Desktop Development

## Scope

本文是 `D:\ideas\rustdesk` 的 Windows 桌面开发真源，覆盖本机工具链、
Flutter/Rust 构建边界、FRB 生成、hwcodec 和快速迭代。安装验收和正式发布
分别见：

- [Windows build and install validation](windows-build-install-validation.md)
- [Windows release standard](windows-release-standard.md)

## Validated Toolchain

- Flutter: `D:\software\flutter-3.41.4`
- Rust: 以仓库和 CI toolchain 配置为准
- FRB codegen: 本机已验证 `flutter_rust_bridge_codegen 1.80.1`
- C/C++ dependencies: vcpkg，静态 x64 Windows triplet

```powershell
$env:Path = "D:\software\flutter-3.41.4\bin;$env:Path"
& "D:\software\flutter-3.41.4\bin\flutter.bat" --version
cargo --version
Get-ChildItem Env:VCPKG*
```

如果 Flutter 首次下载 Windows engine 失败，可临时使用已验证镜像完成预缓存：

```powershell
$env:FLUTTER_STORAGE_BASE_URL = "https://storage.flutter-io.cn"
& "D:\software\flutter-3.41.4\bin\flutter.bat" precache --windows
```

不要长期把镜像变量写入项目配置；预缓存完成后恢复默认环境。

## Build Profiles

Rust-only 预检：

```powershell
cd D:\ideas\rustdesk
cargo check --lib
```

Windows runner，不生成 portable 包：

```powershell
python build.py --flutter --skip-portable-pack
```

本地完整 EXE 打包：

```powershell
python build.py --flutter
```

预期关键产物：

- `flutter\build\windows\x64\runner\Release\hdesk.exe`
- `target\release\librustdesk.dll`
- `target\release\deps\dylib_virtual_display.dll`
- `hdesk-{version}-install.exe`
- `hdesk-{version}-x86_64.exe`

Windows Flutter 构建会启用 `hwcodec`。`build.py` 当前用 29 MiB 作为
`librustdesk.dll` 的最低门禁；历史上的 27/31 MiB 只能用于诊断提示，不能
替代脚本检查。日志出现 `h264=false` 并回退 VP9 时，优先核对 DLL 是否来自
当前 hwcodec 构建。

正式 CI 的 unsigned profile 还会启用 `--portable --hwcodec --vram`。本地
`python build.py --flutter` 是开发预检，不代表已复现完整 CI profile。

仓根的 `build-win.bat` 只运行 Flutter build，不能保证 Rust DLL 是 fresh；
`build-flutter-only.bat` 和 `run-build.bat` 也是历史机器 helper。除非正在修复这些
脚本本身，否则以 `build.py` 和本文命令为准。

## Flutter Rust Bridge

出现 `bridge_generated`、`IntoIntoDart` 或 FFI 签名不一致错误时，重新生成：

```powershell
$env:Path = "D:\software\flutter-3.41.4\bin;$env:Path"
$env:RUST_LOG = "info"
& "C:\Users\keke.qiu\.cargo\bin\flutter_rust_bridge_codegen.exe" `
  --rust-input ".\src\flutter_ffi.rs" `
  --dart-output ".\flutter\lib\generated_bridge.dart" `
  --rust-output ".\src\bridge_generated.rs" `
  --skip-deps-check `
  --llvm-path "D:\software\DevEco Studio\sdk\default\openharmony\native\llvm"
```

OpenHarmony LLVM 路径在本机提供 `bin\libclang.dll`。生成后必须审查 Rust 和
Dart 两侧 diff，不能把生成失败后的半成品提交。

## Fast Rust Iteration

只有在 Flutter runner 与当前源码基线一致、且没有修改 `src/flutter_ffi.rs`、
生成 bridge 或其他 FFI 边界时，才允许热替换 DLL：

```powershell
cargo build --features flutter,hwcodec --lib --release
Copy-Item target\release\librustdesk.dll `
  flutter\build\windows\x64\runner\Release\librustdesk.dll -Force
Copy-Item target\release\deps\dylib_virtual_display.dll `
  flutter\build\windows\x64\runner\Release\dylib_virtual_display.dll -Force
```

FFI、Dart UI、plugin、runner 或资源变更必须完整重建。启动新 runner 前先停止
旧的 runner、安装版和遗留 installer；无法确认旧进程已退出时，不能宣称运行的
是最新构建。

## Verification

按改动范围选择验证：

```powershell
cargo check --lib
cd flutter
flutter test
flutter analyze --no-fatal-warnings --no-fatal-infos <changed-files>
```

多窗口、DPI 或 native crash 改动还必须执行
[multi-window regression matrix](windows-multi-window-and-crash-diagnostics.md)。
