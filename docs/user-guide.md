# LightSpeed User Guide

> **TL;DR:** Install → run as Admin → click **⚡ BOOST MY GAME** → launch game → connect to any server. Done.

---

## What is LightSpeed?

LightSpeed is a free network optimizer for multiplayer games. It reroutes your game traffic through strategically placed relay servers (Boost Servers) to find a faster path to your game server than your ISP's default route.

**What this means for you:**
- Lower in-game ping
- Fewer rubber-banding and lag spikes  
- More consistent connection (especially on intercontinental servers)
- Zero cost — LightSpeed is free and open source

---

## System Requirements

| Requirement        | Details                                           |
|--------------------|---------------------------------------------------|
| Operating System   | Windows 10 / 11 (64-bit)                          |
| Privileges         | Run as **Administrator** for best (deep boost) mode |
| Files              | `WinDivert64.sys` and `WinDivert.dll` next to the `.exe` |
| Internet           | Any connection — LightSpeed works on Wi-Fi and Ethernet |

---

## Quick Start (60 seconds)

### Step 1 — Download

Grab the latest release ZIP from the [Releases page](https://github.com/ShibbityShwab/lightspeed/releases).  
Extract all files to a folder. Keep `WinDivert64.sys` and `WinDivert.dll` in the same folder as `lightspeed-gui.exe`.

### Step 2 — Run as Administrator

Right-click `lightspeed-gui.exe` → **Run as administrator**.

> **Why?** The Deep Boost mode intercepts packets at the OS kernel level, which requires elevated rights — just like VPN software.

If you forget, the app will show a yellow warning and a **🔑 Restart as Administrator** button.

### Step 3 — Choose your Boost Server

The app shows available Boost Servers with their ping to you.  
Pick the one closest to your **game server**, not necessarily closest to your house.

> *Example: If you're in Australia playing on a US East game server, pick the US East Boost Server even if it's farther from you — it has a faster backbone route to the game server.*

### Step 4 — Choose your game

Select your game from the dropdown. LightSpeed knows the UDP port ranges for 9 popular titles and will auto-detect the game server.

### Step 5 — Click ⚡ BOOST MY GAME

The button activates Deep Boost mode. The status changes to **"🎯 Finding your game server…"**

### Step 6 — Launch your game and connect

Open your game normally and connect to any game server. Within a few seconds LightSpeed will see your packets and show:

```
⚡ BOOST ENGAGED — 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

Your in-game ping now reflects the route through the Boost Server. If the Boost Server route is faster than your direct route, you'll see a lower ping.

### Step 7 — Play!

That's it. LightSpeed runs silently in the background. When you're done, click **Stop Boost** or right-click the tray icon → **Disconnect**.

---

## What the numbers mean

| Number             | What it tracks                                                        |
|--------------------|-----------------------------------------------------------------------|
| **Boost Ping**     | Milliseconds from your PC to the Boost Server. Aim for < 100ms.     |
| **Packets Sent**   | Game packets intercepted and forwarded to the Boost Server.          |
| **Packets Returned** | Responses from the Boost Server (forwarded from game server).      |
| **Packets Delivered** | Responses injected back to your game client.                      |

**Healthy session:** Sent ≈ Returned ≈ Delivered. If Delivered is much lower than Returned, see [Troubleshooting](troubleshooting.md).

---

## Reliability Shield (optional)

Enable **Reliability Shield** before clicking BOOST MY GAME to turn on Forward Error Correction (FEC).

- The Boost Server sends a small amount of extra data (~25% more bandwidth).
- If a packet is lost in transit, the server can reconstruct it from the extra data.
- Result: your game sees fewer dropped packets, especially useful on lossy Wi-Fi.

**When to use it:** Enable if you have frequent micro-stutters or packet loss. Disable if you're on a metered connection or if your download is already saturated.

---

## Switching game servers mid-session

If you disconnect from one server and join another (same game, different IP), LightSpeed automatically detects the new server within ~5 seconds.

You'll see the status briefly show **"🎯 Finding your game server…"** again, then lock onto the new server. No manual action needed.

---

## Hiding to the system tray

Click the **×** button to minimise LightSpeed to the system tray (it doesn't quit).  
Double-click the tray bolt icon to bring the window back.  
Right-click the tray icon for quick **Connect / Disconnect / Quit** options.

---

## Advanced — Manual server IP

If auto-detect doesn't work for your game (e.g., custom server port):

1. Click **▶ Advanced — set server manually**
2. Type the server address in `IP:port` format (e.g., `123.45.67.89:28015`)
3. Click **▶ Start Boost (manual)**
4. Connect your game to `127.0.0.1:<port>` as shown in the instruction box

---

## See also

- [FAQ](faq.md) — Common questions
- [Troubleshooting](troubleshooting.md) — Fix common issues
- [Supported Games](supported-games.md) — All 9 supported titles
- [Glossary](glossary.md) — Term definitions
- [Wiki](https://github.com/ShibbityShwab/lightspeed/wiki)
