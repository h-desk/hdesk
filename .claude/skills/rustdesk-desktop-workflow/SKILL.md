---
name: rustdesk-desktop-workflow
description: Use when working on the Windows desktop RustDesk repo at D:\ideas\rustdesk, including client-side network/direct-connect fixes, Flutter desktop builds, Windows packaging, and deployment against the custom hdesk.yunjichuangzhi.cn server.
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

- For fast validation of Rust logic only:
  - Run `cargo check --lib`.
- For a Windows Flutter desktop binary without portable packing:
  - Read [windows-build-and-deploy.md](d:\ohos\hdesk\.claude\skills\rustdesk-desktop-workflow\references\windows-build-and-deploy.md) and use the `skip portable` flow.
- For a full Windows package deliverable:
  - Read [windows-build-and-deploy.md](d:\ohos\hdesk\.claude\skills\rustdesk-desktop-workflow\references\windows-build-and-deploy.md) and use the `full package` flow.
- For remote deployment/testing:
  - Read [windows-build-and-deploy.md](d:\ohos\hdesk\.claude\skills\rustdesk-desktop-workflow\references\windows-build-and-deploy.md), then confirm the target host/port is reachable before trying to ship artifacts.

## Quick Commands

```powershell
git -C D:\ideas\rustdesk status --short
cd D:\ideas\rustdesk
cargo check --lib
```

If `flutter` is not in `PATH`, use the SDK on this machine:

```powershell
$env:Path = "D:\software\flutter\bin;$env:Path"
```

## Detailed Reference

- Windows build, package, and deploy steps:
  - [windows-build-and-deploy.md](d:\ohos\hdesk\.claude\skills\rustdesk-desktop-workflow\references\windows-build-and-deploy.md)
