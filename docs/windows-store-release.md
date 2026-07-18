# HDesk Windows Store Release

## Current Status

Microsoft Store/MSIX 当前是 POC，不是已验证生产渠道。实现入口位于：

- `packaging/msix/build_msix.ps1`
- `packaging/msix/README.md`

在以下门禁全部完成前，官网和 release 文档不得把 Store 描述为唯一或主发布渠道。

## Required Before Production

1. 确认 Partner Center 保留的 Package Identity、Publisher、显示名称和版本规则。
2. 使用正式 identity 生成 MSIX，并完成正式证书/Store 签名链。
3. 通过 Windows App Certification Kit。
4. 在 packaged runtime 禁用或正确隔离 classic `latest.json` self-updater，避免
   Store 更新与应用内更新竞争。
5. 明确 Store build 的 service 策略。当前 POC 是 no-service packaged classic app，
   不具备 classic installer 的 SYSTEM service、secure desktop/UAC 等完整能力。
6. 验证安装、升级、降级限制、卸载、数据迁移、协议唤起和单实例行为。
7. 验证 x64 支持范围、隐私声明、商店素材、年龄分级和审核说明。
8. 完成至少一次 Partner Center private flight，再进行公开提交。

## Local POC

```powershell
powershell -NoProfile -ExecutionPolicy Bypass `
  -File .\packaging\msix\build_msix.ps1 `
  -Publisher "CN=<Partner Center Publisher>" `
  -PublisherDisplayName "<Publisher Display Name>" `
  -PackageName "<Reserved Package Identity>" `
  -DisplayName "HDesk"
```

`-DevSign` 和 `LocalMachine\TrustedPeople` 只用于本地 sideload 验证，不能作为
生产签名或 Store readiness 的证据。

## Promotion Rule

Store 渠道状态只有在 private flight、WACK、updater gating、identity、升级和能力
差异均验证后，才能从 POC 改为 Active。渠道状态变化同时更新：

- `docs/windows-release-standard.md`
- `README.md`
- 官网 Windows 下载入口
