[CmdletBinding()]
param(
    [ValidateNotNullOrEmpty()]
    [string]$FlutterRoot = 'D:\software\flutter-3.41.4',

    [ValidateNotNullOrEmpty()]
    [string]$PythonExecutable = 'python',

    [switch]$SkipBuild,

    [string]$InstallerPath,

    [string]$InstalledExePath,

    [ValidateRange(10, 1800)]
    [int]$InstallTimeoutSeconds = 180,

    [ValidateRange(1, 300)]
    [int]$ConfigTimeoutSeconds = 30,

    [string]$ConfigPath = (Join-Path $env:APPDATA 'HDesk\config\HDesk2.toml'),

    [ValidateNotNullOrEmpty()]
    [string]$ConfigOptionName = 'key',

    [ValidateNotNullOrEmpty()]
    [string]$ExpectedConfigKeyEnvironmentVariable = 'HDESK_EXPECTED_CONFIG_KEY',

    [switch]$SkipLaunch
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw 'WINDOWS_ONLY_SCRIPT'
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$runnerDir = Join-Path $repoRoot 'flutter\build\windows\x64\runner\Release'
$runnerExe = Join-Path $runnerDir 'hdesk.exe'
$serviceName = 'HDesk'

if (-not (Test-Path (Join-Path $repoRoot 'build.py') -PathType Leaf)) {
    throw "REPOSITORY_ROOT_NOT_FOUND: $repoRoot"
}

function Stop-HDeskRuntime {
    $service = Get-Service -Name $script:serviceName -ErrorAction SilentlyContinue
    if ($service -and $service.Status -ne 'Stopped') {
        Stop-Service -Name $script:serviceName -Force -ErrorAction SilentlyContinue
    }

    Get-Process -Name hdesk, rustdesk -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
}

function Test-IsElevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-Sha256 {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if (-not (Test-Path $Path -PathType Leaf)) {
        return $null
    }

    return (Get-FileHash -Algorithm SHA256 -Path $Path).Hash
}

function Resolve-RepositoryRelativePath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }

    return [System.IO.Path]::GetFullPath((Join-Path $script:repoRoot $Path))
}

function Get-SilentInstallProcesses {
    return @(
        Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
            Where-Object {
                $_.Name -match '^(hdesk|rustdesk)(\.exe)?$' -and
                $_.CommandLine -match '--silent-install'
            }
    )
}

function Test-InstalledBundleReady {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExePath,

        [Parameter(Mandatory = $true)]
        [datetime]$MinLastWriteTime
    )

    $installedDir = Split-Path -Parent $ExePath
    $bundleFiles = @(
        $ExePath,
        (Join-Path $installedDir 'data\app.so'),
        (Join-Path $installedDir 'librustdesk.dll')
    )

    foreach ($file in $bundleFiles) {
        if (-not (Test-Path $file -PathType Leaf)) {
            return $false
        }

        $item = Get-Item $file
        if ($item.LastWriteTime -lt $MinLastWriteTime.AddSeconds(-2)) {
            return $false
        }
    }

    return $true
}

function Wait-InstalledExeReady {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Candidates,

        [Parameter(Mandatory = $true)]
        [datetime]$MinLastWriteTime,

        [int]$TimeoutSeconds = 180
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $silentInstallProcesses = @(Get-SilentInstallProcesses)
        foreach ($candidate in $Candidates) {
            if ((Test-InstalledBundleReady -ExePath $candidate -MinLastWriteTime $MinLastWriteTime) -and
                $silentInstallProcesses.Count -eq 0) {
                return $candidate
            }
        }

        Start-Sleep -Milliseconds 500
    }

    throw "INSTALLED_EXE_NOT_READY: timeout_seconds=$TimeoutSeconds"
}

function Compare-InstalledBundleWithRunner {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunnerDirectory,

        [Parameter(Mandatory = $true)]
        [string]$InstalledExe
    )

    $installedDir = Split-Path -Parent $InstalledExe
    $pairs = @(
        @{
            Label = 'HDESK_EXE'
            Runner = (Join-Path $RunnerDirectory 'hdesk.exe')
            Installed = $InstalledExe
        },
        @{
            Label = 'DATA_APP_SO'
            Runner = (Join-Path $RunnerDirectory 'data\app.so')
            Installed = (Join-Path $installedDir 'data\app.so')
        },
        @{
            Label = 'LIBRUSTDESK_DLL'
            Runner = (Join-Path $RunnerDirectory 'librustdesk.dll')
            Installed = (Join-Path $installedDir 'librustdesk.dll')
        }
    )

    $results = @()
    foreach ($pair in $pairs) {
        $runnerHash = Get-Sha256 -Path $pair.Runner
        $installedHash = Get-Sha256 -Path $pair.Installed
        if (-not $runnerHash -or -not $installedHash) {
            throw "INSTALLED_BUNDLE_FILE_MISSING: $($pair.Label)"
        }
        if ($runnerHash -ne $installedHash) {
            throw "INSTALLED_BUNDLE_HASH_MISMATCH: $($pair.Label)"
        }

        $results += [PSCustomObject]@{
            Label = $pair.Label
            RunnerHash = $runnerHash
            InstalledHash = $installedHash
        }
    }

    return $results
}

