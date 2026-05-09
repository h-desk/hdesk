[CmdletBinding()]
param(
    [string]$RunnerDir,
    [string]$OutputDir,
    [string]$PackageName = "HDesk.Desktop",
    [string]$Publisher = "CN=Test Publisher",
    [string]$PublisherDisplayName = "Test Publisher",
    [string]$DisplayName = "HDesk",
    [string]$Description = "HDesk Remote Desktop",
    [string]$Version,
    [ValidateSet("x64", "x86", "arm64", "neutral")]
    [string]$Architecture = "x64",
    [string]$ApplicationId = "HDesk",
    [string]$AppExecutable = "hdesk.exe",
    [string]$MinVersion = "10.0.19041.0",
    [string]$MaxVersionTested = "10.0.26100.0",
    [string]$MakeAppxPath,
    [string]$SignToolPath,
    [switch]$DevSign,
    [switch]$TrustMachineCertificate,
    [switch]$NoPackage
)

$ErrorActionPreference = "Stop"

if (-not $RunnerDir) {
    $RunnerDir = Join-Path $PSScriptRoot "..\..\flutter\build\windows\x64\runner\Release"
}

if (-not $OutputDir) {
    $OutputDir = Join-Path $PSScriptRoot "out"
}

function Get-CargoVersion {
    param([string]$CargoTomlPath)

    if (-not (Test-Path $CargoTomlPath)) {
        throw "Cargo.toml not found: $CargoTomlPath"
    }

    $content = Get-Content -Path $CargoTomlPath -Raw
    $match = [regex]::Match($content, '(?m)^version\s*=\s*"([0-9]+(?:\.[0-9]+){1,3})"')
    if (-not $match.Success) {
        throw "Failed to read package version from $CargoTomlPath"
    }

    return $match.Groups[1].Value
}

function Normalize-MsixVersion {
    param([string]$RawVersion)

    if ([string]::IsNullOrWhiteSpace($RawVersion)) {
        throw "Version cannot be empty"
    }

    $clean = ($RawVersion -split '-', 2)[0]
    $parts = $clean.Split('.') | Where-Object { $_ -ne '' }
    if ($parts.Count -gt 4) {
        throw "MSIX version supports up to 4 numeric parts: $RawVersion"
    }
    foreach ($part in $parts) {
        if ($part -notmatch '^[0-9]+$') {
            throw "MSIX version must be numeric: $RawVersion"
        }
    }
    while ($parts.Count -lt 4) {
        $parts += '0'
    }
    return ($parts -join '.')
}

function Escape-XmlText {
    param([string]$Value)

    return [System.Security.SecurityElement]::Escape($Value)
}

function Resolve-MakeAppx {
    param([string]$ExplicitPath)

    if ($ExplicitPath) {
        if (-not (Test-Path $ExplicitPath)) {
            throw "MakeAppx.exe not found: $ExplicitPath"
        }
        return (Resolve-Path $ExplicitPath).Path
    }

    $kitsRoot = "C:\Program Files (x86)\Windows Kits\10\bin"
    if (-not (Test-Path $kitsRoot)) {
        throw "Windows SDK bin folder not found: $kitsRoot"
    }

    $candidate = Get-ChildItem -Path $kitsRoot -Directory |
        Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName "x64\makeappx.exe" } |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1

    if (-not $candidate) {
        throw "Failed to find MakeAppx.exe under $kitsRoot"
    }

    return $candidate
}

function Resolve-SignTool {
    param([string]$ExplicitPath)

    if ($ExplicitPath) {
        if (-not (Test-Path $ExplicitPath)) {
            throw "SignTool.exe not found: $ExplicitPath"
        }
        return (Resolve-Path $ExplicitPath).Path
    }

    $kitsRoot = "C:\Program Files (x86)\Windows Kits\10\bin"
    if (-not (Test-Path $kitsRoot)) {
        throw "Windows SDK bin folder not found: $kitsRoot"
    }

    $candidate = Get-ChildItem -Path $kitsRoot -Directory |
        Sort-Object Name -Descending |
        ForEach-Object {
            @(
                (Join-Path $_.FullName "x64\signtool.exe"),
                (Join-Path $_.FullName "x86\signtool.exe")
            )
        } |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1

    if (-not $candidate) {
        throw "Failed to find SignTool.exe under $kitsRoot"
    }

    return $candidate
}

