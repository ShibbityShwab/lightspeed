# Supported Games

LightSpeed includes built-in profiles for 9 popular multiplayer games. Each profile defines the UDP port range and process name for auto-detection.

---

## Game List

| # | Game | CLI Flag | Default Ports | Anti-Cheat | Process Name |
|---|------|----------|---------------|------------|-------------|
| 1 | **Rust** | `--game rust` | 28015–28017 | EAC | `RustClient.exe` |
| 2 | **CS2** | `--game cs2` | 27015–27050 | VAC | `cs2.exe` |
| 3 | **Fortnite** | `--game fortnite` | 7000–9000 | EAC + BattlEye | `FortniteClient-Win64-Shipping.exe` |
| 4 | **Dota 2** | `--game dota2` | 27015–27050 | VAC | `dota2.exe` |
| 5 | **Apex Legends** | `--game apex` | 37000–37050 | EAC | `r5apex.exe` |
| 6 | **Valorant** | `--game valorant` | 7000–7500 | Riot Vanguard | `VALORANT-Win64-Shipping.exe` |
| 7 | **Overwatch 2** | `--game ow2` | 3478–6250 | Blizzard Warden | `Overwatch.exe` |
| 8 | **League of Legends** | `--game lol` | 5000–5500 | Riot Vanguard | `League of Legends.exe` |
| 9 | **PUBG: Battlegrounds** | `--game pubg` | 7000–17999 | BattlEye | `TslGame.exe` |

---

## Anti-Cheat Compatibility

LightSpeed is compatible with all major anti-cheat systems:

| System | Games | Status |
|--------|-------|--------|
| **EasyAntiCheat (EAC)** | Rust, Fortnite, Apex Legends | ✅ Permitted |
| **Valve Anti-Cheat (VAC)** | CS2, Dota 2 | ✅ Permitted |
| **BattlEye** | Fortnite, PUBG | ✅ Permitted |
| **Riot Vanguard** | Valorant, League of Legends | ✅ Permitted |
| **Blizzard Warden** | Overwatch 2 | ✅ Permitted |

LightSpeed uses standard OS-level network drivers (WinDivert, nftables, pfctl) — the same class used by commercial optimizers like ExitLag, WTFast, and NoPing. It does **not**:
- Modify game files or memory
- Hook into game processes
- Bypass kernel-level anti-cheat protection
- Hide or spoof your IP address (game servers see your real IP)

---

## Requesting a New Game

To request official support for a game not listed above:

1. [Open a GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues/new?template=game_request.md)
2. Include the game name, default UDP port range, and anti-cheat system
3. If known, include the process name (Windows) or binary name (Linux/macOS)

---

## Manual Server Mode (Any Game)

If your game isn't in the list or uses non-standard ports, use manual server mode:

```bash
# Find your server's IP and port (check in-game console or netstat)
# Then run:
lightspeed --game-server SERVER_IP:PORT --proxy YOUR_PROXY:4434

# Or with a specific local port:
lightspeed --game-server SERVER_IP:PORT --local-port 28015 --proxy YOUR_PROXY:4434
```

Then configure your game to connect to `127.0.0.1:<local_port>`.

### Finding Your Game Server Port

**Linux/macOS:**
```bash
ss -unp | grep <game_binary_name>
```

**Windows (PowerShell as Admin):**
```powershell
Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
  Where-Object LocalPort -gt 1024 |
  Select-Object LocalPort, RemoteAddress, RemotePort
```
