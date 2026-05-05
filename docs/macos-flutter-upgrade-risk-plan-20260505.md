# macOS Flutter 3.41.4 升级风险评估与接力计划

## 目标

这份文档的目标不是立即把 macOS Flutter 基线升级到 3.41.4，而是把升级面、风险分级、停手条件和建议顺序固定下来，供后续 mac 端 AI 继续执行。

本轮结论偏长期主义：

- Windows 空闲 CPU 根因已经确认主要是 Flutter Windows 3.41.2 基线问题，升级到 3.41.4 后问题消失。
- 但这个结论不能直接外推到 macOS。
- macOS 应先吃到共享层优化，再做桌面端单独验证，最后才考虑 CI 升级。

## 当前已确认状态

### 共享层优化已经天然覆盖 macOS

当前已保留的这些优化都在 Flutter Dart 层或 Rust 状态聚合层，macOS 构建一旦使用当前代码，就会自然带上：

- 状态聚合与 bridge 收敛：`src/ui_interface.rs` + `flutter/lib/models/server_model.dart`
- recent peers 事件驱动同步：`flutter/lib/models/ab_model.dart`
- 在线查询收敛：`flutter/lib/common/widgets/peers_view.dart`
- 连接页去 1 秒轮询：`flutter/lib/desktop/pages/connection_page.dart`
- 首页按需权限监听：`flutter/lib/desktop/pages/desktop_home_page.dart`
- 定时器 in-flight 防重入：`flutter/lib/common.dart`
- 0/1 tab 快路径：`flutter/lib/desktop/widgets/tabbar_widget.dart`

### 本地 macOS 入口当前不锁 Flutter 版本

桌面仓的本地入口在：

- `D:\ideas\rustdesk\build.py`

当前特征：

- macOS Flutter 产物目录是 `flutter/build/macos/Build/Products/Release/`
- `build_flutter_dmg()` 走的是 `flutter build macos --release`
- 本地 macOS 构建实际吃哪个 Flutter，取决于执行机器 `PATH` 里的 flutter
- 本地 `build.py` 仍使用 `MACOSX_DEPLOYMENT_TARGET=10.14`

### CI 现在已经做了一个低风险解耦，但还没有真正升级 macOS Flutter

桌面仓 CI 关键文件是：

- `D:\ideas\rustdesk\.github\workflows\flutter-build.yml`

当前状态：

- Windows 已经使用 `WINDOWS_FLUTTER_VERSION=3.41.4`
- macOS desktop 现在单独拆出了 `MACOS_FLUTTER_VERSION=3.24.5`
- iOS / Web / Linux 仍然继续跟随共享的 `FLUTTER_VERSION=3.24.5`

这意味着：

- 以后如果只想试 macOS desktop 升级，可以先只改 `MACOS_FLUTTER_VERSION`
- 不会再被迫同时带动 iOS / Web / Linux
- 但这次还没有真正把 macOS desktop 提到 3.41.4，只是先把风险面拆开了

## 风险分级

### 低风险：只做本地 macOS 机器验证

建议内容：

- 在单台 mac 开发机上把 flutter 指向 3.41.4
- 不动 CI，不动发布链，不动 iOS / Web / Linux
- 只验证桌面端 `flutter build macos --release`

为什么是低风险：

- 失败只影响单台机器
- 不会污染 release/workflow
- 能最快回答“macOS 是否也需要 3.41.4”

### 中风险：只升级 macOS desktop CI

建议内容：

- 只把 `MACOS_FLUTTER_VERSION` 从 3.24.5 提到 3.41.4
- 不动 `FLUTTER_VERSION`
- 让 `build-for-macOS` 两个矩阵目标先验证

为什么是中风险：

- 会碰到 codesign、notarization、create-dmg、Pod、Xcode 工程生成
- 但 blast radius 已经缩到 macOS desktop，不再联动 iOS / Web / Linux

### 高风险：直接升级共享 `FLUTTER_VERSION`

当前不建议做。

原因：

- 共享 `FLUTTER_VERSION` 仍被 iOS、Web、Linux 使用
- 一次修改会把问题从 macOS desktop 扩展到多条平台链路
- 在没有先证明 macOS desktop 自己能稳定通过之前，这个风险没有意义

## 需要重点盯住的风险点

### 1. 3.24.5 专用 patch 不会自动跟到 3.41.4

CI 里有一个条件 patch：

- `flutter_3.24.4_dropdown_menu_enableFilter.diff`

它当前只会在版本是 3.24.5 时应用。macOS desktop 如果改到 3.41.4，这个 patch 默认不会再打。

