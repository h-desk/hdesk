---
name: verify-before-claiming
description: 当用户询问"某功能是否支持"、"能不能做X"、"X有没有Y能力"、"实现X难不难"时，必须先查源码/声明文件验证，禁止凭知识印象直接下结论。本地有克隆仓库时（D:\ideas\rustdesk 等）必须先 grep。
---

# Verify Before Claiming（先验证，再结论）

## 核心原则

**通用知识 ≠ 项目现状。**  
凭架构知识说"不支持"是错的——项目可能已经实现了。凭印象说"支持"也是错的——版本可能已经移除。

## 触发场景

以下任何问法都必须触发此 Skill：
- "X 支持 Y 吗？"
- "能不能实现 X？"
- "做 X 容易吗 / 难不难？"
- "X 有没有 Z 能力？"
- "这个功能要多少工作量？"

## 验证流程（必须执行）

### Step 1：找本地源码
```powershell
# 已知本地仓库
# RustDesk PC 端：D:\ideas\rustdesk
# Cross-repository questions may also require D:\ohos\hdesk or
# D:\ideas\rustdesk-server, but keep each repository's facts in its own docs.
```

### Step 2：grep 关键词
```powershell
# 示例：验证 RustDesk 是否支持 UAC / 安全桌面
rg -n "secure_desktop|uac|elevat|selectInputDesktop|install_service" `
  D:\ideas\rustdesk\src
```

### Step 3：读关键实现
找到匹配行后 `read_file` 读上下文，确认是实现还是仅引用。

### Step 4：基于源码下结论
- 找到实现 → "已支持，实现在 `src/xxx.rs:L123`，原理是..."
- 找不到 → "当前源码中未找到，**不排除**有其他实现路径，建议进一步搜索 `keyword2`"
- 找到但被 feature flag 保护 → "需要开启 `--features xxx` 才有此能力"

## 禁止行为

❌ "UAC 安全桌面在独立的 Winlogon 桌面，普通进程无法访问，需要大量工程"（凭架构知识）  
❌ "这个功能我不太确定，应该不支持"（凭模糊印象）  
❌ "这需要很复杂的实现"（未看源码就估算复杂度）

## 反面案例（已发生，2026-04-19）

**问题**：用户问"远程控制能操作系统 UAC 弹窗吗？"

**错误回答**：说 UAC 安全桌面在 `WinSta0\Winlogon` 独立桌面，普通进程无法截屏/注入，需要大量工程。

**事实**：RustDesk 早已实现：
- `src/platform/windows.cc` → `selectInputDesktop()` 用 `OpenInputDesktop` + `SetThreadDesktop` 切换线程到安全桌面
- `src/server/portable_service.rs` → SYSTEM 权限子进程，检测 `desktop_changed()` 后自动切换
- `src/platform/windows.rs` → `install_me()` 中 `sc create ... start=auto` 注册 SYSTEM 服务

**正确结论边界**：源码证明项目有 secure desktop 和 SYSTEM service 的实现路径，
但是否在当前包中可用仍需验证安装形态、service `ImagePath`、运行权限和实际 UAC
交互。不能从“找到源码”直接跳到“当前发布包一定可用”。

## 适用仓库速查

| 问题类型 | 先搜索的位置 |
|---------|------------|
| RustDesk PC 端能力 | `D:\ideas\rustdesk\src\` |
| RustDesk server | `D:\ideas\rustdesk-server\` |
| HDesk Harmony client | `D:\ohos\hdesk\Application\` |
