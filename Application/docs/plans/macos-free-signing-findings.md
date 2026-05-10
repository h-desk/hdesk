# macOS 免费签名上限调研 Findings

## 官方依据
- Apple Developer Program 免费层只提供学习、Xcode、设备测试；分发需要加入付费会员。
- Xcode Personal Team 仅用于个人设备测试，限制包括：每个平台最多 3 台测试设备、描述文件 7 天过期、App ID 7 天过期。
- Developer ID 页面明确：站外分发 macOS 软件需要 Apple Developer Program/Enterprise Program 成员身份、Developer ID 证书，并提交 notarization。
- Apple 支持文档明确区分两类阻断：
  - 未知开发者：无法验证开发者
  - 未公证或无法完成恶意软件检查：Apple 无法检查 App 是否包含恶意软件
- Apple 支持文档明确：对未公证或未知开发者 App，用户只能在“系统设置 > 隐私与安全性”里点“仍要打开”后放行。

## 分档结论
1. 未签名 / ad hoc / 自签名
- 最多做到“未知开发者/无法验证开发者”这一档。
- 用户必须手动覆盖安全策略，无法得到 Apple 已检查提示。
- 不适合公开下载分发。

2. 免费 Apple ID / Personal Team
- 只适合开发者自己或极少数测试设备短期安装测试。
- 不提供 Developer ID，也不提供 notarization 能力。
- 不能作为面向公众的 macOS 发布方案。

3. 付费会员 + Developer ID 未公证
- 能显示开发者身份，但仍会落到“Apple 无法检查 App 是否包含恶意软件”这一档。
- 用户仍需手动去系统设置里点“仍要打开”。
- 可用于小范围受控分发，不适合普通公众下载。

4. 付费会员 + Developer ID + notarization
- 唯一能达到 Apple 官方认可的低摩擦站外分发方案。
- 首开通常是“此 App 来自互联网，是否打开”级别，而不是恶意软件/未知开发者拦截。
- 这也是达到“Apple 已检查、未检测到恶意软件”语义的唯一路线。

## 对当前项目的直接含义
- 用户要求的“只有图里那种安全提示才行”，免费路线达不到。
- 当前可行上线方案应以 Developer ID + notarization 的 DMG/ZIP 分发为准。
