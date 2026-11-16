---
name: lightspeed-windows
description: Use when running commands, builds, tests, scripts or GUI checks for LightSpeed on this Windows development host, or when a command fails for a platform reason. Covers PowerShell 5.1 limits, the real toolchain, elevation and single-instance quirks, missing Python/bash, and which work must run remotely.
---

# LightSpeed on Windows

> Paths in this document are relative to the repository root, not to this skill directory.

This host is the primary development machine. Everything below was observed on
it; do not substitute a POSIX habit.

## Shell

- The only shell is **Windows PowerShell 5.1**. `pwsh` is not installed.
  - No `??`, no ternary, no `ProcessStartInfo.ArgumentList`, no `$PSVersionTable`
    workarounds that assume 7.x.
  - Check exit codes with `$LASTEXITCODE` after native commands; use
    `-ErrorAction` for cmdlets.
- **No Git Bash, no WSL, no `bash script.sh`.** `git.exe` is a Windows build and
  works directly.
- Run commands with the `pwsh` tool. Do not `cd` in one call and expect the next
  call to remember it; pass `workdir` instead.
- `cmd`/`.bat` files stay CRLF (`.gitattributes`); all other text is LF, so a
  fresh `git checkout` and the `edit` tool agree.

## Toolchain reality

| Tool | State |
|---|---|
| `cargo` / `rustc` | **Installed**, 1.100.0 via `rust-toolchain.toml`; host `x86_64-pc-windows-msvc` |
| `cargo fmt` / `cargo clippy` | Installed (rustup components) |
| `cargo audit` / `cargo deny` | **Missing** — `cargo install cargo-audit cargo-deny`, or run `Check-LightSpeed.ps1 -SkipAudit`; CI's security workflow owns both gates |
| `git` | Installed (`C:\Program Files\Git\cmd\git.exe`) |
| `node` | Installed |
| `gh`, `winget` | Installed |
| `python` / `python3` | **Microsoft Store stubs only** — `python -c ...` exits 9009 |
| `py` | Missing (no `C:\Windows\py.exe`) |
| `pnpm`, `docker`, `choco`, `scoop` | Missing |

Consequences:

- The localization pipeline (`scripts/`), `tools/e2e_test.py`, `tools/load_test.py`
  and `scripts/sync-wiki.py` need a real CPython 3.8+. That is a documented
  prerequisite (`winget install Python.Python.3.12`, then use `py -3`), not
  something to fake. Without it, say the step cannot run here.
- Relay operations (`infra/scripts/*.sh`) are Linux artifacts. They run on the
  relay hosts and in CI, never on this desktop.

## Build and test gates

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
cargo clippy -p lightspeed-gui --all-targets      # GUI is excluded above
cargo test -p lightspeed-gui
cargo test -p lightspeed-client --features windivert-redirect
```

`tools/Check-LightSpeed.ps1` runs the pre-push set; `tools/check.cmd` is the same
gate for a plain `cmd` prompt. The GUI crate is excluded from workspace clippy on
every platform — lint it explicitly on Windows, which is the only platform where
it builds.

## GUI quirks

- A single-instance mutex is shared across **all** builds: a stray
  `target/debug` process silently wins over an installed launch. Check
  `Get-Process lightspeed-gui` before launching; `scripts/dev-gui.ps1` guards it.
- Elevation does not propagate into the agent shell. A medium-integrity shell
  cannot stop an elevated GUI (`Stop-Process -Force` fails "Could not stop
  process"), and a running GUI locks its own exe (`Access is denied (os error 5)`
  on rebuild). Use `scripts/dev-gui.ps1 -Elevate`, stop from the tray, and verify
  installs by file size, not exit code (`scripts/install-gui.ps1` already does).
- egui popups are invisible to accessibility tooling. Verify a collapsed control
  by its selected text and a screenshot, not by the AX tree.
- Launch a GUI build with `Start-Process` and keep the handle; do not leave
  console windows behind.

## Evidence

Screenshots, command transcripts and benchmark tables go to
`wat/evidence/<YYYY-MM-DD>/` (gitignored) and are named in the task report and
the state entry. "It worked" without a path is unverified.

## When the host cannot do it

Say so explicitly and route it:

- Linux relay work → the relay host over SSH, or `infra/` CI.
- macOS packaging → a macOS runner or contributor.
- Python-dependent scripts → install CPython (owner decision) or run in CI.
- Sandbox denials → they are DSH policy; escalate the exact operation once with
  `sandbox_permissions` rather than finding another route.
