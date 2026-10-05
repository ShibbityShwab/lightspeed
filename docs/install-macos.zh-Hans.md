# 在 macOS 上安装 LightSpeed

> [!WARNING]
> 机器翻译，未经母语者审核。以[英文版](install-macos.md)为准。

macOS 是命令行优先的平台。图形界面能为 macOS 编译，但未在真实硬件上测试过，所以受支持的路径是命令行客户端（`lightspeed-client`）。

---

## 下载什么

从 [Releases 页面](https://github.com/ShibbityShwab/lightspeed/releases/latest)获取最新版本。选择与你的 Mac CPU 匹配的压缩包：

| 你的 Mac | 目标平台 | 文件 |
|----------|--------|------|
| Apple Silicon（M1 及更新） | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

不确定自己用的是哪种？运行：

```bash
uname -m
```

`arm64` 表示 Apple Silicon；`x86_64` 表示 Intel。

---

## 安装

Shell 安装脚本是最省事的方式。它会识别你的架构并安装客户端：

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

或者从压缩包手动安装：

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

验证二进制文件可以运行：

```bash
lightspeed-client --version
```

---

## root 权限

macOS 的拦截器使用 `pfctl`（内置的包过滤器）来重定向游戏的 UDP 流量。`pfctl` 需要 root，所以启动拦截器时要用 `sudo` 运行客户端：

```bash
sudo lightspeed-client --start-interceptor --game rust
```

发现、探测和诊断（`--probe-proxies`、`--test-control`、`--check`）不需要 root。

---

## 首次运行

客户端会通过签名注册表自动发现社区中继。不需要填写代理地址。

```bash
# Probe the community relays and print a report
lightspeed-client --probe-proxies

# Start the interceptor for your game
sudo lightspeed-client --start-interceptor --game rust
```

---

## 验证是否正常

**检查中继发现。** 运行：

```bash
lightspeed-client --probe-proxies
```

这会执行一次发现/探测，并打印一份可见的报告，列出每个已发现的中继及其延迟。你应该看到全部八台社区中继（洛杉矶、新泽西、新加坡、法兰克福、东京、孟买、马德里、悉尼）。

**检查控制面注册。** 运行：

```bash
lightspeed-client --test-control
```

这会连接 QUIC 控制面，注册一个会话，发送 ping，然后断开，并打印每一步的结果。注册成功就说明控制面可达、认证正常。

**检查环境。** 运行：

```bash
lightspeed-client --check
```

这会报告拦截器可用性、root 状态、游戏检测和代理可达性。

**检查数据包流转。** 拦截器运行、游戏连接服务器之后，客户端的数据包计数应该开始增长。如果 "Packets Sent" 一直是 0，说明拦截器还没看到游戏流量；参见[故障排查](troubleshooting.md)。

---

## Gatekeeper 提示

如果 macOS 阻止该二进制文件并提示 "cannot be opened because the developer cannot be verified"，请清除隔离属性：

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

只有当你从官方 Releases 页面下载了该二进制文件时才这样做。

---

## 下一步

- [支持的游戏](supported-games.md)
- [故障排查](troubleshooting.md)
- [CLI 参考](CLI-REFERENCE.md)
