# 故障排除

> [!WARNING]
> 机器翻译，未经母语者审核。以[英文版](troubleshooting.md)为准。

---

## 快速诊断

首先运行内置环境检查 - 它可以捕获大多数问题：

```bash
lightspeed --check
```

这将验证：拦截器可用性、数据包过滤工具、游戏配置文件解析和代理连接。

---

## 常见问题

###“拦截器不可用”

**CLI：** 您的操作系统没有所需的数据包过滤工具，或者您缺乏权限。

|操作系统 |必填|如何修复 |
|----|----------|------------|
| Linux | nftables 或 iptables + root | `sudo lightspeed ...` |
| macOS | pfctl（内置）+ root | `sudo lightspeed ...` |
|窗户| WinDivert 驱动程序 + 管理员 |右键单击→以管理员身份运行|

验证：
```bash
lightspeed --check
```

###“未看到游戏流量”/发送的数据包保持为 0

**CLI：** 拦截器无法在预期端口范围内找到游戏数据包。

1. 确保您的游戏**连接到服务器**（不仅仅是主菜单或大厅）
2.用`--scan-processes`验证游戏：
   ```bash
   lightspeed --scan-processes
   ```
3.尝试不同的游戏配置文件或使用手动服务器模式

**GUI (Windows)：** 等待 15 秒。如果出现琥珀色“⚠ 未看到游戏流量”横幅：
1. 打开提升的 PowerShell：
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. 使用**高级→手动设置服务器**中显示的端口

### “🎯 寻找你的游戏服务器......”永远无法解决

检测器在 1.5 秒内未检测到 3 个发送至同一目的地的数据包。

1.确保您已连接到游戏服务器（移动角色以产生流量）
2. 如果 15 秒后仍未检测到数据包，则您的服务器位于非标准端口 - 使用手动服务器模式
3、连接服务器后停止并重新启动拦截器

### 发送的数据包正在爬升，已发送的数据包 = 0

数据包到达代理，但响应未到达您的游戏。通常是防火墙问题。

**Linux：**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**苹果系统：**
```bash
sudo pfctl -d  # Temporarily disable pf to test
```

