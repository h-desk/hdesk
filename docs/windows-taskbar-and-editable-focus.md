# Windows 任务栏与 editable_focus 排查手册

> 本文把 Harmony 端的任务栏复盘迁移到桌面仓库。文中的历史数值、窗口类名和几何示例来自历史复现，不是 Windows 或协议的固定契约。排查时以当前源码和当前日志为准。

## 适用范围

本文覆盖两类容易被混淆的问题：

1. 点击 Windows 任务栏图标后只出现预览，目标应用没有直接恢复到前台。
2. 点击任务栏后鼠标命中或视口发生左右漂移，第一次点击被当成文本输入。

两者的根因和证据链不同。前者首先看 Windows Shell 任务栏/预览交互，后者首先看桌面端 `editable_focus` 的 `window/pane/editor` 几何是否跳变。

当前桌面端相关入口是：

- `D:\ideas\rustdesk\src\platform\windows.rs`：Windows 前景窗口、UI Automation、caret/editor/pane 生成和 editable 状态缓存。
- `D:\ideas\rustdesk\src\server\input_service.rs`：`editable_focus` 轮询、点击意图、proxy 命中和向客户端发送 hint。
- `D:\ideas\rustdesk\libs\hbb_common\protos\message.proto:841`：`EditableFocusHint` 协议定义。
- `D:\ideas\rustdesk\flutter\lib\models\model.dart:1750`：Flutter 端接收 hint，并决定是否自动拉起键盘。

历史文档中出现的 `Application/rustdesk_core`、`RemotePage.ets` 等路径属于 Harmony 工程，不是当前桌面仓库的修改入口。

## 先清理脏运行态

任务栏问题对进程来源和窗口分组非常敏感。复现前必须确认没有混用下列运行物：

- Flutter runner：`flutter\build\windows\x64\runner\Release\hdesk.exe`
- 已安装客户端：以当前安装注册表和实际进程 `Path` 为准；不要假设固定在 `%LOCALAPPDATA%`。
- tray、service 及其拉起的客户端进程。

历史验证中曾经混用 runner、installed、tray、service，导致任务栏分组和 Shell 行为被污染。`HDesk Service` 在早期 A/B 中看似相关，但复验表明它不是单独根因。每次复现都应：

1. 记录现有 `hdesk`/`rustdesk` 进程的 PID、完整路径和启动参数。
2. 停止旧 runner、tray 和已安装实例；必要时停止 service，防止它立刻重新拉起旧客户端。
3. 只启动一种来源的客户端。
4. 再确认任务栏按钮对应的进程路径和 PID 与本次测试一致。

当前仓库的 `scripts/windows/build-install-validate.ps1` 使用以下成功信号：
`INSTALLER_EXIT=0`、三件套 runner/installed hash 相等、管理员环境下
`SERVICE_STATE=Running`，以及 service `ImagePath` 指向安装后的
`HDesk.exe --service`。复现任务栏问题前应运行或等价执行这套检查，不能只看
安装器退出码。

## 问题一：任务栏只出预览

### 已确认的证据

历史日志证明任务栏 surface 可以被识别为 `Taskbar.TaskListButtonAutomationPeer`，这是明确的 non-editable surface，不应继续走 editable 或 virtual-cursor fallback。前景窗口还可能短暂切到 `Qt51514QWindowIcon`、`XamlExplorerHostIslandWindow` 等 Shell preview 宿主。

当前 `D:\ideas\rustdesk\src\platform\windows.rs:279-317` 已将 `tasklistbuttonautomationpeer` 作为显式 non-editable 类片段；`:600-615` 将 `XamlExplorerHostIslandWindow` 等短暂窗口视为 transient foreground。这个识别只说明当前鼠标所在 surface，不等于任务栏一定会恢复目标应用。

如果日志同时表明点击事件已进入桌面端处理链路，就不能把“手机端点击没有送到桌面”作为首要解释。应先判断 Windows Shell 的预览/激活语义，再看输入注入。

### 不要混淆

