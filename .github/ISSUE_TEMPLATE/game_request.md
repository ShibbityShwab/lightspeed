---
name: Game Request
about: Request support for a new game
title: "[Game] "
labels: game-profile
assignees: ''
---

**Before you file this:** you do not have to wait for a release. Add the game yourself in
`profiles.toml` - **Game → New custom game…** in the app, or see the
[user guide](https://github.com/ShibbityShwab/lightspeed/blob/master/docs/user-guide.md) for the file
format. File this request when you would rather have it built in; the details below are what a
built-in profile needs. The maintainer's triage checklist is
[docs/game-requests.md](https://github.com/ShibbityShwab/lightspeed/blob/master/docs/game-requests.md).

**Game name:**

**Platform(s):** [e.g. Windows, Linux, macOS]

**Anti-cheat system:** [e.g. EAC, VAC, BattlEye, Vanguard, None]

**Process name (Windows):** [Task Manager → Details, e.g. `GameClient.exe`]
**Binary name (Linux/macOS):** [e.g. `game-client`]

**Gameplay UDP port range** - the ports the game talks to its **servers** on, not the local
port and not a port-forwarding site's guess:

- From: _____
- To: _____

How to measure it while you are in a match:

1. Open Task Manager → Details and note the game's PID.
2. In PowerShell:
   `Get-NetUDPEndpoint -OwningProcess <PID> | Format-Table LocalPort,RemoteAddress,RemotePort`
   or in cmd: `netstat -ano -p udp | findstr <PID>`
3. The **remote** address and port are the game server. Run it across a few matches and
   report the range you actually see.

Did you observe this in-game, or is it from a port-forwarding site? [in-game is required
for a profile; a site's guess is not enough]

**Are servers matchmade or fixed?**
- [ ] Matchmade - a different server each match (we mark the profile as a dynamic server)
- [ ] Fixed/regional - you pick a server

**Does the game run on Steam Datagram Relay (SDR)?** Valve titles such as CS2, Dota 2 and
Deadlock can. If gameplay goes through SDR, a LightSpeed profile cannot improve it, because
the game already routes through Valve's network and we cannot chain into it. Say "not sure"
if you do not know.

**Store page:** [Steam/Epic/other URL]

**Additional notes:**
Any special networking behavior, multiple executables, launchers, etc.
