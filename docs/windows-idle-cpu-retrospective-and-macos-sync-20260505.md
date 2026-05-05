# Windows 空闲 CPU 复盘与 macOS 同步计划

## 结论

- 这次 Windows 空闲 CPU 异常的根因已经确认是 Flutter Windows 3.41.2 引擎基线，而不是 HDesk 的页面树、插件注册或业务轮询本身。
- Windows 升级到 Flutter 3.41.4 后，`engine_only` 和完整应用空闲态 CPU 都明显回落，不需要再单独引入 182822 的补丁链。
- 本轮代码清理的目标不是回退所有改动，而是删除排障期临时探针，同时保留已经证明有价值的跨平台优化。

## 直接回答当前问题

- 是，之前把 Windows 空闲 CPU 问题真正压下来的主因，就是把 Flutter Windows 基线从 3.41.2 升到 3.41.4。
- 共享层优化也有价值，但它们解决的是轮询、bridge 调用和页面空转问题；这次 Windows 上最关键的根因修复仍然是 Flutter SDK / Engine 升级。
- 这些共享层优化大部分对 macOS 也通用，因为它们主要发生在 Flutter Dart 层和 Rust 状态聚合层。
- macOS 需要特殊适配的不是这些共享优化本身，而是权限、启动方式、菜单栏/托盘形态，以及发布链路。

## 本轮保留的优化

这些改动属于真实的运行时收敛优化，应该继续保留，并作为 macOS 同步的主体：

- `src/ui_interface.rs` 与 `flutter/lib/models/server_model.dart`
  - 把桌面端常用状态聚合到一次状态读取里，减少 Dart 到 Rust 的重复 bridge 调用。
  - `server_model` 端增加 in-flight 保护，把主桌面状态轮询收敛到 2 秒一个共享路径。
- `flutter/lib/models/ab_model.dart`
  - 用事件驱动替代定时 recent peers 同步，避免常驻 500ms 轮询。
- `flutter/lib/common/widgets/peers_view.dart`
  - recent peers 页面加载时不再立刻强制 `_queryOnlines(true)`，只在真实变化后再查询。
- `flutter/lib/desktop/pages/connection_page.dart`
  - 去掉 1 秒一次的本地状态轮询，在线状态改为消费共享状态。
- `flutter/lib/desktop/pages/desktop_home_page.dart`
  - 首页改成按需刷新；macOS 权限监听仅在用户刚触发权限申请后才启动。
  - 这部分对 macOS 最重要，因为屏幕录制、输入监听、辅助权限都容易被错误地做成常驻轮询。
- `flutter/lib/common.dart`
  - `periodic_immediate` 增加 in-flight 保护，避免异步回调重叠。
  - `start_service` 现在会同步刷新 `stop-service` 的响应式状态，减少 UI 与真实状态不一致。
- `flutter/lib/desktop/widgets/tabbar_widget.dart`
  - 0 个 tab 直接返回空，1 个 tab 直接返回页面，避免不必要的 `PageView` 包裹。
- `flutter/lib/desktop/pages/remote_tab_page.dart`
  - 关闭空窗口时保留 `setPreventClose(false)`，这是实际行为修复，不属于 probe。

## 本轮删除的临时内容

这些内容只为 Windows CPU 取证服务，现在已经应该收掉：

- `flutter/windows/runner/flutter_window.cpp`
  - 删除 `HDESK_PROBE_NATIVE_WINDOW_ONLY`
  - 删除 `HDESK_PROBE_SKIP_PLUGIN_REGISTRATION`
  - 删除 `HDESK_PROBE_CREATE_CONTROLLER_ONLY`
  - 删除 `HDESK_PROBE_ENGINE_ONLY`
  - 删除窗口消息统计与日志落盘逻辑
- `flutter/windows/runner/main.cpp`
  - 删除 `HDESK_PROBE_UI_THREAD_PLATFORM`
- `flutter/lib/main.dart`
  - 删除安装页、主窗口、环境初始化相关的 probe 分支
- `flutter/lib/models/native_model.dart`
  - 删除绕过全局事件流、绕过 `mainInit`、提前返回的 probe 分支
- `flutter/lib/desktop/pages/desktop_home_page.dart`
  - 删除左右 pane 的 probe bypass 常量
- `flutter/lib/common.dart`
  - 删除本次 CPU 调查中加入的流程 trace 日志
- `flutter/lib/models/model.dart`
  - 删除本次会话相关的消息框 trace 日志
