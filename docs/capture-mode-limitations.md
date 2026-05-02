# Capture Mode — Known Limitations

> **Status:** Current as of v0.4.x (pcap-based capture). Superseded once WF-009 (WinDivert redirect) is complete.

---

## 1. pcap captures traffic in both directions

### What happens

pcap's BPF filter matches on **port numbers**, not packet direction. A filter like `udp port 28015` captures:

| Direction | Source | Destination | Should tunnel? |
|-----------|--------|-------------|----------------|
| **Outbound** (game → server) | `192.168.x.x:PORT` | `SERVER:28015` | ✅ Yes |
| **Inbound** (server → game) | `SERVER:28015` | `192.168.x.x:PORT` | ❌ No |

### Why inbound packets must be filtered

If an inbound packet is forwarded through the tunnel, the proxy receives it with `orig_dst_addr = 192.168.x.x` (an RFC1918 private address). The proxy's **PrivateDestination** abuse check drops any packet destined for a private IP, because a legitimate game server is always a public IP.

Without filtering, approximately **50% of all tunneled packets are silently dropped** at the proxy — matching exactly the bidirectional capture ratio observed in production logs:

```
sessions=1  packets_relayed=118726  packets_dropped=112702   # ~49% drop
```

### Fix (implemented)

`capture_mode.rs` now calls `is_private_ipv4(*pkt.dst.ip())` immediately on every captured packet and `continue`s (skips) those with a private destination IP. Only true outbound packets — those destined for a public game-server IP — are forwarded through the tunnel.

---

## 2. Windows Defender Firewall blocks tunnel inbound

### What happens

The tunnel socket binds to `0.0.0.0:0` (an ephemeral port chosen by the OS). On Windows, **Windows Defender Firewall** blocks all unsolicited inbound UDP on ephemeral ports by default — even when the application already sent an outbound packet to that address (which would normally create a NAT mapping).

This means proxy responses arrive at the NIC but are silently dropped by the firewall before they reach the tunnel socket. The result: `packets_from_proxy` stays at **0** in the GUI, even though the proxy logs show `packets_relayed` incrementing normally.

### Evidence

- Proxy side shows sessions = 1 and packets_relayed going up → proxy IS sending responses
- Client side shows packets_from_proxy = 0 → responses are not reaching the socket
- Root cause: Windows Firewall drops the inbound UDP before the Rust application sees it

### Fix (implemented)

`capture_mode.rs` now calls `add_firewall_rule()` immediately after capture starts, which runs:

```
netsh advfirewall firewall add rule
    name="LightSpeed Tunnel Inbound"
    protocol=UDP  dir=in  action=allow
    program=<path-to-lightspeed-gui.exe>
```

The rule is **removed** on shutdown via `remove_firewall_rule()`.

### Requirements

- The process must be running as **Administrator** (the GUI already requires this for pcap raw socket access).
- The rule is scoped to the specific executable, minimising the firewall surface.

---

## 3. pcap cannot redirect traffic — in-game ping is unaffected

### What happens

pcap is a **passive observer**. It receives a *copy* of each packet; the original packet is not intercepted or dropped. This means:

- The game client sends packets directly to the game server (original path).
- LightSpeed simultaneously tunnels a *copy* through the proxy (parallel path).
- The game client's active session still uses the **direct path**.
- **In-game ping reflects the direct path**, not the tunnel path.

The GUI's RTT display shows the **tunnel round-trip** (client → proxy → game server → proxy → client), which is typically shorter than the direct ISP path. But because the game ignores the injected packets and continues receiving on the direct path, in-game ping shows the direct-path latency.

This is why a user observed:
> *"The ping in game says like 309 when the ping in the GUI client says like 209."*

The 100ms gap is the difference between the direct-path RTT (309ms, shown in-game) and the tunnel RTT (209ms, shown in GUI). The tunnel IS faster — but the game never uses it.

### What is needed for actual ping improvement

To redirect traffic so the game uses the tunnel path, you need a mechanism that **intercepts** packets before they reach the network stack:

| OS | Mechanism | Status |
|----|-----------|--------|
| Windows | **WinDivert** | Planned — WF-009 |
| Linux | `nftables` / `iptables REDIRECT` | Future |

Until WF-009 is implemented, capture mode provides **measurement and verification** of the tunnel path — it proves the proxy routing is faster, but does not yet deliver that speed improvement to the game session.

---

## Summary Table

| Limitation | Root Cause | Fix Status |
|------------|-----------|------------|
| ~50% proxy drops | pcap captures both directions; inbound dst = private IP fails proxy PrivateDestination check | ✅ Fixed — `is_private_ipv4` filter |
| `packets_from_proxy = 0` | Windows Firewall blocks inbound UDP on ephemeral port | ✅ Fixed — `netsh` firewall rule on start/stop |
| In-game ping unchanged | pcap is passive; game session uses direct path | 🔲 Requires WF-009 (WinDivert) |

---

## Related

- `client/src/modes/capture_mode.rs` — implementation with inline doc comments
- `proxy/src/abuse.rs` — PrivateDestination check
- `wat/workflows.md` — WF-009 (WinDivert redirect, future)
- `docs/architecture.md` — overall system design
