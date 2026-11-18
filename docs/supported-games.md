# Supported Games

LightSpeed includes built-in profiles for 28 popular multiplayer games. Each profile defines the port range (UDP, or TCP for the TCP profiles) and process name for auto-detection.

---

## Game List

| # | Game | CLI Flag | Default Ports | Anti-Cheat | Process Name |
|---|------|----------|---------------|------------|-------------|
| 1 | **Rust** | `--game rust` | 28015-28017 | EAC | `RustClient.exe` |
| 2 | **CS2** | `--game cs2` | 27015-27050 | VAC | `cs2.exe` |
| 3 | **Fortnite** | `--game fortnite` | 7000-9000 | EAC + BattlEye | `FortniteClient-Win64-Shipping.exe` |
| 4 | **Dota 2** | `--game dota2` | 27015-27050 | VAC | `dota2.exe` |
| 5 | **Apex Legends** | `--game apex` | 37000-37050 | EAC | `r5apex.exe` |
| 6 | **Valorant** | `--game valorant` | 7000-7500 | Riot Vanguard | `VALORANT-Win64-Shipping.exe` |
| 7 | **Overwatch 2** | `--game ow2` | 3478-6250 | Blizzard Warden | `Overwatch.exe` |
| 8 | **League of Legends** | `--game lol` | 5000-5500 | Riot Vanguard | `League of Legends.exe` |
| 9 | **PUBG: Battlegrounds** | `--game pubg` | 7000-17999 | BattlEye | `TslGame.exe` |
| 10 | **MapleStory** | `--game maplestory` | 7575-8484 | BlackCipher (NGS) | `MapleStory.exe` |
| 11 | **Genshin Impact** | `--game genshin` | 22101-42472 | None | `GenshinImpact.exe` |
| 12 | **Rocket League** | `--game rocketleague` | 7000-9000 | EAC | `RocketLeague.exe` |
| 13 | **World of Tanks** | `--game wot` | 12000-29999 | None | `WorldOfTanks.exe` |
| 14 | **Dead by Daylight** | `--game deadbydaylight` | 27000-27050 | EAC | `DeadByDaylight-Win64-Shipping.exe` |
| 15 | **Bodycam** | `--game bodycam` | 27000-27050 | None | `Bodycam-Win64-Shipping.exe` |
| 16 | **CS:GO Legacy** | `--game csgo` | 27000-27050 | VAC | `csgo.exe` |
| 17 | **Roblox** | `--game roblox` | 49152-65535 | Byfron (Hyperion) | `RobloxPlayerBeta.exe` |
| 18 | **Project Zomboid** | `--game zomboid` | 16261-16262 | None | `ProjectZomboid64.exe` |
| 19 | **WARDOGS** | `--game wardogs` | 7777-7788 | Elytra (kernel-mode) | `WardogsClient-Win64-Shipping.exe` |
| 20 | **Minecraft** | `--game minecraft` | 19132-19133 | None | `Minecraft.Windows.exe` |
| 21 | **Hunt: Showdown** | `--game hunt` | 20000-20099 | Easy Anti-Cheat (EAC) | `HuntGame.exe` |
| 22 | **Rainbow Six Siege** | `--game rainbowsix` | 10000-10099 | BattlEye + R6 Shieldguard | `RainbowSix.exe` |
| 23 | **Minecraft Java Edition** | `--game minecraft-java` | 25565 (TCP) | None | `javaw.exe` |
| 24 | **ARC Raiders** | `--game arcraiders` | 7600-7777 | Easy Anti-Cheat | `PioneerGame.exe` |
| 25 | **Jedi Knight II: Jedi Outcast (NWH)** | `--game jedioutcast` | 28070 | None reported | `nwhmp.exe` |
| 26 | **Tibia** | `--game tibia` | 7171-7172 (TCP) | BattlEye | `Tibia.exe` |
| 27 | **ROB RUN** | `--game robrun` | 32768-60999 (ephemeral) | Easy Anti-Cheat | `ROB RUN.exe` |
| 28 | **EA SPORTS FC 27** | `--game fc27` | 21000-21050 | EA Javelin | `FC27.exe` |

