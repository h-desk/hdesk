# Windows Release Standard（2026-05-02）

这份文档定义 HDesk Windows 桌面端的标准 release 流程，目标是稳定产出正式签名的 Windows 资产，并把更新服务器所需的文件和元数据一次准备完整。

## 当前渠道决策

- 2026-05-02 起，Windows 桌面端主分发渠道改为 Microsoft Store。
- 官网只保留商店跳转入口；Store 负责下载安装与更新。
- 本文剩余内容保留为备用自建安装包/旧用户兼容链路说明，不再作为默认主发布路径。

## 适用范围

- 仓库：`D:\ideas\rustdesk`
- 平台：Windows Flutter 桌面端
- 发布目标：
  - 官网跳转 Microsoft Store
  - Microsoft Store 主分发与主更新
  - 自建 releases 兼容旧安装包或非 Store 场景

## 先说结论

默认用户路径现在应当是：官网打开产品页，点击 Windows 桌面端按钮后跳转 Microsoft Store，由 Store 负责安装和后续更新。

下面的 MSI、EXE、`latest.json`、SignPath、自建 releases 流程，继续保留给两类场景：

1. Store 页面尚未就绪前的临时兼容分发
2. 明确不走 Store 的测试、灰度或旧安装包用户

每次 Windows release，至少要产出并上传这 3 个文件：

1. `hdesk-{version}-x86_64.msi`
2. `hdesk-{version}-x86_64.exe`
3. `hdesk-{version}-install.exe`

原因：

- `hdesk-{version}-x86_64.msi` 作为官网主推荐安装包，最接近官方 Windows 发布形态。
- `hdesk-{version}-x86_64.exe` 是非 MSI 安装态客户端当前默认查找的自动更新资产。
- `hdesk-{version}-install.exe` 作为 legacy 手动安装别名保留，方便兼容旧链接或旧入口。
- 如果缺少 `.msi` 或 `.exe` 任意一个，Windows 客户端将无法按安装形态稳定更新。

当前 `build.py` 的 Windows 流程已经能生成 `hdesk-{version}-install.exe` 和 `hdesk-{version}-x86_64.exe`。正式 release workflow 还会在此基础上额外构建 `hdesk-{version}-x86_64.msi`，并对“unsigned 目录”和最终 `SignOutput` 资产分别走 SignPath 送签。

## 当前源码约束

### 1. 资产名前缀固定为 `hdesk`

`src/common.rs` 当前定义：

- `OFFICIAL_RELEASE_ASSET_PREFIX = "hdesk"`

因此 Windows 自动更新资产名固定为：

- `hdesk-{version}-x86_64.exe`

### 2. Windows release 文件名版本号来自 `Cargo.toml`

`build.py` 通过 `get_version()` 读取根目录 `Cargo.toml` 的 `version = "..."` 来生成：

- `hdesk-{version}-install.exe`
- `hdesk-{version}-x86_64.exe`

### 3. Flutter UI 显示版本来自 `flutter/pubspec.yaml`

Flutter 侧 `package_info_plus` 会读取平台包版本，因此桌面 UI 看到的版本号应与 `flutter/pubspec.yaml` 对齐。

当前版本来源：

- `Cargo.toml`: `version = "1.0.0"`
- `flutter/pubspec.yaml`: `version: 1.0.0+64`

标准要求：

- 发版前同步更新这两个版本字段。
- `Cargo.toml` 决定 release 文件名。
- `flutter/pubspec.yaml` 决定 Flutter 侧显示版本和构建号。

## 更新服务器当前真实入口

当前 Windows Rust 更新逻辑优先读取：

- `https://releases.hdesk.yunjichuangzhi.cn/latest.json`

这是 `src/common.rs` 里当前生效的机器可读更新元数据地址。

注意：

- `flutter/lib/consts.dart` 里还有 `https://apps.yunjichuangzhi.cn/hdesk/latest.json`
- 但当前桌面 Rust 更新主链路读取的是 `releases.hdesk.yunjichuangzhi.cn/latest.json`
- 当前 `D:\ideas\apps\public\hdesk\` 下也没有现成的 `latest.json`

所以这轮标准流程里，`releases.hdesk.yunjichuangzhi.cn/latest.json` 是权威更新元数据。

## 标准 release 流程

### Step 1. 更新版本号

修改以下两个文件：

1. `Cargo.toml`
2. `flutter/pubspec.yaml`

建议规则：

- 主版本号保持一致，例如都更新到 `1.0.1`
- `pubspec.yaml` 的 `+build` 可单独递增，例如 `1.0.1+65`

### Step 2. 预检环境

在 `D:\ideas\rustdesk` 执行：

```powershell
python --version
cargo --version
& "D:\software\flutter\bin\flutter.bat" --version
git status --short
cargo check --lib
```

预检要求：

- `cargo check --lib` 必须通过
- 当前有未提交改动也可以构建，但要明确知道哪些是源码改动、哪些是打包噪声

### Step 3. 执行本地 Windows 打包预检

在 `D:\ideas\rustdesk` 执行：

```powershell
$env:Path = "D:\software\flutter\bin;$env:Path"
python build.py --flutter
```

说明：

- 这会构建 Flutter runner、Rust DLL、portable packer，并最终产出两个版本化 Windows 包。
- 当前仓内已经修正 Windows 打包阶段对 `python3` / `pip3` 的硬编码依赖，改为复用当前 `python` 解释器，因此此命令应直接可用。
- 这一步仍然是本地/CI 打包基础，不等于正式 release。正式 release 还会经过 SignPath 签名和自建 releases 上传。

### Step 4. 检查 release 产物

构建完成后，至少要核对以下文件：

1. `flutter/build/windows/x64/runner/Release/hdesk.exe`
2. `target/release/librustdesk.dll`
3. `hdesk-{version}-install.exe`
4. `hdesk-{version}-x86_64.exe`

推荐检查命令：

```powershell
Get-ChildItem . -Filter 'hdesk-*.exe' |
  Sort-Object LastWriteTime -Descending |
  Select-Object Name,LastWriteTime,Length

