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
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
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

### "WinDivert open failed" / `FWP_E_IN_USE` (0x8032000A) on Windows

WinDivert registers a WFP callout/filter for each open handle. If a handle is never closed, that filter state lingers, and the next `WinDivertOpen` fails with `FWP_E_IN_USE` (0x8032000A) even though no process is visibly using WinDivert.

Recent LightSpeed builds close both the capture and inject handles on every orderly shutdown path, including Ctrl+C in `--watch` and `--start-interceptor` and **Quit** in the GUI, and the receive loop is unblocked with `WinDivertShutdown` before the close so teardown is deterministic. A hard kill (`taskkill /f`, a crash, or closing the console window) can still leave the WinDivert 2.2.x driver with stale state; that is an upstream driver limitation (basil00/WinDivert#294, #406) that userspace cannot clear once the process is gone.

If you still hit it:

1. **Quit gracefully and wait a moment** — use the CLI Ctrl+C or the GUI's **Quit**, then give the handles a second or two to close before relaunching.
2. **Stop the WinDivert service** (avoids a reboot in some cases; note the driver is shared with other WinDivert apps such as ExitLag):
   ```powershell
   sc stop windivert
   ```
3. **Full shutdown, not restart** — Windows "Restart" can reuse the kernel session that holds the stale state; a full **Shutdown → power on** clears it.

> **Tip:** On v1.2.2 and earlier, a separate bug (data-plane auth rejecting all packets — issue #59) froze the connection and forced users to repeatedly kill the client, which is what triggered most `FWP_E_IN_USE` reports. That auth bug is fixed in v1.2.3.

---

## Windows GUI Issues

These apply to the `lightspeed-gui` app on Windows.

### Quit did nothing and left a zombie process

On versions before v1.4.2, choosing **Quit** from the tray menu could leave the process running in the background (a zombie), so the window closed but the engine kept running and a later launch behaved oddly. v1.4.2 fixes this: Quit now terminates the process cleanly.

If you are on an older build and the process is stuck, end it manually:

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Then upgrade to v1.4.2 or later.

### A second instance opens instead of focusing the first

On versions before v1.4.2, launching the GUI twice stacked a second window and a second engine. The GUI now holds a single-instance guard: a second launch shows a brief "LightSpeed is already running" notice and exits instead of starting another engine. To force a second instance anyway (for diagnostics), pass `--force` or set `LIGHTSPEED_GUI_FORCE=1`.

### No relays discovered

The GUI discovers the community relays through the signed registry. If the relay list stays empty:

1. Confirm you have internet access and that a firewall or VPN is not blocking outbound HTTPS to the registry.
2. Check the GUI log (see below) for a registry fetch or signature-verification error.
3. On the CLI build, run `lightspeed-client --probe-proxies` to see the discovery and probe report directly. If the CLI also finds nothing, the problem is network-side, not GUI-specific.
4. Restart the GUI after fixing connectivity; discovery runs on startup.

### How to find and open the GUI log

The GUI writes its trace log to:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Paste that path into the File Explorer address bar to open the folder, then open `gui-trace.log` in any text editor. Attach it to a bug report.

### "Heartbeat 0 in" in the log

A line like `Heartbeat 0 in` means the engine has sent zero keepalive heartbeats in the current window. In practice it shows up when the client has not established a working control-plane connection yet, so no heartbeats have gone out. Common causes:

- The client has not registered with a relay yet (check the registration line in the status view).
- The selected relay is unreachable.
- The interceptor has not started, so no session is active.

Once registration succeeds and heartbeats start flowing, the counter climbs. If it stays at 0 while a relay shows healthy, run `lightspeed-client --test-control` to isolate whether the control plane is reachable.

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
