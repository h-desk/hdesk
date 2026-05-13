# HDesk macOS 手动放行安装说明

本文档适用于网站分发的手动放行版 HDesk。该版本没有 Apple Developer ID 公证，因此首次打开时会被 macOS Gatekeeper 拦截，这是预期行为。

## 可下载文件

- Intel Mac: `HDesk-<version>-manual-override-x86_64.dmg`
- Apple 芯片 Mac: `HDesk-<version>-manual-override-arm64.dmg`

如果不确定芯片类型：

1. 点击屏幕左上角苹果菜单。
2. 打开“关于本机”。
3. 如果看到“芯片: Apple M1/M2/M3/M4...”，请选择 `arm64` 包。
4. 如果看到“处理器: Intel ...”，请选择 `x86_64` 包。

## 安装步骤

1. 双击下载好的 `.dmg` 文件。
2. 将 `HDesk.app` 拖到 `Applications`。
3. 不要连续双击应用图标重试多次，第一次被系统拦截后直接去系统设置放行。

## 首次打开时如何手动放行

首次打开时，macOS 通常会提示应用无法打开，或者提示开发者未验证。请按下面步骤处理：

1. 保持 `HDesk.app` 已经在“应用程序”目录中。
2. 打开“系统设置”。
3. 进入“隐私与安全性”。
4. 向下滚动到“安全性”区域。
5. 找到系统刚刚拦截 HDesk 的提示。
6. 点击“仍要打开”或同类按钮。
7. 再次确认打开。

如果系统没有立即显示“仍要打开”：

1. 先在“应用程序”里右键 `HDesk.app`。
2. 选择“打开”。
3. 在弹出的确认框中再次点击“打开”。

通常执行一次手动放行后，后续同一版本不会重复拦截。

## 已知现象

- 首次打开时出现“无法验证开发者”或“Apple 无法检查是否包含恶意软件”：正常。
- DMG 本身也可能被 Gatekeeper 标记：正常。
- 只要来源是你的官网，并按上面的步骤手动放行，应用可以正常安装和运行。

## 给用户的简短说明文案

可以直接把下面这段发给用户：

> 这是 HDesk 的 macOS 手动放行版。首次打开时，macOS 可能会提示开发者未验证，这是系统的正常拦截。请先把 HDesk 拖到“应用程序”，再到“系统设置 > 隐私与安全性”里点击“仍要打开”；或者在“应用程序”里右键 HDesk，选择“打开”，再确认一次即可。

## 内部打包命令

当前仓库可直接用下面命令生成双架构手动放行包：

```bash
python3 build.py --flutter --hwcodec --macos-manual-override
```

只打单个架构时：

```bash
python3 build.py --flutter --hwcodec --macos-manual-override --macos-manual-arch arm64
python3 build.py --flutter --hwcodec --macos-manual-override --macos-manual-arch x86_64
```

默认产物目录：`tmp_test/macos-manual-dist/`