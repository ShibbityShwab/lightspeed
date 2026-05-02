# Troubleshooting

---

## Quick checklist

Before diving in, verify:

- [ ] `WinDivert64.sys` and `WinDivert.dll` are in the **same folder** as `lightspeed-gui.exe`
- [ ] LightSpeed is running as **Administrator**
- [ ] Your game is **running** and **connected to a server** (not just at the main menu)
- [ ] You selected the **correct game** in the LightSpeed dropdown

---

## Problem: "Boost Engaged" appears but in-game ping hasn't changed

**Most likely cause:** The Boost Server is not a shorter route than your direct connection for this particular game server. Not all game servers benefit from a relay.

**What to try:**
1. Try a different Boost Server location.
2. Check your Boost Ping in LightSpeed vs your game's native ping. If Boost Ping > game ping, the direct route is already faster.
3. Some games cache the ping display — disconnect and reconnect in-game to force a fresh reading.

---

## Problem: "Packets Sent" stays at 0 after boosting — port not detected {#port-not-detected}

**Why this happens:** LightSpeed auto-detects your game server from outbound UDP packets in a specific port range. If your server runs on a port outside that range the filter never matches and `Packets Sent` stays at **0**.

**Quick diagnosis:** After clicking BOOST MY GAME, wait 15 seconds. If you see the amber **"⚠ No game traffic seen — possible port mismatch"** banner, a port mismatch is the likely cause.

**How to find your server's real port:**

1. Open an **elevated PowerShell** while your game is running:

```powershell
Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
  Where-Object LocalPort -gt 1024 |
  Select-Object LocalPort
```

   Replace `RustClient` with your game's process name (e.g. `cs2`, `VALORANT-Win64-Shipping`).

2. Note the port numbers shown.

**How to fix it:**

1. Click **Stop Boost**.
2. Click **▶ Advanced — set server manually** to expand it.
3. In the **Custom Port Range** field enter the range that covers your server port, e.g. `28015-28999`.
4. Click **⚡ BOOST MY GAME** again.

**Common ranges by game:**

| Game | Range to try |
|------|-------------|
| Rust | `28015-28999` |
| CS2 | `27015-27100` |
| Dota 2 | `27015-27100` |
| Valorant | `7000-7500` |
| PUBG | `7777-7843` |

> **Note:** Default ranges have been widened in v0.5+ to cover most community servers automatically. Update to the latest release to get the widest defaults.

---

## Problem: "🎯 Finding your game server…" never goes away

The debounce detector hasn't seen 3 packets to the same destination within 1.5 seconds.

**What to try:**
1. Make sure you are **connected to a game server** (not just the main menu or lobby).
2. Try moving your character in-game to generate traffic.
3. If packets still aren't detected after 15 seconds, see the **"Packets Sent stays at 0"** section above — your server is likely on a non-standard port.
4. Click **Stop Boost** and try again after connecting to the game server.

---

## Problem: Boost stops working after switching servers

**As of v0.5+:** This is fixed. LightSpeed detects silence from the old server (5 seconds) and automatically re-enters detection for the new server.

If you're on an older version, update to the latest release.

If you're on the latest version and still seeing this:
1. Watch for **"🔄 Server stale — re-entering detection"** in the log (run from command prompt to see logs).
2. If it's not appearing, the 5-second window may not have expired — wait a bit longer after disconnecting before joining the new server.

---

## Problem: Packets Sent climbing, but Packets Delivered = 0

Your packets are reaching the Boost Server, but the spoofed responses aren't arriving at your game. This is almost always a **Windows Firewall** issue.

**What to try:**

**Option A (automatic):** LightSpeed adds a firewall rule automatically on each launch. If it's failing silently:
1. Open an **elevated command prompt** (Run as Administrator)
2. Run: `netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"`
3. If the rule is missing, add it manually:
   ```
   netsh advfirewall firewall add rule name="LightSpeed" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
   ```

**Option B (GUI):** Windows Security → Firewall & network protection → Allow an app through firewall → Add `lightspeed-gui.exe`

---

## Problem: "WinDivert open failed" error

```
WinDivert open failed (need Administrator + WinDivert64.sys)
```

**Solutions:**

1. **Run as Administrator** — right-click → Run as administrator, or use the 🔑 button in the app.
2. **Missing driver files** — ensure `WinDivert64.sys` and `WinDivert.dll` are in the same directory as the `.exe`.
3. **Antivirus blocking the driver** — some antivirus products quarantine WinDivert. Add an exclusion for `WinDivert64.sys`.
4. **Windows version** — Windows 7/8 are not supported. Windows 10 1903+ required.

---

## Problem: Game disconnects when BOOST MY GAME is clicked

The kernel intercept seizes packets before the game can receive server responses, then the inject side fails.

**Most likely causes:**
1. **WinDivert.dll missing** — the inject handle fails to open silently, then all intercepted packets are dropped.
2. **Wrong network interface** — the injected packets go to the wrong adapter. Check the log for `Cached inject interface: IfIdx=N` — if N=0 on the first intercept, this is the issue.

**What to try:**
1. Verify all three files are present.
2. Disconnect any secondary network adapters (e.g., virtual adapters from Docker, VMware, Hamachi).
3. Make sure you're connected to the game server **before** clicking BOOST MY GAME (so the first packet captures the correct interface).

---

## Problem: App closes instead of minimising to tray

The window close button should hide to tray. If it's quitting instead:
- Make sure you have the tray icon visible. If Windows is hiding it, click the `^` arrow in the system tray area to show hidden icons.

---

## Problem: App won't launch / crashes at startup

Run from a command prompt to see the error:
```
cd C:\path\to\lightspeed
lightspeed-gui.exe
```

Common causes:
- Missing Visual C++ Redistributable (install from Microsoft)
- Missing WinDivert files
- Running on an unsupported Windows build

---

## Collecting logs for a bug report

1. Open a command prompt in the LightSpeed folder
2. Run: `lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt`
3. Reproduce the issue
4. Attach `lightspeed-log.txt` to your [GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues)

---

## Still stuck?

- Check [FAQ](faq.md) for common questions
- Search [existing GitHub issues](https://github.com/ShibbityShwab/lightspeed/issues)
- Open a new issue with your log file attached
