# 故障排查

---

## 快速诊断

先运行内置的环境检查，它能发现大多数问题：

```bash
lightspeed --check
```

它会验证：拦截器可用性、包过滤工具、游戏配置解析和代理连通性。

---

## 常见问题

### "Interceptor not available"

**CLI：** 你的操作系统缺少所需的包过滤工具，或者你没有足够权限。

| 操作系统 | 所需条件 | 修复方法 |
|----|----------|------------|
| Linux | nftables 或 iptables + root | `sudo lightspeed ...` |
| macOS | pfctl（内置）+ root | `sudo lightspeed ...` |
| Windows | WinDivert 驱动 + 管理员 | 右键单击 → 以管理员身份运行 |

用以下命令验证：
```bash
lightspeed --check
```

### "No game traffic seen" / Packets Sent 一直是 0

**CLI：** 拦截器在预期的端口范围内找不到游戏数据包。

1. 确认你的游戏**已连接到服务器**（不只是停留在主菜单或大厅）
2. 用 `--scan-processes` 确认游戏：
   ```bash
   lightspeed --scan-processes
   ```
3. 试试别的游戏配置，或者使用手动服务器模式

**图形界面（Windows）：** 等 15 秒。如果出现琥珀色的 "⚠ No game traffic seen" 横幅：
1. 打开提权的 PowerShell：
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. 在**高级 → 手动设置服务器**中使用显示的端口

### "🎯 Finding your game server…" 一直不结束

检测器还没有在 1.5 秒内看到发往同一目的地的 3 个数据包。

1. 确认你已连接到游戏服务器（移动角色以产生流量）
2. 如果 15 秒后仍然检测不到数据包，说明你的服务器用的是非标准端口，请使用手动服务器模式
3. 连接到服务器后，停止并重新启动拦截器

### Packets Sent 在增长，Packets Delivered = 0

数据包到达了代理，但响应没有回到游戏。通常是防火墙问题。

**Linux：**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**macOS：**
```bash
sudo pfctl -d  # Temporarily disable pf to test
```

