# Troubleshooting

---

## Quick Diagnostic

Run the built-in environment check first — it catches most issues:

```bash
lightspeed --check
```

This verifies: interceptor availability, packet filtering tools, game profile resolution, and proxy connectivity.

---

## Common Issues

### "Interceptor not available"

**CLI:** Your OS doesn't have the required packet filtering tools or you lack privileges.

| OS | Required | How to fix |
|----|----------|------------|
| Linux | nftables or iptables + root | `sudo lightspeed ...` |
| macOS | pfctl (built-in) + root | `sudo lightspeed ...` |
| Windows | WinDivert driver + Administrator | Right-click → Run as Administrator |

Verify with:
```bash
lightspeed --check
```

### "No game traffic seen" / Packets Sent stays at 0

**CLI:** The interceptor can't find game packets in the expected port range.

1. Make sure your game is **connected to a server** (not just the main menu or lobby)
2. Verify the game with `--scan-processes`:
   ```bash
   lightspeed --scan-processes
   ```
3. Try a different game profile or use manual server mode

**GUI (Windows):** Wait 15 seconds. If the amber "⚠ No game traffic seen" banner appears:
1. Open elevated PowerShell:
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Use the port shown in **Advanced → set server manually**

### "🎯 Finding your game server…" never resolves

The detector hasn't seen 3 packets to the same destination within 1.5 seconds.

1. Make sure you're connected to a game server (move your character to generate traffic)
2. If packets still aren't detected after 15 seconds, your server is on a non-standard port — use manual server mode
3. Stop and restart the interceptor after connecting to the server

### Packets Sent climbing, Packets Delivered = 0

Packets reach the proxy but responses aren't reaching your game. Usually a firewall issue.

**Linux:**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**macOS:**
```bash
sudo pfctl -d  # Temporarily disable pf to test
```

**Windows:**
```powershell
# Check if the firewall rule exists
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

### Proxy health check fails

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"proxy-1","uptime_secs":86400,...}
```

If unreachable:
- Check the proxy is running: `systemctl status lightspeed-proxy`
- Check firewall allows UDP 4434 and TCP 8080
- Check the proxy logs: `journalctl -u lightspeed-proxy --tail 50`

### Game disconnects when interceptor starts

The interceptor seizes packets before the game can receive responses, and the inject path fails.

**Windows:**
1. Verify `WinDivert64.sys` and `WinDivert.dll` are next to the `.exe`
2. Disconnect secondary network adapters (Docker, VMware, Hamachi virtual adapters)
3. Connect to the game server **before** starting the interceptor

**Linux:**
1. Check nftables rules: `sudo nft list ruleset | grep lightspeed`
2. If rules are stale: `sudo lightspeed --check` to diagnose

---

## Logs for Bug Reports

Run with debug logging to capture detailed diagnostics:

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Attach the log file to your [GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues).

---

## Still Stuck?

- [FAQ](faq.md) — common questions
- [GitHub Issues](https://github.com/ShibbityShwab/lightspeed/issues) — search existing reports
- Open a new issue with your OS, game, and log output
