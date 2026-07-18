---
name: rustdesk-desktop-workflow
description: Use when working on the Windows HDesk desktop repo at D:\ideas\rustdesk, including Flutter/Rust development, packaging, crash diagnosis, and release work.
---

## Use This Skill When

- The task is in the desktop RustDesk repo `D:\ideas\rustdesk`.
- The user asks for Windows desktop development, build, package, or deploy flow.
- The task touches direct connection, relay fallback, IPv4/IPv6 selection, or desktop client behavior.
- You need the local project layout, build commands, output paths, or deployment routine.

## Project Map

- Desktop client repo: `D:\ideas\rustdesk`
- Server repo: `D:\ideas\rustdesk-server`
- Harmony client repo: `D:\ohos\hdesk`
- Custom rendezvous server: `hdesk.yunjichuangzhi.cn:21116`
- Custom relay server: `hdesk.yunjichuangzhi.cn:21117`

## Working Rules

- Start by checking `git -C D:\ideas\rustdesk status --short`.
- The desktop repo is often dirty. Do not revert unrelated changes.
- Treat `D:\ideas\rustdesk\src\rendezvous_mediator.rs` as high risk unless the user explicitly asks to change it.
- Keep diffs tight. Do not do formatting-only edits.
- For Rust-only logic changes, prefer `cargo check --lib` before any full package build.

## Build Selection

- Rust-only validation: `cargo check --lib`.
- Windows runner: `python build.py --flutter --skip-portable-pack`.
- Local packages: `python build.py --flutter`.
- Build/install validation: `scripts/windows/build-install-validate.ps1`.
- Never describe a package as released until signing, published metadata, and
  downloaded artifacts have all been verified.

## Quick Commands

```powershell
git -C D:\ideas\rustdesk status --short
cd D:\ideas\rustdesk
cargo check --lib
```

If `flutter` is not in `PATH`, use the SDK on this machine:

```powershell
$env:Path = "D:\software\flutter-3.41.4\bin;$env:Path"
```

## Canonical Documentation

- [Windows desktop development](../../../docs/windows-desktop-development.md)
- [Build and install validation](../../../docs/windows-build-install-validation.md)
- [Windows release standard](../../../docs/windows-release-standard.md)
- [Multi-window and crash diagnostics](../../../docs/windows-multi-window-and-crash-diagnostics.md)
- [Skill compatibility reference](references/windows-build-and-deploy.md)
