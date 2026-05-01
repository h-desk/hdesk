# macOS Editable Focus / Auto-VP 最小实现方案（2026-05-01）

这份文档用于说明当前 RustDesk 桌面端“焦点变化上报给主控端，驱动 auto_vp / WAV”的实现现状，以及 macOS 侧如何按最小范围补齐同类能力。

## 结论

当前这条链路的桌面端发送侧是 Windows-only。

原因不是 Harmony 侧写死了 Windows，而是桌面端的 `editable_focus` 服务只在 Windows 下真正注册和运行：

- `src/lib.rs` 只在 `#[cfg(windows)]` 下导出 `EditableFocusHintInfo` 和 `get_editable_focus_hint`
- `src/server.rs` 只在 `#[cfg(windows)]` 下注册 `input_service::new_editable_focus()`
- `src/server/input_service.rs` 的 `run_editable_focus()` 在 `#[cfg(not(windows))]` 路径下是空实现，注释直接写着 `Only implemented for Windows in v1`
- 实际采集实现都在 `src/platform/windows.rs`

但协议和 Harmony 侧消费不是 Windows 专属。

Harmony 侧只消费通用的 `editable_focus_hint` 消息：

- `Application/rustdesk_core/src/harmony_ffi.rs` 解析 `Misc.editable_focus_hint`
- `Application/entry/src/main/ets/pages/Index.ets` 把原始 JSON 转发给页面状态
- `Application/entry/src/main/ets/pages/RemotePage.ets` 只看通用字段：
  - `editable`
  - `window`
  - `editor`
  - `caret`
  - `pane`
  - `content_kind`
  - `revision`

所以从架构上看，macOS 只缺“桌面端采集 + 桌面端发送”这半边，不需要重写 Harmony 端 auto_vp。

## 当前源码现状

### Windows 已有能力

Windows 侧当前已经具备完整的 `editable_focus_hint` 采集和发送路径：

1. `src/platform/windows.rs`
   - 定义 `EditableFocusHintInfo`
   - 实现 `get_editable_focus_hint(displays)`
   - 实现 `get_foreground_window_rect()`
   - 使用前台窗口句柄和 UIA 路径区分：
     - 当前前台窗口
     - 当前是否是可编辑控件
     - 编辑区域矩形
     - 光标矩形
     - pane 区域
     - 内容类型

2. `src/server/input_service.rs`
   - `new_editable_focus()` 定时轮询
   - `run_editable_focus()` 取平台 hint
   - `send_editable_focus_hint()` 序列化为 `Misc.editable_focus_hint`
   - `hint_changed()` 根据几何变化和 `foreground_hwnd` 判断是否发生应用切换

3. `src/server.rs`
   - 注册 `editable_focus` 服务

### macOS 已有基础，但还没有 editable_focus 实现

`src/platform/macos.rs` 当前已经有一些可复用的系统能力：

- 辅助功能权限检查
- 输入监控权限检查
- 屏幕录制权限检查
- CoreGraphics 窗口列表访问

这说明 macOS 侧并不是完全没有系统 API 接口，而是还没有把这些能力收敛成和 Windows 对齐的 `EditableFocusHintInfo` 采集函数。

## 为什么现有 auto_vp 不能直接在 Mac 上工作

因为桌面端没有发出 `editable_focus_hint`。

Harmony 侧的 WAV / auto_vp 逻辑不是自己推断 PC 焦点，它完全依赖对端桌面发送的 hint。

当前 macOS 桌面端即使能截图、收输入、跑 Flutter，也不会触发这条链路，原因有两个：

1. `editable_focus` 服务没有注册
2. 非 Windows 的 `run_editable_focus()` 直接空返回

所以现在 Mac 上不是“auto_vp 算法不兼容”，而是“没有输入数据”。

## macOS 最小实现目标

第一阶段不要追求和 Windows 同等复杂度。

目标应该分三级推进。

### V1：先实现前台窗口变化上报

只要上报下面这些字段，就能让 Harmony 侧先恢复窗口级 WAV：

- `editable = false`
- `window`
- `display_idx`
- `content_kind = 0`
- `revision`
- `pane = window`
- `editor = [0,0,0,0]`
- `caret = [0,0,0,0]`

