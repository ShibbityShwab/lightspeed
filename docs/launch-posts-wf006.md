# WF-006 Step 5 — Launch Announcement Posts

Ready-to-post copy for Reddit and Hacker News. Copy-paste the relevant section.

**Posting order (space 30+ min apart on Reddit to avoid spam filters):**
1. r/GlobalOffensive
2. r/FortNiteBR
3. r/DotA2
4. r/rust
5. r/selfhosted
6. r/pcgaming
7. Hacker News "Show HN"

**Best times:** Weekdays 9 am–12 pm EST (= 9 pm–midnight Bangkok time).

---

## 🔴 Reddit — r/GlobalOffensive

**Title:** I built a free open-source ExitLag alternative in Rust — 0.00% packet loss in testing

**Body:**

Hey everyone,

I've been frustrated paying $6–10/month for network optimizers like ExitLag and WTFast, so I built one and open-sourced it — **LightSpeed** ([GitHub](https://github.com/ShibbityShwab/lightspeed)).

**What it does:** Routes your CS2 UDP traffic through proxy nodes that have better routing to game servers than your ISP does. Exactly like ExitLag — but completely free.

**Current stats:**
- 2 live proxy nodes: US-West (Los Angeles) and Singapore
- **0.00% packet loss** on both nodes in load testing
- 31 ms relay through Singapore node (tested from Bangkok)
- Built-in CS2 game profile out of the box

**Why it's actually different from paid tools:**
- Genuinely $0 — no trials, no subscriptions, no freemium
- Fully open source — audit every line yourself
- Never touches game packets in a way that would trigger VAC (anti-cheat safe by design)
- No accounts, no data collection