> **Rainbow Six Siege note:** the profile is **UNVERIFIED against R6
> Shieldguard.** Siege ships three anti-cheat layers - BattlEye, Shieldguard and
> Ubisoft's account-level system - and Shieldguard refuses to launch unless
> Secure Boot, HVCI and a TPM 2.0 are present. LightSpeed forwards UDP without
> injecting code or touching game memory, so it is outside what those systems
> inspect, but Shieldguard's checks are launch-time *environment* checks rather
> than network inspection, and nobody has reported a real match run through this
> profile yet. Aliases: `r6`, `r6s`, `siege`, `rainbow-six`.

> **Hunt: Showdown note:** the UDP gameplay block is 20000-20099, which is
> what the tunnel carries. The game also opens TCP **61088** for its
> backend/menu connection; that port is deliberately NOT part of the profile,
> because LightSpeed captures and tunnels UDP only and a TCP port in the
> capture range would produce a filter that matches nothing.

> **Roblox note:** Roblox has no native Linux client. On Linux, run it through
> Wine/Proton, where the process name may appear truncated. Roblox picks its
> outbound UDP source port from the high ephemeral range (49152-65535) per
> server instance, so capture/intercept mode works better than redirect mode.

> **WARDOGS note:** Bulkhead has not published a client port list, and the
> dedicated server binds an operator-chosen `-port`, so the 7777-7788 range is
> an unverified third-party observation (Unreal's default is 7777). Confirm it
> with a packet capture. WARDOGS is Windows-only for now: its kernel-level
> Elytra anti-cheat blocks Proton on Linux, so capture must run on Windows.

> **EA SPORTS FC 27 note:** the profile carries the UDP gameplay range
> `21000-21050` (server-side ports). The request in
> [#166](https://github.com/ShibbityShwab/lightspeed/issues/166) measured
> `21006-21039` over ~10 matches and expected the real range to be slightly
> wider, so the enclosing 50-port block ships rather than the exact measured
> span. Match hosts are AWS instances assigned per match (mostly
> `eu-south-1`), so **capture/intercept mode is the supported path**: a fixed
> `--game-server IP:PORT` is stale before kickoff and the game cannot be pointed
> at `127.0.0.1`. The game is launched from the EA app (`EADesktop.exe`), which
> is the launcher, not the game - only `FC27.exe` is detected. The match servers
> do not answer ICMP, so this title has no direct-ping anchor and relay ranking
> uses its geometric floor / region prior.

> **Tibia note:** Tibia's login and world connections are TCP `7171`/`7172`,
> and the shipped profile now uses the **TCP path** (protocol v5 terminator +
> relay splice) rather than the old UDP capture. **Target a process** still
> relays UDP only, so select the Tibia game profile rather than a picked
> process. The relay must advertise `TCP_TUNNEL` (all eight community relays
> do; a self-hosted relay needs a proxy build with the TCP lane).

> **Minecraft Java Edition note:** this is the built-in TCP profile carried by
> the tunnel's TCP path (TCP 25565). The design/implementation record is
> [`docs/tcp-tunnel-design.md`](tcp-tunnel-design.md). TCP-primary auto-detection
> and user-profile TCP ports are not exposed yet - see the TCP status below and
> [#176](https://github.com/ShibbityShwab/lightspeed/issues/176).

---

## TCP game traffic (partial support)

The tunnel carries TCP game streams: protocol v5 adds a `TcpExtHeader`, the
client terminates TCP locally and the relay splices a real connection to the game
server, so the server sees one ordinary connection. The built-in **Minecraft
Java Edition** profile (TCP 25565) is the reference profile for this path; the
design/implementation record is
[`docs/tcp-tunnel-design.md`](tcp-tunnel-design.md).

It is not yet a general "any TCP game" capability: the request and profile flow
is built around UDP gameplay, user/custom profiles reject `tcp_ports`, and
TCP-primary auto-detection is not exposed in the GUI. Built-in TCP profiles can
be added by the maintainers (see Tibia below), but a new TCP-only title cannot
be added through the user game-request procedure yet. That work is tracked in
[#176](https://github.com/ShibbityShwab/lightspeed/issues/176); no timeline is
promised.

The shipped **Tibia** profile is carried by the TCP path (TCP `7171` login,
`7172` world; from issues #171/#176). Select the game profile, not **Target a
process** (that path relays UDP only), and use a relay that advertises
`TCP_TUNNEL` - every community relay does.

A known older example is **Angels Online Global**, which uses TCP ports 38201,
38304, 38501, and 38502 with the process `angel.dat`; there is no UDP gameplay
to carry.

If a game uses UDP for gameplay and TCP only for login or patching, LightSpeed
can still accelerate the UDP gameplay traffic.

---

## Community requests under evaluation

The following titles have been requested by the community. The per-request
status and the exact missing fact are maintained in
[`docs/game-requests.md`](game-requests.md); nothing below is promised.

- **Aion 2** - blocked: the measured remote gameplay UDP endpoint (#148).
- **Black Desert Online** - blocked: the measured gameplay UDP port range (#153).
- **Delta Force** - blocked: the measured gameplay endpoint (#168); if the
  gameplay is TCP, #176 is the tracker.
- **Battlefield 6**, **Warface** - under evaluation: measured gameplay UDP port
  range and matchmade-vs-fixed servers (discussion #63).
- **Deadlock** - blocked by Steam Datagram Relay: if its gameplay runs over SDR,
  a relay profile cannot improve it, because the game already routes through
  Valve's own network (discussion #142). The request stays tracked, not promised.
- **Tibia** - TCP profile shipped (login/world `7171`/`7172`); **Target a
  process** remains UDP-only, so use the game profile.

These are requests only. No support is promised, and no port ranges or anti-cheat compatibility claims are made for them at this time. If and when a profile ships, it will appear in the game list above.

---

## Anti-Cheat Compatibility

LightSpeed is compatible with all major anti-cheat systems:

| System | Games | Status |
|--------|-------|--------|
| **EasyAntiCheat (EAC)** | Rust, Fortnite, Apex Legends, Rocket League, Dead by Daylight | ✅ Permitted |
| **Valve Anti-Cheat (VAC)** | CS2, CS:GO Legacy, Dota 2 | ✅ Permitted |
| **BattlEye** | Fortnite, PUBG | ✅ Permitted |
| **Riot Vanguard** | Valorant, League of Legends | ✅ Permitted |
| **Blizzard Warden** | Overwatch 2 | ✅ Permitted |
| **Nexon Game Security (NGS)** | MapleStory | ✅ Permitted |
| **Byfron (Hyperion)** | Roblox | ✅ Permitted |
| **Elytra (VAIIYA)** | WARDOGS | ✅ Permitted |
| **EA Javelin** | EA SPORTS FC 27 | ✅ Permitted (transparent tunnel; not yet field-confirmed with this title) |

LightSpeed uses standard OS-level network drivers (WinDivert, nftables, pfctl). It does **not**:
- Modify game files or memory
- Hook into game processes
- Bypass kernel-level anti-cheat protection
- Hide or spoof your IP address (game servers see your real IP)

---

## Requesting a New Game

To request official support for a game not listed above:

1. [Open a game request](https://github.com/ShibbityShwab/lightspeed/issues/new?template=game_request.md)
2. Include the game name, the **gameplay UDP port range you observed in-game**, the process name (Windows) or binary name (Linux/macOS), and the anti-cheat system
3. Say whether match servers are assigned per match (matchmade) or fixed
4. The full checklist, including how to measure the port and when a profile cannot help (TCP-only, Steam Datagram Relay), is in [`docs/game-requests.md`](game-requests.md)

While you wait, **Target a process** in the app relays any process's UDP traffic without a profile, so a missing entry does not block you today.

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