Get-ChildItem flutter\build\windows\x64\runner\Release\hdesk.exe,
  target\release\librustdesk.dll |
  Select-Object FullName,LastWriteTime,Length
```

验收点：

- 两个版本化 EXE 时间戳必须是本次构建时间
- `librustdesk.dll` 应是当前有效的 hwcodec 版
- 不要误把旧的根目录版本化包当成新产物

## 三个 Windows 资产的语义

### `hdesk-{version}-x86_64.msi`

用途：

- 官网主推荐下载入口
- 新用户标准安装路径
- 已以 MSI 安装的桌面端自动更新目标

### `hdesk-{version}-x86_64.exe`

用途：

- 非 MSI 安装态桌面端 updater 下载目标
- 兼容 EXE 安装或便携更新链

如果缺少这个文件，非 MSI 安装的 Windows 自动更新将无法按当前约定完成下载。

### `hdesk-{version}-install.exe`

用途：

- 兼容旧的人工下载链接或旧脚本
- 当前 PC 本地静默安装
- 可以作为备用 EXE 安装入口

本地静默安装命令：

```powershell
Stop-Process -Name hdesk,rustdesk -Force -ErrorAction SilentlyContinue
& ".\hdesk-{version}-install.exe" --silent-install
```

安装完成后，实际安装目录通常在：

- `C:\Users\<user>\AppData\Local\rustdesk\`

## 更新服务器上传标准

### 必传文件

对版本 `{version}`，至少上传这 4 项：

1. `hdesk-{version}-x86_64.msi`
2. `hdesk-{version}-x86_64.exe`
3. `hdesk-{version}-install.exe`
4. `latest.json`

### 推荐目录结构

推荐在更新服务器上按以下静态路径组织：

```text
/hdesk/releases/download/{version}/hdesk-{version}-x86_64.msi
/hdesk/releases/download/{version}/hdesk-{version}-x86_64.exe
/hdesk/releases/download/{version}/hdesk-{version}-install.exe
/latest.json
```

例如版本 `1.0.0`：

```text
https://releases.hdesk.yunjichuangzhi.cn/hdesk/releases/download/1.0.0/hdesk-1.0.0-x86_64.msi
https://releases.hdesk.yunjichuangzhi.cn/hdesk/releases/download/1.0.0/hdesk-1.0.0-x86_64.exe
https://releases.hdesk.yunjichuangzhi.cn/hdesk/releases/download/1.0.0/hdesk-1.0.0-install.exe
https://releases.hdesk.yunjichuangzhi.cn/latest.json
```

## `latest.json` 标准写法

当前桌面端更新代码会优先读取：

- `version`
- `downloads.windows.x86_64.exe`

其中 `downloads.windows.x86_64.exe` 可以是：

1. 直接下载 URL
2. release page URL

当前标准建议：默认写 release page URL，而不是直接写死 EXE 直链。

原因：

- updater 看到 `/releases/tag/{version}` 时，会在客户端侧按安装形态自动解析成 `.msi` 或 `.exe`。
- 如果这里直接写死 `hdesk-{version}-x86_64.exe`，则 MSI 安装态客户端也会被迫下载 EXE。

### 推荐模板

```json
{
  "version": "1.0.0",
  "downloads": {
    "windows": {
      "x86_64": {
        "exe": "https://releases.hdesk.yunjichuangzhi.cn/hdesk/releases/tag/1.0.0",
        "directExe": "https://releases.hdesk.yunjichuangzhi.cn/hdesk/releases/download/1.0.0/hdesk-1.0.0-x86_64.exe",
        "msi": "https://releases.hdesk.yunjichuangzhi.cn/hdesk/releases/download/1.0.0/hdesk-1.0.0-x86_64.msi"
      },
      "install": {
        "exe": "https://releases.hdesk.yunjichuangzhi.cn/hdesk/releases/download/1.0.0/hdesk-1.0.0-install.exe",
        "msi": "https://releases.hdesk.yunjichuangzhi.cn/hdesk/releases/download/1.0.0/hdesk-1.0.0-x86_64.msi"
      }
    }
  }
}
```

说明：

- 当前 updater 真正消费的是 `downloads.windows.x86_64.exe`
- 推荐把它写成 `/releases/tag/{version}`，这样 MSI/EXE 选择在客户端侧完成
- `install` 节点不是当前 Rust 更新逻辑强依赖，但建议保留，方便官网和后续扩展

## GitHub Actions / SignPath / 自建 releases 配置

当前仓里已新增 HDesk 专用 workflow：

- `.github/workflows/hdesk-windows-release.yml`

这条 workflow 的职责是：

1. 构建 unsigned Windows 目录
2. 用 SignPath 对 unsigned 目录签名
3. 基于 signed 目录构建 `x86_64.exe`、`install.exe`、`x86_64.msi`
4. 再对最终 release 资产做一次 SignPath 签名
5. 生成 `latest.json`
6. 通过 SSH 上传到自建 releases 域名

### 需要配置的 GitHub Secrets

1. `SIGNPATH_API_TOKEN`
2. `HDESK_RELEASES_SSH_KEY`

### 需要配置的 GitHub Variables

1. `SIGNPATH_ORGANIZATION_ID`
2. `SIGNPATH_PROJECT_SLUG`
3. `SIGNPATH_UNSIGNED_SIGNING_POLICY_SLUG`
4. `SIGNPATH_RELEASE_SIGNING_POLICY_SLUG`
5. `HDESK_RELEASES_HOST`
6. `HDESK_RELEASES_PORT`
7. `HDESK_RELEASES_USER`
8. `HDESK_RELEASES_REMOTE_ROOT`
9. `HDESK_RELEASES_PUBLIC_BASE_URL`

### 手动执行方式

在 GitHub Actions 中手动运行：

- `HDesk Windows Release`

可选输入：

- `release_tag`: 允许输入 `1.0.0` 或 `v1.0.0`，workflow 会自动归一化到 `1.0.0`
- `publish_latest`: 是否同步更新 `latest.json`

注意：

- 当前 workflow 会强校验 `Cargo.toml` 的版本必须与 release version 一致。
- 如果将来要接自动 tag 触发，建议统一使用无 `v` 前缀的版本目录语义，避免自建 `/releases/download/{version}/` 路径和资产名错位。

## 发布后本机验收

完成打包和上传前，建议至少在当前 Windows PC 做一次本机安装验收：

### 1. 静默安装

```powershell
Stop-Process -Name hdesk,rustdesk -Force -ErrorAction SilentlyContinue
& ".\hdesk-{version}-install.exe" --silent-install
```

### 2. 确认安装目录已刷新

```powershell
Get-ChildItem "$env:LOCALAPPDATA\rustdesk" |
  Select-Object Name,LastWriteTime,Length
