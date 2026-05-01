# RustDesk macOS AI 执行说明（2026-05-01）

这份说明是给 macOS 上的 AI 直接参考执行的，目标是把当前 HDesk 对应的 RustDesk fork 拉起到可调试状态，并在需要时产出 macOS release 包。

## 先说结论

- 这次必须基于已经包含提交 `caca65e` 的分支或提交来拉取代码。
- 这个提交已经把 `vendor/hwcodec`、`vendor/machine-uid`、`vendor/magnum-opus` 从 gitlink 改成了主仓普通目录；如果 Mac 上 clone 下来后这三个目录又表现成“子模块占位”或“空目录”，说明拿到的不是新提交。
- `libs/hbb_common` 仍然是正常子模块，所以 clone 时要带 `--recurse-submodules`，或者 clone 后执行 `git submodule update --init --recursive`。
- Flutter 的 macOS 工程已经存在，仓内可见 `flutter/macos/Runner.xcodeproj`、`flutter/macos/Podfile`、`flutter/macos/Runner/Configs/AppInfo.xcconfig`。
- 当前 macOS 产物名称仍然是 `RustDesk.app`，不是 `HDesk.app`。这是仓内现状，不是构建失败。
- 当前 macOS Flutter 工程最低目标版本是 `10.14`。

## 仓内已验证事实

- `build.py` 在 macOS Flutter 路径下会把输出目录设为 `flutter/build/macos/Build/Products/Release/`。
- `build.py` 的 macOS Flutter 打包逻辑会先执行：
  - `MACOSX_DEPLOYMENT_TARGET=10.14 cargo build --features flutter --release`
  - `cd flutter && flutter build macos --release`
  - 再把 `target/release/service` 复制到 `RustDesk.app/Contents/MacOS/`
- `flutter/macos/Podfile` 里是 `platform :osx, '10.14'`。
- `flutter/macos/Runner/Configs/AppInfo.xcconfig` 里当前还是：
  - `PRODUCT_NAME = RustDesk`
  - `PRODUCT_BUNDLE_IDENTIFIER = com.carriez.flutterHbb`
- 根仓 `README.md` 的依赖说明明确要求：
  - 安装 Rust 开发环境
  - 安装 C++ 构建环境
  - 安装 `vcpkg`
  - 对 Linux/macOS 安装 `libvpx libyuv opus aom`

## macOS 机器上的建议执行顺序

下面这条路径优先保证“先跑起来，再继续改”。如果 AI 只想最小代价验证桌面端能起，按这个顺序走。

### 1. 拉代码

```bash
git clone --recurse-submodules <你的-rustdesk-fork-url> rustdesk
cd rustdesk
git checkout <包含-caca65e-的分支或提交>
git submodule update --init --recursive
```

检查点：

- `vendor/hwcodec`
- `vendor/machine-uid`
- `vendor/magnum-opus`

这三个目录应该直接有源码文件，不应该再显示成独立子模块入口。

### 2. 先确认本机基础工具

先检查这些命令是否可用：

```bash
xcodebuild -version
flutter --version
flutter doctor -v
rustc --version
cargo --version
python3 --version
pod --version
```

如果这里缺任何一项，先补齐再继续。

### 3. 准备 vcpkg 依赖

仓内 README 明确要求 macOS 侧准备以下库：

```bash
export VCPKG_ROOT="$HOME/vcpkg"
"$VCPKG_ROOT"/vcpkg install libvpx libyuv opus aom
```

如果 `VCPKG_ROOT` 不在这个位置，改成你的真实路径。

### 4. 拉 Flutter 依赖

```bash
cd flutter
flutter pub get
cd ..
```

### 5. 先做一轮最窄 Rust 验证

```bash
cargo check --lib
```

这一步是最便宜的验证。Windows 这边在 vendor 扁平化提交后已经通过，Mac 侧如果失败，优先看环境差异，不要先怀疑 vendor flatten 本身。

### 6. 生成 macOS 运行所需 Rust 产物