**Tech:** Rust + Tokio, custom UDP tunnel, XOR-based FEC at 25% overhead (vs ExitLag's 200–300% via full packet duplication).

**Download v0.3.0** (pre-built binaries, no build tools needed):

- [Windows x64](https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.3.0) — `lightspeed-v0.3.0-windows-x64.zip`
- [Linux x64 / ARM64](https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.3.0) — `.tar.gz`

Or build from source in one step:
```bash
git clone https://github.com/ShibbityShwab/lightspeed.git
cd lightspeed && cargo build --release
```

Landing page: https://shibbityshwab.github.io/lightspeed/

Would love feedback on which regions to prioritise next. What's your worst ping route for CS2?

---

## 🔴 Reddit — r/FortNiteBR

**Title:** I built a free open-source ExitLag alternative in Rust — works for Fortnite

**Body:**

Hey,

Tired of paying $6–10/month for network optimizers like ExitLag just to get playable ping, so I built an open-source one — **LightSpeed** ([GitHub](https://github.com/ShibbityShwab/lightspeed)).

**What it does:** Routes your Fortnite UDP traffic through optimised proxy nodes, reducing ping when your ISP's routing to Epic's servers is bad.

**Current stats:**
- 2 live proxy nodes: US-West (Los Angeles) and Singapore
- **0.00% packet loss** on both nodes under load
- 31 ms relay through Singapore node (tested from Bangkok — great for SEA players)
- Built-in Fortnite game profile

**Why bother vs paid tools:**
- Completely free — $0, forever, no card required
- Open source — you can read and verify every line
- Anti-cheat safe — doesn't modify or inject anything into game packets
- No accounts, no telemetry

**Download v0.3.0** (pre-built, no Rust required):
- [Windows x64 / Linux x64 / Linux ARM64](https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.3.0)

Or: `git clone https://github.com/ShibbityShwab/lightspeed.git && cargo build --release`

Landing page: https://shibbityshwab.github.io/lightspeed/

Which region gives you the worst Fortnite ping? Happy to prioritise next node locations based on feedback.

---

## 🔴 Reddit — r/DotA2

**Title:** I built a free open-source ExitLag alternative in Rust — Dota 2 profile included

**Body:**

Hey r/DotA2,

Built an open-source game network optimizer and wanted to share it here since Dota ping misery is a real thing — **LightSpeed** ([GitHub](https://github.com/ShibbityShwab/lightspeed)).

**The problem it solves:** When your ISP has garbage routing to Valve's game servers (especially for SEA/SA/ME players), routing through a proxy node with better peering can meaningfully cut your ping and jitter.

**Current setup:**
- 2 live nodes: US-West (Los Angeles) and Singapore
- **0.00% packet loss** on both nodes under load test
- Singapore relay: 31 ms latency tested from Bangkok
- Built-in Dota 2 game profile

**vs ExitLag/WTFast:**
- Costs $0 — no subscriptions, ever
- Fully open source — nothing hidden
- Anti-cheat safe by design (passive UDP rerouting, no packet modification)
- No accounts, no data collected

**Download v0.3.0** (pre-built binaries):
- [Windows x64 / Linux x64 / Linux ARM64](https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.3.0)

Landing page: https://shibbityshwab.github.io/lightspeed/

If you're on a high-ping server and want to try it, let me know your region — I'm prioritising new node locations based on where the need is highest.

---

## 🔴 Reddit — r/rust

**Title:** Show r/rust: I built an open-source game network optimizer — UDP tunneling, XOR FEC, linfa ML route selection

**Body:**

Hey rustaceans,

I built **LightSpeed** — an open-source game network optimizer (think ExitLag but free) written entirely in Rust. Wanted to share the technical side since the stack is pretty interesting.

**Repo:** https://github.com/ShibbityShwab/lightspeed  
**Release:** [v0.3.0](https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.3.0) — pre-built binaries for Windows/Linux/ARM64

### Architecture

```
Game (UDP) → Client (pcap capture) → LightSpeed Tunnel → Proxy Node → Game Server
```

**Client stack:**
- `pcap-rs` for packet capture (three backends: Linux `AF_PACKET`, macOS BPF, Windows Npcap)
- `tokio` async runtime for all I/O
- `linfa` ML library for route selection (Random Forest, 11 features, <1 ms inference)
- Custom 20-byte tunnel header with optional 4-byte FEC extension

**Protocol features:**
- XOR-based Forward Error Correction — recovers lost packets at 25% bandwidth overhead (vs ExitLag's 200–300% via full duplication)
- Keepalive flag in header (flag byte `0x01`)
- Multipath capability: simultaneous relay through multiple proxy nodes

**Proxy server:**
- UDP relay with auth token verification
- Prometheus metrics (20+ counters/histograms labelled by region + `node_id`)
- JSON health endpoint at `:8080/health`
- Rate limiting + abuse detection
- ~500 KB RAM per node in production

**ML Route Selector:**
11 features fed into a linfa Random Forest:
- `current_latency_ms`, `historical_p50_ms`, `historical_p95_ms`
- `jitter_ms`, `packet_loss_pct`, `proxy_load`
- `hop_count`, `bgp_as_path_len`, `geographic_distance_km`
- `time_of_day`, `day_of_week`

The online learning loop is fully implemented: every 50 new probe samples the model retrains on live RTT data + a synthetic mix, saves the new model to disk, and swaps it in — no restart required. The first session uses a synthetic bootstrap model; subsequent sessions load the saved one.

**Infrastructure:**
2 live Vultr nodes (LA + Singapore), native binary + systemd (no Docker), Terraform for provisioning, Prometheus + Grafana for monitoring. Total ongoing cost: **$0** (Vultr credits, 60+ months of runway).

Load test: **0.00% packet loss** on both nodes, ~320 pps throughput confirmed.

Happy to answer questions about the FEC implementation, the pcap backends handling different OS quirks, or the ML feature engineering.

---

## 🔴 Reddit — r/selfhosted

**Title:** LightSpeed — self-hostable game network optimizer. Run your own proxy node on any small Linux VPS.

**Body:**

Hey r/selfhosted,

Built a game network optimizer that you can fully self-host — **LightSpeed** ([GitHub](https://github.com/ShibbityShwab/lightspeed)).

**The pitch:** If you have a VPS in a region where gamers have bad ISP routing to game servers, you can run a LightSpeed proxy node and help people in your region cut their ping. The whole stack is open source.

**Quick setup on any Linux VPS (512 MB RAM minimum):**
```bash
curl -sL https://raw.githubusercontent.com/ShibbityShwab/lightspeed/master/infra/scripts/setup-new-node.sh | bash
```

That installs the proxy binary + systemd service + fail2ban rules.

**Minimum requirements:** 512 MB RAM, UDP 4433/4434 open, TCP 8080 for health/metrics. The binary uses ~500 KB RAM in production.

**Monitoring built in:**
```
GET :8080/health   → JSON health status
GET :8080/metrics  → Prometheus exposition format
```

**Free-tier compatible hosting options (personally tested / validated):**
- **Vultr** — vc2-1c-1gb ($6/mo but $300 new-account credit = 50 months free per node)
- Any small Linux VPS with 512 MB RAM and UDP port access will work

> Note: Oracle Cloud and Fly.io are listed in some older docs — both have free tiers that *should* work, but we migrated off OCI due to resource limitations. YMMV.

We currently have community nodes in LA and Singapore. If you're in EU, India, OCE, or anywhere else underserved — would love to coordinate adding a community node there.

Full infra docs: https://github.com/ShibbityShwab/lightspeed/tree/master/infra

---

## 🔴 Reddit — r/pcgaming

**Title:** I made a free open-source alternative to ExitLag/WTFast — written in Rust, $0 forever

**Body:**

Hey r/pcgaming,

Sick of paying ~$7–10/month for network optimizers just to get decent ping in online games, so I built an open-source one — **LightSpeed** ([GitHub](https://github.com/ShibbityShwab/lightspeed)).

**How it works:** The client captures your game's UDP traffic and routes it through a proxy node that has better peering to game servers than your ISP. Same concept as ExitLag, WTFast, etc. — minus the subscription.

**Live nodes:** US-West (Los Angeles) + Singapore  
**Load tested:** 0.00% packet loss at 320 pps  
**Anti-cheat safe:** Passive UDP rerouting, no game memory access

**Built-in game profiles:** CS2, Fortnite, Dota 2 (more coming based on demand)

**Download v0.3.0** — pre-built, no build tools required:
- [Windows x64 / Linux x64 / Linux ARM64](https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.3.0)

Landing page: https://shibbityshwab.github.io/lightspeed/

Happy to hear which games or regions would be most useful to add next.

---

## 🟠 Hacker News — Show HN

**Title:** Show HN: LightSpeed – open-source game network optimizer written in Rust, $0 forever

**URL:** https://github.com/ShibbityShwab/lightspeed

**Text (post as top-level comment):**

Hey HN,

I built LightSpeed — an open-source alternative to paid game network optimizers like ExitLag ($6.50/mo) and WTFast ($9.99/mo). Written in Rust, designed to run entirely on free-tier or low-cost cloud infrastructure.

**How it works:** The client captures game UDP traffic via pcap, wraps it in a custom tunnel protocol, and routes it through proxy nodes with better peering to game servers. A linfa (Rust ML) Random Forest chooses the optimal route based on 11 network features.

**Technical highlights:**
- Custom 20-byte UDP tunnel protocol with optional FEC extension
- XOR-based Forward Error Correction: 25% bandwidth overhead vs. competitors' 200–300% via packet duplication
- Three pcap backends: Linux `AF_PACKET`, macOS BPF, Windows Npcap
- Prometheus metrics + Grafana dashboards out of the box
- Native binary + systemd (not Docker) — ~500 KB RAM per proxy node

**Production results:** 2 live Vultr nodes (US-West + Singapore). Load tested to 0.00% packet loss at 320 pps. Estimated 500–1,000+ concurrent clients per node within free-tier resource limits.

**The $0 claim:** Vultr gives $300 in credits to new accounts — at $6/month per node that's 50 months free per node. And anyone can self-host a community node from the same repo.

**v0.3.0** release with pre-built Windows/Linux/ARM64 binaries: https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.3.0

Happy to discuss the FEC implementation, the ML feature engineering, or the infra approach.

---

## 📋 Post Checklist

- [ ] Post to r/GlobalOffensive
- [ ] Post to r/FortNiteBR (30+ min after previous)
- [ ] Post to r/DotA2 (30+ min after previous)
- [ ] Post to r/rust (30+ min after previous)
- [ ] Post to r/selfhosted (30+ min after previous)
- [ ] Post to r/pcgaming (30+ min after previous)
- [ ] Submit Show HN (https://news.ycombinator.com/submit)

**Tips:**
- Each sub has distinct framing above — don't reuse the same body across gaming subs to avoid spam filters
- For the gaming subs, mentioning your own experience (your actual ping numbers if you have them) performs better than pure feature lists
- The r/rust post will get the most technical engagement and is likely to drive repo stars
- HN Show HN posts do best Mon–Fri 7–9 am EST; the body text goes in a top-level comment on your own post
- Best overall time: weekdays 9 am–12 pm EST (= 9 pm–midnight Bangkok)
