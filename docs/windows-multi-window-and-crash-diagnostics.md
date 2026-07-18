# Windows 多窗口与原生崩溃诊断手册

本文记录 HDesk Windows 桌面端多窗口生命周期、对话框尺寸和原生崩溃的已确认事实、修复约束与回归方法。目标是让后续排查基于可复现证据和统一时间线，而不是根据崩溃前最后一个 Dart 业务动作猜测根因。

适用范围：

- Flutter Windows 主窗口与 `DesktopMultiWindow` 子窗口。
- 远程会话断开、重连、窗口复用及快速连续操作。
- 设置窗口、密码窗口等独立子窗口的 DPI、自适应布局与窗口样式。
- `flutter_windows.dll`、Flutter accessibility bridge 或窗口生命周期相关的 native crash。

## 1. 先区分三类问题

排查时先判断问题属于哪一层，避免把不同故障混为一谈。

| 类型 | 典型表现 | 优先证据 | 首要检查 |
| --- | --- | --- | --- |
| 连接控制 | 点击断开后仍保持连接、按钮可重复点击 | 主日志、`flutter_crash_trace.log`、会话快照 | 是否直接调用 `cmCloseConnection`，pending 是否收敛 |
| 多窗口生命周期 | 断开后重连、连点或复用窗口时崩溃，trace 出现 `target window not found` | tracked window 列表、实际子窗口列表、调用 trace | stale `windowId` 是否被 reconcile/forget |
| Windows 原生窗口/渲染 | 拖拽窗口边框时稳定崩溃，WER 指向 `flutter_windows.dll` | WER、dump、符号栈、精确复现动作 | 窗口是否仍可 resize，崩溃签名是否命中 accessibility bridge |

连接操作、自动重连 timer、FFI dispose 顺序和连续点击都可能放大问题，但不能仅凭它们发生在崩溃前就认定为根因。

## 2. 已确认事实

### 2.1 断开后重连/连点的 stale windowId 问题

已确认的故障链：

1. 主进程维护 remote 等子窗口的 tracked window 列表，以及 active/inactive 集合。
2. 子窗口真正失效后，旧实现没有及时把对应 `windowId` 从所有跟踪集合中移除。
3. 后续 `activeSession`、窗口复用、坐标查询或重连路径仍可能调用失效窗口。
4. trace 中出现 `target window not found`，与“账本认为窗口存在、插件实际已找不到窗口”的状态失配一致。
5. Rust 会话可能已经正常退出，native crash 随后才发生。因此只看连接日志会错误地把正常退出或最后一次点击当成崩溃根因。

当前代码的防护位于 `flutter/lib/utils/multi_window_manager.dart`：

- `_reconcileWindows` 用 `DesktopMultiWindow.getAllSubWindowIds()` 获取实际存活子窗口，剔除 stale ID。
- `_forgetWindow` 同时清理各类型 tracked list、active 集合和 inactive 集合。
- `_safeInvokeMethod` 捕获 `target window not found`，立即 forget 目标并允许调用方继续尝试其他候选窗口。
- `_newSession` 等可能被旁路直接调用的路径也必须先 reconcile，不能只保护公开入口。
- 主窗口不属于这套 stale subwindow 清理范围，禁止把主窗口 ID 误当成失效子窗口清理。

### 2.2 设置/密码窗口 resize crash

已确认的复现动作是拖拽设置窗口或密码窗口边框改变大小。对应的 Windows 崩溃证据为：

- faulting module：`flutter_windows.dll`
- exception code：`0xc0000005`，外层可表现为 `0xc000041d`
- module offset：`0x391b6`
- 符号位置：`ui::AXNode::id`
- 调用路径：`flutter::AccessibilityBridge::CreateRemoveReparentedNodesUpdate`

该签名说明本次 resize crash 的 native 触发点位于 Flutter accessibility bridge 更新语义树的过程。它不能被泛化为所有 `flutter_windows.dll` 崩溃的根因；后续事件必须重新核对模块版本、offset 和符号栈。

同时已确认：

- 设置和密码窗口由 `DesktopMultiWindow` 创建，每个子窗口拥有独立 Flutter engine。
- 这些 child engine 没有注册 `window_manager` 插件。
- 因此子窗口 Dart 入口调用 `window_manager.setResizable(false)` 并不能可靠修改 Windows 原生窗口样式。
- 在 Windows 上仅跳过该无效调用还不够，必须直接修改目标 HWND 的 style。
- 移除 `WS_THICKFRAME` 和 `WS_MAXIMIZEBOX` 后，用户不能再通过边框拖拽或最大化触发该 crash 路径。