这已经足够支持：

- App 切换
- 最大化窗口切换
- 任务栏 / Dock / 切屏后窗口重适配

这一步不解决“点输入框自动弹键盘”，但能先解决“切窗口后主控端视口不跟”的问题。

### V2：补齐 editable + editor + pane

第二阶段再识别当前焦点是否为可编辑元素，并产出：

- `editable = true/false`
- `editor`
- `pane`
- `content_kind`

这一步能恢复大部分输入框 auto_vp。

### V3：补齐 caret 与代码编辑器细分

最后再追求：

- 精确 caret 矩形
- 单行 / 多行 / code editor 分类
- 更稳的同矩形窗口切换检测

这一步才对应 Windows 现在的高完成度体验。

## 建议改动的文件

最小落地范围建议只改 4 个文件。

### 1. `src/lib.rs`

当前：

- `EditableFocusHintInfo` 和 `get_editable_focus_hint` 只在 `#[cfg(windows)]` 下导出

建议：

- 把导出条件放宽到 `#[cfg(any(windows, target_os = "macos"))]`

目的：

- 让服务层可以在 macOS 下调用统一平台接口

### 2. `src/server.rs`

当前：

- `new_editable_focus()` 只在 `#[cfg(windows)]` 下注册

建议：

- 放宽到 `#[cfg(any(windows, target_os = "macos"))]`

目的：

- 让 macOS 服务真正跑起来

### 3. `src/server/input_service.rs`

当前：

- `run_editable_focus()` 的真实实现隐含依赖 Windows 平台接口
- 非 Windows stub 直接空返回
- 某些辅助路径直接调用 `crate::platform::windows::get_foreground_window_rect()`

建议：

- 先把“仅 Windows 可用”的辅助路径继续保留在 `#[cfg(windows)]` 下
- 新增 macOS 路径时，先只走最核心的轮询-发送逻辑：
  - 取 `crate::platform::get_editable_focus_hint(&displays)`
  - 用 `hint_changed()` 比较变化
  - 用 `send_editable_focus_hint()` 发消息
- 对需要 `windows::get_foreground_window_rect()` 的点击代理、cursor proxy、recent editable reuse 等复杂补偿逻辑，第一版直接不移植

目的：

- 把 macOS 实现压成最小可工作的发送器，而不是一开始就复制 Windows 的全部补偿逻辑

### 4. `src/platform/macos.rs`

需要新增：

- `EditableFocusHintInfo` 的 macOS 可见定义，或者把结构提升到共享位置后由各平台填充
- `get_editable_focus_hint(displays: &[DisplayInfo]) -> Option<EditableFocusHintInfo>`
- 可选：`get_foreground_window_rect()` 的 macOS 版本

注意：

- 这一步是核心工作量所在

## macOS 侧推荐实现路线

### 第一步：只拿前台窗口

最先实现下面这件事：

1. 获取 frontmost app
2. 获取 frontmost window 的全局矩形
3. 映射到当前 display
4. 生成一个 `editable=false` 的 hint

可选实现来源：

- `NSWorkspace` 获取前台应用
- CoreGraphics 窗口列表获取窗口信息
- 如果需要更稳定的窗口身份，可记录 `CGWindowID` 或 pid + window tuple

建议 first pass 直接构造：

```text
editable=false
caret=(0,0,0,0)
editor=(0,0,0,0)
window=<frontmost-window-rect>
pane=<same-as-window>
display_idx=<mapped-display>
content_kind=0
foreground_hwnd=<local synthetic window id>
```

说明：

- `foreground_hwnd` 这个字段不会上协议，只用于本地判断是否切窗
- 在 macOS 上不需要真的 HWND，只要是本地稳定窗口 ID 即可

### 第二步：接入 Accessibility 焦点元素

当 V1 跑通后，再用 Accessibility API 获取当前焦点元素：

1. 判断焦点元素是否可编辑
2. 取焦点元素 bounds 作为 `editor`
3. 尝试推导内容区域作为 `pane`
4. 如果能判断文本域类型，再填 `content_kind`

这里不要求第一版就拿到 caret。

