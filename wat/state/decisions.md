# Decision Log

---

## DEC-000: WAT System Initialized
- **Date**: 2026-02-21T20:25:00+07:00
- **Agent**: Autonomy Engine
- **Decision**: Begin with WF-001 (MVP Build) as primary workflow
- **Rationale**: Client and proxy are the foundation for everything else. WF-002 and WF-003 can start in parallel but WF-004 depends on WF-001 + WF-002 completion.
- **Alternatives Considered**: Start with WF-002 (infrastructure first), but code can be developed locally without proxies
- **Status**: ACCEPTED

---

## DEC-001: Use Cargo Feature Gating for Heavy Dependencies
- **Date**: 2026-02-21T21:05:00+07:00
- **Agent**: Architect
- **Decision**: Gate quinn/rustls (`quic`), pcap (`pcap-capture`), and linfa (`ml`) behind optional cargo features. Default build requires only Rust toolchain.
- **Rationale**: These crates require a C compiler (ring, libpcap, BLAS) which adds dev environment complexity. Feature gating allows incremental development — core architecture compiles cleanly, heavy deps added when ready.
- **Alternatives Considered**: (1) Require C compiler for all builds — rejected, too heavy for initial scaffolding. (2) Replace ring with pure-Rust TLS — no mature option exists.
- **Status**: ACCEPTED

---

## DEC-002: Use stable-x86_64-pc-windows-gnu Toolchain
- **Date**: 2026-02-21T21:02:00+07:00
- **Agent**: DevOps
- **Decision**: Use GNU toolchain on Windows (via MSYS2 MinGW-w64) instead of MSVC
- **Rationale**: No Visual Studio Build Tools available on dev machine. MinGW provides gcc + dlltool needed for compilation and linking. MSYS2 is a lightweight alternative.
- **Alternatives Considered**: (1) Install Visual Studio Build Tools (~6GB) — too heavy. (2) Cross-compile from Linux — not practical for daily dev.
- **Impact**: Binaries target windows-gnu ABI. Production releases may switch to MSVC for better Windows integration.
- **Status**: ACCEPTED

---

## DEC-003: 20-byte Custom UDP Tunnel Header
- **Date**: 2026-02-21T21:10:00+07:00
- **Agent**: NetEng
- **Decision**: Use a fixed 20-byte header: 4-bit version, 4-bit flags, 1 byte reserved, 2-byte sequence, 4-byte timestamp, 4+4 byte original IPs, 2+2 byte original ports.
- **Rationale**: Minimal overhead for game packets (typically 50-500 bytes). Fixed size avoids parsing complexity. Sequence numbers enable multipath dedup. Timestamps enable RTT measurement.
- **Alternatives Considered**: (1) Variable-length header with TLV extensions — too complex for MVP. (2) Smaller header without IP fields — can't preserve original addressing. (3) Use existing protocols (GRE, VXLAN) — too much overhead, don't carry original ports.
- **Status**: ACCEPTED

---

## DEC-004: Architecture Design Complete
- **Date**: 2026-02-21T21:10:00+07:00
- **Agent**: Architect
- **Decision**: WF-001 Step 1 (Architecture Design) is complete. Deliverables: `docs/architecture.md`, compiled Rust workspace with all module stubs and trait definitions.
- **Rationale**: All module interfaces defined via traits (PacketCapture, RouteSelector, GameConfig). Both crates compile. 5 unit tests pass for header encode/decode. Protocol specification finalized.
- **Status**: ACCEPTED
- **HANDOFF**: RustDev (Step 2a: implement tunnel engine), NetEng (Step 2b: protocol review — already done)