**Windows：**
```powershell
# Check if the firewall rule exists
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

### 代理健康检查失败

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

如果无法访问：
- 检查代理是否在运行：`systemctl status lightspeed-proxy`
- 检查防火墙是否允许 UDP 4434 和 TCP 8080
- 检查代理日志：`journalctl -u lightspeed-proxy --tail 50`

### 拦截器启动时游戏断线

拦截器在游戏收到响应之前就抢走了数据包，而注入路径失败了。

**Windows：**
1. 确认 `WinDivert64.sys` 和 `WinDivert.dll` 与 `.exe` 在同一目录
2. 断开次要网卡（Docker、VMware、Hamachi 虚拟网卡）
3. 在启动拦截器**之前**先连接到游戏服务器

**Linux：**
1. 检查 nftables 规则：`sudo nft list ruleset | grep lightspeed`
2. 如果规则是过期的：`sudo lightspeed --check` 来诊断

### Windows 上出现 "WinDivert open failed" / `FWP_E_IN_USE` (0x8032000A)

WinDivert 会为每个打开的句柄注册一个 WFP callout/filter。如果某个句柄从未被关闭，那份过滤器状态就会残留，下一次 `WinDivertOpen` 会以 `FWP_E_IN_USE` (0x8032000A) 失败，即使没有任何进程在明面上使用 WinDivert。

较新的 LightSpeed 版本会在每一条正常的关闭路径上同时关闭捕获句柄和注入句柄，包括 `--watch` 和 `--start-interceptor` 下的 Ctrl+C，以及图形界面里的**退出**，并且会先用 `WinDivertShutdown` 解除接收循环的阻塞再关闭，让拆除过程是确定性的。强制结束（`taskkill /f`、崩溃，或者关闭控制台窗口）仍然可能让 WinDivert 2.2.x 驱动留下过期状态；这是上游驱动的局限（basil00/WinDivert#294、#406），进程一旦消失，用户态就无法清除。

如果你还是遇到这个问题：

1. **优雅退出并稍等片刻**：使用 CLI 的 Ctrl+C 或图形界面的**退出**，然后给句柄一两秒时间关闭再重新启动。
2. **停止 WinDivert 服务**（某些情况下可以免去重启；注意该驱动与机器上任何其它基于 WinDivert 的应用共用）：
   ```powershell
   sc stop windivert
   ```
3. **完全关机，而不是重启**：Windows 的"重启"可能复用那个持有过期状态的内核会话；完全**关机 → 开机**可以清除它。

> **提示：** 在 v1.2.2 及更早版本上，还有一个单独的缺陷（数据面认证拒绝所有数据包，issue #59）会卡死连接，迫使用户反复强杀客户端，这正是大多数 `FWP_E_IN_USE` 报告的触发原因。那个认证缺陷已在 v1.2.3 修复。

---

## Windows 图形界面问题

以下内容适用于 Windows 上的 `lightspeed-gui` 应用。

### 单击退出没反应，还留下一个僵尸进程

在 v1.4.2 之前的版本上，从托盘菜单选择**退出**可能会让进程继续在后台运行（一个僵尸进程），所以窗口关了，引擎还在跑，之后再次启动就表现得很奇怪。v1.4.2 修复了这个问题：退出现在会干净地终止进程。

如果你用的是旧版本而且进程卡住了，请手动结束它：

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

然后升级到 v1.4.2 或更高版本。

### 打开了第二个实例，而不是聚焦到第一个

在 v1.4.2 之前的版本上，启动图形界面两次会叠出第二个窗口和第二个引擎。现在图形界面有单实例保护：第二次启动只会短暂提示 "LightSpeed is already running" 然后退出，不会再启动一个引擎。如果出于诊断需要强行开第二个实例，可以传 `--force` 或设置 `LIGHTSPEED_GUI_FORCE=1`。

### 没有发现任何中继

图形界面通过签名注册表发现社区中继。如果中继列表一直是空的：

1. 确认你能上网，并且防火墙或 VPN 没有阻断到注册表的出站 HTTPS。
2. 检查图形界面日志（见下文）中是否有注册表获取或签名验证错误。
3. 在 CLI 版本上，运行 `lightspeed-client --probe-proxies` 直接查看发现和探测报告。如果 CLI 也什么都找不到，那就是网络侧的问题，不是图形界面特有的问题。
4. 修复连通性后重启图形界面；发现过程在启动时运行。

### 如何找到并打开图形界面日志

图形界面会把跟踪日志写入：

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

把这个路径粘贴到文件资源管理器的地址栏即可打开该文件夹，然后用任意文本编辑器打开 `gui-trace.log`。请把它附在缺陷报告里。

### 日志里的 "Heartbeat 0 in"

像 `Heartbeat 0 in` 这样的行表示引擎在当前窗口期内发送了零个心跳保活。实际上，它出现在客户端还没有建立起可用的控制面连接、因而没有发出任何心跳的时候。常见原因：

- 客户端还没有向任何中继注册（检查状态视图中的注册信息那一行）。
- 所选的中继不可达。
- 拦截器尚未启动，所以没有活动会话。

一旦注册成功、心跳开始发送，这个计数就会增长。如果某个中继显示健康但计数一直是 0，请运行 `lightspeed-client --test-control` 来判断控制面是否可达。

---

## 用于缺陷报告的日志

用调试日志运行，以捕获详细的诊断信息：

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

请把日志文件附在你的 [GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues)中。

---

## 还是卡住了？

- [常见问题](faq.md) - 常见疑问
- [GitHub Issues](https://github.com/ShibbityShwab/lightspeed/issues) - 搜索已有的报告
- 提交一个新 issue，附上你的操作系统、游戏和日志输出