这是针对已确认 Flutter engine crash 的产品级隔离策略。若未来升级 Flutter 后准备恢复 resize，必须先完成本文的完整 resize 和 accessibility 回归矩阵，不能直接恢复窗口样式。

### 2.3 DPI 和布局问题

在当前 Windows `window_size`/多窗口组合中，创建窗口时必须明确区分逻辑尺寸与物理像素：

1. 使用 `screen.visibleFrame` 和 `screen.scaleFactor` 获取可用屏幕范围与缩放比例。
2. 将可用物理尺寸除以 `scaleFactor`，得到逻辑 viewport。
3. 在逻辑尺寸中应用 preferred size、屏幕留白、最大高度比例和最小尺寸约束。
4. 将最终逻辑窗口尺寸乘以 `scaleFactor`，转换回物理像素。
5. 在 `visibleFrame` 内按物理像素居中，再把完整 frame 交给子窗口 controller。

对应计算集中在 `computeDpiAwareDialogFrame`。无效、非有限或小于等于零的 scale factor 必须回退为 `1.0`。

密码页内容必须在小高度和高 DPI 下可达：

- 输入区使用 `SingleChildScrollView` 和可见 `Scrollbar`。
- action buttons 固定在底部，不随输入区滚动。
- `ScrollController` 由页面持有并在 `dispose` 中释放。
- 窗口禁用 resize 不能代替内容滚动；两者分别解决 native crash 和内容可达性。

### 2.4 断开连接行为

受控会话的断开应直接调用 `bind.cmCloseConnection(connId: ...)`，不再通过配置项写入后等待另一个窗口轮询消费。

当前交互约束：

- 调用前将目标连接 ID 加入 pending 集合并立即刷新 UI。
- pending 期间禁用对应断开按钮，防止连续重复提交。
- 会话 close 事件或同步后的会话快照不再包含该 ID 时，清除 pending。
- FFI 调用抛错时立即清除 pending，允许重试。
- 调用成功但会话在 5 秒后仍存在时，清除 pending 并记录 timeout，避免按钮永久不可用。

5 秒恢复是 UI 自愈机制，不代表连接已经断开；回归时必须同时验证实际会话列表和按钮状态。

## 3. 修复策略与实现约束

### 3.1 多窗口账本必须自愈

所有“从 tracked list 选择窗口并调用”的入口应遵循同一顺序：

1. 获取插件报告的实际子窗口 ID。
2. reconcile 当前业务列表，清理不存在的 ID。
3. 调用窗口方法时使用统一安全包装。
4. 若命中 `target window not found`，forget 该 ID 并继续处理剩余候选。
5. 非 missing-target 错误保留原始异常，不得无条件吞掉。

新增窗口类型或新增窗口复用入口时，应加入 `_trackedWindowLists` 和 reconcile 覆盖。只在 UI 按钮层加 try/catch 不能修复窗口账本失配。

### 3.2 Windows 子窗口禁用 resize

Windows 子窗口应通过 Win32 FFI 修改 top-level window style：

- 清除 `WS_THICKFRAME`。
- 清除 `WS_MAXIMIZEBOX`。
- style 变化后调用 `SetWindowPos`，包含 `SWP_FRAMECHANGED`、`SWP_NOMOVE`、`SWP_NOSIZE`、`SWP_NOZORDER` 和 `SWP_NOACTIVATE`。
- 非 Windows 平台继续使用其已注册且有效的窗口管理实现。

按精确标题查找窗口时，实现必须满足以下安全门槛：

1. 使用 `FindWindowEx` 遍历同名顶层窗口，而不是接受第一个全局匹配。
2. 使用 `GetWindowThreadProcessId` 获取 HWND 所属 PID，并与 `GetCurrentProcessId`
   比较；跳过其他进程的同名窗口，绝不能修改其 style。
3. 检查 `GetWindowThreadProcessId` 是否失败。
4. 检查 `GetWindowLongPtr`/`SetWindowLongPtr` 的错误状态；注意返回零既可能是合法旧值，也可能是失败，需要结合 `SetLastError(0)` 与 `GetLastError()` 判断。
5. 检查 `SetWindowPos` 的布尔返回值。
6. 任一 API 失败均返回 `false` 并输出不含敏感信息的诊断日志，不能在 style 未生效时报告成功。

精确标题只应作为当前兼容方案。若同一进程未来允许存在多个同名设置/密码窗口，
应改为在创建链路中传递或可靠绑定 HWND，避免选中同进程内的错误实例。

### 3.3 DPI 计算必须可单测

尺寸计算保持为无平台副作用的纯函数，至少覆盖：

