@echo off
cd /d D:\ideas\rustdesk
set "PATH=D:\software\flutter\bin;%PATH%"
set RUST_LOG=info
echo [1/3] Generating FRB bindings...
"C:\Users\keke.qiu\.cargo\bin\flutter_rust_bridge_codegen.exe" ^
  --rust-input "src\flutter_ffi.rs" ^
  --dart-output "flutter\lib\generated_bridge.dart" ^
  --rust-output "src\bridge_generated.rs" ^
  --skip-deps-check ^
  --llvm-path "D:\software\DevEco Studio\sdk\default\openharmony\native\llvm"
if %ERRORLEVEL% neq 0 (
  echo [FRB codegen FAILED]
  exit /b 1
)
echo [2/3] Regenerating freezed files via build_runner...
cd /d D:\ideas\rustdesk\flutter
flutter pub run build_runner build --delete-conflicting-outputs
if %ERRORLEVEL% neq 0 (
  echo [build_runner FAILED]
  exit /b 1
)
echo [3/3] Building Flutter Windows...
cd /d D:\ideas\rustdesk
python build.py --flutter --skip-portable-pack
