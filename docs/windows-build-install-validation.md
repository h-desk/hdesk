# Windows 构建、安装与运行验收

本文档是 HDesk Windows 桌面端本机构建和安装验收的仓内真源。入口脚本为：

```powershell
scripts\windows\build-install-validate.ps1
```

脚本从自身所在目录推导仓库根目录，不依赖固定的 `D:\ideas\rustdesk` 路径。它用于开发完成后的本机预检，不代替正式签名、上传和发布流程。

## 三种运行形态

排查 Windows 问题时必须先区分以下三种形态，不能只凭窗口外观或任务栏状态判断正在运行哪套代码。

### Runner

默认路径：

```text
flutter\build\windows\x64\runner\Release\hdesk.exe
```

Runner 是 Flutter 和 Rust 的最新本地构建产物，适合快速验证代码，但它不证明安装器包含了同一套文件。

### Installed app

Installed app 是 `hdesk-*-install.exe --silent-install` 实际写入的应用。安装位置取决于安装模式和机器环境，常见位置包括：

```text
%LOCALAPPDATA%\hdesk\hdesk.exe
%LOCALAPPDATA%\rustdesk\hdesk.exe
C:\Program Files\HDesk\hdesk.exe
D:\software\HDesk\hdesk.exe
```

验收脚本不会仅比较 EXE。以下三件套必须同时与 Runner 的 SHA-256 一致：

```text
hdesk.exe
data\app.so
librustdesk.dll
```

### Service

Windows 服务名为 `HDesk`，预期二进制命令行为：

```text
"<installed exe>" --service
```

管理员环境下，脚本会把服务校正到本轮 Installed app，启动服务，并检查状态和命令行路径。非管理员环境不会把无法安装服务判为构建失败，输出必须明确为：

```text
SERVICE_STATE=SKIPPED_NOT_ELEVATED
SERVICE_PATH=SKIPPED_NOT_ELEVATED
```

## 标准用法

在普通或管理员 PowerShell 中运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\windows\build-install-validate.ps1
```

默认使用 Flutter `D:\software\flutter-3.41.4`，依次执行：

1. 停止现有 `hdesk`、`rustdesk` 进程和可停止的 `HDesk` 服务。
2. 执行 `python build.py --flutter`。
3. 选择仓库根目录中最新的 `hdesk-*-install.exe` 并执行 `--silent-install`。
4. 等待 silent installer 子进程退出，并等待安装目录三件套全部写入完成。
5. 比较 Runner 和 Installed app 的三组 SHA-256。
6. 管理员环境下校正并启动 `HDesk` 服务；非管理员环境明确跳过。
7. 启动 Installed app，确认进程没有立即退出。
8. 检查 `%APPDATA%\HDesk\config\HDesk2.toml` 中的 `key` 非空。

Flutter SDK 在其他位置时：

```powershell
.\scripts\windows\build-install-validate.ps1 `
  -FlutterRoot 'E:\sdk\flutter-3.41.4'
```

已有 fresh build，只重新安装和验收时：

```powershell
.\scripts\windows\build-install-validate.ps1 -SkipBuild
```

显式指定安装器或 Installed app 候选路径时：

```powershell
.\scripts\windows\build-install-validate.ps1 `
  -SkipBuild `
  -InstallerPath '.\hdesk-1.0.1-install.exe' `
  -InstalledExePath "$env:LOCALAPPDATA\hdesk\hdesk.exe"