这不一定是坏事，但需要确认：

- 这个 patch 是否只是旧版本兼容修补
- 还是桌面 UI 某些交互确实依赖它

### 2. macOS workflow 里有一个对 Flutter 源码的硬编码 workaround

`flutter-build.yml` 会对 Flutter 源码里的 `binding.dart` 做一条 `sed` 修改，标注为 workaround Flutter issue 133533。

这一步在 3.41.4 上有三种可能：

- 仍然适用
- 已经不需要
- 因源码结构变化而失效甚至打坏构建

这是升级时必须优先验证的点。

### 3. 本地 mac 构建目标版本与 CI arm64 不一致

当前差异：

- 本地 `build.py` 仍按 `10.14`
- CI 在 arm64 job 里会临时把多个文件抬到 `12.3`

这意味着后续 mac AI 需要分清：

- 本地 smoke build 是否只是为了验证构建可过
- 还是要和 CI/release 的最小系统版本保持一致

### 4. macOS 发布链路本身就比 Windows 更脆弱

升级 Flutter 版本后，需要重点盯住：

- `flutter/macos/Podfile`
- `flutter/macos/Runner.xcodeproj/project.pbxproj`
- `Cargo.toml` 里的 `osx_minimum_system_version`
- codesign
- notarization
- `create-dmg`

如果这些链路任何一个先炸，不要同时继续推进更大范围的版本统一。

### 5. macOS branding 仍可能保留 RustDesk 历史值

已知本地记忆显示：

- 产物名仍可能是 `RustDesk.app`
- bundle identifier 仍可能不是 HDesk 的最终品牌值

这不是 Flutter 版本升级本身的 blocker，但它会干扰对“升级是否成功”的判断，尤其在 codesign/notarization 阶段。

## 建议执行顺序

### 阶段 1：只做 mac 本地验证

目标：先回答“3.41.4 在 mac desktop 上是否可构建、是否值得继续”。

建议步骤：

1. 在单台 mac 机器上安装或切换到 Flutter 3.41.4
2. 跑 `flutter doctor -v`
3. 在 `D:\ideas\rustdesk` 执行桌面构建入口
4. 验证空闲 CPU、权限链路、菜单栏常驻、窗口关闭/重新激活、文件复制粘贴

通过标准：

- `flutter build macos --release` 可过
- 主界面空闲 CPU 没有明显恶化
- 授权后不会继续无意义轮询
- 菜单栏/窗口行为没有明显回归

### 阶段 2：只升级 macOS desktop CI

只有阶段 1 通过后，再考虑：

- 把 `MACOS_FLUTTER_VERSION` 提到 3.41.4
- 保持 `FLUTTER_VERSION=3.24.5` 不动

验证重点：

- x86_64 job
- aarch64 job
- arm64 的 `--screencapturekit`
- codesign / notarization / dmg 打包

### 阶段 3：再决定要不要动 iOS / Web / Linux

只有在 macOS desktop CI 连续稳定后，才有必要讨论是否调整共享 `FLUTTER_VERSION`。

这一步当前不建议提前做。

## 停手条件

满足任一条件就先停在当前阶段，不继续扩大范围：

- 本地 mac 3.41.4 构建直接卡在 Pod / Xcode / Flutter 源码 patch
- 空闲 CPU 没有改善，或者已经被共享层优化吃掉主要收益
- menu bar / window focus / permission flow 出现明显回归
- macOS CI 的 codesign / notarization / dmg 链路出现新故障

## 建议交给 mac 端 AI 的起始检查点

建议下一位 mac 端 AI 从这几个文件开始：

- `D:\ideas\rustdesk\build.py`
- `D:\ideas\rustdesk\.github\workflows\flutter-build.yml`
- `D:\ideas\rustdesk\flutter/macos/Podfile`
- `D:\ideas\rustdesk\flutter/macos/Runner.xcodeproj/project.pbxproj`
- `D:\ideas\rustdesk\Cargo.toml`

建议先回答这 3 个问题：

1. 单机 mac 3.41.4 能否完整构建 desktop release？
2. `binding.dart` workaround 在 3.41.4 上是否仍需保留？
3. macOS desktop 升级后，是否值得继续推进到 CI？

## 本轮最终建议

本轮不直接在仓库里把 macOS Flutter 版本升到 3.41.4。

理由不是反对升级，而是当前最合理的长期策略应该是：

- 先隔离 blast radius
- 再做单机验证
- 再做 mac desktop CI
- 最后才考虑跨平台统一

这条路径长期成本更低，也更适合让后续 mac 端 AI 接力执行。