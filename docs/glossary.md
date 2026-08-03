# Glossary

---

## Core Concepts

**Proxy Node (Boost Server):** A lightweight UDP relay server (~500KB RAM) that forwards game traffic between your PC and game servers through a faster backbone path. You run your own proxy on any Linux VPS.

**Interceptor:** The OS-level component that captures outbound game UDP packets before they leave your network interface and redirects them through the proxy. Uses nftables/iptables (Linux), pfctl (macOS), or WinDivert (Windows).

**Tunnel:** The encapsulated UDP connection between your PC and the proxy node. Game packets are wrapped in a 20-byte LightSpeed header that preserves the original source/destination IPs.

**Session Token:** An 8-bit authentication token assigned by the proxy during registration. Included in every data-plane packet to prevent unauthorized relay usage.

---

## Routing & Performance

**FEC (Forward Error Correction):** XOR-based packet loss recovery. For every K data packets, one parity packet is sent. If any single packet in the block is lost, it can be reconstructed from the remaining packets. Uses ~25% bandwidth at K=4 (vs. ExitLag's 200% duplication approach).

**K (Block Size):** Number of data packets per FEC parity packet. Default is 4 (25% overhead). Higher K = less overhead but less protection against burst loss.

**RTT (Round-Trip Time):** The time in milliseconds for a packet to travel from your PC to the proxy and back. Measured continuously during keepalive probing.

**Keepalive:** Periodic empty packets sent between client and proxy to measure RTT and maintain the session. Sent every 5 seconds.

**Multipath:** Sending data packets on one path and FEC parity packets on a secondary path. If the primary path drops a packet, it's recovered from the parity on the secondary path.

---

## Modes

**Interceptor Mode (`--start-interceptor`):** Kernel-level MITM that transparently redirects game traffic through the proxy. No game configuration needed — the game connects normally.

**Redirect Mode (`--game-server`):** The game connects to localhost on a specific port, and LightSpeed forwards traffic to the real server through the proxy. No root required.

**Capture Mode (`--capture`):** Uses libpcap to read packets from a network interface. Requires the `pcap-capture` feature and elevated privileges.

**Watch Mode (`--watch`):** Monitors for a game process and automatically starts the interceptor when the game launches.

---

## Protocol

**Tunnel Header:** 20-byte binary header prepended to every game packet in the tunnel. Contains version, flags, session token, sequence number, timestamp, and original source/destination IP/port.

**FEC Header:** 4-byte extension appended after the tunnel header when FEC is active. Contains block ID, packet index, and block size.

**Control Plane:** QUIC-based channel for proxy registration, session token distribution, and health checks. Separate from the data plane (UDP tunnel).

**Data Plane:** The actual UDP tunnel carrying game packets. Unencrypted by design to remain compatible with anti-cheat systems.

---

## Infrastructure

**VPS (Virtual Private Server):** A cloud-hosted virtual machine running Linux. LightSpeed proxies run on VPS instances with as little as 512MB RAM.

**systemd:** Linux service manager. The proxy runs as a systemd service for automatic startup and crash recovery.

**GHCR (GitHub Container Registry):** Where pre-built Docker images of the proxy are published: `ghcr.io/shibbityshwab/lightspeed-proxy`.

---

## Anti-Cheat

**EAC (EasyAntiCheat):** Used by Rust, Fortnite, Apex Legends. Compatible with LightSpeed.

**VAC (Valve Anti-Cheat):** Used by CS2, Dota 2. Compatible.

**BattlEye:** Used by Fortnite, PUBG. Compatible.

**Riot Vanguard:** Used by Valorant, League of Legends. Compatible.

**Blizzard Warden:** Used by Overwatch 2. Compatible.
