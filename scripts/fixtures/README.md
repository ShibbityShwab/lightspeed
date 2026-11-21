# Loopback starvation fixture (WF-081b evidence)

Reproduces the real `NO RETURN PATH` / `RelayStarved` state on this Windows host
with the **shipped binaries only**, and produces the live window captures and
accessibility evidence that retired part of the WF-081b U list.

## The chain (nothing faked in the product)

```
synthetic game double (renamed powershell host "ROB RUN.exe", UDP every 100 ms)
   -> GUI presence scanner matches the built-in ROB RUN profile (no UI clicks)
   -> real engine + real WinDivert interceptor (needs an elevated shell)
   -> real QUIC control registration to the shipped lightspeed-proxy on 127.0.0.1
   -> tunnel packets reach the proxy; its allowlist refuses the fake destination
   -> the proxy returns nothing; the real no-reply watchdog raises
      "Relay return path is starving the session" after two 8 s windows
   -> window: NO RETURN PATH + warning line; tray: RelayStarved tooltip
```

The **warning state, counters, watchdog, route surface and tray mapping are the
shipped code paths**. Only the game process and the relay's destination policy
are fixture inputs.

## Network posture: loopback only

- The proxy is bound to `127.0.0.1` only. Control `4433` (the client's
  registration port is a compile-time constant), data `24434`, health `18080`.
  **Port 8080 is never bound** — other suites expect it free.
- The fake game server is a **public** literal (the client's interceptor
  deliberately never captures non-public destinations). During the run the
  script installs a `/32` loopback blackhole route for it, so the GUI's direct
  ping/traceroute cannot leave the host, and the relay's allowlist refuses to
  forward to it, so nothing is emitted toward it either.
- The GUI's registry-discovery HTTPS is blocked by a program-scoped outbound
  firewall rule (TCP/443 for `lightspeed-gui.exe`); the run log shows
  `Relay discovery failed … os error 10013` and the saved env relay is used.
- Both guards (route + firewall rule) and all listeners are removed by
  `-Action stop`; the script also backs up/restores the user's GUI config and
  relay history, and removes the WinDivert DLL/SYS copies it placed next to
  the debug binary.

## Usage

```powershell
# elevated shell, from the repo root; build first:
cargo build -p lightspeed-gui ; cargo build -p lightspeed-proxy --features quic

powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fixtures\starvation-fixture.ps1 `
  -Action start -Language en -EvidenceDir <evidence>\run
#   wait for the state (the script's Wait-Starved reads the live UIA text; the
#   log marker is "Relay return path is starving the session")
powershell ... -Action capture -Language en -EvidenceDir <evidence>\run
powershell ... -Action probe   -Language en -EvidenceDir <evidence>\run
powershell ... -Action stop
```

## What the evidence shows (see `wat/evidence/2026-11-21/wf081b-evidence/`)

| Evidence | Result |
|---|---|
| Window captures 320×420 and 320×760, EN/DE/RU | `run/window-320x{420,760}-{en,de,ru}-starved.png`, exact client sizes, warning line + localized state word (`NO RETURN PATH` / `Kein Rückweg` / `Нет возврата`) |
| UIA text rows per language/size | `run/ui-text-<lang>-<size>.txt` |
| Live relay/watchdog counters | `run/gui-*-starved.log` (`intercepted≈70/window`, `from_relay=0`, `streak≥2`, `fault=NoReplyFromRelay`) |
| Tray fifth state | UIA name of the notification-area icon during the state: `⚡ LightSpeed — relay is not returning your game traffic`; `run/tray-*` captures |
| Chrome measurement | `run/chrome-measurement.json`; method `scripts/fixtures/measure-chrome.py` |
| Shortcut probes | `run/probe-evidence.json`, `run/shortcut-probes-*.json` |

Known limits (recorded, not hidden): another agent's GUI may run on this shared
desktop/log; captures are attributed to the fixture's own PID via UIA/window
handles and the log excerpts are filtered by the loopback relay/server tuple.
The tray context menu did not surface in either the right-click or Apps-key
automation attempt (the menu is a tray-thread HMENU popup); the live menu capture
therefore remains U. The `NO RETURN PATH` background uses the warning frame with
honest English fallback copy in DE/RU (Q5 translation debt).

**Cleanup contract:** `-Action stop` removes the listeners, route, firewall rule,
WinDivert copies, and restores the user's config/log state. Never leave the
fixture running: the relay's control port is a shared-host resource.
