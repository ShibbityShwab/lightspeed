# Frequently Asked Questions

---

## Getting Started

### Do I need to pay for LightSpeed?

No. LightSpeed is completely free and open source under the MIT license.

### Will LightSpeed get me banned?

No. LightSpeed works exactly like commercial network optimizers (ExitLag, WTFast, NoPing). It uses a standard Windows network driver (WinDivert) that is permitted by EAC (Rust), VAC (CS2), and Vanguard (Valorant). It does not modify game files or memory.

### Why do I need to run as Administrator?

Deep Boost mode intercepts packets at the Windows kernel level to completely reroute your game traffic. This requires elevated privileges — the same reason VPN software and antivirus programs need admin rights. If you don't want to run as admin, use the **Advanced — set server manually** option instead (no admin required, but less effective).

### What files do I need to keep next to the .exe?

- `lightspeed-gui.exe`
- `WinDivert64.sys`
- `WinDivert.dll`

All three must be in the same folder. Download WinDivert from [reqrypt.org/windivert.html](https://reqrypt.org/windivert.html) if the files are missing.

---

## The Boost

### How does LightSpeed actually improve my ping?

Your ISP routes your packets along whatever path it prefers, which often isn't the fastest. LightSpeed sends your packets through a Boost Server in a major data center that has high-speed backbone connections to game server regions. If the Boost Server's path to the game server is shorter or less congested than your ISP's path, your ping drops.

### My boost ping went UP. Why?

If the Boost Server adds distance to your path (e.g., you're already close to the game server), using LightSpeed will increase your ping. Try a different Boost Server, or if none are faster, don't use LightSpeed for that particular server.

### Which Boost Server should I pick?

Pick the server closest to your **game server**, not your own location. For example:
- Playing on a US West game server → pick US West Boost Server
- Playing on a Singapore game server from Australia → pick Singapore Boost Server

### How long does it take to detect my game server?

Usually 1–3 seconds after you connect in-game. LightSpeed watches for 3 packets to the same destination within 1.5 seconds before locking on.

### I switched servers in-game and the boost stopped working. What do I do?

Nothing — this is now handled automatically. LightSpeed detects that the old server has gone quiet (5 seconds of silence) and re-enters detection mode. You'll see **"🎯 Finding your game server…"** briefly, then it locks onto the new server. If it takes longer than 10 seconds, try Stop Boost → BOOST MY GAME again.

---

## Reliability Shield

### What is Reliability Shield?

It's Forward Error Correction (FEC). The Boost Server sends a small amount of extra redundancy data alongside your game packets. If a packet is dropped between you and the Boost Server, the server can reconstruct it from the extra data — your game never sees the drop.

### Should I enable Reliability Shield?

Enable it if you experience micro-stutters or packet loss. Disable it if:
- You're on a metered / capped connection (it adds ~25% traffic)
- Your connection is already fully saturated (it could make things worse)
- You're on a great connection with < 0.1% packet loss (no benefit)

---

## Errors and Troubleshooting

### The app says "Needs to run as Administrator"

Right-click `lightspeed-gui.exe` → **Run as administrator**, or click the **🔑 Restart as Administrator** button in the app.

### No game detected / packets not seen

1. Make sure your game is actually running and connected to a server.
2. Select the correct game in the dropdown.
3. Click **🔄 Rescan** if the game was launched after LightSpeed.
4. If your game server uses a non-standard port, use **Advanced — set server manually**.

### Packets Sent is climbing but Packets Delivered is 0

Your packets are reaching the Boost Server but the spoofed responses aren't making it back to your game client. This is usually a Windows Firewall issue. LightSpeed tries to add a firewall rule automatically. If it fails:
1. Open Windows Defender Firewall → Advanced Settings
2. Add an Inbound Rule → Program → browse to `lightspeed-gui.exe` → Allow

### The app crashes immediately on launch

Make sure `WinDivert64.sys` and `WinDivert.dll` are in the same folder as the `.exe`.

See [Troubleshooting](troubleshooting.md) for more detailed steps.

---

## Privacy and Security

### Does LightSpeed read my game traffic?

LightSpeed intercepts your UDP packets to forward them through the Boost Server. It can see (but does not log or store) the source/destination IP addresses and UDP payload sizes. Game content (e.g., player positions) passes through encrypted by the game's own protocol. See the full [Privacy Policy](privacy.md).

### Is the Boost Server run by LightSpeed?

Currently the Boost Servers are community-run. In future releases, a dedicated server network will be available.

---

## Other

### Does LightSpeed work on Linux / Mac?

Not yet. The WinDivert kernel driver is Windows-only. Linux support using eBPF/nftables is planned.

### Can I run LightSpeed while using a VPN?

Generally no — both try to intercept your network traffic and will conflict. Disable your VPN before using LightSpeed.

### Where do I report bugs?

Open an issue on [GitHub](https://github.com/ShibbityShwab/lightspeed/issues) with your log output (launch from command prompt to see logs).