因为 Harmony 侧很多 auto_vp 场景只需要：

- 当前窗口 rect
- 当前编辑区域 rect
- 基础内容类型

### 第三步：再追 caret 和 code-pane 分类

这一步只在前两步稳定后再做。

Windows 当前有大量补偿逻辑用于：

- Qt
- Chromium
- AWT
- 光标附近 fallback
- 同窗口复用
- synth pane

这些都不适合在 macOS 第一轮照搬。

## 最小实现时应该故意不做的事

为了控制风险，第一轮 macOS 实现建议先不要做：

- 点击后 editable proxy reuse
- cursor proxy reuse
- recent editable hint cache 复用
- Windows UIA 等价的复杂多层 descendant 探测
- 代码编辑器特殊 pane 合成
- caret 精准跟踪

这些功能都属于“提高体验”的补偿层，不是“让链路先工作”的必需品。

## 对 Harmony 侧的影响

如果 macOS 能发出通用 `editable_focus_hint`，Harmony 侧原则上不需要协议改动。

原因：

- `Application/rustdesk_core/src/harmony_ffi.rs` 解析的是通用消息字段
- `Application/entry/src/main/ets/pages/RemotePage.ets` 用的是通用几何与 revision 规则
- WAV 早期路径本来就支持“只根据 foreground window change 触发”

所以 macOS V1 一旦发出非 editable 的窗口变化 hint，主控端就应该先恢复窗口级 auto_vp / WAV。

## 建议的开发顺序

1. 在 `src/platform/macos.rs` 新增最小 `get_editable_focus_hint()`，先只返回非 editable window-level hint
2. 放宽 `src/lib.rs` 导出条件
3. 放宽 `src/server.rs` 服务注册条件
4. 让 `src/server/input_service.rs` 在 macOS 下走最小轮询发送路径
5. 先验证“切 App 触发 Harmony WAV”
6. 再补 Accessibility 焦点元素识别
7. 再补 editor / pane
8. 最后才考虑 caret 和 code-pane 增强

## 最小验收标准

### V1 验收

满足以下任一场景即可认为 V1 成功：

- Mac 远端在两个普通窗口之间切换时，Harmony 主控端能收到新的 `editable_focus_hint`
- Harmony 端 `RemotePage` 能在键盘未打开时对新的 `window` 触发 WAV
- 即使 `editable=false`，窗口切换后视口仍能重新适配

### V2 验收

满足以下场景：

- 点击 macOS 远端输入框后，Harmony 端能收到 `editable=true`
- `editor` 不为零矩形
- 主控端能按输入框位置弹键盘并完成 auto_vp

## 风险点

### 1. 权限问题

macOS 的 editable focus 方案大概率依赖：

- Accessibility 权限
- 可能还需要 Input Monitoring / Screen Recording 的既有权限组合

如果权限不完整，第一轮应优先退化成“只上报前台窗口”。

### 2. frontmost window 与 focused element 不一致

macOS 下前台窗口、焦点元素、辅助功能树的同步时序可能不稳定。

所以不要一开始就把窗口切换检测和 editor 精确识别绑定在一起；先把 window-level hint 跑通更稳。

### 3. 同矩形切窗

两个最大化窗口可能共享完全相同的矩形。

因此 macOS 本地实现仍然需要一个“窗口身份”字段，用于替代 Windows 的 `foreground_hwnd` 比较逻辑。

## 推荐的验证命令

实现后先跑最窄验证：

```bash
cargo check --lib
```

如果要走 Flutter macOS 实机链路，再按仓内已有流程：

```bash
MACOSX_DEPLOYMENT_TARGET=10.14 cargo build --features flutter --release
cd flutter
flutter build macos --release
cp -f ../target/release/service build/macos/Build/Products/Release/RustDesk.app/Contents/MacOS/
open build/macos/Build/Products/Release/RustDesk.app
```

## 一句话路线图

先把 macOS 版 `editable_focus_hint` 做成“窗口级非 editable 事件源”，让 Harmony 的 WAV 先恢复；再逐步补焦点元素、编辑框和 caret，而不是一开始照搬 Windows 全套 UIA 补偿逻辑。