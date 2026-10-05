# LightSpeed 用户指南

> 一步一步教你用 LightSpeed 降低延迟。

---

## LightSpeed 的工作原理

你的 ISP 为游戏流量选择的路由是按成本优化的，不是按速度优化的。LightSpeed 会截获你游戏的 UDP 数据包，并通过**中继节点**进行隧道转发。中继节点是数据中心里的一台轻量服务器，通过高速骨干网连接到各游戏服务器区域。默认情况下你使用的是**社区中继网络**（八台中继由赞助商出资，通过签名注册表自动发现，无需任何配置）。你也可以自己搭建代理。如果这条路径比 ISP 的默认路由更快，你的延迟就会下降。

```
Your PC ──→ ISP (slow path) ──→ Game Server        ❌ High ping
Your PC ──→ LightSpeed Proxy (fast backbone) ──→ Game Server   ✅ Low ping
```

---

## 前置条件

- `lightspeed` CLI 工具或 `lightspeed-gui`（Windows）。不需要配置代理：客户端会自动发现社区中继。
- 使用拦截器模式需要：root/管理员权限
- 可选：如果你偏好自建，可以准备自己的代理节点（参见[部署代理](deploy-proxy.md)）

---

## 我需要哪个应用？

| 你在什么平台 | 下载 | 原因 |
|-----------|----------|-----|
| **Windows** | `lightspeed-gui`（MSI 或 ZIP） | 图形界面是独立应用，已经包含客户端引擎和 WinDivert 驱动。你**不需要** CLI。 |
| **Linux** | `lightspeed-gui`（或 `lightspeed-client`） | 图形界面可以在 Linux 上运行（系统托盘是占位实现）；CLI 面向进阶用户。 |
| **macOS** | `lightspeed-client` | 尚无经过测试的图形界面。图形界面能为 macOS 编译，但**未**在真实硬件上测试过。 |
| **自己托管代理** | `lightspeed-proxy` | 只有在 VPS 上运行中继节点时才需要。 |

> **你始终只需要一个包。** 如果你是 Windows 玩家，下载 `lightspeed-gui` 就够了，忽略其他的。`lightspeed-client` 面向 Linux 进阶用户和 macOS 玩家；`lightspeed-proxy` 面向自建服务的用户。

---

## 快速开始（CLI，全平台适用）

### 1. 检查你的环境

```bash
lightspeed --check
```

这会验证你的操作系统是否具备所需的包过滤工具（Linux 上是 nftables/iptables，macOS 上是 pfctl，Windows 上是 WinDivert）。

### 2. 探测你的中继

```bash
lightspeed --probe-proxies
```

显示到每个已发现中继的延迟。客户端首次运行时会自动选择最快的一个；你也可以手动指定，选择离**游戏服务器**最近的那个，而不是离你最近的。

### 3. 启动拦截器

```bash
# Linux/macOS (requires root)
sudo lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Windows (requires Administrator)
lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

### 4. 启动游戏

像平常一样连接任意服务器。LightSpeed 会从出站数据包自动识别游戏服务器，几秒内开始隧道转发。

### 5. 监控

CLI 会显示实时统计：
```
⚡ OPTIMIZING - 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

---

## 快速开始（图形界面，Windows）

### 1. 下载

从 [Releases](https://github.com/ShibbityShwab/lightspeed/releases) 获取最新版本。解压所有文件，让 `WinDivert64.sys` 和 `WinDivert.dll` 与 `lightspeed-gui.exe` 放在同一目录。

### 2. 以管理员身份运行

右键单击 `lightspeed-gui.exe` → **以管理员身份运行**。拦截器需要内核级访问权限（和 VPN 软件一样）。

### 3. 选择中继和游戏

图形界面会自动发现社区中继，并在首次运行时选择最快的一个。你可以从下拉菜单中指定其它中继，然后选择你的游戏。

### 4. 点击 **⚡ OPTIMIZE MY ROUTE**

状态会变成 "🎯 Finding your game server…"

### 5. 启动游戏

连接任意服务器。LightSpeed 会在几秒内自动识别。

---

### macOS 图形界面（未经测试）

图形界面能为 macOS 编译，但**未**在真实硬件上测试过。发布包只提供裸 `tar.xz`（cargo-dist 0.32 不支持 `.app`/`.dmg`），所以要生成正式的应用程序包，需要在 Mac 上运行：

```bash
cargo build --release -p lightspeed-gui
./tools/package-macos.sh 1.6.5
```

这会生成 `LightSpeed.app` 和 `LightSpeed-1.6.5.dmg`。应用是临时签名的，所以第一次启动需要右键单击 → 打开（或者执行
`xattr -dr com.apple.quarantine LightSpeed.app`）。

---

## 选择合适的中继

| 你所在的地区 | 游戏服务器位置 | 最佳中继区域 |
|-----------|---------------|-------------------|
| 澳大利亚 | 美西 | 美西（洛杉矶） |
| 欧洲 | 美东 | 美东（新泽西） |
| 东南亚 | 新加坡 | 新加坡 |
| 南亚 | 印度 | 孟买 |
| 东亚 | 日本 | 东京 |
| 南美 | 美东 | 美东（新泽西） |
| 任意地区 | 同一区域 | 离游戏服务器最近的那个 |

> **经验法则：** 选择离**游戏服务器**最近的中继，不是离你最近的。你的流量路径是 PC → 中继 → 游戏服务器，所以中继到游戏服务器这一段最关键。

---

## 前向纠错（FEC）

FEC 会增加约 25% 的带宽开销，用来在不重传的情况下恢复丢失的数据包。

**适合启用的场景：**
- 你有丢包（画面微卡顿、角色回拉）
- 你在 Wi-Fi 上，干扰时有时无

**适合关闭的场景：**
- 你的连接已经饱和
- 你在按流量计费或有流量上限的连接上
- 你的丢包率低于 0.1%（没有收益）

```bash
# CLI: enable FEC with default block size (K=4)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec

# Custom block size (K=8 → 12.5% overhead)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec --fec-k 8
```

---

## 进阶：手动服务器模式

如果自动识别不生效（端口非标准、游戏不常见）：

```bash
# Redirect mode: game connects to localhost, LightSpeed forwards to real server
lightspeed --game rust --game-server 123.45.67.89:28015 --proxy YOUR_PROXY:4434
```

然后把游戏配置为连接 `127.0.0.1:<port>`（LightSpeed 会打印本地端口）。

---

## 会话中途切换服务器

当你断开一个服务器并连接另一个时，LightSpeed 会自动识别。状态会短暂显示 "🎯 Finding your game server…"，然后锁定新的目标。无需手动操作。

---

## 系统托盘（Windows 图形界面）

- 点击 **×** 最小化到托盘（不会退出）
- 双击闪电图标恢复窗口
- 右键单击可以快速连接/断开/退出

---

## 另请参阅

- [CLI 参考](CLI-REFERENCE.md) - 每个参数都有说明
- [常见问题](faq.md) - 常见疑问
- [故障排查](troubleshooting.md) - 解决各种问题
- [部署代理](deploy-proxy.md) - 运行你自己的代理
- [支持的游戏](supported-games.md) - 游戏兼容性