```

`-InstalledExePath` 只用于安装位置不在内置候选列表时。它仍然会执行安装器，并要求文件在本轮安装后重新写入。

## 配置键验收

默认检查 `HDesk2.toml` 中的 `key` 是否存在且非空。脚本不会打印该值。

需要与预期值做精确比对时，通过进程环境变量注入，不要把值写进仓库、脚本或命令行参数：

```powershell
$env:HDESK_EXPECTED_CONFIG_KEY = '<从安全渠道取得的期望值>'
try {
  .\scripts\windows\build-install-validate.ps1
} finally {
  Remove-Item Env:HDESK_EXPECTED_CONFIG_KEY -ErrorAction SilentlyContinue
}
```

输出含义：

- `CONFIG_KEY_STATE=PRESENT`：配置项存在且非空。
- `CONFIG_KEY_MATCH=MATCHED`：已提供期望值，并且精确匹配。
- `CONFIG_KEY_MATCH=SKIPPED_NO_EXPECTED_VALUE`：未提供期望值，只检查了存在性。

如需检查其他文件或配置项，可使用 `-ConfigPath`、`-ConfigOptionName` 和 `-ExpectedConfigKeyEnvironmentVariable`。

## 成功输出

一次完整成功验收至少应包含：

```text
INSTALLER_EXIT=0
HDESK_EXE_RUNNER_HASH=<sha256>
HDESK_EXE_INSTALLED_HASH=<same sha256>
DATA_APP_SO_RUNNER_HASH=<sha256>
DATA_APP_SO_INSTALLED_HASH=<same sha256>
LIBRUSTDESK_DLL_RUNNER_HASH=<sha256>
LIBRUSTDESK_DLL_INSTALLED_HASH=<same sha256>
BUNDLE_HASH_STATUS=MATCHED
SERVICE_STATE=Running
SERVICE_PATH="<installed exe>" --service
START_STATE=RUNNING
CONFIG_KEY_STATE=PRESENT
```

部分 portable installer 会返回 `128`，脚本将 `0` 和 `128` 都视为可接受，但仍要求后续文件、服务和启动验收全部通过。

关键判定：

- 三组 `RUNNER_HASH` 与对应 `INSTALLED_HASH` 必须逐组一致。
- 管理员环境中 `SERVICE_STATE` 必须为 `Running`，`SERVICE_PATH` 必须指向本轮 `INSTALLED_EXE`。
- 普通用户环境必须看到 `SKIPPED_NOT_ELEVATED`，不能误报为服务安装成功。
- `START_STATE` 必须为 `RUNNING`；`STARTED_PID` 是本轮启动的 Installed app，不是 Runner 或 installer runtime。

## 为什么要等待 silent installer

`hdesk-*-install.exe --silent-install` 的最外层进程退出，不等于安装子进程已经完成。曾出现过脚本在子进程仍以 `--silent-install` 运行时，就把该进程当成 Installed app 并启动验证，造成以下假象：

- 以为最新 UI 没有进入安装包。
- 以为任务栏或 service 行为回归。
- 实际观察的是半安装态进程或旧安装目录。

当前脚本只有在以下条件同时满足后才继续：

- 没有 `hdesk` 或 `rustdesk` 的 `--silent-install` 进程。
- Installed app 三件套均存在。
- 三件套均在本轮安装开始后重新写入。

超时会报 `INSTALLED_EXE_NOT_READY`，不要通过直接启动某个已存在的 EXE 绕过该检查。

## 常见误判

### “Runner 正常，所以安装包正常”

不成立。Runner 只证明本地构建可运行。必须安装后比较 `hdesk.exe`、`data\app.so`、`librustdesk.dll` 三件套。

### “看到 hdesk.exe 进程，所以 Installed app 已启动”

不成立。该进程可能是 Runner、silent installer 子进程、Installed app 或 service。使用以下命令核对路径和命令行：

```powershell
Get-CimInstance Win32_Process |
  Where-Object { $_.Name -match '^(hdesk|rustdesk)(\.exe)?$' } |
  Select-Object Name,ProcessId,ExecutablePath,CommandLine
```

### “服务正在运行，所以服务版本正确”

不成立。旧服务可能仍指向另一安装目录。必须同时检查状态和 `PathName`：

```powershell
Get-CimInstance Win32_Service -Filter "Name='HDesk'" |
  Select-Object Name,State,ProcessId,PathName
```

### “安装器退出码为 0，所以安装完成”

不成立。必须继续等待 silent installer 子进程结束和三件套落盘，随后完成 hash 对齐。

### “非管理员环境没有服务，所以安装失败”

不成立。非管理员环境只验证构建、安装目录、配置和 UI 启动，服务阶段明确输出 `SKIPPED_NOT_ELEVATED`。正式发布前仍应至少执行一次管理员验收。

## 只做诊断时

`-SkipLaunch` 可跳过 UI 启动，但配置键仍会被检查；如果本机没有既有有效配置，检查会失败。发布前的完整验收不应使用该参数。

脚本发生错误时会使用稳定的错误前缀，例如：

```text
BUILD_FAILED
INSTALLER_NOT_FOUND
INSTALLED_EXE_NOT_READY
INSTALLED_BUNDLE_HASH_MISMATCH
SERVICE_NOT_RUNNING
CONFIG_KEY_MISSING
CONFIG_KEY_MISMATCH
INSTALLED_APP_EXITED_EARLY
```

这些前缀可用于 CI 或人工日志检索，但本脚本会安装应用、停止进程并可能重建 Windows 服务，不应在共享生产机器上执行。