```

### 3. 启动安装后的程序

```powershell
Start-Process "$env:LOCALAPPDATA\rustdesk\hdesk.exe"
Get-Process hdesk | Select-Object Name,Id,Path,StartTime
```

验收标准：

- 进程路径应指向 `%LOCALAPPDATA%\rustdesk\hdesk.exe`
- 时间戳应是本次安装后的新时间
- UI 版本号应与本次 release 对应

## 当前已验证可用的命令组合

这台机器上已经验证通过的 Windows release 组合是：

```powershell
$env:Path = "D:\software\flutter\bin;$env:Path"
Set-Location 'D:\ideas\rustdesk'
cargo check --lib
python build.py --flutter
& '.\hdesk-1.0.0-install.exe' --silent-install
Start-Process "$env:LOCALAPPDATA\rustdesk\hdesk.exe"
```

## 常见坑

### 1. 只上传 `install.exe`

结果：

- 用户能手动下载安装
- 自动更新会失败，因为 updater 仍按 `hdesk-{version}-x86_64.exe` 找文件

### 2. 只改 `Cargo.toml`，没改 `flutter/pubspec.yaml`

结果：

- 打包文件名版本正确
- Flutter UI 显示的版本可能还是旧的

### 3. 只看 runner，不看版本化包

结果：

- 可能误以为 release 包已更新，实际上根目录 `hdesk-*.exe` 仍是旧时间戳

### 4. 把 `libs/portable/app_metadata.toml` 当成源码改动

结果：

- 打包后这个文件会被刷新时间戳
- 它是发布构建副产物，不应被误判为功能修改

## 最终标准

每次 Windows 正式 release，按下面这 6 条执行：

1. 同步更新 `Cargo.toml` 和 `flutter/pubspec.yaml` 版本。
2. 运行 `cargo check --lib`。
3. 运行 `python build.py --flutter`。
4. 确认生成 `hdesk-{version}-install.exe` 和 `hdesk-{version}-x86_64.exe`。
5. 上传两个 EXE 到更新服务器，并更新 `latest.json`。
6. 在当前 PC 至少做一次 `--silent-install` 本机验收。

只要这 6 步都完成，Windows release 就视为达标。