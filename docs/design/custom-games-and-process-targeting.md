# Custom game profiles and process-scoped interception

Design plan for [discussion #146](https://github.com/ShibbityShwab/lightspeed/discussions/146).
Status: **proposed, awaiting owner review.** Base revision `c46c6b7`. Research date 2026-10-07.

## 1. The request

Discussion #146 ("Add Custom Game (Server) be Saved", iTerryTie, 2026-10-05) asks for three things:

1. Persist the **Server** and **Custom Port Range** fields when "Game: Manual (custom server)" is selected.
2. Save custom game entries with pre-filled connection details, selectable from a list.
3. Intercept all traffic of a chosen `.exe`.

The owner replied in-thread: "I will probably do all of those but port detection by process was not working well when I first started the project; I can look at that again and see if there is a better cross platform way now."

Ask 1 already shipped in `c46c6b7` (`manual_server` + `manual_port_range` are persisted in the GUI `config.toml`,
with a regression test named for this discussion). This plan covers asks 2 and 3.

## 2. What exists today (verified in tree)

**Profiles are compile-time.** Each game is a module implementing `GameConfig` (`client/src/games/mod.rs`),
registered in `GAME_REGISTRY` and `detect_game()`. Adding one game touches six code sites plus docs, README,
CLI reference and the website, and two count guards fail if any site is missed
(`games/mod.rs` tests; `protocol/src/control.rs:1001` asserts `GAME_IDS.len() == 23`).
That is exactly the cost this feature removes for *user* games.

**The GUI game list is derived** from `all_game_keys()` (`client-gui/src/app.rs:94`), so any registry change
flows through automatically. The "Manual / Custom..." mode persists its fields
(`client-gui/src/config.rs:69-79`) and starting it calls `engine.start_redirect`
(`client-gui/src/app.rs:1230`), which is a *local UDP relay* (`UdpRedirect`): the user must point the game at
`127.0.0.1:<port>`. It is not transparent interception.

**Two functional gaps matter here.** The persisted custom port range currently influences nothing:
`start_boost` parses it and discards the value (`let _ =`, `client-gui/src/app.rs:651-652`), and
`engine.start_interceptor` takes no range (`engine.rs:914-920`), so boost always uses the profile's
`ports()`. And `manual_server` only feeds the Manual redirect path. Persisting without actuating is why the
range must be wired into the target model as part of P1, not treated as done because the field round-trips.

**Telemetry already tolerates unknown games.** `game_id::id_for_key` falls back to `UNKNOWN = 0`
(`protocol/src/control.rs:105-112`), and the register message carries that byte. Custom profiles need **no
protocol or wire change**.

**Two interception paths coexist.**

- `capture/windivert_redirect.rs` (Windows, `windivert-redirect` feature): the legacy engine, UDP and TCP
  (Minecraft Java uses its TCP terminator, `docs/tcp-tunnel-design.md`), filter by port/transport.
- `interceptor/*` (the OOP `TrafficInterceptor` framework, used by the GUI via `engine.start_interceptor`):
  - Windows `interceptor/windows.rs`: WinDivert at the **network layer**, filter is `outbound and udp and
    (port range)`. A known PID is logged but **not** used in the filter (`windows.rs:129-146`): the network
    layer cannot match `processId`, only the flow/socket layers can. This is the concrete gap behind "port
    detection by process was not working well".
  - Linux `interceptor/linux.rs`: nftables/iptables `REDIRECT` for **one** game-server address at a time,
    with a conservative rotation state machine (`interceptor/rotation.rs`) and a shadow-direct probe that is
    exempted by an fwmark rule.
  - macOS `interceptor/macos.rs`: pf `rdr` anchor for **one** server at a time, with a `no rdr` probe
    exemption.

**Process scanning is shell-based and single-purpose.** `interceptor/process_scanner.rs` runs
`tasklist`/`netstat` (Windows), `ps`/`lsof` (macOS), `/proc` + `ss` (Linux), maps image names to PIDs, and
returns connected IPv4 UDP/TCP routes. It does not track child processes (game launchers and anti-cheat
helpers spawn the real game), has no native API path, and is called synchronously on the UI thread.

**Other verified details.** The legacy `engine.start_windivert_for_game` path has no live caller at HEAD
(dormant; still the reference implementation of the UDP MITM and TCP terminator). `capture/{windows,macos,
linux}.rs` are doc-only placeholders and every platform captures through `pcap_backend.rs`. The built-in
count guards are `games/mod.rs:769-818` (`GAME_REGISTRY.len() == 23`) and `protocol/src/control.rs:1001`; the
doc comment on `GAME_REGISTRY` still says 20 and is stale.

**WinDivert can do this job today.** Verified in the pinned dependency: `windivert` 0.7.0-beta.4 exposes the
**flow layer** (`divert/flow.rs`, `WinDivertAddress<FlowLayer>::process_id()`), and the WinDivert 2.2
documentation defines `WINDIVERT_DATA_FLOW` as `{ Endpoint, ParentEndpoint, ProcessId, LocalAddr, RemoteAddr,
LocalPort, RemotePort, Protocol }`. Flow-layer events are emitted per flow with the owning process, which is
precisely the attribution primitive the network layer lacks. No new dependency is needed.

**Cross-platform facts that shape the design** (sources in section 9):

- No portable process-event API exists. Windows has ETW/WMI, macOS has kqueue `EVFILT_PROC`, Linux has
  pidfd/netlink connector/eBPF - each native, each different. Portable code polls.
- Windows `GetExtendedTcpTable/UdpTable` with `*_OWNER_PID` classes give socket-to-PID mapping with no
  documented elevation requirement, but the UDP table has no remote endpoint, so it cannot see a connected
  UDP flow's peer.
- Linux `/proc/<pid>/fd` inode scans work for the current user (or root); `NETLINK_SOCK_DIAG` gives uid +
  inode; `bpf_get_current_pid_tgid` gives exact attribution but needs root/CAP_BPF.
- macOS `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` is the `lsof`/`nettop` mechanism; it needs root (or the
  `com.apple.system-task-ports` entitlement) to see other processes.
- pf cannot match a PID. Windows WFP can match an app id, but redirecting needs a callout **driver** (EV
  signing + maintenance). macOS per-process steering really needs a Network Extension (paid Apple account,
  entitlements, user approval). Mullvad needed three completely different mechanisms for the same feature
  and still documents unavoidable leak cases.

## 3. Goals, constraints, non-goals

**Goals.** Ask 2: user-managed game profiles and saved server presets that survive restarts, usable from the
GUI and the CLI. Ask 3: point LightSpeed at a process and have its traffic intercepted, on Windows first,
Linux second, macOS best-effort - all behind one code path.

**Constraints.** `[COST_STUB]` $0.00 ongoing: no paid drivers, no Apple Developer Program, no commercial SDK.
`[ETHICS_STUB]` anti-cheat safety and no unfair advantage. `[TRANSPARENCY_STUB]` no payload modification, IP
preserved. `[PRIVACY_STUB]` no new data collection, as section 4.7 spells out. Repo quality gates: fmt, clippy
`-Dwarnings`, full crate suites, mutation-tested guards. Protocol wire ids are append-only and are not needed
here.

**The relay abuse guard is a hard boundary, not a tuning knob.** This is the constraint that shapes every
number in the design, so it goes above the others. The relay bans a client source on a single rule: more than
**10 distinct destinations in 10 s** from one source IP is abuse, and the ban lasts **3600 s**. The blast
radius is what makes the rule a boundary rather than a statistic. A ban lands on the source IP, so it takes
down **every concurrent built-in session on that client** (the player's normal LightSpeed boost included) and
**every device behind a shared NAT on that IP**. One custom profile that walks a rotating destination list too
fast costs the household its optimizer for an hour. Nothing in this design may approach that line, and no
setting the user can reach may raise the client's rate past it.

**Non-goals for v1.** macOS Network Extension; IPv6 (every interception path in the tree is IPv4 today:
`SocketAddrV4`, `Ipv4Addr`); TCP in process mode (Phase 5); editing built-in profiles; a driver-writing
effort. And one non-goal that deserves its own line, because it is the tempting shortcut: **v1 does not solve
the destination-rate problem by changing the protocol.** Three crates would move (client, protocol, relay)
for a feature whose payoff is one client-side cap, and any fix in that direction hands the threshold to the
client. The relay would then have to trust a number the client sent, which is the trust boundary the guard
exists to defend. The cap stays a client-side constant that the client can prove it honors, and the relay
keeps enforcing its own rule from its own observation (section 5.3). Nobody may propose a protocol change for
this in v1; if the constant proves too tight, the answer is a different constant.

**Non-goals for v1.** macOS Network Extension; IPv6 (every interception path in the tree is IPv4 today:
`SocketAddrV4`, `Ipv4Addr`); TCP in process mode (Phase 5); editing built-in profiles; a driver-writing
effort.

## 4. Design A: user game profiles and saved servers

### 4.1 Storage

One TOML file, one writer at a time, read by both surfaces:

| OS      | Path                                                    |
|---------|---------------------------------------------------------|
| Windows | `%APPDATA%\light-speed\profiles.toml`                   |
| macOS   | `~/Library/Application Support/light-speed/profiles.toml` |
| Linux   | `$XDG_CONFIG_HOME/light-speed/profiles.toml` (default `~/.config`) |

This is the same config directory the GUI already uses (`client-gui/src/paths.rs`, `config_dir()`). The
resolver moves into the client crate (`client/src/paths.rs`) and the GUI delegates to it, so there is exactly
one implementation. CLI overrides: `--profiles <path>` and `--no-user-profiles`.

Deliberately **not** the main `lightspeed.toml`: that file is a per-invocation config selected with
`--config`, often from a repo checkout; profiles are per-user state.

### 4.2 Schema

```toml
version = 1

# Ask 2: named games with pre-filled details, selectable later.
[[games]]
name = "My Game"                    # required, display name
key = "my-game"                     # optional; slug of name when absent
process_names = ["MyGame.exe"]      # optional; enables auto-detect and process targeting
udp_ports = "27015-27050"           # optional; "lo-hi" or a single port
dynamic_server = false              # optional; rotating servers (like Fortnite)
notes = "Community server uses UDP 27015"  # optional, shown in the UI

# Ask 2, Manual mode: saved server presets.
[[servers]]
name = "My community server"
address = "203.0.113.7:27015"
udp_ports = "27015-27050"           # optional override for the Manual port field
```

Validation, enforced at load and at save:

- **`tcp_ports` is not a field.** A `[[games]]` entry that carries `tcp_ports` is rejected, at load and at
  save, with that reason printed. The interception path is UDP-only, so a TCP port list would never reach a
  filter. The legacy TCP terminator in `capture/windivert_redirect.rs` has no live caller at HEAD, and
  accepting an inert option recreates the dead-control bug class already seen at `client-gui/src/app.rs:651`,
  where a parsed port range gets dropped on the floor and the setting quietly does nothing. Better to refuse
  the key than to honor it in name only. TCP in process mode stays a later phase (section 3), and the schema
  gains the field if and when the path exists.
- **`pid` is retired.** The field is listed here only to say it is gone: `pid` is never parsed, in either
  direction, and a profile that still carries one is neither accepted nor upgraded. Process identity comes
  from `process_names` (name plus a fresh scan) instead of a stored number, which survives restarts and PID
  reuse. See section 5 for the identity checks that replace it.
- `key` matches `[a-z0-9-]{1,24}` and is unique among user profiles. **Built-ins always outrank user claims:**
  when a user `key`, a user `process_names` entry, or a user alias matches a built-in key, a built-in alias
  (`detect_game`'s match arms, including `r6`, `mc`, `pz`, ...), or a built-in process name, the built-in
  wins. A save that would claim one is a hard error; a load that carries one is a warning. Both name the
  built-in that won (`"key 'minecraft' is reserved by built-in game 'minecraft'"`). Shadowing is never
  silent, and a user profile can't take over a built-in's detection, ports, or display name by accident.
- At least one of `process_names` or `udp_ports` for a `[[games]]` entry.
- Port ranges: `1..=65535`, `lo <= hi`; reuses the GUI's `parse_custom_port_range` semantics.
- Caps: 64 games, 64 servers (bounds the UI and the rule sets; a profile is one config line, not an app).
- Unknown keys are ignored (forward compatibility); malformed entries are skipped **and reported**, matching
  the tolerant-but-visible style of the GUI config loader.

### 4.3 Resolution and precedence

`games::resolve(key) -> Result<Box<dyn GameConfig>, ResolveError>` is the **only** public resolution API. Nothing
else in the tree resolves a game key, and the picker, the CLI, the engine and the tests all come through it. The
order is fixed and tested:

1. built-in `detect_game(key)` first, so a built-in key always means the built-in profile and a shipped
   behaviour can't change under a user's feet;
2. then a user profile whose key matches exactly;
3. then the existing Unknown game error shape, extended with a `Your profiles:` line that lists the user keys.

The head of the message is unchanged so the string every existing caller matches on, and every existing test that
asserts it, keeps holding. The added line carries the user's own keys, sorted, and is empty-safe: a fresh install
with no `profiles.toml` still gets just `Supported:`, same as today.

`detect_game` becomes **private** (`pub(crate)`), reachable only through `resolve()`. That keeps one path, removes
the old "check users first" ambiguity for good, and leaves the built-in meaning untouched: the registry guards and
the docs/website counts still count built-in games exactly as they do now. `all_game_keys()` gains a companion
`all_game_keys_with_user()`.

User profiles are loaded once per process into a cached set, with an explicit `reload()` for the GUI after an
edit. The GUI's profile editor calls `reload()` on every save, so a profile edit takes effect on the next resolve
without a restart, and no file watching is added in v1.

### 4.4 Trait fit

A `UserGameProfile` struct implements `GameConfig`. The one real wrinkle: a struct that owns its `String`s cannot
hand out `&[&str]` with the lifetime of a `&self` borrow that ends when the call returns. The trait method is
widened:

```rust
fn process_names(&self) -> Vec<&str>;
```

A `UserGameProfile` borrows from `&self`, so the `Vec` it returns is cheap to build and never outlives the
profile it points at. The 23 built-in impls return `self.names.to_vec()` over their existing static tables; that
is mechanical, and it's the whole cost. Call sites that only iterate keep working once the binding is a `Vec`
instead of a slice.

**`Box::leak` is rejected.** The original draft recommended interning each profile's strings at load with
`Box::leak`, on the grounds that `process_name_table` already does it. It doesn't hold here, because the lifetime
is not the same. Built-in names leak once, from `'static` tables, for the whole process. A user profile leaks on
every load, and the GUI calls `reload()` after each editor save, so the leak compounds with edits for the life of
the session: every save strands the previous profile's strings permanently. The 64-profile cap bounds one file, not
a session's worth of saves, so it isn't a bound on the leak at all. That is not an acceptable trade for a GUI the
user is meant to edit freely.

A **blanket default** implementation was considered too, i.e. a trait default that calls `process_names()` and
introduces no per-impl method, which would have avoided the 23 mechanical edits. It's invalid for exactly the
reason above: the signature it would delegate to is the one that needs changing, so the default either keeps
`&[&str]` (the `String`-owning profile still can't implement it) or widens the trait anyway. It also can't be a
default over an abstract method because there is no name to delegate to that isn't the very constraint being
removed. Circular, so it's out; the mechanical edits are the honest path.

`anti_cheat()` returns `"user-defined"`; `typical_pps()` and `packet_size_range()` return the same neutral
defaults the interceptor already tolerates (they only feed diagnostics); `transport()` is always UDP;
`build_capture_filter()` is inherited unchanged.

### 4.5 CLI

- `--list-games` prints "Built-in games" (unchanged) followed by "Your profiles" when any exist.
- `--game <key>` resolves user profiles first, so `lightspeed --game my-game ...` and
  `--intercept --game my-game` work without new flags.
- `--profiles <path>` / `--no-user-profiles` for reproducibility and debugging.
- No profile-writing subcommand in v1: the GUI is the editor; the file is hand-editable and documented.
  (A `lightspeed profiles add` can come later if asked for.)

### 4.6 GUI

- **Game dropdown:** "Auto (detect)", "Manual / Custom...", built-in games, then a "My games" group with the
  user profiles, then "New custom game...".
- **Editor sheet** (same sheet pattern as Settings): name, process names, UDP range, TCP range (advanced),
  dynamic server, notes. Live validation with the existing invalid-input styling; save/delete; the resulting
  key is shown so `--game <key>` is discoverable.
- **Manual mode:** the Server row gains a "Saved servers" dropdown (fills the address and port fields) and a
  "Save as..." action that appends a `[[servers]]` entry. This is the direct answer to ask 2's wording.
- **Process targeting:** Phase 3 adds a picker in the Manual/custom flow: choose a running process (or browse
  to an `.exe`) and the app either matches it to a profile or offers to create one with
  `process_names = ["that.exe"]`. Selecting it switches interception to the process target.
- All new strings go through `i18n::t` with new catalog keys; missing translations fall back to English, so
  the i18n work is additive and does not block the feature.

### 4.7 Telemetry

Custom profiles report `game_id = UNKNOWN (0)`, unchanged wire format. The wire id table is untouched: no
`CUSTOM` entry is appended in P1.

**KNOWN LIMITATION: custom profiles are invisible in the metrics.** Every user profile collapses into the one
`UNKNOWN (0)` bucket, and that bucket already holds every unrecognised or future game, so a custom-profile
session is indistinguishable from a failed detection. No per-profile, per-key, or custom-versus-unknown split
exists in the data. That is a real gap in what we can see, and it is recorded as a limitation rather than
papered over.

We revisit that gap when either of these holds: (1) the owner needs adoption numbers for the feature, or (2)
the `UNKNOWN (0)` bucket has grown large enough that it obscures the built-in metrics we do rely on. The
remedy is already scoped: append `CUSTOM = 24` to the wire id table, a one-line change the existing id guards
cover, then map resolved user profiles onto it at the point the id is chosen. Until one of those conditions
fires, the bucket stays as it is, and P1 ships with no wire change at all.

**Privacy: opt-in aggregate latency only, and adoption is not a goal.** The telemetry stays exactly what
`[PRIVACY_STUB]` allows: an opt-in aggregate latency measurement, nothing keyed to a profile, a game, a
process name, or a destination. Putting the resolved profile key on the wire would be new data collection and
is out of bounds here. Adoption measurement is explicitly not an objective of this work. If a future owner
wants it, that is a deliberate product decision with its own privacy review, not something P1 slips in.

### 4.8 Docs and surfaces

- `docs/supported-games.md`, the README table, `web/index.html` counts, and the registry guards **do not
  move**: they describe built-ins. That is the core elegance of this design.
- `docs/CLI-REFERENCE.md` gains `--profiles` / `--no-user-profiles` and a "Custom games" section documenting
  the file, the schema and the precedence rule.
- `docs/user-guide.md` gains a "Add your own game" walkthrough; `docs/faq.md` a short entry.
- `docs/archive/feedback-triage.md` points at `docs/protocol.md` for a game-profile format that was never
  written there; the archive note should point at this document instead.
- `CHANGELOG.md` under `[Unreleased]`.

## 5. Design B: process-scoped interception

### 5.1 Target model

`InterceptorConfig` grows a target while keeping the existing fields working:

```rust
enum CaptureTarget {
    Ports { udp: (u16, u16), tcp: (u16, u16) },   // today's behaviour
    Process { names: Vec<String> },               // new: all traffic of the process
}
```

Mapping: a profile with `process_names` resolves to `Process` **when the platform supports it** (see the
matrix below), otherwise `Ports`; a profile with only ports resolves to `Ports`; Manual mode stays `Ports` +
explicit server seed.

**`InterceptorConfig` carries resolved flags, not display names.** Today it holds the display strings a
profile's name maps to (`dynamic_server_for_name`, `process_names_for_name`), which means a `String` lookup,
and a chance to miss, at the moment a filter is built. Here it carries the resolved values instead:
`dynamic_server: bool`, `transport`, and `process_names: Vec<String>` (or `Vec<&str>` borrowed from the
resolved profile). `resolve()` is what fills them; the filter builders just read them. A custom profile whose
plugin or game id has no built-in display name then can't silently fall out of a lookup, and an
`InterceptorConfig` built from a user profile behaves the same as one built from a built-in. The tests for
this are named for the case that matters: **a `dynamic_server = true` custom profile must not seed from
`initial_routes.first()`**, because a rotating title's first route is a lobby that moved on. Seeding there
would tunnel traffic at an address the match left minutes ago, which is the failure mode "port detection by
process was not working well" describes from the other side. It falls out of the flags above: the resolved
`dynamic_server` flag selects detection-only seeding inside the tracker, with no display-name lookup in the
path. Built-ins can adopt `Process` later in one line per game, because every built-in already declares
`process_names()`.

For `Ports`, a user-supplied range wins over the profile default: the Manual field first, then the profile's
`udp_ports`, then `ports()`. That is what finally makes the persisted "Custom Port Range" functional, and it
is the minimum change that turns the shipped-but-inert field into behaviour.

### 5.2 Flow attribution service

A new module (working name `interceptor/flows.rs`) owns one question: "which flows does this process own right
now". Attribution is separate from steering. This service answers the first; the data plane in 5.3 and 5.4 acts
on the answer.

**The Windows shape is a static network-layer filter plus userspace attribution.** The divert filter is built
once from the flow set and stays fixed until the set changes: today's `outbound and udp and (port range)`
shape with the tracked server addresses folded in. Who owns a flow comes from userspace table enumeration, not
from the filter and not from a flow-layer handle. Two earlier shapes are rejected here:

- **Flow-primary.** Driving the filter off WinDivert flow-layer events. An FLOW handle only reports activity it
  observes after it opens, so a flow the game established before the handle existed never raises an event and
  gets zero attribution. A boost started after the game launches - the common case - would see nothing. The
  FLOW layer stays useful, but only as an attribution source (below), never as the steering input.
- **Swap-on-flow-change.** Rebuilding the divert handle every time the flow set moves. WinDivert has no filter
  mutation API, so each rebuild is stop, close, reopen, and the gap that follows is a window where packets
  travel direct. Cheap for rare changes, unacceptable as the steady-state mechanism when the flow set churns.
  What survives from this shape is narrow: one rebuild for a target switch, or for a flow set that settles on
  new addresses. That is a rare reconfiguration, never a per-flow reaction.

**Attribution sources.** Two candidates carry the process id on Windows, and neither is weighted yet:

- **FLOW layer** (`Flow.ProcessId`): per-flow events with ProcessId, 5-tuple and endpoint ids. Accurate and
  event-driven for flows that start while the handle is open. Blind to pre-existing flows.
- **SOCKET layer** (`SOCKET_CONNECT` / `SOCKET_BIND`, both carrying ProcessId): reports the connection attempt
  before the first datagram leaves, so it sees flows no FLOW handle observed, including the ones that were
  already running when we attached.

Both are available in the pinned `windivert` 0.7.0-beta.4 dependency, so the choice is a design call, not a
dependency constraint. Neither is picked here: the B-3 benchmark decides, and that call locks only after the
numbers land. Whichever loses stays available as a secondary signal for the flows the other misses.

**Pre-existing flows.** Flows already established when the boost starts must be attributed anyway, because
boost-after-launch is the target case, not an edge. The FLOW handle opened at attach time cannot supply them,
and userspace enumeration is what fills the gap:

- `GetExtendedUdpTable` with the owner-PID class, walking sockets to PIDs. Two documented holes: the UDP table
  carries no remote endpoint, so it maps a PID to a local port and nothing more, and the same local port can be
  bound by more than one process. That is why the fallback is local-port plus the same N-packet debounce the
  multi-flow engine already uses: a port only locks to a process after the debounce confirms it.
- `GetExtendedTcpTable` with the owner-PID class for TCP, where the remote endpoint does exist.
- The WinDivert FLOW layer, opened early enough, adds what it can, and it fails open where it can't.

**Linux:** `/proc` inode map (already implemented in `process_scanner.rs`) plus ppid walking for launcher
children; `sock_diag` as a faster source later. **macOS:** `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` under the root
privilege the pf backend already requires.

**Process lifetime:** a bounded poll (500 ms while an interception is active) with per-OS identity (executable
path plus start time on Windows; pid plus starttime on Linux; libproc start time on macOS) to defeat PID reuse
and to catch launcher and anti-cheat children. No portable process-event API exists, so polling stays the v1
answer; per-OS events are a later optimization.

**Honesty rule.** A flow the scanner cannot attribute is reported as UNATTRIBUTED. Tag it, count it, show it.
Never fold it into the covered total, never let it read as handled by process mode. A boost that did not capture
what it claimed has to say so, and the coverage number has to be the one we can stand behind.

This service is also what the GUI's process picker lists (name, pid, path, running-flag) - one implementation,
two consumers.

### 5.3 Multi-flow engine

Today one server at a time is tracked (`order.rs` on Windows, `rotation.rs` on Linux/macOS). Process mode
generalizes both, Windows first, into one bounded and fail-open map:

- key: **flow identity**, the local and remote address pair. One entry per flow, not one per destination, so
  a game talking to the same server from two sockets gets two entries and neither steals the other's lock.
- value: an `order.rs` [`ServerTracker`](../../client/src/interceptor/order.rs) plus the flow's last-seen
  stamp. The tracker owns detection and lock state (`Detecting` to `Tunneled` to `Stale`) with the semantics
  already tested there: `effective_seeds` seeding from `initial_routes`, the N-packet debounce inside the
  detection window, the 5 s staleness timeout, and empty payloads never counting. All of it sits behind
  `observe(dst, now, payload_len)`, which is I/O-free and takes `now` explicitly, so the map's behaviour is
  deterministic under test on every platform.
- **`dynamic_server` rotation falls out of entry aging.** A rotating title (Fortnite is the example in
  `order.rs`) gets no seeds, so its entries start in detection and lock on whatever address the match is
  actually using. When the match moves, the old entry stops receiving packets, goes stale on the existing
  5 s timeout, and the next address locks through the normal debounce. No rotation-specific branch, no
  pre-seeded dead lobby IP: the aging that already exists is the rotation mechanism.
- one global bound, applied to the number of live entries, not to a second count of "tunneled" ones. An entry
  past the cap is not admitted, its flow passes through direct, and the app keeps working (fail-open). One
  number, one meaning: how many destinations this client may touch through the relay at once.
- **the cap is provably safe, not approximately safe.** The relay bans a source that reaches more than 10
distinct destinations in 10 s, and a ban lands on the source IP for 3600 s. Consequences land outside this
client: every concurrent built-in session on the machine is torn down, and every device behind the same NAT
is cut off with it. So the cap needs a proof, not a vibe. The shipped constant, its window, and the worst
case the client can reach through that window all have to sit below the relay threshold with headroom, and
the arithmetic has to be written down next to the constant.
- **the client cap is bound to the relay threshold by a cross-crate guard test.** The constant lives in the
client crate; the threshold lives in the relay. A comment cannot hold those two numbers together, because a
later edit to either one slides them past each other in silence. The guard test asserts the client's
worst-case destinations per window against the relay's threshold and **fails CI when the cap is raised past
it**. Mutation check: bump the client constant over the line, watch the guard fail, restore. If the relay
threshold ever moves, the same test moves with it, and it fails until the client follows.
- **the client warns before it gets close.** A single destination slipping over the cap would be a silent
ban for the user and everyone sharing their address, so the client cannot wait for the limit to actually
bind. As the tracked destination count approaches the cap, the status line raises a warning (the same
`interceptor_error` slot the rest of the flow state uses, at warning severity), naming the count, the cap,
and what a ban would cost. Approaching the cap is a visible state, not a log line.
- **v1 does not fix this at the protocol layer.** Moving the threshold into a protocol message (a rate
negotiation, a server-issued budget) was considered and rejected, for two reasons. It is three-crate scope
creep for a one-constant problem (client, protocol and relay all move), and it relocates the threshold into
the **client trust boundary**, which is where an abuser already sits. A number the client reports about
itself cannot be a guard. The relay keeps enforcing its own rule from its own observation, and the client
keeps proving it stays under it.
- **the cap is picked from a B-1 measurement, not from this page.** The benchmark below measures how many
destinations a real session reaches in a 10 s window, and that result sets the constant. How much headroom
the cap carries is likewise the benchmark's answer, not this document's. No number is fixed here except the
one that cannot move: the relay's 10-in-10 s line and its 3600 s ban.
- the tunnel already carries original addresses per packet (`[TRANSPARENCY_STUB]`), so multiplexing several
  flows through one session needs no wire change.
- proxy compatibility: covered by the two bullets above. The guard test is what keeps the cap on the safe
  side of the relay's rule; the plan leaves the relay guard untouched.

**Rejected because built-ins were treated as movable.** One alternative surfaced during review and is
recorded here so it is not re-proposed: make the built-ins data-driven (load the shipped 23 from the same
profile file a user profile uses, then fold custom profiles into the same store). It was rejected on the P1
premise itself. The premise of P1 is that **built-ins do not move**: `GAME_REGISTRY`, `detect_game`, the
two count guards, `docs/supported-games.md`, the README table and the website counts all stay as they are,
and that immobility is the whole reason the feature can ship in about a day. Data-driven built-ins invert
that. They put the 23 shipped games on the same footing as user entries, which means the count guards stop
meaning anything, the docs stop tracking a fixed list, and every built-in port and process-name regression
touches user-editable state. It also grows the surface an unsaved edit can break. The design keeps built-ins
fixed and user profiles additive; anything that needs the built-ins to move is a different project.

### 5.4 Platform matrix (what ships, and how)

| OS | Attribution | Data-plane steering | Real-time new flows | Verdict |
|----|-------------|---------------------|---------------------|---------|
| Windows | FLOW + SOCKET layer candidates, unweighted pending B-3; `GetExtendedTcpTable/UdpTable` for pre-existing flows | One static network-layer divert filter, rebuilt only on set change | Within one scan interval; FLOW/SOCKET events when live | Full support |
| Linux | /proc inode map + ppid | nftables named set of `ipv4_addr . inet_service`, refreshed from the scanner, replaced atomically with `rotation.rs` semantics | Within the refresh interval | Full support (root), gated by section 6.2 |
| macOS | libproc via `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)`, bounded by Full Disk Access | pf `rdr` rules per flow (anchor rebuilt on change) | Only after the first packets of a new flow | Best effort, documented; owner gate OQ4 |

**Windows detail.** Steering is one static network-layer divert handle. Its filter string lists the tunneled
flows (`outbound and udp and (ip.DstAddr == A and udp.DstPort == a or ...) and ip.DstAddr != <proxy>`), and no
part of it depends on a flow-layer event. A FLOW handle, if opened, runs as a sniffer only
(`set_recv_only().set_sniff()` in the crate) and feeds attribution, never the filter. WinDivert has no
filter-mutation API, so when the set has to change the sequence is stop recv, close, open with the new string.
Rebuilds are debounced (about 250 ms), and only fire for a target switch or a settled flow set, never per
observed flow. Two overlapping divert handles would double-deliver, which is why the swap is serialized rather
than dual-open. The first packets of a brand-new flow may go direct before attribution lands; for games that
is harmless (one or two connectionless datagrams), and it is the accepted trade for not sending *all* UDP
through user space. "Capture all UDP and filter in user space" remains the fallback mode if a game is found
that establishes flows faster than the poll can see them.

**Linux detail.** Primary mechanism is a named set of `ipv4_addr . inet_service` pairs, refreshed from the
scanner and replaced atomically. The scanner answers "which destinations does this process own right now"
(the same P2 service shipped for Windows in P3), and the answer becomes set elements. A refresh is a
single `nft replace element` (the atomic replacement primitive for set elements), so a rule evaluation
never sees a half-written set and no packet is dropped between the old and new contents.

Refreshes follow the conservative semantics `rotation.rs` already encodes, not a blind write of every scan:

- **Silence gate.** One scan that comes back empty does not authorise a teardown. The set only shrinks after
  the scanner is silent across a bounded confirmation window, derived from the same N-packet debounce the
  detection window uses. A game that is quiet for one poll does not lose its rule.
- **Consecutive confirmations.** Every element added or removed is confirmed by consecutive scans before it
  changes. A destination that appears once does not lock; a destination that disappears once does not unlock.
  This is what stops a churning scanner from churning the ruleset.
- **Teardown after empty scans.** Repeated empty scans retire the set and the rules that reference it,
  returning the box to a clean state. That is the terminal transition, and it is never entered from a single
  miss.
- **Atomic element replace.** Additions and removals go through `nft replace element`, so the set is never
  observed partially applied.

The probe fwmark exemption and the relay-address exclusion are preserved unchanged.

**cgroup v2 is a later opt-in experiment, not the primary path.** It was the leading candidate until two
things ruled it out for v1. First, the `socket cgroupv2` expression attributes a socket at creation time, so
moving a process into the leaf mid-session re-homes nothing: the sockets that already exist keep the cgroup
they were born in, and the game stays uncaptured until it happens to open a new one. A scan-and-refresh set
has no such blind spot, because it tracks destinations rather than origin cgroups. Second, a leaf created by
root sits outside systemd's management. The tool that owns the session's cgroup tree does not know the leaf
exists, so the placement can be undone or contested by anything that does know, and the state becomes an
out-of-band artifact on the user's machine. Neither reason is fatal on its own, which is why this is deferred
rather than deleted: an owner who wants to benchmark cgroup v2 on a kernel that supports it, on a machine
where the systemd interaction is acceptable, can run it as an opt-in experiment after the set path ships.

**Ethics note.** `[ETHICS_STUB]` bounds what any of this may touch. Process mode may observe the target's
own flows and steer them through the relay; it must not change the environment a protected game can see.
If entering a cgroup, changing a device namespace, or loading an inspection hook alters what the game reads
back about its own process, that is a change to a protected game's observable environment, and it's out of
bounds regardless of how well the mechanism performs. Anti-cheat sees an unreconfigured host or the feature
does not ship. This is the reason the set path, which changes only firewall state, wins over every
process-modifying alternative.

**macOS detail.** pf cannot match a PID, so the anchor gets one `rdr` rule per tracked flow off the
client's current flows, refreshed on a short interval (500 ms to 1 s). Established flows are captured; the
first packets of a brand-new flow may go direct. That is the honest limit of what $0 and no kext can do on
macOS; the doc for this feature says so instead of implying parity.

The attribution side is bounded by two grants, and only one of them is cheap. Reading your own process's
sockets needs no grant at all. Reading any other process's sockets through `proc_pidfdinfo` requires the
`com.apple.system-task-ports` entitlement, and that entitlement is issued by Apple to signed,
notarised, approved software, which is the wall the paid account sits behind. **Full Disk Access does not
substitute for it.** Full Disk Access is a TCC privacy grant over files and volumes; it opens paths, not
other processes' file descriptors, and an admin account does not grant it either. Admin gets you `sudo` and
the ability to write `/etc/pf.conf`, which is the pf side of this design and nothing more. So the honest
bound is: pf side needs root (already required by the existing backend); attribution side needs Full Disk
Access for whatever the storage and log paths require, plus the system-task-ports entitlement for other
processes' sockets, and we can't get the latter without the paid program that `[COST_STUB]` rules out.

**Prior art.** Mullvad's macOS split tunneling ran into exactly this wall and answered it with a privileged
helper built on `eslogger` (the EndpointSecurity log tool) plus Full Disk Access, at macOS 13 and later; the
entitlement work lives in their own signing and approval pipeline. Their tree is the reference for what is
possible on a stock Mac, and it stops short of the per-app steering a Network Extension would give. We cite
it for the shape of the helper, the `eslogger` usage, and the Full Disk Access requirement, not as a path to
parity. Reaching parity means a NETransparentProxyProvider and the paid Apple account behind it, and the
cost stub says no. Parity is therefore not on the table, and macOS process mode ships only if the owner
accepts that gap.

**Security and privilege framing.** The attribution helper is expected to ask for Full Disk Access, following
the Mullvad prior art, because the scan reads paths that TCC protects. Full Disk Access is a privacy grant
and not a capability, and it does not reach another process's sockets at all; the system-task-ports
entitlement is what does, and only Apple's signing pipeline hands it out. Admin, separately, is what the pf
side needs and is not either of those. Presenting the helper as "needs Full Disk Access" is therefore honest
about the file side and misleading about the socket side unless the entitlement is named too, and the UI copy
should say both.

**Owner gate OQ4.** macOS process mode does not start on the strength of this page. It starts when the owner
rules on **OQ4**: accept the documented best-effort ceiling (pf-steered, first packets of a new flow may go
direct, attribution bounded by system-task-ports) and ship it, or hold the platform at port-set parity with
the attribution feature deferred until the entitlement question is reopened. The line for OQ4, and the
recommended answer, are in section 8.

**TCP.** Process mode ships UDP-first. TCP in process mode is Phase 5: the Windows terminator already exists
(`capture/windivert_redirect.rs`, `docs/tcp-tunnel-design.md`) and would be generalized to multi-flow; Linux
and macOS would redirect TCP to a local listener speaking the same v5 datagram path. The capture-time race
(a SYN must be caught before the handshake completes) is the known hard part and is called out rather than
hand-waved.

### 5.5 Safety policy for process mode

- public IPv4 destinations only (`is_public_ipv4`, already the rule everywhere);
- denylist by default: DNS (53), DHCP (67/68), mDNS (5353), SSDP (1900), NetBIOS (137-139) - relaying these
  through a game relay is wrong even for "all traffic";
- the relay's own address stays excluded in every filter builder (asserted by test, as today);
- fail-open: anything unattributable or over the cap goes direct; the game is never broken by our bookkeeping;
- the bypass gate and telemetry policy are unchanged;
- no injection (`LD_PRELOAD`, DLL injection), no driver, no GPL/commercial SDK code: studied as prior art only.

### 5.6 UI for ask 3

"Target a running app" in the Manual/custom flow (and on a profile): pick from the live process list or browse
an `.exe`; the status column shows "Target: MyGame.exe (PID 1234) - 3 flows via <backend>"; errors surface in
the same `interceptor_error` slot as today. The status line reports what was actually attributed: `3 flows via
<backend>`, plus `2 UNATTRIBUTED` when the scanner could not place a flow. Marking a process is the explicit
consent gate; nothing is intercepted without it.

**Where the picker's process list comes from is an open owner gate (OQ5), and the default is the small version.**
What's pending is only the *scope* of the scanner, not whether ask 3 works. The default that ships is a minimal
Windows scanner extension: enumerate running processes, read each one's image path, and establish per-process
identity from it plus the start time, the one tuple that survives a PID being recycled by an unrelated process
and that lets the list survive a refresh. That's the same identity the attribution service already needs, so the
picker and the engine share one source and the picker tests what the engine will actually see.

**Nothing on this path is polled on the egui thread.** The scanner is already shell-based and synchronous today,
which is one of the reasons it misbehaves; here it runs off the UI thread on its own timer with a bounded
interval, and the frame loop only ever reads the last published snapshot. A pick list that pauses the UI on
`tasklist` is not a pick list anybody keeps. The larger scanner options - native Win32 table enumeration,
launcher/child-process trees, and the cross-platform variants - stay behind OQ5 and are revisited only if the
minimal scanner proves insufficient. Section 6.2's Windows field gate applies to this list the same way it
applies to the engine: nothing about it is claimed working until a real game has been targeted through it.

## 6. Phasing

| Phase | Deliverable | Answers | Size | Risk |
|-------|-------------|---------|------|------|
| P1 | Profiles + saved servers: schema, paths, resolve(), CLI flags, GUI editor/dropdowns, docs, tests; wire the custom range into the target | Ask 2, and the range finally taking effect | ~1 day | Low |
| P3 | Windows process mode: static divert filter with userspace attribution (FLOW/SOCKET choice locked by B-3), multi-flow engine, port-scoped rebuilds; built-ins can switch to Process target | Ask 3 on Windows | ~2-3 days | Medium (attribution on pre-existing flows) |
| P4 | Linux process mode: scanner-refreshed nftables named set (section 5.4); cgroup v2 stays a later opt-in experiment | Ask 3 on Linux | ~2 days | Medium (kernel variance) |
| P5 | macOS flow-set pf + TCP process mode + polish | Parity, best effort | ~2 days | High on macOS, by nature |

### 6.1 The v1 boundary

v1 is P1 plus P3, and nothing else: user profiles, saved servers, and the custom port range wired into the
target (P1), then one minimal single-flow Windows process mode (P3). A player points the client at a single
Windows process, the picker lists it, its one live flow is attributed and steered, and everything unattributed
goes direct. That is the whole v1 process feature.

The cross-platform attribution service that earlier drafts put at P2 is **out of v1**. So are Linux and macOS
process mode. Neither is deleted: both move behind the gate below, because the expensive part of this feature
isn't building the scanner. It's discovering whether process-targeted interception of a live game is safe to
ship at all, and Windows is where we can learn that at zero cost and in days rather than weeks.

Why the service went first in the old shape, and why that ordering was wrong: it was groundwork sized for
three platforms before any of them had proved the concept on one. It's also the piece with the most open
questions (native API choice, child-process walking, PID-reuse identity) and the least value on its own.
A P3 that ships as one flow on Windows can answer those questions with real packets; a service built ahead of
them answers them with guesses.

### 6.2 The Windows field gate

P4 (Linux) and P5 (macOS) **begin only after** two things are true:

1. the Windows phase (P3) has shipped, and
2. one real Windows game is **field-verified**: a human runs the boost-after-launch pattern against a live
   game as admin and observes that attribution actually landed on the game's flow, with **zero relay bans**
   over a defined window.

Both halves matter. Attribution observed alone proves the mechanism; zero bans over the window proves the
abuse guard holds with a process-scoped target. A pass on one and not the other means the gate stays shut.
The result gets recorded, pass or fail, in the decision log with the game, the build and the window.

There's no partial credit and no "start Linux in parallel to save time". If field verification fails, the
next work is fixing Windows, not porting it. A mechanism that doesn't hold on the primary platform won't hold
on the other two, where the process-visibility story is strictly worse: nftables and pf can't match a PID at
all, so P4 and P5 have to lean harder on polling and per-flow rules than the Windows shape does.

### 6.3 Phase table

| Phase | Deliverable | Answers | Size | Risk |
|-------|-------------|---------|------|------|
| P1 | Profiles + saved servers: schema, paths, `resolve()`, CLI flags, GUI editor/dropdowns, docs, tests; wire the custom range into the target | Ask 2, and the range finally taking effect | ~1 day | Low |
| P3 (v1) | Minimal Windows process mode: one flow at a time, static divert filter with userspace attribution (the FLOW vs SOCKET call locked by B-3), the bounded flow map, port-scoped rebuilds | Ask 3 on Windows | ~2-3 days | Medium (attribution on pre-existing flows) |
| P4 (gated) | Linux process mode: scanner-refreshed nftables named set; cgroup v2 stays a later opt-in experiment | Ask 3 on Linux | ~2 days | Medium (kernel variance) |
| P5 (gated) | macOS flow-set pf, TCP process mode, polish | Parity, best effort | ~2 days | High on macOS, by nature |

The old P2 is listed with them only as a status marker: **deferred out of v1**, to be revived after the field
gate if the multi-platform scanner is still worth its cost.

| Phase | Status |
|-------|--------|
| P2, cross-platform attribution service | Deferred out of v1. Not before the field gate; a minimal Windows-only scanner is what P3 needs. |

Each phase still ships on its own. P1 closes the discussion's first two asks by itself, and P3 closes the
third on the primary platform. P4 and P5 are the same work with worse primitives, done once the gate above
says the idea survived contact with a real game.

## 7. Verification

Repo discipline applies to every phase: `cargo fmt --all --check`, clippy with `-Dwarnings`, and the full
crate suites (never a module filter), on the ubuntu and windows CI jobs; new guards are mutation-tested
(break it, watch it fail, restore).

**Two measurements gate the Windows default.** Neither is a CI test, and both are prerequisites: the Windows
process mode does not become the default target until they have run and the numbers are recorded in
`wat/state/decisions.md` with the build, the game and the window. Available evidence first, default second.

1. **The before/after latency gate.** Instrument the interception path and measure round-trip latency with
   process mode off and with it on, same machine, same game, same session length. If steering a real
game's flow through user space costs more latency than the boost recovers, process mode is a regression and
   does not ship as the default. "Feels fine" is not the gate; the two numbers are.
2. **The capture-all-UDP throughput benchmark.** The fallback mode in section 5.4 (capture all UDP, filter in
   user space) is the answer for a game whose flows outrun the poll. Before that fallback can be adopted, or
   before process mode is locked as default, measure what capturing all UDP actually costs: maximum
   sustained throughput, packet loss under load, and CPU at the measured rate. Broadcast, mDNS and other
   background UDP hit this path too, so the benchmark includes an idle machine, not just a game session.

Until both land, process mode stays opt-in behind the picker and the Manual/custom flow, exactly as the
section 5.6 consent gate describes.

- **P1:** schema parse/round-trip and validation tests (collisions, ranges, caps, malformed entries);
  resolution precedence tests; a guard that a user profile never enters `GAME_REGISTRY` counts
  (mutation: add one to the registry list, watch the count guard fail, restore); GUI config integration
  tests next to the existing `manual_*` round-trip; a test that a user range reaches `InterceptorConfig`
  (the `let _ =` at `app.rs:651` must not survive P1); a test that the resolved id for a user profile is
  `UNKNOWN (0)`, so the section 4.7 limitation cannot drift into an accidental wire change; `--list-games`
  output test; manual check of the real surface (`lightspeed --list-games`, `--game my-game --dry-run`, GUI
  editor).
- **P1, the guard test:** a cross-crate guard binding the client destination cap to the relay's abuse
  threshold, so a raised cap fails CI instead of sliding under the ban line, mutation-tested like the other
  guards. It lives where both numbers are visible, and it carries the arithmetic for the worst case the
  client can reach in the relay's 10 s window.
- **P3:** the flow table is a pure state machine with the `order.rs` test style (seeded, debounced, stale,
  capped, fail-open); filter-builder string tests assert the proxy exclusion and flow clauses; the divert
  swap is exercised on Windows CI for compilation and unit-level logic, and by a documented manual QA pass
  (admin + a real game) that cannot run in CI - state that gap, do not imply coverage. The same manual pass
  is what feeds the latency gate and the throughput benchmark above.
- **P4:** nft script generation tests in the existing string-assertion style (set element add and replace,
  silence gate, consecutive confirmations, teardown after empty scans); a root-gated integration test on the
  Linux CI runner where possible.
- **P5:** pf anchor generation tests; TCP terminator tests extended for multi-flow; macOS smoke runs in CI
  cover compilation only.

## 8. Decisions to confirm (owner)

1. **Storage**: profiles in a per-user `profiles.toml` (recommended) versus a section in `lightspeed.toml`.
2. **Scope**: any user-marked process (recommended, with the public-destination rule, the denylist and the
   8-flow cap) versus games-only.
3. **macOS scope (OQ4)**: accept best-effort runtime steering on the flow set, on the honest bound stated in
   section 5.4 (pf needs root as it does today; attributing another process's sockets needs Full Disk Access
   plus the `com.apple.system-task-ports` entitlement, and administrator rights grant neither of those)
   versus holding macOS at port-set parity with process mode undecided. Recommended: accept the best-effort
   bound. The alternative isn't a stronger macOS implementation, it's macOS process mode waiting on an
   entitlement we have no path to acquire under `[COST_STUB]`, and the cost stub is not being reopened for
   this.
4. **Discussion**: post a progress reply to #146 pointing at P1 once it lands, or keep it silent until then.
5. **Process picker scope (OQ5)**: ship the minimal Windows scanner extension (running processes, image path,
   start-time identity, snapshots published off the UI thread) versus building out the larger scanner now
   (native Win32 table enumeration, launcher and anti-cheat child trees, the Linux and macOS variants).
   Recommended: the minimal extension. It is the identity the attribution service already needs, it tests what
   the engine will actually see, and the larger scanner only earns its cost once a real game has shown the
   small one short. Section 5.6 says what the default is and why nothing here runs on the egui thread.

## 9. References

WinDivert documentation (flow/socket layers, `WINDIVERT_DATA_FLOW`, `Flow.ProcessId`);
<https://reqrypt.org/windivert-doc.html>. Microsoft `GetExtendedTcpTable` / `GetExtendedUdpTable` owner-PID
classes; <https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable>.
Mullvad split tunneling: Windows WFP driver keyed by image + PID registry
(<https://github.com/mullvad/win-split-tunnel>), Linux cgroup v2 + nft `socket cgroupv2`
(<https://github.com/mullvad/mullvadvpn-app/tree/main/talpid-cgroup>), macOS EndpointSecurity via `eslogger`
with Full Disk Access, macOS 13 and later
(<https://github.com/mullvad/mullvadvpn-app/tree/main/talpid-core/src/split_tunnel/macos>).
Apple EndpointSecurity framework, the entitlement and TCC context behind `eslogger`;
<https://developer.apple.com/documentation/endpointsecurity>.
Apple `com.apple.system-task-ports` entitlement (signed and issued by Apple; Full Disk Access and admin
rights grant neither);
<https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.system-task-ports>.
nftables sets and atomic element replacement, the prior art behind the primary Linux mechanism;
<https://netfilter.org/projects/nftables/manpage.html>.
The `[ETHICS_STUB]` rule for protected games (no observable environment change) is defined in `wat/rules.md`.
sing-box process attribution (<https://github.com/SagerNet/sing-box/tree/main/common/process>).
Rust WinDivert FLOW/SOCKET attribution precedent
(<https://github.com/reffer-191/bandwidth-limiter>). Linux `sock_diag(7)`
(<https://man7.org/linux/man-pages/man7/sock_diag.7.html>), `pidfd_open(2)`
(<https://man7.org/linux/man-pages/man2/pidfd_open.2.html>). macOS `proc_info.h`
(<https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h>). Apple Network Extension
(<https://developer.apple.com/documentation/networkextension>).