- 100% 缩放下 preferred size 居中。
- 150% 和 200% 缩放下逻辑/物理像素转换。
- 小屏幕下宽高钳制和边距。
- 密码窗口独立的最小尺寸及 height factor。
- 无效 scale factor 回退。
- `visibleFrame` 带非零 left/top 时仍在目标显示器内居中。

不要把尺寸计算散落到页面 widget 或 Win32 FFI 中，否则难以验证多显示器和高 DPI 行为。

## 4. 原生崩溃排查顺序

### 4.1 固定复现条件

先记录以下条件，再开始修改代码：

- 应用版本、commit 和构建模式。
- Flutter engine/runner 是否为本次构建产物。
- Windows 版本、显示缩放比例和显示器数量。
- 窗口类型：主窗口、remote 子窗口、设置窗口或密码窗口。
- 触发动作及频率：单次断开、快速连点、重连、拖拽 resize、最大化等。
- 是否开启屏幕阅读器或其他 accessibility 工具。

每轮只改变一个变量。必须用新启动的进程验证，避免旧 runner、旧 DLL 或残留子窗口污染结果。

### 4.2 建立统一时间线

Windows 下 `flutter_crash_trace.log` 默认位于：

```text
%APPDATA%\HDesk\log\flutter_crash_trace.log
```

同一复现轮次收集：

1. 应用主日志及 session start/exit 时间。
2. `flutter_crash_trace.log`，包含时间戳和 PID。
3. Windows Application event log 中的 Application Error/WER 事件。
4. 与该 PID 和时间对应的 dump。
5. 构建时使用的 `hdesk.exe`、`flutter_windows.dll` 和匹配符号。

可用 PowerShell 快速查看最近的应用崩溃事件：

```powershell
Get-WinEvent -FilterHashtable @{
  LogName = 'Application'
  StartTime = (Get-Date).AddMinutes(-30)
} | Where-Object {
  $_.Id -in 1000, 1001 -and $_.Message -match 'hdesk|flutter_windows'
} | Select-Object TimeCreated, Id, ProviderName, Message
```

时间线至少对齐到秒，并核对 PID。不要把上一次运行遗留的 dump 与本次日志拼在一起。

### 4.3 先比较崩溃签名，再分析业务动作

依次记录：

- faulting module 和文件版本。
- exception code。
- module offset。
- faulting process ID。
- dump 中的 top frames 和首个有意义符号。

若模块版本不同，即使 offset 相同也不能视为同一 crash；若模块版本相同但 offset/符号不同，应建立新的问题分支。

使用匹配符号在 WinDbg 中至少执行：

```text
.symfix
.reload /f
!analyze -v
k
lmvm flutter_windows
```

符号化结果负责定位 native 触发层，主日志负责还原业务和窗口生命周期。两者必须结合，任何一侧都不能独立证明完整根因。

### 4.4 按症状选择分支

发现 `target window not found` 时：

1. 比较 tracked window ID 与 `getAllSubWindowIds()`。
2. 检查失效 ID 是否从所有类型列表和 active/inactive 集合移除。
3. 检查调用是否绕过 `_safeInvokeMethod` 或 `_reconcileWindows`。
4. 检查主窗口是否被错误纳入 subwindow reconcile。
5. 检查 session exit 后数秒内是否又出现第二次 session start，以区分 stale-window 与 timer/自动重连问题。

拖拽设置/密码窗口时崩溃且命中 accessibility bridge 签名时：

1. 先用窗口样式探针确认 `WS_THICKFRAME` 和 `WS_MAXIMIZEBOX` 是否实际清除。
2. 确认运行的是新构建 runner，而不是安装目录中的旧版本。
3. 检查 title 查找是否命中当前进程 HWND，所有 Win32 API 是否返回成功。
4. 在 100%、150%、200% DPI 下分别打开窗口，确认 frame 未超出可用屏幕。
5. 禁用 resize 后重复原触发动作，确认没有新 WER/dump。
6. 若仍崩溃，重新符号化新 dump；不要沿用旧的 `0x391b6` 结论。

点击断开无效时：

1. 确认日志出现 `ServerModel.closeDesktopControlledSessions` 及目标 ID。
2. 确认调用链直达 `cmCloseConnection`，没有回退到配置轮询协议。
3. 检查 close 事件或会话快照是否移除目标 ID。
4. 检查 5 秒后是否出现 timeout 并恢复按钮。
5. 区分“UI 已恢复但连接仍存在”与“连接已断开但快照未更新”。

## 5. 回归矩阵

### 5.1 必跑功能矩阵

