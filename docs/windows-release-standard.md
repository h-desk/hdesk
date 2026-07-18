# HDesk Windows Release Standard

## Channel Status

状态基线：2026-07-18。渠道状态必须按事实更新，不能把目标架构写成已跑通流程。

| Channel | Status | Current role |
|---|---|---|
| Classic EXE self-update on AGC Storage/Hosting | Active | 当前线上 `1.0.1` 的已验证生产链路 |
| GitHub Actions + SignPath + SSH | Not operationally verified | 目标签名链路；可见历史运行均未完成发布 |
| Microsoft Store / MSIX | POC | 尚未完成 Partner Center、WACK 和 updater gating |

线上机器可读真源是：

- `https://releases.hdesk.yunjichuangzhi.cn/latest.json`

当前 AGC 发布脚本仍位于 Harmony 工作区：

- `D:\ohos\hdesk\Application\scripts\tools\release_hdesk.mjs`
- `D:\ohos\hdesk\Application\scripts\tools\deploy_hdesk_hosting.mjs`

在脚本迁入专属基础设施仓之前，不得复制出第二份修改版。发布时使用现有脚本，
并按本文的签名和验收门禁执行。

## Version Sources

发版前同步检查：

1. `Cargo.toml` 的版本决定版本化包名。
2. `flutter/pubspec.yaml` 决定 UI 和 runner 内嵌版本。
3. `src/common.rs` 中 `OFFICIAL_WINDOWS_UPDATE_VERSION` 与 fallback URL 必须与
   本次稳定版本一致，或者在发布前明确不更新 fallback。
4. `latest.json` 的 `version`、release page、direct EXE、install EXE 和可选 MSI
   必须指向同一版本。

不允许只校验 `Cargo.toml`。文件名正确但 UI、资源或 fallback 仍为旧版本同样是
发布失败。

## Local Release Gate

```powershell
cd D:\ideas\rustdesk
$env:Path = "D:\software\flutter-3.41.4\bin;$env:Path"

cargo check --lib
python build.py --flutter
powershell -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\windows\build-install-validate.ps1
```

必须检查 fresh runner 和每个待发布资产：

```powershell
$artifacts = @(
  '.\flutter\build\windows\x64\runner\Release\hdesk.exe',
  '.\hdesk-<version>-install.exe',
  '.\hdesk-<version>-x86_64.exe'
)

Get-Item $artifacts | Select-Object Name,Length,LastWriteTime,
  @{Name='FileVersion';Expression={$_.VersionInfo.FileVersion}},
  @{Name='ProductVersion';Expression={$_.VersionInfo.ProductVersion}}

Get-AuthenticodeSignature $artifacts |
  Select-Object Path,Status,@{Name='Signer';Expression={$_.SignerCertificate.Subject}}
```

发布门禁：

- runner、installed app 的 `hdesk.exe`、`data\app.so`、`librustdesk.dll` 哈希一致。
- 文件名版本、runner 内嵌版本、portable EXE 内嵌版本一致。
- 对外 EXE/MSI 的 Authenticode 状态必须是 `Valid`；unsigned 包不得上传。
- 管理员验证时 HDesk service 为 Running 且 `ImagePath` 指向本次 installed EXE
  加 `--service`；非管理员验证必须明确记录 `SKIPPED_NOT_ELEVATED`。
- 从最终安装路径启动 clean UI，不能把 `--silent-install` 子进程当成应用实例。

`libs/portable/app_metadata.toml` 的时间戳是打包副产物，不作为功能变更提交。

## Current Production Publish Path

在 SignPath workflow 尚未完成一次端到端成功运行前，当前现网发布按 AGC 手工链执行：

1. 通过上面的本地门禁，并取得已签名资产。
2. 先运行 `release_hdesk.mjs --dry-run`，核对版本、资产和目标。
3. 正式运行 `release_hdesk.mjs` 上传资产并重写 release 元数据。
4. 运行 `deploy_hdesk_hosting.mjs --dry-run`。
5. 正式运行 `deploy_hdesk_hosting.mjs` 发布 portal、tag page 和 `latest.json`。
6. 从线上重新下载 `latest.json` 和资产，再校验版本、哈希与 Authenticode。

脚本参数和 AGC 登录方式以
`D:\ohos\hdesk\Application\docs\windows-desktop-release-playbook.md` 为历史操作
参考，但渠道状态和发布门禁以本文为准。该外部手册不得保存设备密码或其他凭据。

## Target Signed Workflow

仓库目标 workflow 是 `.github/workflows/hdesk-windows-release.yml`。它构建
unsigned runner、两阶段 SignPath、MSI/EXE、`latest.json` 并通过 SSH 发布。

启用前必须满足：

- 所有 SignPath 和 SSH variables/secrets 已配置。
- workflow 同时校验 `Cargo.toml` 与 `flutter/pubspec.yaml` 主版本。
- 使用目标 release profile 完成一次非生产演练。
- 签名后的 EXE/MSI、安装、service、self-update 和 rollback 全部通过。
- 至少一次 workflow run 完整成功后，才可把该渠道状态改为 Active。

不能因为 YAML 存在就声称正式发布成功。

## Metadata Contract

Updater 权威读取：

- `version`
- `downloads.windows.x86_64.exe`

release page URL 用于让客户端根据当前 MSI/EXE 安装形态选择资产。保持以下字段：

- `downloads.windows.x86_64.directExe`
- `downloads.windows.x86_64.msi`，有 MSI 时
- `downloads.windows.install.exe`

上传后必须验证 URL 返回 200，并确认下载到的是本次资产而不是旧缓存。

## Rollback

发版前备份当前线上 `latest.json` 和上一版本下载 URL。出现问题时：

1. 立即停止更新 `latest.json`，不要删除已发布旧资产。
2. 将 `latest.json` 恢复到上一稳定版本并重新部署 hosting。
3. 验证客户端重新解析到上一版本 release page/asset。
4. 保留失败版本供取证，记录 runner、安装包、签名、hash 和 workflow/script 日志。

发布成功的定义是：签名资产可下载、元数据正确、clean install 可用、service 状态
符合运行权限、更新与回滚路径均验证完成。
