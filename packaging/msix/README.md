# HDesk MSIX POC

This directory contains the first MSIX Store proof-of-concept scaffold for the
Windows desktop client.

Current scope:

- package the existing Flutter Windows runner output as a `packagedClassicApp`
- generate a minimal `AppxManifest.xml` from a template
- stage logo assets from the existing `res/` directory
- create an `.msix` package with `MakeAppx.exe`
- optionally self-sign the package for local `Add-AppxPackage` testing
- intentionally run as a no-service Store-style standard build; Windows service install paths stay disabled inside packaged runtime

This scaffold does not yet handle:

- Store-specific updater gating or disabling `latest.json` self-update logic
- `desktop6:Service` declaration for the current Windows service path
- signing or Store submission metadata alignment with the final Partner Center identity
- package validation via WACK

Behavior note:

- This MSIX path is currently the Store-oriented standard build.
- It does not install or start the existing Windows service from inside packaged runtime.
- Full SYSTEM-backed coverage for secure desktop, UAC, and similar system UI scenarios remains part of the classic installer/enhanced build path.

Example:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\packaging\msix\build_msix.ps1 `
  -Publisher "CN=Your Partner Center Publisher" `
  -PublisherDisplayName "Your Company" `
  -PackageName "YourReservedPackageIdentity" `
  -DisplayName "HDesk"

powershell -NoProfile -ExecutionPolicy Bypass -File .\packaging\msix\build_msix.ps1 `
  -Publisher "CN=HDesk Dev Publisher" `
  -PublisherDisplayName "HDesk Dev Publisher" `
  -PackageName "HDesk.Desktop" `
  -DisplayName "HDesk" `
  -DevSign

Add-AppxPackage -Path .\packaging\msix\out\packages\HDesk.Desktop_1.0.0.0_x64.msix
```

Useful switches:

- `-NoPackage`: only stage files and render `AppxManifest.xml`
- `-RunnerDir`: point at another runner output directory
- `-Version`: override the MSIX package version; defaults to `Cargo.toml` version with a `.0` revision appended if needed
- `-DevSign`: create or reuse a current-user self-signed code-signing certificate that matches `-Publisher`, sign the package, and export a `.cer` file for machine trust import
- `-TrustMachineCertificate`: import the exported dev certificate into `LocalMachine\TrustedPeople`; requires an elevated PowerShell session and is intended only for local sideload testing

Important for local install:

- `Add-AppxPackage` does not use the User certificate store for package identity verification.
- For a self-signed dev package, an administrator must import the exported `.cer` into `LocalMachine\TrustedPeople` before installation.
- Non-elevated `-DevSign` is still useful because it produces a signed `.msix` and the matching certificate file for that admin import step.

Expected output:

- staging directory: `packaging/msix/out/stage`
- package file: `packaging/msix/out/packages/<PackageName>_<Version>_<Architecture>.msix`