| 场景 | 操作 | 预期结果 | 必查证据 |
| --- | --- | --- | --- |
| 单会话主动断开 | 点击一次断开 | 立即进入 pending，连接关闭后会话消失 | `cmCloseConnection` 日志、pending 清除 |
| 单会话连续点击 | 快速点击断开 | pending 期间按钮不可重复提交 | 只有一次有效关闭请求 |
| 多会话全部断开 | 点击全部断开 | 所有非 pending ID 并行关闭 | 每个连接 ID 都被处理 |
| 断开调用失败 | 模拟 FFI 抛错 | pending 立即清除，可再次点击 | failure 日志、UI 恢复 |
| 断开无状态回执 | 会话 5 秒仍存在 | pending 超时清除，实际会话仍显示 | timeout 日志、按钮恢复 |
| 被控端断开后重连 | 断开后立即重连 | 不调用 stale 子窗口，不崩溃 | 无 `target window not found` 或能自愈 forget |
| 快速重复重连 | 连续执行多轮连接/断开 | tracked list 与实际窗口一致 | 无新 WER/dump |
| 子窗口被外部关闭 | 关闭 remote 子窗口后再次发起会话 | reconcile 清理旧 ID，创建/选择有效窗口 | stale ID 被 forget |
| 设置窗口 resize | 尝试拖动边框、双击标题栏、最大化 | 窗口不可 resize/maximize，进程稳定 | style 位已清除，无新 crash |
| 密码窗口小屏显示 | 在低高度屏幕打开并填写 | 所有输入可滚动访问，按钮固定可见 | 无 overflow，Scrollbar 可用 |

### 5.2 显示矩阵

至少覆盖：

| 显示环境 | 设置窗口 | 密码窗口 | 验证重点 |
| --- | --- | --- | --- |
| 1920x1080，100% | 打开、关闭、重复打开 | 输入、校验、提交/取消 | preferred size 和居中 |
| 1920x1080，150% | 同上 | 同上 | 逻辑/物理转换 |
| 1920x1080，200% | 同上 | 同上 | 高 DPI 下不越界、内容可滚动 |
| 低分辨率/低可用高度 | 同上 | 同上 | height factor、底部按钮可见 |
| 双显示器、不同 DPI | 在两个显示器分别打开 | 在两个显示器分别打开 | 使用目标屏 visible frame，不跨屏偏移 |
| 非零任务栏位置或工作区原点 | 打开并检查四边 | 打开并检查四边 | 正确处理 `visibleFrame.left/top` |

### 5.3 Accessibility 回归

由于已确认 crash 位于 Flutter accessibility bridge，发布前至少执行：

- 默认系统状态下重复打开/关闭设置和密码窗口 20 次。
- 使用键盘 Tab/Shift+Tab 遍历全部输入和按钮。
- 开启 Windows Narrator 后重复打开、聚焦、输入、校验、关闭。
- 内容滚动时确认焦点对象仍可见，底部 action buttons 不跳动。
- 远程会话断开/重连过程中打开和关闭设置窗口。
- 整轮验证后检查 Application event log 与 dump 目录，不能只看 UI 是否仍在响应。

## 6. 发布前门禁

涉及多窗口、断开、DPI 或 Win32 style 的改动，至少完成：

1. `flutter test test/settings_dialog_utils_test.dart`。
2. 对改动文件执行定向 `flutter analyze`，不得新增 error。
3. Windows release 构建成功，并确认测试的是 fresh runner。
4. 用样式探针确认设置/密码窗口不可 resizable、不可 maximizable。
5. 完成 100%、150%、200% DPI 和至少一种低可用高度验证。
6. 完成单会话、多会话、快速连点和 5 秒 timeout 回归。
7. 完成断开后重连和 stale child window 自愈回归。
8. 检查本轮测试时间之后没有新 WER 事件和 crash dump。
9. 检查 `git diff --check`，且不提交日志、dump、设备信息或凭据。

## 7. 诊断结论模板

每次同类问题关闭前，使用以下结构记录，便于以后比较签名：

```text
复现版本/commit：
Windows/Flutter 版本：
显示器与 DPI：
窗口类型：
最小复现步骤：
首次异常时间和 PID：
faulting module/version：
exception code/offset：
符号化 top frames：
主日志关键时间线：
flutter_crash_trace 关键时间线：
tracked windows 与实际 windows 差异：
已确认事实：
排除项：
修复策略：
回归矩阵结果：
是否产生新 WER/dump：
```

结论中必须区分“观察到的事实”“根据证据推导的根因”和“为隔离风险采用的产品策略”。例如：accessibility bridge 栈是已符号化事实，禁用 resize 是当前修复策略；它不自动证明所有窗口崩溃都来自同一 Flutter 缺陷。