- `flutter/lib/desktop/pages/remote_tab_page.dart`
  - 删除本次会话相关的关窗 trace 日志

## 哪些内容不要同步到 macOS

以下内容是 Windows 特定实现或本轮调查工具，不应该被“照搬到 macOS”：

- Windows runner C++ probe 代码
- Windows 安装器/服务相关脚本和行为
- Windows 专用的 Flutter 3.41.2/3.41.4 根因结论本身

注意：Windows 的根因是 Windows 引擎基线，不意味着 macOS 也一定存在同一个引擎问题。macOS 需要先测量，再决定是否要同步升级 Flutter 基线。

## macOS 需要额外注意的适配点

- 权限链路
  - macOS 的屏幕录制、辅助功能、输入监听、麦克风授权状态变化更容易被误做成常驻轮询。
  - 当前 `desktop_home_page.dart` 的按需权限监听策略适合保留，但要重点验证授权完成后监听能否及时停掉。
- 启动与常驻模型
  - Windows 这里有安装器、服务、`--tray`、`--server` 这套链路；macOS 不应该照搬这套服务逻辑。
  - macOS 需要关注的是 app bundle 启动、登录项、菜单栏常驻和前后台切换，而不是 Windows Service。
- 菜单栏与窗口激活
  - macOS 的菜单栏常驻和窗口 focus/activate 语义与 Windows 不同，`--tray` 相关体验要单独实测。
  - 单 tab / 空 tab 快路径可以共用，但窗口关闭和重新激活行为要看 macOS 本地交互是否一致。
- 发布链路
  - 如果后面要统一 macOS Flutter 版本，除了本地构建入口，还要一起检查 CI、codesign、notarization、打包脚本。
  - 不要只升级某一台开发机的 Flutter SDK，否则很容易复现 Windows 这次“源码升级了，但实际运行物不是同一基线”的偏差。

## macOS 同步建议

详细的 macOS Flutter 基线升级风险评估与接力顺序，见 `Application/docs/macos-flutter-upgrade-risk-plan-20260505.md`。

### 第一阶段：直接吃到共享层优化

macOS 下一步不需要先做新的大改，只要基于当前共享 Flutter/Rust 代码构建一版，就已经会自然带上下面这些优化：

- `ui_interface.rs` + `server_model.dart` 的状态聚合
- `ab_model.dart` 的事件驱动 recent peers 同步
- `peers_view.dart` 的在线查询收敛
- `connection_page.dart` 的状态轮询移除
- `desktop_home_page.dart` 的按需权限监听
- `common.dart` 的定时器 in-flight 防重入
- `tabbar_widget.dart` 的 0/1 tab 快路径

### 第二阶段：只在 macOS 实测有问题时再升级 Flutter 基线

如果 macOS 空闲 CPU、窗口 idle 唤醒频率、权限弹框后的持续轮询仍然异常，再做下面的验证：

1. 确认当前 macOS 构建到底在用哪个 Flutter 版本。
2. 如果 macOS 仍在 3.41.2 或其他更老基线，先用当前共享代码测空闲 CPU，再决定是否需要把 macOS Flutter 也统一到 3.41.4。
3. 如果要统一，优先更新 macOS 本地构建入口和 CI，不要只改某一个开发机路径。

## macOS 验证清单

建议按这个顺序执行：

1. 构建与版本确认
   - `flutter --version`
   - `flutter build macos --release`
2. 空闲态 CPU
   - 应用启动后停留在主界面 30 到 60 秒
   - 用 Activity Monitor 或 `top -pid <pid> -stats pid,cpu,threads,mem` 观察空闲占用
3. 权限链路
   - 验证屏幕录制、辅助功能、输入监听、音频录制提示卡片
   - 确认权限授予后，页面会收敛，不会继续 1 秒轮询
4. 连接页状态
   - 验证服务启动/停止、在线状态、视频连接数是否正常刷新
5. recent peers / tab 行为
   - 验证 recent peers 加载后不会立刻触发一轮无意义在线查询
   - 验证单 tab 和空 tab 状态下没有额外页面容器负担

## 推荐执行顺序

1. 先让 macOS 构建吃到当前共享层优化并做一次 idle 复测。
2. 只有在 macOS 仍出现明显 idle CPU 或唤醒异常时，再推进 Flutter 基线统一。
3. 如果要统一 Flutter 版本，把本地入口、CI 工作流、安装/打包脚本一起改，避免再次出现“源码已升级、实际运行仍是旧版本”的偏差。