function Test-IsElevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Export-CertificateFile {
    param(
        [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
        [string]$Path
    )

    $directory = Split-Path -Path $Path -Parent
    if ($directory) {
        New-Item -Path $directory -ItemType Directory -Force | Out-Null
    }

    $bytes = $Certificate.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert)
    [System.IO.File]::WriteAllBytes($Path, $bytes)
}

function Import-CertificateToLocalMachineTrustedPeople {
    param([string]$Path)

    if (-not (Test-IsElevated)) {
        throw "-TrustMachineCertificate requires an elevated PowerShell session"
    }

    Import-Certificate -FilePath $Path -CertStoreLocation "Cert:\LocalMachine\TrustedPeople" | Out-Null
}

function Get-OrCreateDevCertificate {
    param([string]$Subject)

    if ($Subject -notmatch '^CN=') {
        throw "-DevSign requires -Publisher to be an X.509 subject string such as CN=HDesk Dev Publisher"
    }

    $now = Get-Date
    $cert = Get-ChildItem -Path Cert:\CurrentUser\My |
        Where-Object {
            $_.Subject -eq $Subject -and
            $_.HasPrivateKey -and
            $_.NotAfter -gt $now
        } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1

    if (-not $cert) {
        $cert = New-SelfSignedCertificate `
            -Type CodeSigningCert `
            -Subject $Subject `
            -CertStoreLocation "Cert:\CurrentUser\My"
    }

    return $cert
}

function Sign-MsixPackage {
    param(
        [string]$PackagePath,
        [string]$ToolPath,
        [string]$Thumbprint
    )

    & $ToolPath sign /fd SHA256 /sha1 $Thumbprint /s My $PackagePath
    if ($LASTEXITCODE -ne 0) {
        throw "SignTool signing failed with exit code $LASTEXITCODE"
    }
}

function New-CleanDirectory {
    param([string]$Path)

    if (Test-Path $Path) {
        Remove-Item -Path $Path -Recurse -Force
    }
    New-Item -Path $Path -ItemType Directory | Out-Null
}

function Copy-RequiredAsset {
    param(
        [string]$Source,
        [string]$Destination
    )

    if (-not (Test-Path $Source)) {
        throw "Required asset not found: $Source"
    }
    Copy-Item -Path $Source -Destination $Destination -Force
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$cargoTomlPath = Join-Path $repoRoot "Cargo.toml"
$templatePath = Join-Path $PSScriptRoot "AppxManifest.template.xml"

if (-not $Version) {
    $Version = Normalize-MsixVersion (Get-CargoVersion -CargoTomlPath $cargoTomlPath)
} else {
    $Version = Normalize-MsixVersion $Version
}

$RunnerDir = (Resolve-Path $RunnerDir).Path
if (-not (Test-Path (Join-Path $RunnerDir $AppExecutable))) {
    throw "Runner executable not found: $(Join-Path $RunnerDir $AppExecutable)"
}

if (-not (Test-Path $templatePath)) {
    throw "Manifest template not found: $templatePath"
}

$makeAppx = $null
if (-not $NoPackage) {
    $makeAppx = Resolve-MakeAppx -ExplicitPath $MakeAppxPath
}

if ($TrustMachineCertificate -and -not $DevSign) {
    throw "-TrustMachineCertificate can only be used together with -DevSign"
}

$signTool = $null
if ($DevSign) {
    $signTool = Resolve-SignTool -ExplicitPath $SignToolPath
}

$stageDir = Join-Path $OutputDir "stage"
$assetsDir = Join-Path $stageDir "Assets"
$packageDir = Join-Path $OutputDir "packages"
$packageFileName = "{0}_{1}_{2}.msix" -f $PackageName, $Version, $Architecture
$packagePath = Join-Path $packageDir $packageFileName

New-CleanDirectory -Path $stageDir
New-Item -Path $assetsDir -ItemType Directory -Force | Out-Null
New-Item -Path $packageDir -ItemType Directory -Force | Out-Null

Copy-Item -Path (Join-Path $RunnerDir '*') -Destination $stageDir -Recurse -Force

$resDir = Join-Path $repoRoot "res"
Copy-RequiredAsset -Source (Join-Path $resDir "128x128.png") -Destination (Join-Path $assetsDir "StoreLogo.png")
Copy-RequiredAsset -Source (Join-Path $resDir "128x128.png") -Destination (Join-Path $assetsDir "Square150x150Logo.png")
Copy-RequiredAsset -Source (Join-Path $resDir "64x64.png") -Destination (Join-Path $assetsDir "Square44x44Logo.png")
Copy-RequiredAsset -Source (Join-Path $resDir "64x64.png") -Destination (Join-Path $assetsDir "Square71x71Logo.png")
Copy-RequiredAsset -Source (Join-Path $resDir "128x128.png") -Destination (Join-Path $assetsDir "Wide310x150Logo.png")
Copy-RequiredAsset -Source (Join-Path $resDir "128x128.png") -Destination (Join-Path $assetsDir "SplashScreen.png")

$manifestTemplate = Get-Content -Path $templatePath -Raw
$manifestContent = $manifestTemplate
$replacements = [ordered]@{
    "{{PACKAGE_NAME}}" = (Escape-XmlText $PackageName)
    "{{PUBLISHER}}" = (Escape-XmlText $Publisher)
    "{{VERSION}}" = $Version
    "{{ARCHITECTURE}}" = $Architecture
    "{{DISPLAY_NAME}}" = (Escape-XmlText $DisplayName)
    "{{PUBLISHER_DISPLAY_NAME}}" = (Escape-XmlText $PublisherDisplayName)
    "{{DESCRIPTION}}" = (Escape-XmlText $Description)
    "{{APPLICATION_ID}}" = (Escape-XmlText $ApplicationId)
    "{{APP_EXECUTABLE}}" = (Escape-XmlText $AppExecutable)
    "{{MIN_VERSION}}" = $MinVersion
    "{{MAX_VERSION_TESTED}}" = $MaxVersionTested
}

foreach ($entry in $replacements.GetEnumerator()) {
    $manifestContent = $manifestContent.Replace($entry.Key, $entry.Value)
}

$manifestPath = Join-Path $stageDir "AppxManifest.xml"
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($manifestPath, $manifestContent, $utf8NoBom)

Write-Host "MSIX staging directory ready:" $stageDir
Write-Host "MSIX manifest:" $manifestPath

if ($NoPackage) {
    Write-Host "Skipping MakeAppx packaging because -NoPackage was specified."
    Write-Output $stageDir
    exit 0
}

if (Test-Path $packagePath) {
    Remove-Item -Path $packagePath -Force
}

& $makeAppx pack /o /d $stageDir /p $packagePath
if ($LASTEXITCODE -ne 0) {
    throw "MakeAppx packaging failed with exit code $LASTEXITCODE"
}

if ($DevSign) {
    $devCert = Get-OrCreateDevCertificate -Subject $Publisher
    Sign-MsixPackage -PackagePath $packagePath -ToolPath $signTool -Thumbprint $devCert.Thumbprint
    $certDir = Join-Path $OutputDir "certs"
    $certFileName = "{0}_{1}.cer" -f $PackageName, $Version
    $certPath = Join-Path $certDir $certFileName
    Export-CertificateFile -Certificate $devCert -Path $certPath
    if ($TrustMachineCertificate) {
        Import-CertificateToLocalMachineTrustedPeople -Path $certPath
        Write-Host "Development certificate imported into LocalMachine\\TrustedPeople:" $certPath
    } else {
        Write-Host "Development certificate exported:" $certPath
        Write-Host "Local MSIX install still requires admin trust import. Run this in an elevated PowerShell:"
        Write-Host ("Import-Certificate -FilePath '{0}' -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople'" -f $certPath)
    }
    Write-Host "MSIX package signed for local testing with cert:" $devCert.Subject
    Write-Host "MSIX signing thumbprint:" $devCert.Thumbprint
}

Write-Host "MSIX package created:" $packagePath
Write-Output $packagePath