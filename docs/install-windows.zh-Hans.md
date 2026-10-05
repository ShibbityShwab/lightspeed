# 在 Windows 上安装 LightSpeed

> [!WARNING]
> 机器翻译，未经母语者审核。以[英文版](install-windows.md)为准。

Windows 是图形界面优先的平台。`lightspeed-gui` 包是一个独立应用，已经包含客户端引擎和 WinDivert 驱动，所以你永远不需要另外下载客户端。

---

## 下载什么

从 [Releases 页面](https://github.com/ShibbityShwab/lightspeed/releases/latest)获取最新版本。对于 Windows x86_64（所有当前的 Intel 和 AMD 电脑），选择以下之一：

| 文件 | 适用场景 |
|------|-------------|
| `lightspeed-gui-...-windows-msvc.msi` | 推荐。安装到 Program Files，添加开始菜单快捷方式，并注册卸载程序。 |
| `lightspeed-gui-...-windows-msvc.zip` | 便携版。解压到任意位置，运行 `lightspeed-gui.exe`。 |

两者都包含相同的图形界面以及 `WinDivert.dll` 和 `WinDivert64.sys`。

> **Windows on ARM64 目前还不是发布目标。** 发布版本面向 `x86_64-pc-windows-msvc` 构建。在 ARM64 设备上，x86_64 版本会通过 Windows 的模拟层运行，但没有在真实的 ARM64 硬件上测试过。

### Windows CLI（不受支持）

还有一个 Windows 命令行版本以 zip 形式发布（`lightspeed-client-...-windows-msvc.zip`）。它是为了脚本和无人值守场景提供的，但**不受支持**：Windows 上推荐走图形界面。如果你要用它，请从提权的终端启动（见下文）。

---

## 安装（MSI）

1. 下载 `.msi`。
2. 双击它，按向导操作。Windows SmartScreen 可能提示发布者未知；发布版本有 Sigstore 背书，如果你想可以自行验证该背书。
3. 从开始菜单启动 **LightSpeed**。

## 安装（便携 zip）

1. 下载 `.zip`。
2. 右键单击它，选择**全部解压缩**，解压到一个你有写权限的文件夹（例如 `C:\LightSpeed`）。不要在压缩包内直接运行。
3. 运行 `lightspeed-gui.exe`。

---

## 管理员权限

LightSpeed 使用 WinDivert 驱动截获游戏的 UDP 流量。WinDivert 需要**管理员**权限。

- 图形界面在需要启动拦截器时会请求提权。接受 UAC 提示即可。
- 如果你运行 CLI 版本，请从**管理员**终端启动（右键单击 Windows Terminal 或 PowerShell，然后选择**以管理员身份运行**）。

没有提权，拦截器无法挂载，你会在状态视图中看到拦截器错误。

---

## 首次运行

1. 启动图形界面并接受 UAC 提示。
2. 图形界面会通过签名注册表自动发现社区中继。不需要填写代理地址。
3. 从游戏列表中选择你的游戏。
4. 启动拦截器，然后启动游戏并连接服务器。

图形界面会显示中继状态、注册状态和数据包计数，让你看到流量是否在走。

---

## 验证是否正常

**检查中继发现和注册。** 打开图形界面的状态视图。你应该看到社区中继已列出（洛杉矶、新泽西、新加坡、法兰克福、东京、孟买、马德里、悉尼），状态正常，还有一行注册信息显示 QUIC/auth 握手成功。如果你用的是 CLI 版本，运行：

```powershell
lightspeed-client.exe --probe-proxies
```

这会执行一次发现/探测，并打印一份可见的报告，列出每个已发现的中继及其延迟。你应该看到全部八台社区中继。

**检查控制面注册。** 使用 CLI 版本时：

```powershell
lightspeed-client.exe --test-control
```

这会连接 QUIC 控制面，注册一个会话，发送 ping，然后断开，并打印每一步的结果。注册成功就说明控制面可达、认证正常。

**检查数据包流转。** 在图形界面中，游戏连接服务器后，数据包计数应该开始增长。如果 "Packets Sent" 一直是 0，说明拦截器还没看到游戏流量；参见[故障排查](troubleshooting.md)。

---

## 日志

图形界面会把跟踪日志写入：

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

把这个路径粘贴到文件资源管理器的地址栏即可打开。如果出了问题，请把它附在缺陷报告里。日志各行的含义参见[故障排查](troubleshooting.md)。

---

## 卸载

- **MSI：** 设置 → 应用 → 已安装的应用 → LightSpeed → 卸载。
- **Zip：** 删除解压出来的文件夹。`%LOCALAPPDATA%\Lightspeed\` 下的日志文件会保留；如果你想要彻底清理，请手动删除。

---

## 下一步

- [支持的游戏](supported-games.md)
- [故障排查](troubleshooting.md)
- [CLI 参考](CLI-REFERENCE.md)
