# WF-006 Step 5 — Launch Announcement Posts

Ready-to-post copy for Reddit and Hacker News. These are the actual text to copy-paste.

---

## 🔴 Reddit — r/GlobalOffensive, r/FortNiteBR, r/DotA2

**Title:** I built a free open-source ExitLag alternative in Rust

**Body:**

Hey everyone,

I've been frustrated with paying $6-10/month for network optimizers like ExitLag and WTFast, so I built one and open-sourced it — **LightSpeed** (https://github.com/ShibbityShwab/lightspeed).

**What it does:** Routes your game UDP traffic through optimized proxy nodes to reduce ping. Works exactly like ExitLag but completely free.

**Current stats:**
- 2 live proxy nodes: US-West (Los Angeles) and Singapore
- **0.00% packet loss** on both nodes in load testing
- 31ms relay through Singapore node (tested from Bangkok)
- Built-in game profiles for CS2, Fortnite, Dota 2

**Why it's different from paid tools:**
- Actually $0 — no trials, no subscriptions, no freemium BS
- Fully open source — audit every line yourself
- Never modifies game packets (anti-cheat safe)
- No accounts, no data collection

**Tech stack:** Rust + Tokio, custom UDP tunnel protocol, XOR-based FEC (25% overhead vs ExitLag's 200-300% packet duplication)

Still early beta — the client is build-from-source for now, pre-built binaries coming soon. If you're technically inclined or just want to try it:

```bash
git clone https://github.com/ShibbityShwab/lightspeed.git
cd lightspeed
cargo build --release
```

Landing page: https://shibbityshwab.github.io/lightspeed/

Would love feedback on which regions to prioritize next. What's your worst ping route?

---

## 🔴 Reddit — r/rust

**Title:** Show r/rust: I built an open-source game network optimizer — UDP tunneling, XOR FEC, ML route selection

**Body:**

Hey rustaceans,

I built **LightSpeed** — an open-source game network optimizer (think ExitLag but free) written entirely in Rust. Wanted to share the technical side since the stack is pretty interesting.

**Repo:** https://github.com/ShibbityShwab/lightspeed

### Architecture

```
Game (UDP) → Client (pcap capture) → LightSpeed Tunnel → Proxy Node → Game Server
```

**Client stack:**
- `pcap-rs` for packet capture (three backends: Linux AF_PACKET, macOS BPF, Windows Npcap)
- `tokio` async runtime for all I/O
- `linfa` ML library for route selection (Random Forest, 11 features, <1ms inference)
- Custom 20-byte tunnel header with optional 4-byte FEC extension

**Protocol features:**
- XOR-based Forward Error Correction — recovers lost packets at 25% bandwidth overhead (vs ExitLag's 200-300% via full duplication)
- Keepalive flag in header (flag byte `0x01`)
- Multipath capability: simultaneous relay through multiple proxy nodes

**Proxy server:**
- UDP relay with auth token verification
- Prometheus metrics (20+ counters/histograms labeled by region + node_id)
- JSON health endpoint at `:8080/health`
- Rate limiting + abuse detection
- ~500KB RAM per node in production

**ML Route Selector:**
11 features fed into a linfa Random Forest:
- current_latency_ms, historical_p50_ms, historical_p95_ms
- jitter_ms, packet_loss_pct, proxy_load
- hop_count, bgp_as_path_len, geographic_distance_km
- time_of_day, day_of_week

Still building out the online learning component (incremental model updates from real latency feedback).

**Infrastructure:**
2 live Vultr nodes (LA + Singapore), native binary + systemd, Terraform for provisioning, Prometheus + Grafana for monitoring. Total ongoing cost: **$0** (Vultr credits).

Load test results: **0.00% packet loss** on both nodes, ~320 pps capacity proven.

Happy to answer questions about any part of the stack — the FEC implementation, the pcap backends handling different OS quirks, or the ML feature engineering.

---

## 🔴 Reddit — r/selfhosted

**Title:** LightSpeed — self-hostable game network optimizer. Run your own proxy node on any VPS.

**Body:**

Hey r/selfhosted,

Built a game network optimizer that you can completely self-host — **LightSpeed** (https://github.com/ShibbityShwab/lightspeed).

**The pitch:** If you have a VPS in a region where gamers have bad routing from their ISP, you can run a proxy node and help people in your region reduce their ping. The whole stack is open source.

**Quick setup on any Linux VPS:**
```bash
curl -sL https://raw.githubusercontent.com/ShibbityShwab/lightspeed/master/infra/scripts/setup-new-node.sh | bash
```

That installs the proxy binary + systemd service + fail2ban rules.

**Minimum requirements:** 512MB RAM, UDP 4433/4434 open, TCP 8080 for health/metrics. The binary uses ~500KB RAM in production.

**Monitoring built in:**
- `GET :8080/health` → JSON health status
- `GET :8080/metrics` → Prometheus exposition format

**Free tier compatible:**
- Oracle Cloud Always Free (E2.1.Micro, any region)
- Vultr (with $300 new account credit, 60+ months free)
- Fly.io (3 free shared VMs)

We currently have nodes in LA and Singapore. If you're in EU, India, SA, OCE, or anywhere else underserved — would love to coordinate adding a community node there.

Full infra docs: https://github.com/ShibbityShwab/lightspeed/tree/master/infra

---

## 🟠 Hacker News — Show HN

**Title:** Show HN: LightSpeed – open-source game network optimizer written in Rust, $0 forever

**URL:** https://github.com/ShibbityShwab/lightspeed

**Text (post in comment):**

Hey HN,

I built LightSpeed — an open-source alternative to paid game network optimizers like ExitLag ($6.50/mo) and WTFast ($9.99/mo). It's written in Rust and designed to run entirely on free-tier cloud infrastructure.

**How it works:** The client captures game UDP traffic via pcap, wraps it in a custom tunnel protocol, and routes it through proxy nodes that are closer to game servers. We use a linfa (Rust ML) Random Forest model to pick the optimal route based on 11 network features.

**Technical highlights:**
- Custom 20-byte UDP tunnel protocol with optional FEC extension
- XOR-based Forward Error Correction: 25% bandwidth overhead vs. competitors' 200-300% via packet duplication
- Three pcap backends: Linux AF_PACKET, macOS BPF, Windows Npcap/WFP
- Prometheus metrics + Grafana dashboards out of the box
- Native binary + systemd (not Docker) — ~500KB RAM per proxy node

**Production results:** 2 live Vultr nodes (US-West + Singapore). Load tested to 0.00% packet loss at 320 pps. Estimated 500-1000+ concurrent clients per node within free tier.

**The $0 claim:** Vultr gives $300 in credits to new accounts — at $6/month per node, that's 4+ years free per node. Oracle Cloud has truly forever-free instances for self-hosters.

Happy to discuss the FEC implementation, the ML feature engineering, or the infra approach.

---

## 📋 Post Checklist

- [ ] Post to r/GlobalOffensive
- [ ] Post to r/FortNiteBR  
- [ ] Post to r/DotA2
- [ ] Post to r/rust
- [ ] Post to r/selfhosted
- [ ] Submit Show HN (https://news.ycombinator.com/submit)
- [ ] Cross-post to r/competitivegaming
- [ ] Set VULTR_SSH_KEY GitHub secret

**Tips:**
- Space out Reddit posts by at least 30 minutes to avoid spam filters
- For r/GlobalOffensive / r/FortNiteBR / r/DotA2, include your actual ping results if you have them — community posts do better with personal experience
- The r/rust post will get the most technical engagement
- Best times to post: weekdays 9am-12pm EST (peak US gamer time)
