# Supported Games

LightSpeed supports auto-detection for the following games.  
All use UDP game-server traffic. Ports shown are the default range (auto-detected).

| Game             | Default Port(s) | Auto-detect | Notes                                          |
|------------------|-----------------|-------------|------------------------------------------------|
| Rust             | 28015–28017     | ✅           | EAC compatible. F1 console for manual connect. |
| CS2              | 27015–27050     | ✅           | VAC compatible.                                |
| Dota 2           | 27015–27030     | ✅           | VAC compatible.                                |
| Valorant         | 7000–7500       | ✅           | Vanguard compatible.                           |
| Apex Legends     | 37000–37050     | ✅           | EAC compatible.                                |
| League of Legends| 5000–5500       | ✅           |                                                |
| PUBG             | 7777–7843       | ✅           | BattlEye compatible.                           |
| Fortnite         | 9000            | ✅           | EAC compatible.                                |
| Overwatch 2      | 3724            | ✅           | Battle.net launcher.                           |

---

## Adding a game not in this list

Use **Advanced — set server manually**:
1. Find your game server's IP and port (check in-game console, or use `netstat -n` while connected)
2. Open the Advanced panel in LightSpeed
3. Type `server_ip:server_port`
4. Click **▶ Start Boost (manual)**

To request official support for a game, open a [GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues) with the game name and UDP port range.

---

## Anti-cheat notes

LightSpeed uses **WinDivert**, a standard Windows network driver used by ExitLag, WTFast, NoPing, and other commercial optimizers. It does not:
- Modify game files or memory
- Hook game processes
- Bypass anti-cheat kernel protection

All major anti-cheat systems (EAC, VAC, BattlEye, Vanguard) permit this class of network driver. If you have concerns, disable LightSpeed before launching ranked play.