“点击后只出预览、不直接恢复”是任务栏/Shell 交互问题；“点击后鼠标左右漂移”是 editable_focus 几何问题。两者可以连续发生，但不能用同一个补丁或同一组日志结论替代。

## 问题二：任务栏点击后漂移

### 几何证据

历史日志中曾出现同一前景窗口内的 pane 错跳：稳定 pane 示例为 `pane=(282,214,776,521)`，`pane_candidate` 丢失后退化成 `pane=(805,-9,1124,1158)`。这些坐标只是历史复现样本，不能直接写入应用特判。

当前协议和桌面实现已经有三层几何：

| 层 | 语义 | 用途 |
|---|---|---|
| `editor` | 真实输入控件或可编辑宿主 | 输入框位置、键盘锚点、命中映射 |
| `pane` | 用户当前应优先看到的主内容面板 | 横向 framing、整体可见范围和缩放 |
| `window` | 整个前台窗口 | 显示器归属和兜底约束 |

协议字段位于 `D:\ideas\rustdesk\libs\hbb_common\protos\message.proto:841-850`，Windows 侧结构位于 `D:\ideas\rustdesk\src\platform\windows.rs:121-142`。不要把 `editor` 同时当作主要内容区域，也不要把 `window` 当作精确编辑区域。

当前 `D:\ideas\rustdesk\src\platform\windows.rs:735-766` 仍包含通用 split-view heuristic：当 editor 宽度小于 window 的 60%、扩展后至少比 editor 宽 20% 且 pane 至少 400 像素时，尝试生成 `editor.left -> window.right` 的 pane。这些阈值是当前实现细节，不是协议保证。

同一窗口、同一显示器、下一帧 editor 仍落在上一帧可信 pane 内时，当前代码在 `D:\ideas\rustdesk\src\platform\windows.rs:789-803` 只保留上一帧 split pane。这与“冻结上一帧 non-editable 的 window/pane”不同，后者会把 stale 几何保留过久。

## `editable=true` 的五条历史激活路径

以下五条路径来自历史复盘。当前源码已经经过多轮调整，名称和入口可能变化，必须用当前日志确认实际命中路径。

| 历史路径 | 触发条件 | 当前桌面源码核对点 |
|---|---|---|
| cursor/click proxy reuse | 点击或光标落在最近一次 editor/proxy 的容差范围内 | `src/server/input_service.rs:581-592,1222-1368` |
| preserve prior virtual-cursor hint | 新 hint 是 synthetic editor，上一 hint 有可信 pane | `src/platform/windows.rs:337-347,422-464` |
| sticky reuse | 前景窗口相同，短时 non-editable 结果仍在 sticky TTL 内 | `src/platform/windows.rs:152-157,477-489` |
| hard-negative/transient reuse | transient Shell 窗口或短时前景切换，复用上一 hint | `src/platform/windows.rs:370-419,491-505` |
| caret fallback | UIA 无可用 editor，但 `GetGUIThreadInfo` 仍提供可信 caret | `src/platform/windows.rs:1945-1990` |

历史复盘记录过 24 像素 proxy 容差、8 像素收紧值、700ms sticky 窗口和 3 秒 transient 窗口。当前源码仍有 700ms/3 秒 Windows sticky 常量（`:152-153`），`input_service.rs` 另有 45 秒 recent hint cache（`:479-481`），不可把这些时间窗混为一谈。当前 hover proxy 使用 8 像素收紧值（`input_service.rs:1321-1336`），click intent 的标准容差仍是 24 像素（`:479,581-590`）。

当前实现还包含 Qt caret confirmation 等额外路径（`windows.rs:160-164,1992-1995`），所以“五条”是历史问题地图，不是未来实现必须保持的数量。

### visible-bottom-input drift 守卫

Chrome find bar 等细长底部输入框可能在光标已经移向任务栏时仍保持键盘焦点。当前 `D:\ideas\rustdesk\src\platform\windows.rs:1734-1768` 会在 editor 看起来是 visible-bottom-input 且光标离开超过 8 像素时返回 `kind=-2`；`windows.rs:1920-1939` 清空 sticky 并跳过 caret fallback，避免再次生成 `editable=true`。

