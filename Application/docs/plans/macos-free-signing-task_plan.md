# macOS 免费签名上限调研

## 目标
厘清 macOS 站外分发中，免费或低成本路线分别最多能做到什么 Gatekeeper 体验，并明确哪一档能达到“Apple 已检查、未检测到恶意软件”。

## 验证范围
- 免费 Apple ID / Xcode Personal Team
- 未签名、ad hoc、自签名
- 付费 Apple Developer Program 下的 Developer ID 未公证
- Developer ID + notarization

## 已验证结论
- 免费 Apple ID 只支持个人设备测试，不支持面向公众的正式分发。
- Apple 官方将 Notarization & Developer ID 列为 Apple Developer Program 会员权益。
- 对外分发 macOS App 时，官方要求使用 Developer ID 并进行 notarization 以获得更好的用户打开体验。
- 未公证或未知开发者 App 仍可被用户在“隐私与安全性”里手动放行，但不属于低摩擦分发。

## 输出要求
- 给出每一档的用户首开提示
- 给出是否需要“仍要打开”
- 给出是否适合公开下载分发
- 明确推荐的实际落地方案