先按仓里 `build.py` 的 macOS 逻辑执行：

```bash
MACOSX_DEPLOYMENT_TARGET=10.14 cargo build --features flutter --release
```

说明：

- 这里不要只跑 `cargo build --features flutter --lib --release`。
- `build.py` 在 macOS 路径里用的是不带 `--lib` 的 `cargo build --features flutter --release`，因为后面还需要 `target/release/service`。

### 7. 构建 Flutter macOS app

```bash
cd flutter
flutter build macos --release
cp -f ../target/release/service build/macos/Build/Products/Release/RustDesk.app/Contents/MacOS/
open build/macos/Build/Products/Release/RustDesk.app
```

如果这里能正常起 app，说明“Mac clone + vendor flatten + Flutter macOS 工程”这条链路已经通了。

## 如果只是想本地调试，不急着打包 DMG

优先走上面的 release 路径。原因很简单：

- 仓内已验证的 macOS 自动流程本来就是 `cargo build` + `flutter build macos` + 手动复制 `service`。
- `flutter run -d macos` 虽然理论上可以试，但仓里明确存在 `service` 注入这一步，所以直接走 release 路径更接近真实产物。

如果一定要试运行态调试，可以这样试：

```bash
cd flutter
flutter run -d macos
```

但如果出现“app 能起来、后台服务行为不完整、控制链路异常、或 service 缺失”这类问题，直接回退到上一节的 release 路径，不要在 `flutter run` 上死磕。

## 如果要产出 DMG

仓里的标准入口是：

```bash
python3 build.py --flutter
```

说明：

- 这条命令会走 `build_flutter_dmg(...)`。
- 它会调用 `flutter build macos --release`。
- 它会把 `target/release/service` 复制进 app bundle。
- 它还会调用 `create-dmg`。

所以如果只是调试，不建议第一步就跑它；如果要正式产出 DMG，再跑这条。

## 看到这些现象时，不要误判

### 1. 构建成功但产物还是 `RustDesk.app`

这是仓内当前真实配置，不是失败。macOS Flutter Runner 里 `PRODUCT_NAME` 还是 `RustDesk`。

### 2. `vendor` 看起来又像子模块

先看你 checkout 的是不是包含 `caca65e` 的提交。只要分支正确，这 3 个 vendor 目录现在就应该是普通目录。

### 3. `flutter build macos` 成功，但运行时行为异常

先确认这一步有没有做：

```bash
cp -f ../target/release/service build/macos/Build/Products/Release/RustDesk.app/Contents/MacOS/
```

仓内 `build.py` 明确做了这一步，所以漏掉它会让运行时结果偏离标准打包路径。

### 4. 只 clone 主仓但没拉子模块

`libs/hbb_common` 仍然需要正常初始化，缺它不属于这次 vendor flatten 的问题。

## 建议 AI 在 Mac 上的决策顺序

如果你让另一个 AI 直接执行，建议它按这个判断顺序来：

1. 先确认当前分支是否包含 `caca65e`。
2. 再确认 `vendor/hwcodec`、`vendor/machine-uid`、`vendor/magnum-opus` 是普通目录而不是 gitlink 占位。
3. 再确认 `git submodule update --init --recursive` 已完成。
4. 再确认 `VCPKG_ROOT` 和 `libvpx/libyuv/opus/aom` 已就绪。
5. 先跑 `cargo check --lib`。
6. 再跑 `MACOSX_DEPLOYMENT_TARGET=10.14 cargo build --features flutter --release`。
7. 再跑 `flutter build macos --release`。
8. 最后复制 `target/release/service` 到 `RustDesk.app/Contents/MacOS/` 并启动 app。

## 当前最值得记住的一点

这次 Windows 上已经验证过：`caca65e` 之后，vendor flatten 提交本身是可提交、可校验、且 `cargo check --lib` 能通过的。Mac 上如果再出问题，优先从本机环境、子模块初始化、vcpkg、Xcode/Pod/Flutter 工具链这几项查，不要先把问题归因到这次 vendor 目录处理。