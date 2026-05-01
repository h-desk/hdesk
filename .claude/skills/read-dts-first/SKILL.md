---
name: read-dts-first
description: 使用任何 SDK API 前必须先读 .d.ts 声明文件确认能力。当判断某 API 是否存在、查参数类型、排查调用报错、或在文档中记录 API 能力时触发。禁止仅凭 Skills/笔记/搜索结果下结论。同样适用于判断开源项目（如 RustDesk）是否支持某功能——必须先 grep 源码，不能凭印象。
---

# Read Source First（源码即真相）

判断任何能力时，**源码是唯一真相**。Skills、笔记、历史文档、记忆都是二手信息，会过时。

## 两类场景都必须遵守

### 场景 A：HarmonyOS SDK API
当需要确认某个 SDK 的 API 能力时：

1. **找到声明文件**：在对应 `node_modules` 下搜索 `.d.ts`
2. **读完整个 class/interface**：不要只搜关键词，通读所有方法——同一能力在不同 SDK 侧可能叫不同名字
3. **以 `.d.ts` 为准**：与笔记/Skills 冲突时，信 `.d.ts`

### 场景 B：开源项目能力判断（RustDesk 等）
当用户问"X 功能是否支持/能否实现"时：

1. **先 grep 本地克隆的源码**（`D:\ideas\rustdesk`）
   ```powershell
   Select-String -Path "D:\ideas\rustdesk\src\**\*.rs" -Pattern "关键词" -Recurse
   ```
2. **找到实现再下结论**；找不到才说"当前源码中未找到实现"
3. **禁止说"很难做/不支持/需要大量工程"**——除非源码确认没有

**反面案例**（UAC 控制，2026-04-19）：
- ❌ 错误：凭记忆说"UAC 安全桌面在独立 Winlogon 桌面，普通进程无法访问，需要大量工程"
- ✅ 正确：grep 后发现 `selectInputDesktop()` + SYSTEM portable service 早已完整实现，安装即可用

## 本项目常用声明文件路径

| SDK | 路径 |
|-----|------|
| 云函数 DB 查询 | `CloudProgram/cloudfunctions/*/node_modules/@hw-agconnect/cloud-server/database-service/query/CloudDBZoneQuery.d.ts` |
| 云函数 DB 操作 | `CloudProgram/cloudfunctions/*/node_modules/@hw-agconnect/cloud-server/database-service/services/CloudDBZone.d.ts` |
| 云函数入口 | `CloudProgram/cloudfunctions/*/node_modules/@hw-agconnect/cloud-server/index.d.ts` |
| 端侧 CloudFoundation | `Application/entry/oh_modules/@kit.CloudFoundationKit/` |
