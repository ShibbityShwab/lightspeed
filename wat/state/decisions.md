# Decision Log

All architecture and process decisions, with rationale and status.

| ID | Decision | Rationale | Status | Date |
|----|----------|-----------|--------|------|
| DEC-001 | Use Rust for both client and proxy | Performance, safety, single language reduces cognitive load | ACCEPTED | 2026-02-22 |
| DEC-002 | Unencrypted UDP tunnel | Zero overhead, anti-cheat compatible, IP preservation | ACCEPTED | 2026-02-22 |
| DEC-003 | Oracle Cloud Always Free for proxy hosting | 4 ARM OCPUs + 24GB RAM + 10TB egress at $0/month | ACCEPTED | 2026-02-22 |
| DEC-004 | linfa for ML (not Python) | Keep everything in Rust, sub-1ms inference, no FFI overhead | ACCEPTED | 2026-02-22 |
| DEC-005 | QUIC (quinn) for control plane | Reliable, multiplexed, 0-RTT reconnect, built-in TLS | ACCEPTED | 2026-02-22 |
| DEC-006 | Token-based auth (QUIC registration → data-plane) | Lightweight, no per-packet crypto, abuse prevention | ACCEPTED | 2026-02-22 |
| DEC-007 | Destination IP validation (block RFC1918) | Prevent proxy from being used to scan internal networks | ACCEPTED | 2026-02-22 |
| DEC-008 | MVP Release v0.1.0 — GitHub + CI/CD | Public repo, 3-platform release, GitHub Actions | ACCEPTED | 2026-02-22 |
| DEC-009 | 3-region MVP: Ashburn, Frankfurt, Singapore | Best coverage for NA/EU/SEA game servers within free tier budget (3 of 4 OCPUs) | ACCEPTED | 2026-02-22 |
| DEC-010 | ARM Ampere A1 Flex (1 OCPU, 6GB per node) | Optimal split of free tier resources: 3 nodes with headroom for 4th | ACCEPTED | 2026-02-22 |
| DEC-011 | Docker + host networking for proxy deployment | Host networking eliminates NAT overhead for UDP; Docker for easy updates | ACCEPTED | 2026-02-22 |
| DEC-012 | Cloud-init for node bootstrapping | Single-touch provisioning: Docker, firewalld, fail2ban, kernel tuning auto-configured | ACCEPTED | 2026-02-22 |
| DEC-013 | GHCR (GitHub Container Registry) for Docker images | Free for public repos, integrated with GitHub Actions, multi-arch support | ACCEPTED | 2026-02-22 |
| DEC-014 | Defense-in-depth security: firewalld + fail2ban + app-level rate limiting | Three layers: OS firewall, IP banning, application rate limits | ACCEPTED | 2026-02-22 |