function Wait-ServiceDeleted {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [int]$TimeoutSeconds = 30
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $service = Get-CimInstance Win32_Service -Filter "Name='$Name'" -ErrorAction SilentlyContinue
        if (-not $service) {
            return
        }
        Start-Sleep -Milliseconds 500
    }

    throw "SERVICE_DELETE_TIMEOUT: $Name"
}

function Wait-ServiceRunning {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [int]$TimeoutSeconds = 30
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $service = Get-CimInstance Win32_Service -Filter "Name='$Name'" -ErrorAction SilentlyContinue
        if ($service -and $service.State -eq 'Running') {
            return $service
        }
        Start-Sleep -Milliseconds 500
    }

    throw "SERVICE_NOT_RUNNING: $Name"
}

function Ensure-HDeskService {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExePath
    )

    $expectedPath = '"' + $ExePath + '" --service'
    $service = Get-CimInstance Win32_Service -Filter "Name='$script:serviceName'" -ErrorAction SilentlyContinue

    if ($service -and $service.PathName -ne $expectedPath) {
        Stop-Service -Name $script:serviceName -Force -ErrorAction SilentlyContinue
        & sc.exe delete $script:serviceName | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "SERVICE_DELETE_FAILED: exit_code=$LASTEXITCODE"
        }
        Wait-ServiceDeleted -Name $script:serviceName
        $service = $null
    }

    if (-not $service) {
        New-Service -Name $script:serviceName -BinaryPathName $expectedPath -DisplayName 'HDesk Service' -StartupType Automatic | Out-Null
    }

    $service = Get-Service -Name $script:serviceName -ErrorAction Stop
    if ($service.Status -ne 'Running') {
        Start-Service -Name $script:serviceName
    }

    $service = Wait-ServiceRunning -Name $script:serviceName
    if ($service.PathName -ne $expectedPath) {
        throw 'SERVICE_PATH_MISMATCH_AFTER_INSTALL'
    }

    return $service
}

function Wait-ConfigOptionValue {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [string]$OptionName,

        [int]$TimeoutSeconds = 30
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $pattern = '^\s*{0}\s*=\s*[''"](?<value>.*)[''"]\s*$' -f [regex]::Escape($OptionName)

    while ((Get-Date) -lt $deadline) {
        if (Test-Path $Path -PathType Leaf) {
            foreach ($line in Get-Content $Path) {
                $match = [regex]::Match($line, $pattern)
                if ($match.Success) {
                    return $match.Groups['value'].Value
                }
            }
        }
        Start-Sleep -Milliseconds 500
    }

    return $null
}

function Get-InstalledExeCandidates {
    param(
        [string]$ExplicitPath
    )

    if ($ExplicitPath) {
        return @((Resolve-RepositoryRelativePath -Path $ExplicitPath))
    }

    $candidates = @(
        'D:\software\HDesk\hdesk.exe',
        'D:\software\HDesk\HDesk.exe',
        (Join-Path $env:LOCALAPPDATA 'hdesk\hdesk.exe'),
        (Join-Path $env:LOCALAPPDATA 'rustdesk\hdesk.exe'),
        (Join-Path $env:ProgramFiles 'HDesk\hdesk.exe'),
        (Join-Path $env:ProgramFiles 'HDesk\HDesk.exe'),
        (Join-Path $env:ProgramFiles 'RustDesk\rustdesk.exe')
    )

    return @($candidates | Where-Object { $_ } | Select-Object -Unique)
}

