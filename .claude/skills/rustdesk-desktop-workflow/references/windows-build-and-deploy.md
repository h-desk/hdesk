# Windows Build And Deploy

This compatibility reference intentionally contains no independent project
facts. The repository documentation is authoritative:

- [Development and build profiles](../../../../docs/windows-desktop-development.md)
- [Build and install validation](../../../../docs/windows-build-install-validation.md)
- [Release channels and publishing](../../../../docs/windows-release-standard.md)
- [Microsoft Store status](../../../../docs/windows-store-release.md)
- [Multi-window and native crash diagnosis](../../../../docs/windows-multi-window-and-crash-diagnostics.md)

Stable local entry points:

```powershell
cd D:\ideas\rustdesk
$env:Path = "D:\software\flutter-3.41.4\bin;$env:Path"

cargo check --lib
python build.py --flutter --skip-portable-pack
python build.py --flutter
powershell -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\windows\build-install-validate.ps1
```

The package names are `hdesk-{version}-install.exe` and
`hdesk-{version}-x86_64.exe`. Do not use legacy `rustdesk-*` names when
validating HDesk artifacts.