应验证日志中出现：

```text
editable_focus suppress visible-bottom-input drift: ... (kind=-2)
```

没有这条日志，就不能声称本次修改命中了 drift 路径。

## 禁止的错误修法

- 不要在 Harmony 端先加点击坐标补偿，除非桌面日志已经证明注入坐标本身错误。
- 不要在 `src/server/input_service.rs` 延后、冻结或长期复用上一帧 non-editable hint 的 `window/pane` 来压 `CHANGED`；历史验证表明这只会把右偏变成左偏。
- 不要全局关闭 `editable_focus`、键盘自动拉起或所有 proxy reuse；这会破坏正常输入框首次点击。
- 不要扩大 proxy 容差来“提高命中率”。任务栏边缘更容易因此被当成编辑区。
- 不要把 `pane = editor` 当成 split-view 的修复，也不要用应用名写 `if WeChat`/`if Notion` 分支。
- 不要只运行 service 或只运行 runner 来复现另一种安装形态的问题。
- 不要把历史坐标、历史 TTL 或某个 Chrome/Qt 类名当成协议契约；先看当前日志和当前源码。

## 正确排查顺序

1. **清理运行态**：停止旧 runner、tray、installed 和 service 拉起的客户端，确认唯一进程来源及路径。
2. **收集桌面日志**：Rust 日志目录由 `D:\ideas\rustdesk\libs\hbb_common\src\config.rs:744-804` 通过 `ProjectDirs` 和 `log_path()` 计算，不能按 runner 或安装目录猜测；先从启动日志或运行时实际文件确认路径。Flutter crash trace 另写入 `%APPDATA%\HDesk\log\flutter_crash_trace.log`（`flutter/lib/utils/desktop_crash_trace.dart:22-30`）。历史复盘中的 `hdesk_rCURRENT.log` 路径应视为机器相关示例。
3. **先分型**：只出预览时查 `Taskbar.TaskListButtonAutomationPeer`、Shell preview 类名和前景窗口切换；漂移时查 `editable_focus CHANGED` 及 `window/editor/pane`。
4. **确认 pane 是否跳变**：重点看 same-window 情况下 pane 是否突然退化为 `editor.left -> window.right` 的大矩形，或从可信 pane 变成 window-sized pane。
5. **定位 editable 路径**：按日志检查 `proxy reuse/miss`、`cursor proxy reuse/miss`、`preserve prior split pane`、`preserve prior virtual-cursor hint`、`sticky reuse`、`hard negative reuse`、`caret fallback`、`kind=-2`。
6. **最后才查注入链路**：只有日志出现 `mouse_inject before/after`、`mouse_dispatch path` 或等价的当前实现证据时，才继续追鼠标注入坐标；没有这些证据，不要先改手机端映射。
7. **按真实来源回归**：至少分别验证 runner、已安装客户端、带 service 的安装态，并覆盖 Chrome find bar、任务栏图标、普通输入框、split-view/Electron/VS Code 等场景。

## 长期设计原则

长期方向是保持协议向后兼容，使用 `pane` 控制整体可见范围，使用 `editor` 控制输入锚点，`window` 只做兜底和显示器判断。移动端策略应趋向“进入编辑时对齐、滚动时稳定、发生遮挡时再矫正”，而不是每次 caret 小幅移动都重算视口。

新增协议字段必须追加而不是改写既有编号；旧客户端缺少 `pane` 时回退到 editor/caret/window 逻辑。Windows 端优先从 UIA ancestor 提取语义 pane，失败时使用通用 split-view 规则，不为单个应用堆特判。

## 历史来源

- `D:\ohos\hdesk\Application\docs\windows-taskbar-preview-no-restore-root-cause.md`
- `D:\ohos\hdesk\Application\docs\windows-taskbar-click-drift-root-cause.md`
- `D:\ohos\hdesk\Application\docs\windows-editable-focus-taskbar-fix-retrospective.md`
- `D:\ohos\hdesk\Application\docs\editable-focus-pane-rect-long-term-design.md`