$originalPath = $env:Path
Push-Location $repoRoot
try {
    if (-not $SkipBuild) {
        $flutterExe = Join-Path $FlutterRoot 'bin\flutter.bat'
        if (-not (Test-Path $flutterExe -PathType Leaf)) {
            throw "MISSING_FLUTTER_SDK: $FlutterRoot"
        }

        $env:Path = (Join-Path $FlutterRoot 'bin') + ';' + $env:Path
        Stop-HDeskRuntime
        & $PythonExecutable (Join-Path $repoRoot 'build.py') --flutter
        $buildExit = $LASTEXITCODE
        if ($buildExit -ne 0) {
            throw "BUILD_FAILED: exit_code=$buildExit"
        }
    }

    if (-not (Test-Path $runnerExe -PathType Leaf)) {
        throw "RUNNER_EXE_NOT_FOUND: $runnerExe"
    }

    Stop-HDeskRuntime

    if ($InstallerPath) {
        $installerFullPath = Resolve-RepositoryRelativePath -Path $InstallerPath
        if (-not (Test-Path $installerFullPath -PathType Leaf)) {
            throw "INSTALLER_NOT_FOUND: $installerFullPath"
        }
        $installer = Get-Item $installerFullPath
    } else {
        $installer = Get-ChildItem -Path $repoRoot -Filter 'hdesk-*-install.exe' -File |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1
        if (-not $installer) {
            throw 'INSTALLER_NOT_FOUND'
        }
    }

    $installStartedAt = Get-Date
    & $installer.FullName --silent-install
    $installerExit = $LASTEXITCODE

    $exeCandidates = Get-InstalledExeCandidates -ExplicitPath $InstalledExePath
    $installedExe = Wait-InstalledExeReady `
        -Candidates $exeCandidates `
        -MinLastWriteTime $installStartedAt `
        -TimeoutSeconds $InstallTimeoutSeconds

    $bundleResults = Compare-InstalledBundleWithRunner `
        -RunnerDirectory $runnerDir `
        -InstalledExe $installedExe

    Stop-HDeskRuntime

    $serviceState = 'SKIPPED_NOT_ELEVATED'
    $servicePath = 'SKIPPED_NOT_ELEVATED'
    $servicePid = $null
    if (Test-IsElevated) {
        $service = Ensure-HDeskService -ExePath $installedExe
        $serviceState = $service.State
        $servicePath = $service.PathName
        $servicePid = $service.ProcessId
    }

    $startedPid = $null
    $startedResponding = $null
    $startState = 'SKIPPED_BY_REQUEST'
    if (-not $SkipLaunch) {
        $process = Start-Process -FilePath $installedExe -PassThru
        Start-Sleep -Seconds 2
        $process.Refresh()
        if ($process.HasExited) {
            throw "INSTALLED_APP_EXITED_EARLY: exit_code=$($process.ExitCode)"
        }
        $startedPid = $process.Id
        $startedResponding = $process.Responding
        $startState = 'RUNNING'
    }

    $configValue = Wait-ConfigOptionValue `
        -Path $ConfigPath `
        -OptionName $ConfigOptionName `
        -TimeoutSeconds $ConfigTimeoutSeconds
    if ([string]::IsNullOrWhiteSpace($configValue)) {
        throw "CONFIG_KEY_MISSING: path=$ConfigPath option=$ConfigOptionName"
    }

    $expectedConfigValue = [Environment]::GetEnvironmentVariable(
        $ExpectedConfigKeyEnvironmentVariable,
        [EnvironmentVariableTarget]::Process
    )
    $configMatchState = 'SKIPPED_NO_EXPECTED_VALUE'
    if (-not [string]::IsNullOrWhiteSpace($expectedConfigValue)) {
        if ($configValue -cne $expectedConfigValue) {
            throw "CONFIG_KEY_MISMATCH: path=$ConfigPath option=$ConfigOptionName"
        }
        $configMatchState = 'MATCHED'
    }

    Write-Output ('REPOSITORY_ROOT=' + $repoRoot)
    Write-Output ('RUNNER_EXE=' + $runnerExe)
    Write-Output ('INSTALLER=' + $installer.FullName)
    Write-Output ('INSTALLER_EXIT=' + $installerExit)
    foreach ($result in $bundleResults) {
        Write-Output ($result.Label + '_RUNNER_HASH=' + $result.RunnerHash)
        Write-Output ($result.Label + '_INSTALLED_HASH=' + $result.InstalledHash)
    }
    Write-Output 'BUNDLE_HASH_STATUS=MATCHED'
    Write-Output ('INSTALLED_EXE=' + $installedExe)
    Write-Output ('SERVICE_STATE=' + $serviceState)
    Write-Output ('SERVICE_PATH=' + $servicePath)
    if ($servicePid) {
        Write-Output ('SERVICE_PID=' + $servicePid)
    }
    Write-Output ('START_STATE=' + $startState)
    if ($startedPid) {
        Write-Output ('STARTED_PID=' + $startedPid)
        Write-Output ('STARTED_RESPONDING=' + $startedResponding)
    }
    Write-Output ('CONFIG_PATH=' + $ConfigPath)
    Write-Output 'CONFIG_KEY_STATE=PRESENT'
    Write-Output ('CONFIG_KEY_MATCH=' + $configMatchState)

    if ($installerExit -ne 0 -and $installerExit -ne 128) {
        throw "INSTALLER_FAILED: exit_code=$installerExit"
    }
} finally {
    $env:Path = $originalPath
    Pop-Location
}
