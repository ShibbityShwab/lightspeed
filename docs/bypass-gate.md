# Do-No-Harm Bypass Gate

The client picks the relay nearest to itself, then tunnels unconditionally. For
domestic sessions that relay is often a continent away from the game server, so
the TUNNEL is slower than connecting directly (the first like-for-like sample
was -56 ms). The bypass gate fixes that: it tunnels a session only when the
relay actually helps, and otherwise lets the game connect directly.

## Default: dry_run

The gate ships in `dry_run` mode. It measures the like-for-like advantage and
logs the decision it would make, and **changes nothing**. Tunneling behavior is
identical to before.

This default is deliberate. On Linux (nftables/iptables) and macOS (pfctl) the
redirect is a NAT rule, and deleting it does not unhook conntrack. A mid-flow
teardown would blackhole the game. A decision made in the first release could
not be safely retracted, so the safe default is to make no decision at all.

## Modes

Set `[route] bypass` in `lightspeed.toml`, or use a CLI flag. CLI wins.

| Mode | CLI flag | Behavior |
|---|---|---|
| `dry_run` (default) | `--bypass-dry-run` | Measure and log, change nothing. |
| `auto` | (config only) | Act only where it is safe (see below). |
| `never` | `--no-bypass` | Always tunnel. Complete rollback. |
| `always` | `--force-direct` | Never tunnel. Testing only. |

Tuning keys, all in `[route]`: `bypass_margin_ms` (default 8),
`bypass_keep_ms` (default 5), `bypass_dwell_s` (default 120),
`bypass_probation_s` (default 20).

## What `auto` does, per platform

- **Linux (diverting):** `Direct` means *do not install the redirect rule*. It
  is decided before capture starts, so the gate never tears down a live flow. A
  server already on the relay stays on it until the flow ends; the per-server
  preference applies on the next install. The tier-1 pre-gate receives the live
  client-to-relay RTT from the interceptor's own 5-second keepalive: a fresh
  sample (within `CLIENT_RELAY_TTL`, 15 s) lets `auto` compare the first hop
  against the direct path and decline the redirect when the first hop alone
  already costs as much as the whole direct route. When keepalives stop, or the
  relay is swapped before a new sample lands, the value goes stale and the gate
  installs the redirect as usual. This is the input the pre-gate was designed
  for; it is now supplied on Linux.
- **Windows (non-diverting):** the original packet is only held, so a per-packet
  switch is safe. This wiring lands after the gate's dry-run data is collected.
- **macOS** currently has no shadow sampler and its interceptor does not consult
  the bypass gate, so it always keeps the relay. This is a known gap, not a
  silent decision.

## Safety properties

- Fails open to the relay on any missing sample, unhealthy relay, or config
  parse failure. A bug degrades to today's behavior, never to a new one.
- Asymmetric hysteresis: it takes more evidence to leave the relay (-8 ms) than
  to keep it (+5 ms).
- Two agreeing evaluation windows, an EMA, a dwell time, and a hard cap of two
  flips per server per session. It cannot thrash mid-match.
- The tier-1 pre-gate is a sufficient condition: if the client-to-relay first
  hop alone already costs as much as the whole direct path, the tunnel cannot
  win. It is the only check safe before a rule exists.
- The pre-gate's client-to-relay input is time-bounded: a keepalive sample older
  than 15 s is discarded, and a missing or stale value leaves the pre-gate inert
  rather than comparing old data. It is never fed a relayed end-to-end RTT,
  which would not be a first-hop cost.

## Counters and logs

`EngineStatus` and `InterceptorStats` expose `bypass_allowed`,
`bypass_refused`, `bypass_decisions`, `bypass_flips`, and
`bypass_pre_gate_rtt`. The gate logs each decision with the server, state,
computed action, smoothed advantage, reason, and whether it was a dry run.
`bypass_pre_gate_rtt` counts pre-gate evaluations that received a real
client-to-relay RTT, so an operator can confirm the gate is getting live input
rather than failing open; a debug log records the same call with both inputs.

## Rollback

- `--no-bypass` or `[route] bypass = "never"` is the complete rollback. No code
  change, no rebuild.
- `--bypass-dry-run` returns to the shipped default.
- The feature is non-destructive: it only declines to install a rule or stops
  sending to the relay. Nothing about the tunnel, protocol, or relay changes.
