---
name: hdesk-project-knowledge
description: HDesk HarmonyOS remote desktop project knowledge. Use when working in `d:\ohos\hdesk` on ArkTS pages, Rust core packaging, HAP build/deploy, debug capture collection, or repo organization rules.
---

## Root rules

- At repo root, only `Application/`, `CloudProgram/`, and dot-prefixed infra files or directories may exist as project content.
- Put archived debug outputs under `Application/artifacts/`.
- Use `Application/artifacts/logs/` for build logs, hilog dumps, diag outputs, symbol dumps.
- Use `Application/artifacts/screenshots/` for screenshots and captured phone images.
- Use `Application/artifacts/layouts/` for `dumpLayout` JSON and other UI tree exports.
- Use `Application/artifacts/packages/` for generated zip or hap inspection packages.
- Use `Application/artifacts/extracted/` for unpacked hap contents and probe build outputs.
- Use `Application/docs/session-notes/` for long chat or test notes.
- Use `Application/docs/certs/` for signing or certificate inspection text dumps.
- Use `Application/scripts/tools/` for one-off helper scripts that are still worth keeping.
- Do not create new non-dot files or directories at repo root. Harmony build manifests belong under `Application/`, for example `Application/build-profile.json5`, `Application/hvigorfile.ts`, and `Application/oh-package.json5`.
- Keep `Application/entry/src/main/resources/rawfile/agconnect-services.json` present for local AGC builds, but keep it gitignored because it is environment-specific.

## Project map

- Bundle: `top.wangqiukeke.hdesk`
- Application root: `Application/`
- Entry module: `Application/entry/`
- ArkTS pages: `Application/entry/src/main/ets/pages/`
- Ability entry: `Application/entry/src/main/ets/entryability/EntryAbility.ets`
- Native bridge typing: `Application/entry/src/main/cpp/types/libhdesk/index.d.ts`
- Rust core: `Application/rustdesk_core/`
- Signed HAP output: `Application/entry/build/default/outputs/default/entry-default-signed.hap`
- Companion website repo: `D:\ideas\apps`
- HDesk website entry: `D:\ideas\apps\hdesk\index.html`
- HDesk website page: `D:\ideas\apps\hdesk\Main.vue`
- HDesk agreement and privacy pages: `D:\ideas\apps\public\hdesk\`
- Harmony server defaults:
  - `hbbs`: `hdesk.yunjichuangzhi.cn:21116`
  - `hbbr`: `hdesk.yunjichuangzhi.cn:21117`

## Build workflow

### Full rebuild after Rust changes

```powershell
cd D:\ohos\hdesk\Application\rustdesk_core
cargo build --release --target aarch64-unknown-linux-ohos

cd D:\ohos\hdesk\Application
Copy-Item "rustdesk_core\target\aarch64-unknown-linux-ohos\release\librdcore.so" `
  "rustdesk_core\libs\arm64-v8a\librdcore.so" -Force

Remove-Item "entry\build\default\intermediates" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item ".hvigor\incremental" -Recurse -Force -ErrorAction SilentlyContinue

& "D:\ohos\command-line-tools\bin\hvigorw.bat" assembleHap --no-daemon
```

### ArkTS-only rebuild

```powershell
cd D:\ohos\hdesk\Application
Remove-Item "entry\build\default\intermediates" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item ".hvigor\incremental" -Recurse -Force -ErrorAction SilentlyContinue
& "D:\ohos\command-line-tools\bin\hvigorw.bat" assembleHap --no-daemon
```

### Install and launch

```powershell
cd D:\ohos\hdesk\Application
hdc app install -r "entry\build\default\outputs\default\entry-default-signed.hap"
hdc shell aa force-stop top.wangqiukeke.hdesk
hdc shell aa start -a EntryAbility -b top.wangqiukeke.hdesk
```

## Debug commands

```powershell
# Pull Rust diag log from device
hdc shell "tail -20 /data/app/el2/100/base/top.wangqiukeke.hdesk/haps/entry/files/rdcore_diag.txt"

# Capture screenshot to artifacts
hdc shell uitest screenCap -p /data/local/tmp/sc.png
hdc file recv /data/local/tmp/sc.png D:\ohos\hdesk\Application\artifacts\screenshots\sc.png

# Capture UI layout tree to artifacts
hdc shell uitest dumpLayout -p /data/local/tmp/layout.json
hdc file recv /data/local/tmp/layout.json D:\ohos\hdesk\Application\artifacts\layouts\layout.json

# Restart app quickly
hdc shell aa force-stop top.wangqiukeke.hdesk
hdc shell aa start -a EntryAbility -b top.wangqiukeke.hdesk
```

## Editing constraints

- Prefer `apply_patch` for `.ets`, `.json`, `.md`, and config changes.
- Do not use PowerShell `Set-Content` to rewrite `.ets` files. It has already caused UTF-8 corruption in this repo.
- When adding pages, update both:
  - `Application/entry/src/main/resources/base/profile/main_pages.json`
  - `Application/entry/src/main/ets/entryability/EntryAbility.ets` if the launch route changes
- If a change affects app startup, verify both cold start and reconnect flow.

## Runtime notes

- `Application/entry/src/main/ets/pages/LaunchPage.ets` is the cold-start shell. It handles the first-run agreement before routing to `Index.ets`.
- `Application/entry/src/main/ets/pages/Index.ets` is the connect page and home screen.
- `Application/entry/src/main/ets/pages/RemotePage.ets` owns the active remote session, reconnect-in-place behavior, zoom, and gesture mapping.
- `Application/entry/src/main/ets/pages/AccountCenter.ets` now contains the real client-side member flow: AGC Auth initialization, Huawei account upgrade, IAP product query, purchase, and subscription restore.
- For module-scoped resources such as `Application/entry/src/main/resources/rawfile/agconnect-services.json` and preferences under `haps/entry/preferences`, use a page or module `UIAbilityContext` from `getUIContext().getHostContext()`.
- Do not use `EntryAbility.this.context` as the long-lived context for AGC/Auth/prefs lookups. It can miss module rawfiles or read or write the wrong preference scope.
- `Application/entry/src/main/ets/services/AuthService.ets` owns AGC Auth init plus guest-mode fallback; app startup must remain usable even if AGC init fails.
- `Application/entry/src/main/ets/services/PaymentService.ets` currently uses IAPKit current-entitlement and unfinished-order restore as the source of truth for VIP state.
- Server-side entitlement sync and cloud functions are still future work; keep the current flow degradable even when those backends are absent.
