# Current Phase — WF-015: Token Auth & Client Session Stamping

**Workflow:** WF-015
**Agent:** RustDev + QAEngineer + SecOps
**Status:** ✅ Token auth wired end-to-end — `require_auth` now defaults to `true` (pending 1.1.0 tag)
**Last updated:** 2026-08-18

---

## Completed Steps (WF-015)

| Step | Description | Status |
|------|-------------|--------|
| 1 | `session.rs` — global atomic session-token holder | ✅ Done |
| 2 | `ControlClient::connect()` sets the token after `RegisterAck` | ✅ Done |
| 3 | `register_session()` best-effort registration helper (quic-gated) | ✅ Done |
| 4 | Stamp `.with_session_token()` on every data-plane header site | ✅ Done |
| 5 | Wire registration into `main.rs` modes + GUI engine entry points | ✅ Done |
| 6 | `quic` feature enabled in Dockerfile + cargo-dist client build + GUI dep | ✅ Done |
| 7 | `require_auth` default flipped to `true` (config + tomls + provision.sh) | ✅ Done |

---

## Token Auth Summary

- **Client** registers over QUIC, receives a session token, and stamps it into every
  data-plane packet header.
- **Proxy** (with `require_auth = true`, the new default) validates the (IP, token) pair
  per packet; unregistered clients are rejected.
- **Backward compatible**: a client without the `quic` feature sends token `0`, which the
  proxy accepts only when `require_auth = false`.

---

## Next Action

1. **Tag and push v1.1.0** (token auth release — pending explicit release step)
2. **WF-016**: Configurable ports (issue #39)
3. **WF-017**: TCP tunnel (issue #10)