**Windows：**
```powershell
# Check if the stable rule exists (one rule, not one per boost)
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed WinDivert Tunnel" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

旧版本的规则被命名为 `LightSpeed WinDivert Tunnel <port>`；当前
构建删除了他们使用的端口的规则并保留一个稳定的规则。任意
剩余的每端口规则可以安全删除：
`netsh advfirewall firewall delete rule name="LightSpeed WinDivert Tunnel 4434"`
（与`netsh advfirewall firewall show rule name=all`所示的端口匹配）。

### `sendmsg error: ... 10040` / `WSAEMSGSIZE` (Windows) 或 `EMSGSIZE` (Linux/macOS)

客户端尝试发送的数据报大于**真实**路径 MTU。的
默认隧道钳位为 1492 字节（PPPoE 安全），但 VPN、DPI 垫片如
Zapret 或减少 MTU 的适配器可以更小，因此适合夹具的数据包
仍然被操作系统拒绝（`Windows error 10040`）。

修复 — 降低 `config.toml` 中隧道的路径 MTU 上限：

```toml
[tunnel]
# 1280-1500. Start at 1400, then 1350 if the error persists.
mtu = 1400
```

或在 CLI 上传递 `--mtu 1400`。当前的构建也降低了预算
当发送被拒绝时，自动重试该数据报一次
允许碎片，并在状态中记录操作系统错误而不是
将继电器报告为静音。

在客户端不变的情况下立即解决方法：降低 Windows 适配器的
MTU 使游戏发出较小的数据包，然后重新启动游戏：

```powershell
netsh interface ipv4 show subinterfaces
netsh interface ipv4 set subinterface "Ethernet" mtu=1400 store=persistent
```

### 代理健康检查失败

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

如果无法访问：
- 检查代理是否正在运行：`systemctl status lightspeed-proxy`
- 检查防火墙是否允许 UDP 4434 和 TCP 8080
- 检查代理日志：`journalctl -u lightspeed-proxy --tail 50`

### QUIC 控制平面出现 `error 120` / `no_application_protocol` 故障

客户端无法使用中继完成 QUIC 控制平面握手。当
一次性空报价重试也失败了，日志以此判决结束（引用
客户的逐字记录； `{err}` 是底层 QUIC 错误，`{addr}` 是
回复者地址）：

```text
{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 (no_application_protocol), and the empty-offer retry also failed. The client cannot tell these two causes apart from the handshake alone: (1) a QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN revision -- version skew, where the relay requires an ALPN name this client does not offer.
```

**客户已经做了什么。** 当前客户始终提供版本化的 ALPN 名称
`lightspeed-control/1`。如果中继应答 TLS 警报 120，则客户端重试
**一次**，带有空的 ALPN 报价，然后记住中继的其余答案
的过程，所以在正常的客户端操作中注定的额外握手发生在
每次发射每次中继最多一次。对于以下中继来说，成功的回退是正常的
未升级（继电器默认为`[network] control_alpn = "off"`）：
`QUIC connection established`行显示`alpn=none`，并且注册继续进行。

**最后失败是什么意思。** 空盘重试也失败了，所以有
恰好有两种可能性，仅靠握手无法区分它们：

1. **您路径上的 QUIC 拦截中间件** - VPN、防病毒/防火墙
   HTTPS/QUIC检查，或不携带ALPN扩展的ISP设备
   ([#140](https://github.com/ShibbityShwab/lightspeed/issues/140))。如果每个继电器
   在一个网络上失败，但相同的中继在另一个网络上工作，路径是公共的
   因素。
2. **版本偏差** - 中继需要此客户端不提供的 ALPN 修订版
   （例如，继电器设置为 `control_alpn = "required"` 并运行协议
   版本比该客户端更新）。这在设计上是大声失败的，从来没有作为一个沉默的
   降级：当前客户端针对 `lightspeed-control/1` 上的中继进行协商，
   并且需要该客户端未提供的任何内容的中继会以 120 拒绝它。

**如何缩小范围：**

- 运行 `lightspeed-client --test-control --proxy <relay-ip>` 查看哪个阶段
  继电器位于。连接线上和最终汇总中的 `alpn=` 值是
  协商应用协议：`lightspeed-control/1` 表示中继接受了
  版本化报价，`none` 意味着它在回退后接受了空报价（较旧的报价）
  中继），并且本节判定失败的运行就是上面的情况。
- 如果该网络上的每个中继都发生故障，但相同的中继在其他地方工作，则怀疑是
  middlebox：禁用VPN或安全产品的HTTPS/QUIC检查并重试，
  然后将 UDP 4433 捕获附加到 #140。
- 如果只有一个中继失败，请将该中继的 ALPN 模式与您的客户端版本进行比较：
  继电器的启动线及其 `GET /health` 上的 `control_alpn` 字段报告
  配置模式（请参阅 [CLI 参考](CLI-REFERENCE.md#proxy-configuration)）。如果你
  操作该继电器，`control_alpn = "off"` 恢复旧版本的先前行为
  客户；操作员程序和客户优先排序规则位于
  [部署代理](deploy-proxy.md) 和 [ALPN 部署计划](quic-alpn-rollout.md)。

### 拦截器启动时游戏断开连接

拦截器在游戏接收响应之前捕获数据包，并且注入路径失败。

**Windows：**
1.验证`WinDivert64.sys`和`WinDivert.dll`是否在`.exe`旁边
2. 断开辅助网络适配器（Docker、VMware、Hamachi 虚拟适配器）
3. **在**启动拦截器之前连接到游戏服务器

**Linux：**
1.查看nftables规则：`sudo nft list ruleset | grep lightspeed`
2. 如果规则已过时：`sudo lightspeed --check` 进行诊断

### Windows 上的“WinDivert 打开失败”/`FWP_E_IN_USE` (0x8032000A)

只有带有真实捕获过滤器的句柄——intercept（`udp and outbound …`）和 shadow（`udp and inbound …`）——才拥有 WFP callout/filter。整个进程生命周期持有的 pin 句柄和 inject 句柄使用 `"false"` 过滤器，不转发任何流量，也不拥有 WFP 对象。因此 WFP callout/filter 并不是“每个打开的句柄一个”，持有 pin 也不是 WFP 状态锁：在 2026-11-23 的现场记录中，pin 保持打开，而下一次 intercept 打开仍然以 `FWP_E_IN_USE` 失败。

`FWP_E_IN_USE` 表示 WFP 对象仍被引用（例如强制结束后内核尚未完成删除的上一个进程的过滤器），或者某个 callout 正在被并发注册/注销。“没有进程明显在使用 WinDivert”并不能证明没有引用，而且目前没有采集 WFP 状态，所以不要把错误文本当作根本原因。

LightSpeed 在每条正常关闭路径上（`--watch`/`--start-interceptor` 中的 Ctrl+C、GUI 中的**退出**）关闭 intercept、inject 和 shadow 句柄，在关闭前用 `WinDivertShutdown` 解除接收循环的阻塞，并等待所有者线程确认。这种确定性针对的是应用的**句柄所有权**；内核侧的 WFP teardown 是另一条路径，其失败对用户态不可见。

如果仍然出现：

1. **重试一次 Start。** 下一个补丁会在 pin 保持期间自动重试 `FWP_E_IN_USE`（有界，约 5 秒），并可能重新创建 pin 一次；1.12.5 仅在**没有**持有 pin 时重试，因此那里重试意味着先 Stop 再重新 Optimize。
2. **正常退出并稍等片刻**：使用 CLI 的 Ctrl+C 或 GUI 的**退出**，然后给句柄一两秒时间关闭再重新启动。
3. **停止 WinDivert 服务**（管理员权限；这是显式重置，LightSpeed 不会自动执行，并且会影响机器上任何其他基于 WinDivert 的应用——请先确认没有其他 WinDivert 客户端在运行）：
   ```powershell
   sc stop windivert
   ```
4. **完全关机**是最后手段，且仅当无法排除外部 WinDivert 使用者时使用：Windows **重启**可能复用持有旧状态的内核会话，因此完整的**关机 → 开机**才是唯一可靠的整机清理方式。它不是首选方案，LightSpeed 不会在没有该理由时建议重启。

WinDivert 2.2 在最后一个句柄关闭后仍保持驱动程序加载：WFP 提供程序及其 20 个子层仍然存在（1 / 20），`WinDivert64.sys` 会一直锁定，直到停止服务或重启机器。pin 不会改变这一点，也不能保证 Stop 时驱动程序会被卸载。

> **提示：** 在 v1.2.2 及更早版本中，另一个缺陷（数据平面认证拒绝所有数据包，issue #59）会冻结连接，迫使用户反复强制结束客户端，从而引发了大多数 `FWP_E_IN_USE` 报告。该认证缺陷已在 v1.2.3 中修复。

---

## Windows GUI 问题

这些适用于 Windows 上的 `lightspeed-gui` 应用程序。

### Quit 没有执行任何操作并留下了僵尸进程

在 v1.4.2 之前的版本中，从托盘菜单中选择 **退出** 可能会使进程在后台运行（僵尸），因此窗口关闭，但引擎继续运行，并且稍后的启动行为异常。 v1.4.2 修复了这个问题：现在退出可以干净地终止进程。

如果您使用的是较旧的版本并且该过程被卡住，请手动结束它：

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

然后升级到v1.4.2或更高版本。

### 打开第二个实例而不是聚焦第一个实例

在 v1.4.2 之前的版本中，启动 GUI 两次会堆叠第二个窗口和第二个引擎。 GUI 现在拥有单实例防护：第二次启动会显示简短的“LightSpeed 已在运行”通知并退出，而不是启动另一个引擎。要强制使用第二个实例（用于诊断），请传递 `--force` 或设置 `LIGHTSPEED_GUI_FORCE=1`。

### 未发现中继

GUI 通过签名注册表发现社区中继。如果中继列表保持为空：

1. 确认您可以访问 Internet，并且防火墙或 VPN 不会阻止到注册表的出站 HTTPS。
2. 检查 GUI 日志（见下文）是否有注册表获取或签名验证错误。
3. 在 CLI 构建上，运行 `lightspeed-client --probe-proxies` 直接查看发现和探测报告。如果 CLI 也没有发现任何问题，则问题出在网络方面，而不是 GUI 特定的问题。
4. 修复连接后重新启动 GUI；发现在启动时运行。

### 如何查找并打开GUI日志

GUI 将其跟踪日志写入：

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

将该路径粘贴到文件资源管理器地址栏中以打开该文件夹，然后在任何文本编辑器中打开 `gui-trace.log`。将其附加到错误报告中。

### 日志中的“心跳 0”

像 `Heartbeat 0 in` 这样的行意味着引擎在当前窗口中发送了零个保活心跳。实际上，当客户端尚未建立工作控制平面连接时，它就会出现，因此还没有心跳发出。常见原因：

- 客户端尚未向中继注册（检查状态视图中的注册行）。
- 所选继电器无法到达。
- 拦截器尚未启动，因此没有活动会话。

一旦注册成功并且心跳开始流动，计数器就会上升。如果在继电器显示正常时它保持为 0，请运行 `lightspeed-client --test-control` 来隔离控制平面是否可达。

---

## 错误报告日志

使用调试日志记录运行以捕获详细的诊断信息：

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

将日志文件附加到您的 [GitHub 问题](https://github.com/ShibbityShwab/lightspeed/issues)。

---

## 仍然卡住吗？

- [FAQ](faq.md) - 常见问题
- [GitHub 问题](https://github.com/ShibbityShwab/lightspeed/issues) - 搜索现有报告
- 使用您的操作系统、游戏和日志输出打开一个新问题